use crate::context::*;
use crate::types::*;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_formatting(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = FormattingOptions::deserialize(params)
        .expect("Params should follow FormattingOptions structure");
    let req_params = DocumentFormattingParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        options: params,
        work_done_progress_params: Default::default(),
    };
    ctx.call::<Formatting, _>(
        meta,
        req_params,
        move |ctx: &mut Context, meta: EditorMeta, result: Option<Vec<TextEdit>>| {
            let text_edits = result.unwrap_or_default();
            super::range_formatting::editor_range_formatting(meta, text_edits, ctx)
        },
    );
}
