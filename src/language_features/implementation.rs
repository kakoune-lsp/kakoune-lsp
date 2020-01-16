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

pub fn text_document_implementation(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();
    let req_params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
    };
    ctx.call::<GotoImplementation, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_implementation(meta, result, ctx)
    });
}

pub fn editor_implementation(
    meta: EditorMeta,
    result: Option<GotoImplementationResponse>,
    ctx: &mut Context,
) {
    let mut locations = match goto_definition_response_to_locations(result) {
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
        "lsp-show-implementations {} {}",
        editor_quote(&ctx.root_path),
        editor_quote(&content),
    );
    ctx.exec(meta, command);
}
