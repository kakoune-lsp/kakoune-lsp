use context::*;
use languageserver_types::request::Request;
use languageserver_types::*;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use types::*;
use url::Url;

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
                let path = location.uri.to_file_path().unwrap();
                let filename = path.to_str().unwrap();
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
                                    Path::new(filename)
                                        .strip_prefix(&ctx.root_path)
                                        .ok()
                                        .and_then(|p| Some(p.to_str().unwrap()))
                                        .or_else(|| Some(filename))
                                        .unwrap(),
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
            "edit! -scratch *references*
             cd %§{}§
             try %{{ set buffer working_folder %sh{{pwd}} }}
             set buffer filetype grep
             set-register '\"' %§{}§
             exec -no-hooks p",
            ctx.root_path, content,
        );
        ctx.exec(meta.clone(), command);
    };
}
