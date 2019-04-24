use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::request::Request;
use lsp_types::*;
use ropey::Rope;
use serde::Deserialize;
use serde_json::{self, Value};
use std::fs::File;
use std::io::BufReader;
use url::Url;

pub fn text_document_references(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let req_params = PositionParams::deserialize(params.clone()).unwrap();
    let req_params = ReferenceParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &req_params.position, ctx).unwrap(),
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
                let text = Rope::from_reader(BufReader::new(file.unwrap())).unwrap();
                group
                    .map(|location| {
                        let position = location.range.start;
                        let loc_line = position.line as usize;
                        if loc_line < text.len_lines() {
                            let line = text.line(loc_line);
                            let p = lsp_position_to_kakoune(&position, &text, &ctx.offset_encoding);
                            format!("{}:{}:{}:{}", name, p.line, p.byte, line)
                        } else {
                            error!(
                                "End of file reached, line {} not found in {}",
                                loc_line, name,
                            );
                            String::from("\n")
                        }
                    })
                    .join("")
            })
            .join("");

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
    let req_params = PositionParams::deserialize(params.clone()).unwrap();
    let req_params = ReferenceParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &req_params.position, ctx).unwrap(),
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
    let document = ctx.documents.get(&meta.buffile);
    if document.is_none() {
        return;
    }
    let document = document.unwrap();
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
            .map(|location| {
                format!(
                    "{}|Reference",
                    lsp_range_to_kakoune(&location.range, &document.text, &ctx.offset_encoding)
                )
            })
            .join(" ");
        let command = format!(
            "set-option window lsp_references {} {}",
            meta.version, ranges,
        );
        ctx.exec(meta.clone(), command);
    };
}
