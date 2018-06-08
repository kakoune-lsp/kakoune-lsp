use context::*;
use itertools::Itertools;
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
        // Sort locations by (filename, line)
        let mut locations = locations.to_vec();

        locations
            .sort_unstable_by_key(|location| {
              (location.uri.to_file_path(),
              location.range.start.line)
          });

        let content = locations
            .iter()
            .group_by(|location|{
              location.uri.to_file_path()
            })
            .into_iter()
            .map(|(filename, group)| {
                let filename = filename.unwrap();
                let name = filename.to_str().unwrap();
                let file = File::open(name);
                if file.is_err() {
                    error!("Failed to open referenced file: {}", name);
                    return group
                        .map(|_loc| String::new())
                        .collect::<Vec<String>>()
                        .join("\n");
                }
                let mut buffer = BufReader::new(file.unwrap()).lines();
                let mut next_buf_line = 0;
                return group.map(|location| {
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
                                Path::new(name)
                                    .strip_prefix(&ctx.root_path)
                                    .ok()
                                    .and_then(|p| Some(p.to_str().unwrap()))
                                    .or_else(|| Some(name))
                                    .unwrap(),
                                p.line + 1,
                                p.character + 1,
                                line
                            )
                        }
                        Some(Err(e)) => {
                            error!("Failed to read line {} in {}: {}", name, loc_line, e);
                            return String::new();
                        }
                        None => {
                            error!("End of file reached, line {} not found in {}", loc_line, name,);
                            return String::new();
                        }
                    }
                })
                .collect::<Vec<String>>()
                .join("\n");
            })
            .collect::<Vec<String>>()
            .join("\n");

        let command = format!(
            "eval -try-client %opt[toolsclient] %☠
             edit! -scratch *references*
             cd %§{}§
             try %{{ set buffer working_folder %sh{{pwd}} }}
             set buffer filetype grep
             set-register '\"' %§{}§
             exec -no-hooks p
             ☠",
            ctx.root_path, content,
        );
        ctx.exec(meta.clone(), command);
    };
}
