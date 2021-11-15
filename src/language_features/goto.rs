use crate::context::Context;
use crate::position::*;
use crate::types::{EditorMeta, EditorParams, PositionParams};
use crate::util::{editor_quote, short_file_path};
use itertools::Itertools;
use lsp_types::request::{GotoDefinition, GotoImplementation, GotoTypeDefinition, References};
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn goto(meta: EditorMeta, result: Option<GotoDefinitionResponse>, ctx: &mut Context) {
    let locations = match result {
        Some(GotoDefinitionResponse::Scalar(location)) => vec![location],
        Some(GotoDefinitionResponse::Array(locations)) => locations,
        Some(GotoDefinitionResponse::Link(locations)) => locations
            .into_iter()
            .map(
                |LocationLink {
                     target_uri: uri,
                     target_range: range,
                     ..
                 }| Location { uri, range },
            )
            .collect(),
        None => return,
    };
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

pub fn goto_location(meta: EditorMeta, Location { uri, range }: &Location, ctx: &mut Context) {
    let path = uri.to_file_path().unwrap();
    let path_str = path.to_str().unwrap();
    if let Some(contents) = get_file_contents(path_str, ctx) {
        let pos = lsp_range_to_kakoune(range, &contents, ctx.offset_encoding).start;
        let command = format!(
            "eval -try-client %opt{{jumpclient}} -verbatim -- edit -existing {} {} {}",
            editor_quote(path_str),
            pos.line,
            pos.column,
        );
        ctx.exec(meta, command);
    }
}

pub fn goto_locations(meta: EditorMeta, locations: &[Location], ctx: &mut Context) {
    let select_location = locations
        .iter()
        .group_by(|Location { uri, .. }| uri.to_file_path().unwrap())
        .into_iter()
        .map(|(path, locations)| {
            let path_str = path.to_str().unwrap();
            let contents = match get_file_contents(path_str, ctx) {
                Some(contents) => contents,
                None => return "".into(),
            };
            locations
                .map(|Location { range, .. }| {
                    let pos = lsp_range_to_kakoune(range, &contents, ctx.offset_encoding).start;
                    if range.start.line as usize >= contents.len_lines() {
                        return "".into();
                    }
                    format!(
                        "{}:{}:{}:{}",
                        short_file_path(path_str, &ctx.root_path),
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
        editor_quote(&ctx.root_path),
        editor_quote(&select_location),
    );
    ctx.exec(meta, command);
}

pub fn text_document_definition(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();
    let req_params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        partial_result_params: Default::default(),
        work_done_progress_params: Default::default(),
    };
    ctx.call::<GotoDefinition, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        goto(meta, result, ctx);
    });
}

pub fn text_document_implementation(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();
    let req_params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        partial_result_params: Default::default(),
        work_done_progress_params: Default::default(),
    };
    ctx.call::<GotoImplementation, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        goto(meta, result, ctx);
    });
}

pub fn text_document_type_definition(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();
    let req_params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        partial_result_params: Default::default(),
        work_done_progress_params: Default::default(),
    };
    ctx.call::<GotoTypeDefinition, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        goto(meta, result, ctx);
    });
}

pub fn text_document_references(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();
    let req_params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        context: ReferenceContext {
            include_declaration: true,
        },
        partial_result_params: Default::default(),
        work_done_progress_params: Default::default(),
    };
    ctx.call::<References, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        goto(meta, result.map(GotoDefinitionResponse::Array), ctx);
    });
}
