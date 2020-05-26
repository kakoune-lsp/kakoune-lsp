use crate::context::*;
use crate::text_edit::apply_text_edits_to_buffer;
use crate::types::*;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_range_formatting(meta: EditorMeta, params: EditorParams, range: Range, ctx: &mut Context) {
    let params = FormattingOptions::deserialize(params)
        .expect("Params should follow FormattingOptions structure");
    let req_params = DocumentRangeFormattingParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        range: range,
        options: params,
        work_done_progress_params: Default::default(),
    };
    ctx.call::<RangeFormatting, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_range_formatting(meta, result, ctx)
    });
}

pub fn editor_range_formatting(meta: EditorMeta, result: Option<Vec<TextEdit>>, ctx: &mut Context) {
    let document = ctx.documents.get(&meta.buffile);
    if document.is_none() {
        // Nothing to do, but sending command back to the editor is required to handle case when
        // editor is blocked waiting for response via fifo.
        ctx.exec(meta, "nop".to_string());
        return;
    }
    let document = document.unwrap();
    match result {
        None => {
            // Nothing to do, but sending command back to the editor is required to handle case when
            // editor is blocked waiting for response via fifo.
            ctx.exec(meta, "nop".to_string());
            return;
        }
        Some(text_edits) => {
            ctx.exec(
                meta,
                apply_text_edits_to_buffer(None, &text_edits, &document.text, &ctx.offset_encoding),
            );
        }
    }
}
