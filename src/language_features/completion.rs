use crate::context::*;
use crate::markup::*;
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

    // Length of the longest label in the current completion list
    let maxlen = items.iter().map(|x| x.label.len()).max().unwrap_or(0);
    let escape_bar = |s: &str| s.replace("|", "\\|");
    let snippet_prefix_re = Regex::new(r"^[^\[\(<\n\$]+").unwrap();

    let mut inferred_offset: Option<u32> = None;
    let mut can_infer_offset = true;

    let force_plaintext = ctx
        .config
        .language
        .get(&ctx.language_id)
        .and_then(|l| l.workaround_server_sends_plaintext_labeled_as_markdown)
        .unwrap_or(false);
    let items = items
        .into_iter()
        .map(|x| {
            let doc = x.documentation.map(|doc| match doc {
                Documentation::String(s) => s,
                Documentation::MarkupContent(content) => content.value,
            });

            // Combine the 'detail' line and the full-text documentation into
            // a single string. If both exist, separate them with a horizontal rule.
            let markdown = {
                let mut markdown = String::new();

                if let Some(detail) = x.detail {
                    markdown.push_str(&detail);

                    if doc.is_some() {
                        markdown.push_str("\n\n---\n\n");
                    }
                }

                if let Some(doc) = doc {
                    markdown.push_str(&doc);
                }

                markdown
            };

            let doc = if !markdown.is_empty() {
                let markup = markdown_to_kakoune_markup(markdown, force_plaintext);
                format!(
                    "info -markup -style menu -- %§{}§",
                    markup.replace("§", "§§")
                )
            } else {
                // When the user scrolls through the list of completion candidates, Kakoune
                // does not clean up the info box. We need to do that explicitly, in this case by
                // requesting an empty one.
                "info -style menu ''".to_string()
            };

            let entry = match x.kind {
                Some(k) => format!(
                    "{}{} {{MenuInfo}}{:?}",
                    &x.label,
                    " ".repeat(maxlen - x.label.len()),
                    k
                ),
                None => x.label.clone(),
            };

            let insert_text = x
                .text_edit
                .and_then(|cte| {
                    let document = match ctx.documents.get(&meta.buffile) {
                        Some(doc) => doc,
                        None => {
                            warn!("No document in context for file: {}", &meta.buffile);
                            can_infer_offset = false;
                            return None;
                        }
                    };

                    match cte {
                        CompletionTextEdit::Edit(text_edit) => {
                            // The generic textEdit property is not supported yet (#40).  However,
                            // we can support simple text edits that only replace the token left
                            // of the cursor. Kakoune will do this very edit if we simply pass it
                            // the replacement string as completion.
                            let range = lsp_range_to_kakoune(
                                &text_edit.range,
                                &document.text,
                                ctx.offset_encoding,
                            );

                            if can_infer_offset {
                                match inferred_offset {
                                    None => inferred_offset = Some(range.start.column),
                                    Some(offset) if offset != range.start.column => {
                                        can_infer_offset = false;
                                        inferred_offset = None
                                    }
                                    _ => (),
                                }
                            };

                            if range.start.line == params.position.line
                            && range.end.line == params.position.line
                            // Not sure why this case happens, see #455
                            && (range.end.column == params.position.column
                                || range.end.column + 1 == params.position.column)
                            {
                                Some(text_edit.new_text)
                            } else {
                                None
                            }
                        }
                        CompletionTextEdit::InsertAndReplace(_) => {
                            can_infer_offset = false;
                            None
                        }
                    }
                })
                .or(x.insert_text)
                .unwrap_or(x.label);

            // If snippet support is both enabled and provided by the server,
            // we'll need to perform some transformations on the completion commands.
            if ctx.config.snippet_support && x.insert_text_format == Some(InsertTextFormat::Snippet)
            {
                let snippet = insert_text;
                let insert_text = snippet_prefix_re
                    .find(&snippet)
                    .map(|x| x.as_str())
                    .unwrap_or(&snippet);

                let command = format!(
                    "{}\nlsp-snippets-insert-completion {} {}",
                    doc,
                    editor_quote(&regex::escape(insert_text)),
                    editor_quote(&snippet)
                );

                editor_quote(&format!(
                    "{}|{}|{}",
                    escape_bar(insert_text),
                    escape_bar(&command),
                    escape_bar(&entry),
                ))
            } else {
                editor_quote(&format!(
                    "{}|{}|{}",
                    escape_bar(&insert_text),
                    escape_bar(&doc),
                    escape_bar(&entry),
                ))
            }
        })
        .join(" ");

    let p = params.position;
    let offset = inferred_offset.unwrap_or(params.completion.offset);
    let command = format!(
        "set window lsp_completions {}.{}@{} {}\n",
        p.line, offset, meta.version, items
    );

    ctx.exec(meta, command);
}
