use std::collections::HashMap;

use crate::capabilities::{attempt_server_capability, CAPABILITY_FORMATTING};
use crate::context::*;
use crate::controller::can_serve;
use crate::types::*;
use crate::util::editor_quote;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use url::Url;

pub fn text_document_formatting(
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    params: FormattingOptions,
    ctx: &mut Context,
) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|server| attempt_server_capability(*server, &meta, CAPABILITY_FORMATTING))
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
                    "lsp-formatting-sync"
                } else {
                    "lsp-formatting"
                };
                let cmd = format!("{} {}", cmd, server.name);
                format!("{} {}", editor_quote(&server.name), editor_quote(&cmd))
            })
            .join(" ");
        ctx.exec_fifo(meta, response_fifo, format!("lsp-menu {}", choices));
        return;
    }

    let (server_id, _) = eligible_servers[0];
    let mut req_params = HashMap::new();
    req_params.insert(
        server_id,
        vec![DocumentFormattingParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            options: params.clone(),
            work_done_progress_params: Default::default(),
        }],
    );

    ctx.call::<Formatting, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, mut results| {
            let text_edits = results
                .first_mut()
                .and_then(|(_, v)| v.take())
                .unwrap_or_default();
            super::range_formatting::editor_range_formatting(
                meta,
                response_fifo,
                (server_id, text_edits),
                ctx,
            )
        },
    );
}
