use std::borrow::Cow;

use itertools::Itertools;
use lsp_types::{
    request::InlayHintRequest, InlayHint, InlayHintLabel, InlayHintParams, Position, Range,
    TextDocumentIdentifier, Url,
};

use crate::{
    capabilities::{attempt_server_capability, CAPABILITY_INLAY_HINTS},
    context::{Context, RequestParams},
    markup::escape_kakoune_markup,
    position::{lsp_position_to_kakoune, parse_kakoune_range},
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
                    lsp_position_to_kakoune(&position, &document.text, server.offset_encoding);
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
        "evaluate-commands -buffer {} -- {}",
        editor_quote(&meta.buffile),
        editor_quote(&command)
    );
    ctx.exec(meta, command)
}

#[derive(Debug)]
pub struct InlayHintApplyParams {
    pub selection_desc: String,
}

pub fn apply_inlay_hint(meta: EditorMeta, params: InlayHintApplyParams, ctx: &mut Context) {
    let Some(document) = ctx.documents.get(&meta.buffile) else {
        return;
    };

    // for now let's use cursor as the target
    let (_, cursor) = parse_kakoune_range(&params.selection_desc);

    let Some((server_id, hint)) = ctx
        .inlay_hints
        .get(&meta.buffile)
        .unwrap()
        .iter()
        .min_by_key(|(server_id, hint)| {
            let server = ctx.server(*server_id);
            let kak_pos =
                lsp_position_to_kakoune(&hint.position, &document.text, server.offset_encoding);
            // manhattan distance from cursor
            let dl = kak_pos.line.abs_diff(cursor.line) as u64;
            let dc = kak_pos.column.abs_diff(cursor.column) as u64;
            dl.saturating_mul(1000) + dc
        })
    else {
        ctx.show_error(meta, "no inlay hint near cursor");
        return;
    };

    let edits = match hint.text_edits.clone() {
        Some(edits) if !edits.is_empty() => edits,
        _ => {
            ctx.show_error(meta, "this inlay hint doesn't include textEdits");
            return;
        }
    };
    let uri = Url::from_file_path(&meta.buffile).unwrap();
    apply_text_edits(*server_id, meta, uri, edits, ctx);
}
