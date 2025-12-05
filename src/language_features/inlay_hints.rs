use itertools::Itertools;
use lsp_types::{
    request::InlayHintRequest, InlayHint, InlayHintLabel, InlayHintParams, Position, Range,
    TextDocumentIdentifier, Url,
};

use crate::{
    capabilities::{attempt_server_capability, CAPABILITY_INLAY_HINTS},
    context::{Context, RequestParams},
    markup::escape_kakoune_markup,
    position::lsp_position_to_kakoune,
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
        .into_iter()
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
                let server = ctx.server(server_id);
                let position =
                    lsp_position_to_kakoune(&position, &document.text, server.offset_encoding);
                let label = match label {
                    InlayHintLabel::String(s) => s,
                    InlayHintLabel::LabelParts(parts) => {
                        parts.iter().map(|x| x.value.as_str()).collect()
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
    let version = meta.version;
    let command = format!("set-option buffer lsp_inlay_hints {version} {ranges}");
    let command = format!(
        "evaluate-commands -buffer {} -verbatim -- {}",
        editor_quote(&meta.buffile),
        &command
    );
    ctx.exec(meta, command)
}
