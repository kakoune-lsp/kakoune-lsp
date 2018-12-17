use context::*;
use itertools::Itertools;
use languageserver_types::request::Request;
use languageserver_types::*;
use serde::Deserialize;
use serde_json::{self, Value};
use std::fs::File;
use std::io::{BufRead, BufReader};
use types::*;
use url::Url;
use util::*;

pub fn text_document_references(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let req_params = PositionParams::deserialize(params.clone());
    if req_params.is_err() {
        error!("Params should follow PositionParams structure");
        return;
    }
    let req_params = req_params.unwrap();
    let position = req_params.position;
    let req_params = ReferenceParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position,
        context: ReferenceContext {
            include_declaration: true,
        },
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::References::METHOD.into(), params),
    );
    ctx.call(id, request::References::METHOD.into(), req_params);
}

pub fn editor_references(meta: &EditorMeta, result: Value, ctx: &mut Context) {
    let result = serde_json::from_value(result).expect("Failed to parse references response");
    if let Some(mut locations) = match result {
        ReferencesResponse::Array(locations) => Some(locations),
        ReferencesResponse::None => None,
    } {
        // Sort locations by (filename, line)
        locations.sort_unstable_by_key(|location| {
            (location.uri.to_file_path(), location.range.start.line)
        });

        let content = locations
            .iter()
            .group_by(|location| location.uri.to_file_path())
            .into_iter()
            .map(|(filename, group)| {
                let filename = filename.unwrap();
                let file = File::open(&filename);
                let name = filename
                    .strip_prefix(&ctx.root_path)
                    .ok()
                    .and_then(|p| Some(p.to_str().unwrap()))
                    .or_else(|| filename.to_str())
                    .unwrap();

                if file.is_err() {
                    error!("Failed to open referenced file: {}", name);
                    return group.map(|_loc| String::new()).join("\n");
                }
                let mut buffer = BufReader::new(file.unwrap()).lines();
                let mut next_buf_line = 0;
                group
                    .map(|location| {
                        let p = location.range.start;
                        let loc_line = p.line as usize;
                        while next_buf_line != loc_line {
                            buffer.next();
                            next_buf_line += 1;
                        }
                        next_buf_line += 1;
                        match buffer.next() {
                            Some(Ok(line)) => {
                                return format!(
                                    "{}:{}:{}:{}",
                                    name,
                                    p.line + 1,
                                    p.character + 1,
                                    line
                                )
                            }
                            Some(Err(e)) => {
                                error!("Failed to read line {} in {}: {}", name, loc_line, e);
                                String::new()
                            }
                            None => {
                                error!(
                                    "End of file reached, line {} not found in {}",
                                    loc_line, name,
                                );
                                String::new()
                            }
                        }
                    })
                    .join("\n")
            })
            .join("\n");

        let command = format!(
            "lsp-show-references {} {}",
            editor_quote(&ctx.root_path),
            editor_quote(&content),
        );
        ctx.exec(meta.clone(), command);
    };
}

pub fn text_document_references_highlight(
    meta: &EditorMeta,
    params: EditorParams,
    ctx: &mut Context,
) {
    let req_params = PositionParams::deserialize(params.clone());
    if req_params.is_err() {
        error!("Params should follow PositionParams structure");
        return;
    }
    let req_params = req_params.unwrap();
    let position = req_params.position;
    let req_params = ReferenceParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position,
        context: ReferenceContext {
            include_declaration: true,
        },
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (
            meta.clone(),
            "textDocument/referencesHighlight".into(),
            params,
        ),
    );
    ctx.call(id, request::References::METHOD.into(), req_params);
}

pub fn editor_references_highlight(meta: &EditorMeta, result: Value, ctx: &mut Context) {
    let result = serde_json::from_value(result).expect("Failed to parse references response");
    if let Some(mut locations) = match result {
        ReferencesResponse::Array(locations) => Some(locations),
        ReferencesResponse::None => None,
    } {
        // Sort locations by (filename, line)
        locations.sort_unstable_by_key(|location| {
            (location.uri.to_file_path(), location.range.start.line)
        });

        let ranges = locations
            .iter()
            .filter(|location| {
                location.uri.to_file_path().unwrap().to_str().unwrap() == meta.buffile
            })
            .map(|location| format!("{}|Reference", lsp_range_to_kakoune(location.range)))
            .join(" ");
        let command = format!(
            "set-option window lsp_references {} {}",
            meta.version, ranges,
        );
        ctx.exec(meta.clone(), command);
    };
}
