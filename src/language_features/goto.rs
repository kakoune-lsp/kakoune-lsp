use crate::capabilities::{
    attempt_server_capability, CAPABILITY_DEFINITION, CAPABILITY_IMPLEMENTATION,
    CAPABILITY_REFERENCES, CAPABILITY_TYPE_DEFINITION,
};
use crate::context::{Context, RequestParams};
use crate::position::*;
use crate::types::{EditorMeta, KakouneRange, PositionParams, ServerId};
use crate::util::{editor_quote, short_file_path};
use indoc::formatdoc;
use itertools::Itertools;
use lsp_types::request::{
    GotoDeclaration, GotoDefinition, GotoImplementation, GotoTypeDefinition,
    GotoTypeDefinitionResponse, References, Request,
};
use lsp_types::*;
use url::Url;

pub fn goto(
    meta: EditorMeta,
    results: Vec<(ServerId, Option<GotoDefinitionResponse>)>,
    ctx: &mut Context,
) {
    // HACK: When using multiple language servers, we might get duplicates here. Filter them out.
    let mut seen: Vec<GotoDefinitionResponse> = vec![];
    let locations: Vec<_> = results
        .into_iter()
        .filter_map(|(server_id, v)| match v {
            None => None,
            Some(response) => {
                if seen.iter().any(|r| *r == response) {
                    return None;
                }
                seen.push(response.clone());
                Some((server_id, response))
            }
        })
        .flat_map(|(server_id, response)| match response {
            GotoDefinitionResponse::Scalar(location) => vec![(server_id, location)],
            GotoDefinitionResponse::Array(locations) => {
                locations.into_iter().map(|v| (server_id, v)).collect()
            }
            GotoDefinitionResponse::Link(locations) => locations
                .into_iter()
                .map(
                    |LocationLink {
                         target_uri: uri,
                         target_selection_range: range,
                         ..
                     }| (server_id, Location { uri, range }),
                )
                .collect(),
        })
        .collect();

    match locations.len() {
        0 => {}
        1 => {
            goto_location(meta, &locations[0], ctx);
        }
        _ => {
            goto_locations(meta, &locations, ctx);
        }
    }
}

pub fn edit_at_range(buffile: &str, range: KakouneRange, in_normal_mode: bool) -> String {
    let normal = if in_normal_mode { "" } else { "<a-semicolon>" };
    formatdoc!(
        "edit -existing {}
         select {}
         execute-keys {normal}<c-s>{normal}vv",
        editor_quote(buffile),
        range,
    )
}

fn goto_location(
    meta: EditorMeta,
    (server_id, Location { uri, range }): &(ServerId, Location),
    ctx: &mut Context,
) {
    let path = uri.to_file_path().unwrap();
    let path_str = path.to_str().unwrap();
    if let Some(contents) = get_file_contents(path_str, ctx) {
        let server = ctx.server(*server_id);
        let range = lsp_range_to_kakoune(range, &contents, server.offset_encoding);
        let command = format!(
            "evaluate-commands -try-client %opt{{jumpclient}} -- {}",
            editor_quote(&edit_at_range(path_str, range, true)),
        );
        ctx.exec(meta, command);
    }
}

fn goto_locations(meta: EditorMeta, locations: &[(ServerId, Location)], ctx: &mut Context) {
    let select_location = locations
        .iter()
        .chunk_by(|(_, Location { uri, .. })| uri.to_file_path().unwrap())
        .into_iter()
        .map(|(path, locations)| {
            let path_str = path.to_str().unwrap();
            let contents = match get_file_contents(path_str, ctx) {
                Some(contents) => contents,
                None => return "".into(),
            };
            locations
                .map(|(server_id, Location { range, .. })| {
                    let server = ctx.server(*server_id);
                    let pos = lsp_range_to_kakoune(range, &contents, server.offset_encoding).start;
                    if range.start.line as usize >= contents.len_lines() {
                        return "".into();
                    }
                    // Let's use the main server root path to dictate how
                    // file paths should look like in the goto buffer.
                    format!(
                        "{}:{}:{}:{}",
                        short_file_path(path_str, ctx.main_root(&meta)),
                        pos.line,
                        pos.column,
                        contents.line(range.start.line as usize),
                    )
                })
                .join("")
        })
        .join("");
    let command = format!(
        "lsp-show-goto-choices {} {}",
        editor_quote(ctx.main_root(&meta)),
        editor_quote(&select_location),
    );
    ctx.exec(meta, command);
}

pub fn text_document_definition(
    declaration: bool,
    meta: EditorMeta,
    params: PositionParams,
    ctx: &mut Context,
) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_DEFINITION))
        .collect();
    if eligible_servers.is_empty() && meta.servers.len() > 1 {
        let cmd = format!(
            "lsp-show-error %[no server supports {}]",
            request::GotoDefinition::METHOD
        );
        ctx.exec(meta, cmd);
        return;
    }
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, server_settings)| {
            (
                server_id,
                vec![GotoDefinitionParams {
                    text_document_position_params: TextDocumentPositionParams {
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
                    partial_result_params: Default::default(),
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    let req_params = RequestParams::Each(req_params);
    if declaration {
        ctx.call::<GotoDeclaration, _>(
            meta,
            req_params,
            move |ctx: &mut Context, meta, results| goto(meta, results, ctx),
        );
    } else {
        ctx.call::<GotoDefinition, _>(meta, req_params, move |ctx: &mut Context, meta, results| {
            goto(meta, results, ctx)
        });
    }
}

pub fn text_document_implementation(meta: EditorMeta, params: PositionParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_IMPLEMENTATION))
        .collect();
    if eligible_servers.is_empty() && meta.servers.len() > 1 {
        let cmd = format!(
            "lsp-show-error %[no server supports {}]",
            request::GotoImplementation::METHOD
        );
        ctx.exec(meta, cmd);
        return;
    }
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, server_settings)| {
            (
                server_id,
                vec![GotoDefinitionParams {
                    text_document_position_params: TextDocumentPositionParams {
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
                    partial_result_params: Default::default(),
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<GotoImplementation, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| goto(meta, results, ctx),
    );
}

pub fn text_document_type_definition(meta: EditorMeta, params: PositionParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_TYPE_DEFINITION))
        .collect();
    if eligible_servers.is_empty() && meta.servers.len() > 1 {
        let cmd = format!(
            "lsp-show-error %[no server supports {}]",
            request::GotoTypeDefinition::METHOD
        );
        ctx.exec(meta, cmd);
        return;
    }
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, server_settings)| {
            (
                server_id,
                vec![GotoDefinitionParams {
                    text_document_position_params: TextDocumentPositionParams {
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
                    partial_result_params: Default::default(),
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<GotoTypeDefinition, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| goto(meta, results, ctx),
    );
}

pub fn text_document_references(meta: EditorMeta, params: PositionParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_REFERENCES))
        .collect();
    if eligible_servers.is_empty() && meta.servers.len() > 1 {
        let cmd = format!(
            "lsp-show-error %[no server supports {}]",
            request::References::METHOD
        );
        ctx.exec(meta, cmd);
        return;
    }
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, server_settings)| {
            (
                server_id,
                vec![ReferenceParams {
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
                    context: ReferenceContext {
                        include_declaration: true,
                    },
                    partial_result_params: Default::default(),
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<References, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            let results = results
                .into_iter()
                .map(|(server_id, loc)| (server_id, loc.map(GotoTypeDefinitionResponse::Array)))
                .collect();
            goto(meta, results, ctx);
        },
    );
}
