use crate::context::*;
use crate::position::lsp_range_to_kakoune;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use regex::Regex;
use serde::Deserialize;
use url::Url;

pub fn text_document_completion(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = TextDocumentCompletionParams::deserialize(params).unwrap();
    let req_params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        context: None,
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    ctx.call::<Completion, _>(meta, req_params, |ctx: &mut Context, meta, result| {
        editor_completion(meta, params, result, ctx)
    });
}

pub fn editor_completion(
    meta: EditorMeta,
    params: TextDocumentCompletionParams,
    result: Option<CompletionResponse>,
    ctx: &mut Context,
) {
    if result.is_none() {
        return;
    }
    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    let unescape_markdown_re = Regex::new(r"\\(?P<c>.)").unwrap();
    let maxlen = items.iter().map(|x| x.label.len()).max().unwrap_or(0);
    let escape_bar = |s: &str| s.replace("|", r"\|");
    let snippet_prefix_re = Regex::new(r"^[^\[\(<\n\$]+").unwrap();

    let items = items
        .into_iter()
        .map(|x| {
            let mut doc = x.documentation.map(|doc| {
                match doc {
                    Documentation::String(st) => st,
                    Documentation::MarkupContent(mup) => match mup.kind {
                        MarkupKind::PlainText => mup.value,
                        // NOTE just in case server ignored our documentationFormat capability
                        // we want to unescape markdown to make text a bit more readable
                        MarkupKind::Markdown => unescape_markdown_re
                            .replace_all(&mup.value, r"$c")
                            .to_string(),
                    },
                }
            });

            if let Some(detail) = x.detail {
                doc = doc.map(|doc| format!("{}\n\n{}", detail, doc));
            }

            let doc = doc
                .map(|doc| format!("info -style menu -- %§{}§", doc.replace("§", "\\§")))
                .unwrap_or_else(|| String::from("nop"));

            let mut entry = x.label.clone();
            if let Some(k) = x.kind {
                entry += &std::iter::repeat(" ")
                    .take(maxlen - x.label.len())
                    .collect::<String>();
                entry += &format!(" {{MenuInfo}}{:?}", k);
            }
            // The generic textEdit property is not supported yet (#40).
            // However, we can support simple text edits that only replace the token left of the
            // cursor. Kakoune will do this very edit if we simply pass it the replacement string
            // as completion.
            let is_simple_text_edit = x.text_edit.as_ref().map_or(false, |cte| {
                let document = match ctx.documents.get(&meta.buffile) {
                    Some(doc) => doc,
                    None => {
                        warn!("No document in context for file: {}", &meta.buffile);
                        return false;
                    }
                };
                if let CompletionTextEdit::Edit(text_edit) = cte {
                    let range =
                        lsp_range_to_kakoune(&text_edit.range, &document.text, ctx.offset_encoding);
                    range.start.line == params.position.line
                        && range.end.line == params.position.line
                        && (range.end.column == params.position.column // Not sure why this case happens, see #455
                            || range.end.column + 1 == params.position.column)
                } else {
                    false
                }
            });
            let insert_text = &if is_simple_text_edit {
                if let CompletionTextEdit::Edit(te) = x.text_edit.unwrap() {
                    te.new_text
                } else {
                    x.insert_text.unwrap_or(x.label)
                }
            } else {
                x.insert_text.unwrap_or(x.label)
            };
            let do_snippet = ctx.config.snippet_support;
            let do_snippet = do_snippet
                && x.insert_text_format
                    .map(|f| f == InsertTextFormat::Snippet)
                    .unwrap_or(false);
            if do_snippet {
                let snippet = insert_text;
                let insert_text = snippet_prefix_re
                    .find(snippet)
                    .map(|x| x.as_str())
                    .unwrap_or(&snippet);
                let command = format!(
                    "{}\nlsp-snippets-insert-completion {} {}",
                    doc,
                    editor_quote(&regex::escape(insert_text)),
                    editor_quote(snippet)
                );
                let command = format!("eval -verbatim -- {}", command);
                editor_quote(&format!(
                    "{}|{}|{}",
                    escape_bar(insert_text),
                    escape_bar(&command),
                    escape_bar(&entry),
                ))
            } else {
                editor_quote(&format!(
                    "{}|{}|{}",
                    escape_bar(insert_text),
                    escape_bar(&doc),
                    escape_bar(&entry),
                ))
            }
        })
        .join(" ");
    let p = params.position;
    let command = format!(
        "set window lsp_completions {}.{}@{} {}\n",
        p.line, params.completion.offset, meta.version, items
    );
    ctx.exec(meta, command);
}
