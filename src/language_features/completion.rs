use crate::capabilities::attempt_server_capability;
use crate::capabilities::CAPABILITY_COMPLETION;
use crate::context::*;
use crate::markup::*;
use crate::position::*;
use crate::text_edit::apply_text_edits;
use crate::types::*;
use crate::util::*;
use indoc::formatdoc;
use itertools::Itertools;
use lazy_static::lazy_static;
use lsp_types::request::*;
use lsp_types::*;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::TryInto;
use unicode_width::UnicodeWidthStr;
use url::Url;

pub fn text_document_completion(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .language_servers
        .iter()
        .filter(|srv| attempt_server_capability(*srv, &meta, CAPABILITY_COMPLETION))
        .collect();

    let params = TextDocumentCompletionParams::deserialize(params).unwrap();
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_name, server_settings)| {
            (
                server_name.clone(),
                vec![CompletionParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier {
                            uri: Url::from_file_path(&meta.buffile).unwrap(),
                        },
                        position: get_lsp_position(
                            server_settings,
                            &meta.buffile,
                            &params.position,
                            ctx,
                        )
                        .unwrap(),
                    },
                    context: None,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<Completion, _>(
        meta,
        RequestParams::Each(req_params),
        |ctx: &mut Context, meta, results| editor_completion(meta, params, results, ctx),
    );
}

fn sort_text(item: &CompletionItem) -> &str {
    item.sort_text.as_ref().unwrap_or(&item.label)
}

fn editor_completion(
    meta: EditorMeta,
    params: TextDocumentCompletionParams,
    results: Vec<(String, Option<CompletionResponse>)>,
    ctx: &mut Context,
) {
    let mut items: Vec<(String, CompletionItem)> = results
        .into_iter()
        .flat_map(|(server_name, items)| {
            let items = match items {
                Some(CompletionResponse::Array(items)) => items,
                Some(CompletionResponse::List(list)) => list.items,
                None => vec![],
            };

            items.into_iter().map(move |v| (server_name.clone(), v))
        })
        .collect();

    // TODO Group by server?
    items.sort_by(|(_left_server, left), (_right_server, right)| {
        sort_text(left).cmp(sort_text(right))
    });

    let version = meta.version;
    ctx.completion_items = items;
    ctx.completion_items_timestamp = version;
    let items = &ctx.completion_items;
    if ctx.completion_last_client != meta.client {
        ctx.completion_last_client = meta.client.clone();
    }

    if items.is_empty() {
        return;
    }

    // Maximum display width of any completion label.
    let maxwidth = items
        .iter()
        .map(|(_, x)| UnicodeWidthStr::width(x.label.as_str()))
        .max()
        .unwrap_or(0);

    let mut inferred_offset: Option<u32> = None;
    let mut can_infer_offset = true;

    let items = items
        .iter()
        .enumerate()
        .map(|(completion_item_index, (server_name, x))| {
            let server = &ctx.language_servers[server_name];
            let maybe_resolve = if server
                .capabilities
                .as_ref()
                .and_then(|caps| caps.completion_provider.as_ref())
                .and_then(|compl| compl.resolve_provider)
                .unwrap_or(false)
            {
                "lsp-completion-item-resolve\n"
            } else if x
                .additional_text_edits
                .as_ref()
                .is_some_and(|edits| !edits.is_empty())
            {
                "lsp-completion-on-accept %{ lsp-completion-item-resolve-request false }\n"
            } else {
                ""
            };
            let on_select = formatdoc!(
                "lsp-completion-item-selected {completion_item_index}
                 {maybe_resolve}info -markup -style menu -- %§{}§",
                completion_menu_text(x).replace('§', "§§")
            );

            let entry = match x.kind {
                Some(k) => format!(
                    "{}{} {{MenuInfo}}{:?}",
                    escape_kakoune_markup(&x.label),
                    " ".repeat(maxwidth - UnicodeWidthStr::width(x.label.as_str())),
                    k
                ),
                None => escape_kakoune_markup(&x.label),
            };

            let is_simple_text_edit = x.text_edit.as_ref().is_some_and(|cte| {
                let document = match ctx.documents.get(&meta.buffile) {
                    Some(doc) => doc,
                    None => {
                        warn!("No document in context for file: {}", &meta.buffile);
                        return false;
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
                            server.offset_encoding,
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
                        range.start.line == params.position.line
                            && range.end.line == params.position.line
                    }
                    CompletionTextEdit::InsertAndReplace(_) => false,
                }
            });
            if !is_simple_text_edit {
                can_infer_offset = false;
                inferred_offset = None;
            }
            let specified_insert_text = x.insert_text.as_ref().unwrap_or(&x.label);
            let eventual_insert_text = x
                .text_edit
                .as_ref()
                .map(|cte| match cte {
                    CompletionTextEdit::Edit(text_edit) => &text_edit.new_text,
                    CompletionTextEdit::InsertAndReplace(text_edit) => &text_edit.new_text,
                })
                .unwrap_or(specified_insert_text);

            fn completion_entry(insert_text: &str, on_select: &str, menu: &str) -> String {
                editor_quote(&format!(
                    "{}|{}|{}",
                    escape_tuple_element(insert_text),
                    escape_tuple_element(on_select),
                    escape_tuple_element(menu),
                ))
            }

            // If snippet support is both enabled and provided by the server,
            // we'll need to perform some transformations on the completion commands.
            if ctx.config.snippet_support && x.insert_text_format == Some(InsertTextFormat::SNIPPET)
            {
                lazy_static! {
                    static ref SNIPPET_TABSTOP_RE: Regex = Regex::new(r"\$(?P<i>\d+)").unwrap();
                    // {
                    static ref SNIPPET_PLACEHOLDER_RE: Regex =
                        Regex::new(r"\$\{(?P<i>\d+):?(?P<placeholder>[^}]+)\}").unwrap();
                        // {
                    static ref SNIPPET_ESCAPED_METACHARACTERS_RE: Regex =
                        Regex::new(r"\\([$}\\,|])").unwrap();
                }
                let mut snippet = eventual_insert_text.to_string();
                if !snippet.contains("$0") && !snippet.contains("${0") {
                    snippet += "$0";
                }
                let insert_text = specified_insert_text;
                let insert_text = SNIPPET_TABSTOP_RE.replace_all(insert_text, "");
                let insert_text = SNIPPET_PLACEHOLDER_RE.replace_all(&insert_text, "$placeholder");
                // Unescape metacharacters.
                let insert_text = SNIPPET_ESCAPED_METACHARACTERS_RE.replace_all(&insert_text, "$1");
                // There's some issue with multiline insert texts, and they also don't work well in the UI, so display on one line
                let insert_text = insert_text.replace('\n', "");

                let on_select = formatdoc!(
                    "{on_select}
                     lsp-snippets-insert-completion {}",
                    editor_quote(&snippet)
                );

                completion_entry(&insert_text, &on_select, &entry)
            } else {
                // Due to implementation reasons, we currently do not support filter text
                // with snippets.
                let specified_filter_text = x.filter_text.as_ref().unwrap_or(&x.label);
                let (insert_text, on_select) = if specified_filter_text != eventual_insert_text {
                    // Simulate filter-text support by giving the filter-text to Kakoune
                    // but expand to the insert-text when the completion is accepted.
                    let on_select = formatdoc!(
                        "{on_select}
                         lsp-snippets-insert-completion {}",
                        editor_quote(&(eventual_insert_text.to_string() + "$0"))
                    );
                    (specified_filter_text, on_select)
                } else {
                    (eventual_insert_text, on_select)
                };
                completion_entry(insert_text, &on_select, &entry)
            }
        })
        .join(" ");

    let line = params.position.line;
    let offset = inferred_offset.unwrap_or(params.completion.offset);
    let command = formatdoc!(
        "set-option window lsp_completions {line}.{offset}@{version} {items}
         set-option window lsp_completions_timestamp {version}"
    );
    let command = format!("evaluate-commands -- {}", editor_quote(&command));

    ctx.exec(meta, command);
}

fn completion_menu_text(x: &CompletionItem) -> String {
    // Combine the 'detail' line and the full-text documentation into
    // a single string. If both exist, separate them with a horizontal rule.
    let mut markup = String::new();

    if let Some(detail) = x.detail.as_ref() {
        markup.push_str(&escape_kakoune_markup(detail));

        if x.documentation.is_some() {
            markup.push_str("\n\n---\n\n");
        }
    }

    match x.documentation.as_ref() {
        Some(Documentation::String(s)) => markup.push_str(&escape_kakoune_markup(s)),
        Some(Documentation::MarkupContent(content)) => match content.kind {
            MarkupKind::PlainText => markup.push_str(&escape_kakoune_markup(&content.value)),
            MarkupKind::Markdown => markup.push_str(&markdown_to_kakoune_markup(&content.value)),
        },
        _ => (),
    }

    markup
}

pub fn completion_item_resolve(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let CompletionItemResolveParams {
        completion_item_timestamp,
        completion_item_index,
        pager_active,
    } = CompletionItemResolveParams::deserialize(params).unwrap();

    if ctx.completion_last_client.is_none() || meta.client != ctx.completion_last_client {
        return;
    }

    if completion_item_timestamp != ctx.completion_items_timestamp {
        return;
    }

    if completion_item_index >= ctx.completion_items.len().try_into().unwrap() {
        error!(
            "ignoring request to resolve completion item of invalid index {completion_item_index}"
        );
        return;
    }

    let (server_name, item, detail, documentation) = if pager_active {
        let (server_name, item) = &ctx.completion_items[completion_item_index as usize];
        // Stop if there is nothing interesting to resolve.
        if item.detail.is_some() && item.documentation.is_some() {
            return;
        }
        (
            server_name.clone(),
            item.clone(),
            item.detail.clone(),
            item.documentation.clone(),
        )
    } else {
        // Since we're the only user of the completion items, we can clear them.
        let (server_name, item) = ctx
            .completion_items
            .drain(..)
            .nth(completion_item_index as usize)
            .unwrap();

        match item.additional_text_edits {
            Some(edits) if !edits.is_empty() => {
                // Not sure if this case ever happens, the spec is unclear.
                let uri = Url::from_file_path(&meta.buffile).unwrap();
                apply_text_edits(&server_name, &meta, uri, edits, ctx);
                return;
            }
            _ => (),
        }

        (server_name, item, None, None)
    };

    let mut req_params = HashMap::new();
    req_params.insert(server_name, vec![item]);

    ctx.call::<ResolveCompletionItem, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            if let Some((server_name, new_item)) = results.into_iter().next() {
                editor_completion_item_resolve(
                    &server_name,
                    ctx,
                    meta,
                    pager_active,
                    detail,
                    documentation,
                    new_item,
                )
            }
        },
    );
}

fn editor_completion_item_resolve(
    server_name: &ServerName,
    ctx: &mut Context,
    meta: EditorMeta,
    pager_active: bool,
    old_detail: Option<String>,
    old_documentation: Option<Documentation>,
    new_item: CompletionItem,
) {
    if pager_active {
        if new_item.detail == old_detail || new_item.documentation == old_documentation {
            return;
        }
        ctx.exec(
            meta,
            format!(
                "info -markup -style menu -- %§{}§",
                completion_menu_text(&new_item).replace('§', "§§")
            ),
        );
    } else if let Some(resolved_edits) = new_item.additional_text_edits {
        let uri = Url::from_file_path(&meta.buffile).unwrap();
        apply_text_edits(server_name, &meta, uri, resolved_edits.clone(), ctx)
    }
}
