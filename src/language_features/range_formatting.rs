use std::collections::HashMap;

use crate::capabilities::{attempt_server_capability, CAPABILITY_RANGE_FORMATTING};
use crate::context::*;
use crate::text_edit::{apply_text_edits_to_buffer, TextEditish};
use crate::types::*;
use crate::util::editor_quote;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_range_formatting(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .language_servers
        .iter()
        .filter(|server| attempt_server_capability(*server, &meta, CAPABILITY_RANGE_FORMATTING))
        .filter(|(server_name, _)| {
            if let Some(fmt_server) = &meta.server {
                *server_name == fmt_server
            } else {
                true
            }
        })
        .collect();
    if eligible_servers.is_empty() {
        if meta.fifo.is_some() {
            ctx.exec(meta, "nop");
        }
        return;
    }

    // Ask user to pick which server to use for formatting when multiple options are available.
    if eligible_servers.len() > 1 {
        let choices = eligible_servers
            .into_iter()
            .map(|(server_name, _)| {
                let cmd = if meta.fifo.is_some() {
                    "lsp-range-formatting-sync"
                } else {
                    "lsp-range-formatting"
                };
                let cmd = format!("{} {}", cmd, server_name);
                format!("{} {}", editor_quote(server_name), editor_quote(&cmd))
            })
            .join(" ");
        ctx.exec(meta, format!("lsp-menu {}", choices));
        return;
    }

    let params = RangeFormattingParams::deserialize(params)
        .expect("Params should follow RangeFormattingParams structure");

    let (server_name, _) = eligible_servers[0];
    let mut req_params = HashMap::new();
    req_params.insert(
        server_name.clone(),
        params
            .ranges
            .iter()
            .map(|range| DocumentRangeFormattingParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path(&meta.buffile).unwrap(),
                },
                range: *range,
                options: params.formatting_options.clone(),
                work_done_progress_params: Default::default(),
            })
            .collect(),
    );

    let server_name = server_name.clone();
    ctx.call::<RangeFormatting, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            let text_edits = results
                .into_iter()
                .filter_map(|(_, v)| v)
                .flatten()
                .collect::<Vec<_>>();
            editor_range_formatting(meta, (server_name, text_edits), ctx)
        },
    );
}

pub fn editor_range_formatting<T: TextEditish<T>>(
    meta: EditorMeta,
    result: (ServerName, Vec<T>),
    ctx: &mut Context,
) {
    let (server_name, text_edits) = result;
    let server = &ctx.language_servers[&server_name];
    let cmd = ctx.documents.get(&meta.buffile).and_then(|document| {
        apply_text_edits_to_buffer(
            &meta.client,
            None,
            text_edits,
            &document.text,
            server.offset_encoding,
            false,
        )
    });
    match cmd {
        Some(cmd) => ctx.exec(meta, cmd),
        // Nothing to do, but sending command back to the editor is required to handle case when
        // editor is blocked waiting for response via fifo.
        None => ctx.exec(meta, "nop"),
    }
}
