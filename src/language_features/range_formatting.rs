use std::collections::HashMap;

use crate::capabilities::{attempt_server_capability, CAPABILITY_RANGE_FORMATTING};
use crate::context::*;
use crate::controller::can_serve;
use crate::position::{kakoune_range_to_lsp, parse_kakoune_range};
use crate::text_edit::{apply_text_edits_to_buffer, TextEditish};
use crate::types::*;
use crate::util::editor_quote;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use url::Url;

pub fn text_document_range_formatting(
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    params: RangeFormattingParams,
    ctx: &mut Context,
) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|server| {
            attempt_server_capability(ctx, *server, &meta, CAPABILITY_RANGE_FORMATTING)
        })
        .filter(|(server_id, _)| {
            meta.server
                .as_ref()
                .map(|fmt_server| {
                    can_serve(
                        ctx,
                        *server_id,
                        fmt_server,
                        &ctx.server_config(&meta, fmt_server).unwrap().root,
                    )
                })
                .unwrap_or(true)
        })
        .collect();
    if eligible_servers.is_empty() {
        return;
    }

    // Ask user to pick which server to use for formatting when multiple options are available.
    if eligible_servers.len() > 1 {
        let choices = eligible_servers
            .into_iter()
            .map(|(_server_id, server)| {
                let cmd = if response_fifo.is_some() {
                    "lsp-range-formatting-sync"
                } else {
                    "lsp-range-formatting"
                };
                let cmd = format!("{} {}", cmd, server.name);
                format!("{} {}", editor_quote(&server.name), editor_quote(&cmd))
            })
            .join(" ");
        ctx.exec_fifo(meta, response_fifo, format!("lsp-menu {}", choices));
        return;
    }

    let Some(document) = ctx.documents.get(&meta.buffile) else {
        warn!(
            ctx.to_editor(),
            "No document in context for file: {}", &meta.buffile
        );
        return;
    };

    let (server_id, server) = eligible_servers[0];
    let mut req_params = HashMap::new();
    req_params.insert(
        server_id,
        params
            .ranges
            .iter()
            .map(|s| {
                let (range, _cursor) = parse_kakoune_range(s);
                kakoune_range_to_lsp(&range, &document.text, server.offset_encoding)
            })
            .map(|range| DocumentRangeFormattingParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path(&meta.buffile).unwrap(),
                },
                range,
                options: params.formatting_options.clone(),
                work_done_progress_params: Default::default(),
            })
            .collect(),
    );

    ctx.call::<RangeFormatting, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            let text_edits = results
                .into_iter()
                .filter_map(|(_, v)| v)
                .flatten()
                .collect::<Vec<_>>();
            editor_range_formatting(meta, response_fifo, (server_id, text_edits), ctx)
        },
    );
}

pub fn editor_range_formatting<T: TextEditish<T>>(
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    result: (ServerId, Vec<T>),
    ctx: &mut Context,
) {
    let (server_id, text_edits) = result;
    let server = ctx.server(server_id);
    let Some(cmd) = ctx.documents.get(&meta.buffile).and_then(|document| {
        apply_text_edits_to_buffer(
            ctx.to_editor(),
            &meta.client,
            None,
            text_edits,
            &document.text,
            server.offset_encoding,
            false,
        )
    }) else {
        return;
    };
    ctx.exec_fifo(meta, response_fifo, cmd);
}
