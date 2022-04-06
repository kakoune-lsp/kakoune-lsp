use crate::context::*;
use crate::markup::*;
use crate::position::*;
use crate::text_edit::apply_text_edits;
use crate::types::*;
use crate::util::*;
use indoc::formatdoc;
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
    let items = match result {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    };

    ctx.completion_items = items;
    let items = &ctx.completion_items;
    if ctx.completion_last_client != meta.client {
        ctx.completion_last_client = meta.client.clone();
    }

    if items.is_empty() {
        return;
    }

    // Length of the longest label in the current completion list
    let maxlen = items.iter().map(|x| x.label.len()).max().unwrap_or(0);
    let snippet_prefix_re = Regex::new(r"^[^\[\(<\n\$]+").unwrap();

    let mut inferred_offset: Option<u32> = None;
    let mut can_infer_offset = true;

    let items = items
        .iter()
        .enumerate()
        .map(|(completion_item_index, x)| {
            let doc = x.documentation.as_ref().map(|doc| match doc {
                Documentation::String(s) => s,
                Documentation::MarkupContent(content) => &content.value,
            });

            // Combine the 'detail' line and the full-text documentation into
            // a single string. If both exist, separate them with a horizontal rule.
            let markdown = {
                let mut markdown = String::new();

                if let Some(detail) = x.detail.as_ref() {
                    markdown.push_str(detail);

                    if doc.is_some() {
                        markdown.push_str("\n\n---\n\n");
                    }
                }

                if let Some(doc) = doc {
                    markdown.push_str(doc);
                }

                markdown
            };

            let maybe_set_index = if ctx
                .capabilities
                .as_ref()
                .and_then(|caps| caps.completion_provider.as_ref())
                .and_then(|compl| compl.resolve_provider)
                .unwrap_or(false)
            {
                format!(
                    "set-option window lsp_completions_selected_item {}; ",
                    completion_item_index
                )
            } else {
                "".to_string()
            };
            let on_select = if !markdown.is_empty() {
                let markup = markdown_to_kakoune_markup(markdown);
                format!(
                    "{}info -markup -style menu -- %§{}§",
                    &maybe_set_index,
                    markup.replace('§', "§§")
                )
            } else {
                // When the user scrolls through the list of completion candidates, Kakoune
                // does not clean up the info box. We need to do that explicitly, in this case by
                // requesting an empty one.
                maybe_set_index + "info -style menu ''"
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

            let maybe_filter_text = if !params.have_kakoune_feature_filtertext {
                None
            } else {
                let specified_filter_text = x.filter_text.as_ref().unwrap_or(&x.label);
                let specified_insert_text = x
                    .text_edit
                    .as_ref()
                    .map(|cte| match cte {
                        CompletionTextEdit::Edit(text_edit) => &text_edit.new_text,
                        CompletionTextEdit::InsertAndReplace(text_edit) => &text_edit.new_text,
                    })
                    .or(x.insert_text.as_ref())
                    .unwrap_or(&x.label);
                if specified_filter_text == specified_insert_text {
                    None
                } else {
                    Some(specified_filter_text.clone())
                }
            };

            let insert_text = x.text_edit.as_ref().and_then(|cte| {
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
                            Some(text_edit.new_text.clone())
                        } else {
                            None
                        }
                    }
                    CompletionTextEdit::InsertAndReplace(_) => {
                        can_infer_offset = false;
                        None
                    }
                }
            });
            let insert_text = insert_text
                .or_else(|| x.insert_text.clone())
                .unwrap_or_else(|| x.label.clone());

            fn completion_entry(
                insert_text: &str,
                maybe_filter_text: &Option<String>,
                on_select: &str,
                menu: &str,
            ) -> String {
                if let Some(filter_text) = maybe_filter_text {
                    editor_quote(&format!(
                        "{}|{}|{}|{}",
                        escape_tuple_element(insert_text),
                        escape_tuple_element(filter_text),
                        escape_tuple_element(on_select),
                        escape_tuple_element(menu),
                    ))
                } else {
                    editor_quote(&format!(
                        "{}|{}|{}",
                        escape_tuple_element(insert_text),
                        escape_tuple_element(on_select),
                        escape_tuple_element(menu),
                    ))
                }
            }

            // If snippet support is both enabled and provided by the server,
            // we'll need to perform some transformations on the completion commands.
            if ctx.config.snippet_support && x.insert_text_format == Some(InsertTextFormat::SNIPPET)
            {
                let snippet = insert_text;
                let insert_text = snippet_prefix_re
                    .find(&snippet)
                    .map(|x| x.as_str())
                    .unwrap_or(&snippet);

                let command = formatdoc!(
                    "{}
                     lsp-snippets-insert-completion {} {}",
                    on_select,
                    editor_quote(&regex::escape(insert_text)),
                    editor_quote(&snippet)
                );

                completion_entry(insert_text, &maybe_filter_text, &command, &entry)
            } else {
                completion_entry(&insert_text, &maybe_filter_text, &on_select, &entry)
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

pub fn completion_item_resolve(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let CompletionItemResolveParams {
        completion_item_index,
    } = CompletionItemResolveParams::deserialize(params).unwrap();

    if ctx.completion_last_client.is_none() || meta.client != ctx.completion_last_client {
        return;
    }

    // Since we're the only user of the completion items, we can clear them.
    let item = ctx
        .completion_items
        .drain(..)
        .nth(completion_item_index as usize)
        .unwrap();

    match item.additional_text_edits {
        Some(edits) if !edits.is_empty() => {
            // Not sure if this case ever happens, the spec is unclear.
            let uri = Url::from_file_path(&meta.buffile).unwrap();
            apply_text_edits(&meta, &uri, edits, ctx);
            return;
        }
        _ => (),
    }

    ctx.call::<ResolveCompletionItem, _>(meta, item, |tx: &mut Context, meta, new_item| {
        editor_completion_item_resolve(tx, meta, new_item)
    });
}

fn editor_completion_item_resolve(ctx: &mut Context, meta: EditorMeta, item: CompletionItem) {
    if let Some(resolved_edits) = item.additional_text_edits {
        let uri = Url::from_file_path(&meta.buffile).unwrap();
        apply_text_edits(&meta, &uri, resolved_edits, ctx)
    }
}
