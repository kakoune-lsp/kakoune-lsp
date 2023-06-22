use std::collections::HashMap;

use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::{request::*, *};
use serde::Deserialize;

pub fn call_hierarchy_prepare(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = CallHierarchyParams::deserialize(params)
        .expect("Params should follow CallHierarchyParams structure");
    let req_params = ctx
        .language_servers
        .iter()
        .map(|(server_name, server_settings)| {
            let position =
                get_lsp_position(server_settings, &meta.buffile, &params.position, ctx).unwrap();
            let uri = Url::from_file_path(&meta.buffile).unwrap();
            (
                server_name.clone(),
                vec![CallHierarchyPrepareParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier::new(uri),
                        position,
                    },
                    work_done_progress_params: WorkDoneProgressParams::default(),
                }],
            )
        })
        .collect();

    ctx.call::<CallHierarchyPrepare, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            request_call_hierarchy(meta, ctx, params.incoming_or_outgoing, results);
        },
    )
}

fn request_call_hierarchy(
    meta: EditorMeta,
    ctx: &mut Context,
    incoming_or_outgoing: bool,
    results: Vec<(ServerName, Option<Vec<CallHierarchyItem>>)>,
) {
    let result = results
        .into_iter()
        .find(|(_, response)| response.is_some())
        .and_then(|(server_name, item)| item.map(|item| (server_name, item)));

    // TODO Can we get multiple items here?
    let (server_name, item) =
        match result.and_then(|(server_name, r)| r.into_iter().next().map(|v| (server_name, v))) {
            Some(item) => item,
            None => return,
        };

    if incoming_or_outgoing {
        let mut params = HashMap::new();
        params.insert(
            server_name,
            vec![CallHierarchyIncomingCallsParams {
                item: item.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            }],
        );

        ctx.call::<CallHierarchyIncomingCalls, _>(
            meta,
            RequestParams::Each(params),
            move |ctx: &mut Context, meta, results| {
                if let Some(result) = results.first() {
                    format_call_hierarchy_calls(meta, ctx, incoming_or_outgoing, &item, result);
                }
            },
        );
    } else {
        let mut params = HashMap::new();
        params.insert(
            server_name,
            vec![CallHierarchyOutgoingCallsParams {
                item: item.clone(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            }],
        );

        ctx.call::<CallHierarchyOutgoingCalls, _>(
            meta,
            RequestParams::Each(params),
            move |ctx: &mut Context, meta, results| {
                if let Some(result) = results.first() {
                    format_call_hierarchy_calls(meta, ctx, incoming_or_outgoing, &item, result);
                }
            },
        );
    }
}

fn format_location(
    server_name: &ServerName,
    meta: &EditorMeta,
    ctx: &mut Context,
    uri: &Url,
    position: Position,
    prefix: &str,
    suffix: &str,
) -> String {
    let server = &ctx.language_servers[server_name];
    let filename = uri.to_file_path().unwrap();
    let filename = short_file_path(filename.to_str().unwrap(), &server.root_path);
    let position = get_kakoune_position_with_fallback(server, &meta.buffile, position, ctx);
    format!(
        "{}{}:{}:{}: {}\n",
        prefix, filename, position.line, position.column, suffix,
    )
}

trait CallHierarchyCall<'a> {
    fn caller_or_callee(&self) -> &CallHierarchyItem;
    fn caller(&'a self, other: &'a CallHierarchyItem) -> &'a CallHierarchyItem;
    fn callsites(&self) -> &Vec<Range>;
}

impl<'a> CallHierarchyCall<'a> for CallHierarchyIncomingCall {
    fn caller_or_callee(&self) -> &CallHierarchyItem {
        &self.from
    }
    fn caller(&'a self, _callee: &'a CallHierarchyItem) -> &'a CallHierarchyItem {
        &self.from
    }
    fn callsites(&self) -> &Vec<Range> {
        &self.from_ranges
    }
}

impl<'a> CallHierarchyCall<'a> for CallHierarchyOutgoingCall {
    fn caller_or_callee(&self) -> &CallHierarchyItem {
        &self.to
    }
    fn caller(&'a self, caller: &'a CallHierarchyItem) -> &'a CallHierarchyItem {
        caller
    }
    fn callsites(&self) -> &Vec<Range> {
        &self.from_ranges
    }
}

fn format_call_hierarchy_calls<'a>(
    meta: EditorMeta,
    ctx: &mut Context,
    incoming_or_outgoing: bool,
    item: &'a CallHierarchyItem,
    result: &'a (ServerName, Option<Vec<impl CallHierarchyCall<'a>>>),
) {
    let (server_name, result) = result;
    let result = match result {
        Some(result) => result,
        None => return,
    };

    let first_line_suffix = format!(
        "{} - list of {}",
        &item.name,
        if incoming_or_outgoing {
            "callers"
        } else {
            "callees"
        },
    );

    let contents = format_location(
        server_name,
        &meta,
        ctx,
        &item.uri,
        item.range.start,
        "",
        &first_line_suffix,
    ) + &result
        .iter()
        .map(|call| {
            let caller = call.caller(item);
            let callsite_filename = caller.uri.to_file_path().unwrap();
            let caller_or_calle = call.caller_or_callee();

            format_location(
                server_name,
                &meta,
                ctx,
                &caller_or_calle.uri,
                caller_or_calle.range.start,
                "  ",
                &caller_or_calle.name,
            ) + &call
                .callsites()
                .iter()
                .map(|range| {
                    let line = get_file_contents(callsite_filename.to_str().unwrap(), ctx)
                        .map(|text| text.line(range.start.line as usize).to_string())
                        .unwrap_or_default();
                    let line = line
                        .strip_suffix("\r\n")
                        .or_else(|| line.strip_suffix('\n'))
                        .unwrap_or(&line);
                    format_location(
                        server_name,
                        &meta,
                        ctx,
                        &caller.uri,
                        range.start,
                        "    ",
                        line,
                    )
                })
                .join("")
        })
        .join("");

    let command = if incoming_or_outgoing {
        "lsp-show-incoming-calls"
    } else {
        "lsp-show-outgoing-calls"
    };
    let server = &ctx.language_servers[server_name];
    let command = format!(
        "{} {} {}",
        command,
        editor_quote(&server.root_path),
        editor_quote(&contents),
    );
    ctx.exec(meta, command);
}
