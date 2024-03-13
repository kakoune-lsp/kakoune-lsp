use std::collections::HashMap;

use crate::capabilities::{attempt_server_capability, CAPABILITY_FORMATTING};
use crate::context::*;
use crate::types::*;
use crate::util::editor_quote;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_formatting(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .language_servers
        .iter()
        .filter(|server| attempt_server_capability(*server, &meta, CAPABILITY_FORMATTING))
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
                    "lsp-formatting-sync"
                } else {
                    "lsp-formatting"
                };
                let cmd = format!("{} {}", cmd, server_name);
                format!("{} {}", editor_quote(server_name), editor_quote(&cmd))
            })
            .join(" ");
        ctx.exec(meta, format!("lsp-menu {}", choices));
        return;
    }

    let params = FormattingOptions::deserialize(params)
        .expect("Params should follow FormattingOptions structure");

    let (server_name, _) = eligible_servers[0];
    let mut req_params = HashMap::new();
    req_params.insert(
        server_name.clone(),
        vec![DocumentFormattingParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            options: params.clone(),
            work_done_progress_params: Default::default(),
        }],
    );

    let server_name = server_name.clone();
    ctx.call::<Formatting, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, mut results| {
            let text_edits = results
                .first_mut()
                .and_then(|(_, v)| v.take())
                .unwrap_or_default();
            super::range_formatting::editor_range_formatting(meta, (server_name, text_edits), ctx)
        },
    );
}
