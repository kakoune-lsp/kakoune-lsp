use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use ropey::Rope;
use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use url::Url;

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
        work_done_progress_params: Default::default(),
    };
    ctx.call::<References, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_references(meta, result, ctx)
    });
}

pub fn editor_references(meta: EditorMeta, result: Option<Vec<Location>>, ctx: &mut Context) {
    let mut locations = match result {
        Some(locations) => locations,
        None => return,
    };
    // Sort locations by (filename, line)
    locations
        .sort_unstable_by_key(|location| (location.uri.to_file_path(), location.range.start.line));

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
                        format!("{}:{}:{}:{}", name, p.line, p.column, line)
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
    ctx.exec(meta, command);
}

pub fn text_document_references_highlight(
    meta: EditorMeta,
    params: EditorParams,
    ctx: &mut Context,
) {
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
        work_done_progress_params: Default::default(),
    };
    ctx.call::<References, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_references_highlight(meta, result, ctx)
    });
}

pub fn editor_references_highlight(
    meta: EditorMeta,
    result: Option<Vec<Location>>,
    ctx: &mut Context,
) {
    let document = ctx.documents.get(&meta.buffile);
    if document.is_none() {
        return;
    }
    let document = document.unwrap();
    if let Some(mut locations) = result {
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
        ctx.exec(meta, command);
    };
}
