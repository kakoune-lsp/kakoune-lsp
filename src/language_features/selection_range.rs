use crate::context::Context;
use crate::position::{kakoune_position_to_lsp, lsp_range_to_kakoune};
use crate::types::{EditorMeta, EditorParams, PositionParams};
use crate::util::editor_quote;
use lsp_types::request::SelectionRangeRequest;
use lsp_types::{SelectionRange, SelectionRangeParams, TextDocumentIdentifier};
use serde::Deserialize;
use url::Url;

pub fn expand_selection(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => return,
    };
    let params = PositionParams::deserialize(params).unwrap();
    let req_params = SelectionRangeParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        positions: vec![kakoune_position_to_lsp(
            &params.position,
            &document.text,
            &ctx.offset_encoding,
        )],
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    ctx.call::<SelectionRangeRequest, _>(meta, req_params, move |ctx, meta, result| {
        editor_expand_selection(meta, result, ctx);
    });
}

pub fn editor_expand_selection(
    meta: EditorMeta,
    result: Option<Vec<SelectionRange>>,
    ctx: &mut Context,
) {
    let result = result.unwrap_or_else(Vec::new);
    let (result, client, document) = match (
        result.first(),
        &meta.client,
        ctx.documents.get(&meta.buffile),
    ) {
        (Some(result), Some(client), Some(document)) => (result, client, document),
        _ => return,
    };
    let range = lsp_range_to_kakoune(
        result
            .parent
            .as_ref()
            .map(|x| &x.range)
            .unwrap_or(&result.range),
        &document.text,
        &ctx.offset_encoding,
    );
    let command = format!("edit {}; select {}", &meta.buffile, range);
    let command = format!(
        "eval -client {} {}",
        editor_quote(&client),
        editor_quote(&command),
    );
    ctx.exec(meta, command);
}
