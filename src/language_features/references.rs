use context::*;
use languageserver_types::request::Request;
use languageserver_types::*;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use types::*;
use util::*;

pub fn text_document_references(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let req_params = PositionParams::deserialize(params.clone());
    if req_params.is_err() {
        error!("Params should follow PositionParams structure");
        return;
    }
    let req_params = req_params.unwrap();
    let position = req_params.position;
    let req_params = ReferenceParams {
        text_document: TextDocumentIdentifier {
            uri: path_to_uri(&meta.buffile),
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

pub fn editor_references(
    meta: &EditorMeta,
    _params: &PositionParams,
    result: ReferencesResponse,
    ctx: &mut Context,
) {
    if let Some(locations) = match result {
        ReferencesResponse::Array(locations) => Some(locations),
        ReferencesResponse::None => None,
    } {
        let content = locations
            .iter()
            .map(|location| {
                let p = location.range.start;
                let filename = location.uri.path();
                let file = File::open(filename);
                if file.is_err() {
                    error!("Failed to open referenced file: {}", filename);
                    return String::new();
                }
                let line_num = p.line as usize;
                for (i, line) in BufReader::new(file.unwrap()).lines().enumerate() {
                    if i == line_num {
                        match line {
                            Ok(line) => {
                                return format!(
                                    "{}:{}:{}:{}",
                                    filename,
                                    p.line + 1,
                                    p.character + 1,
                                    line
                                )
                            }
                            Err(e) => {
                                error!("Failed to read line {} in {}: {}", filename, line_num, e);
                                return String::new();
                            }
                        }
                    }
                }
                return String::new();
            })
            .collect::<Vec<String>>()
            .join("\n");
        let command = format!(
            "edit! -scratch *references*\nset buffer filetype grep\nset-register '\"' %§{}§\nexec -no-hooks p",
            content,
        );
        ctx.exec(meta.clone(), command);
    };
}
