use std::{borrow::Cow, collections::HashMap};

use itertools::Itertools;
use lsp_types::{
    request::InlayHintRequest, InlayHint, InlayHintLabel, InlayHintParams, Position, Range,
    TextDocumentIdentifier, TextEdit, Url,
};

use crate::{
    capabilities::{attempt_server_capability, CAPABILITY_INLAY_HINTS},
    context::{Context, RequestParams},
    markup::escape_kakoune_markup,
    position::{
        kakoune_range_to_lsp, lsp_position_to_kakoune, parse_kakoune_range, ranges_overlap,
    },
    text_edit::apply_text_edits,
    types::{EditorMeta, ServerId},
    util::{editor_quote, escape_tuple_element},
};

#[derive(Debug, PartialEq, Clone, Default)]
pub struct InlayHintsOptions {
    pub buf_line_count: u32,
}

pub fn inlay_hints(meta: EditorMeta, params: InlayHintsOptions, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_INLAY_HINTS))
        .collect();
    if eligible_servers.is_empty() {
        return;
    }

    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, _)| {
            (
                server_id,
                vec![InlayHintParams {
                    work_done_progress_params: Default::default(),
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    range: Range::new(Position::new(0, 0), Position::new(params.buf_line_count, 0)),
                }],
            )
        })
        .collect();
    ctx.call::<InlayHintRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            let results = results
                .into_iter()
                .flat_map(|(server_id, v)| {
                    let v: Vec<_> = v
                        .unwrap_or_default()
                        .into_iter()
                        .map(|v| (server_id, v))
                        .collect();
                    v
                })
                .collect();
            inlay_hints_response(meta, results, ctx)
        },
    );
}

pub fn inlay_hints_response(
    meta: EditorMeta,
    inlay_hints: Vec<(ServerId, InlayHint)>,
    ctx: &mut Context,
) {
    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => return,
    };

    let ranges = inlay_hints
        .iter()
        .map(
            |(
                server_id,
                InlayHint {
                    position,
                    label,
                    padding_left,
                    padding_right,
                    ..
                },
            )| {
                let server = ctx.server(*server_id);
                let position =
                    lsp_position_to_kakoune(position, &document.text, server.offset_encoding);
                let label = match label {
                    InlayHintLabel::String(s) => Cow::Borrowed(s),
                    InlayHintLabel::LabelParts(parts) => {
                        Cow::Owned(parts.iter().map(|x| x.value.as_str()).collect())
                    }
                };
                let padding_left = if padding_left.unwrap_or(false) {
                    " "
                } else {
                    ""
                };
                let padding_right = if padding_right.unwrap_or(false) {
                    "{Default} "
                } else {
                    ""
                };
                let label = escape_tuple_element(&escape_kakoune_markup(&label));
                editor_quote(&format!(
                    "{position}+0|{padding_left}{{InlayHint}}{label}{padding_right}",
                ))
            },
        )
        .join(" ");

    ctx.inlay_hints.insert(meta.buffile.clone(), inlay_hints);

    let version = meta.version;
    let command = format!("set-option buffer lsp_inlay_hints {version} {ranges}");
    let command = format!(
        "evaluate-commands -buffer {} -verbatim -- {}",
        editor_quote(&meta.buffile),
        &command
    );
    ctx.exec(meta, command)
}

#[derive(Debug)]
pub enum InlayHintApplyKind {
    /// Select the closest hint on the same line as the cursors
    Nearest,
    /// Select ANY hints whose position overlaps with the selections
    Selected,
}

#[derive(Debug)]
pub struct InlayHintApplyParams {
    pub selections_desc: Vec<String>,
    pub kind: InlayHintApplyKind,
}

/// This function applies TextEdits contained in inlay hints, where the user's selections_desc
/// describe which inlay hints we'll apply.
/// The heuristic we use to pick the inlay hints to apply depends on the InlayHintApplyKind
/// specified in the Params.
pub fn inlay_hint_apply(meta: EditorMeta, params: InlayHintApplyParams, ctx: &mut Context) {
    // we operate per selection/cursor
    for selection_desc in &params.selections_desc {
        let (kak_range, cursor) = parse_kakoune_range(selection_desc);

        let edits_by_server: HashMap<usize, Vec<TextEdit>> = {
            let Some(document) = ctx.documents.get(&meta.buffile) else {
                return;
            };

            // all hints for this buffile, with their server_id and position
            // the kak position is later used by the Nearest apply kind
            let Some(all_hints) = ctx.inlay_hints.get(&meta.buffile) else {
                return;
            };
            let all_hints = all_hints.iter().map(|(server_id, hint)| {
                let server = ctx.server(*server_id);
                let kak_pos =
                    lsp_position_to_kakoune(&hint.position, &document.text, server.offset_encoding);
                (*server_id, hint, kak_pos)
            });

            // an iterator of the inlay hints we select to apply
            let hints_to_apply: Box<dyn Iterator<Item = (ServerId, &InlayHint)> + '_> =
                // the filtering/selection logic depends on which kind of apply the user wants
                match &params.kind {
                    // nearest: closest hint on same line
                    InlayHintApplyKind::Nearest => {
                        let it = all_hints
                            // only consider same line as cursor
                            .filter(|(_, _, pos)| pos.line == cursor.line)
                            // pick hint with minimum distance from cursor
                            .min_by_key(|(_, _, pos)| pos.column.abs_diff(cursor.column) as u64)
                            .map(|(server_id, hint, _)| (server_id, hint))
                            .into_iter();
                        Box::new(it)
                    }
                    // selected: ALL hints whose position is inside the selection range
                    InlayHintApplyKind::Selected => {
                        let it = all_hints
                            .filter(|(server_id, hint, _)| {
                                let server = ctx.server(*server_id);
                                ranges_overlap(
                                    // ranges_overlap needs LSP Ranges, so we must convert
                                    // this kak range to the LSP range format
                                    kakoune_range_to_lsp(
                                        &kak_range,
                                        &document.text,
                                        server.offset_encoding,
                                    ),
                                    // hints only have a single position, but we can cheat by
                                    // building a zero-sized range where start == end
                                    Range {
                                        start: hint.position,
                                        end: hint.position,
                                    },
                                )
                            })
                            .map(|(server_id, hint, _)| (server_id, hint));
                        Box::new(it)
                    }
                };

            // because each hint comes with its own server_id, we respect that and build a map.
            // even though it's most likely the server_ids will all be the same, this is more correct
            let mut edits_by_server: HashMap<usize, Vec<TextEdit>> = HashMap::new();

            for (server_id, hint) in hints_to_apply {
                if let Some(edits) = hint.text_edits.as_ref().filter(|e| !e.is_empty()) {
                    edits_by_server
                        .entry(server_id)
                        .or_default()
                        .extend_from_slice(edits);
                }
            }
            edits_by_server
        };

        if edits_by_server.is_empty() {
            ctx.show_error(meta.clone(), "no textedits available to apply");
            continue;
        }

        let uri = Url::from_file_path(&meta.buffile).unwrap();

        // FIXME: https://github.com/kakoune-lsp/kakoune-lsp/issues/873
        // we expect this to break if multiple servers provide InlayHints with textedits
        // for the same buffile, because the servers won't know about each other.
        // So if server A inserts a line at line N, that messes up server B's edits at line >N.
        //
        // Solution could be to combine all edits from all servers into a Vec,
        // and sort them by start (or end?) position.
        // If the ranges don't overlap, should be correct.
        for (server_id, edits) in edits_by_server {
            apply_text_edits(server_id, meta.clone(), uri.clone(), edits, ctx);
        }
    }
}
