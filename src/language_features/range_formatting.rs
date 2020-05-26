use crate::context::*;
use crate::text_edit::apply_text_edits_to_buffer;
use crate::types::*;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_range_formatting(meta: EditorMeta, params: EditorParams, ranges: Vec<Range>, ctx: &mut Context) {
    let params = FormattingOptions::deserialize(params)
        .expect("Params should follow FormattingOptions structure");
    let req_params = ranges.into_iter().map(|range|
      DocumentRangeFormattingParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        range: range,
        options: params.clone(),
        work_done_progress_params: Default::default(),
    }).collect();
    ctx.batch_call::<RangeFormatting, _>(meta, req_params, move |ctx: &mut Context, meta, results| {
        let result = results.into_iter().flatten().flatten().collect();
        editor_range_formatting(meta, result, ctx)
    });
}

pub fn editor_range_formatting(meta: EditorMeta, text_edits: Vec<TextEdit>, ctx: &mut Context) {
    let document = ctx.documents.get(&meta.buffile);
    if text_edits.len() == 0 {
        // Nothing to do, but sending command back to the editor is required to handle case when
        // editor is blocked waiting for response via fifo.
        ctx.exec(meta, "nop".to_string());
        return;
    }
    let document = document.unwrap();
    ctx.exec(
        meta,
        apply_text_edits_to_buffer(None, &text_edits, &document.text, &ctx.offset_encoding),
    );
}
