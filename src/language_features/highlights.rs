use crate::context::Context;
use crate::position::*;
use crate::types::{EditorMeta, EditorParams, PositionParams};
use itertools::Itertools;
use lsp_types::{
    request::DocumentHighlightRequest, DocumentHighlight, DocumentHighlightKind,
    DocumentHighlightParams, TextDocumentIdentifier, TextDocumentPositionParams,
};
use serde::Deserialize;
use url::Url;

pub fn text_document_highlights(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();
    let req_params = DocumentHighlightParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        partial_result_params: Default::default(),
        work_done_progress_params: Default::default(),
    };
    ctx.call::<DocumentHighlightRequest, _>(
        meta,
        req_params,
        move |ctx: &mut Context, meta, result| editor_document_highlights(meta, result, ctx),
    );
}

pub fn editor_document_highlights(
    meta: EditorMeta,
    result: Option<Vec<DocumentHighlight>>,
    ctx: &mut Context,
) {
    let document = ctx.documents.get(&meta.buffile);
    if document.is_none() {
        return;
    }
    let document = document.unwrap();
    let ranges = match result {
        Some(highlights) => highlights
            .into_iter()
            .map(|highlight| {
                format!(
                    "{}|{}",
                    lsp_range_to_kakoune(&highlight.range, &document.text, ctx.offset_encoding),
                    if highlight.kind == Some(DocumentHighlightKind::WRITE) {
                        "ReferenceBind"
                    } else {
                        "Reference"
                    }
                )
            })
            .join(" "),
        None => "".to_string(),
    };
    let command = format!(
        "set-option window lsp_references {} {}",
        meta.version, ranges,
    );
    ctx.exec(meta, command);
}
