use crate::context::*;
use crate::text_edit::apply_text_edits_to_buffer;
use crate::types::*;
use lsp_types::request::Request;
use lsp_types::*;
use serde::Deserialize;
use serde_json::{self, Value};
use url::Url;

pub fn text_document_formatting(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let options = FormattingOptions::deserialize(params.clone());
    if options.is_err() {
        error!("Params should follow FormattingOptions structure");
    }
    let options = options.unwrap();
    let req_params = DocumentFormattingParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        options,
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::Formatting::METHOD.into(), params),
    );
    ctx.call(id, request::Formatting::METHOD.into(), req_params);
}

pub fn editor_formatting(
    meta: &EditorMeta,
    _params: EditorParams,
    result: Value,
    ctx: &mut Context,
) {
    let result = serde_json::from_value(result).expect("Failed to parse formatting response");
    let document = ctx.documents.get(&meta.buffile);
    if document.is_none() {
        // Nothing to do, but sending command back to the editor is required to handle case when
        // editor is blocked waiting for response via fifo.
        ctx.exec(meta.clone(), "nop".to_string());
        return;
    }
    let document = document.unwrap();
    match result {
        TextEditResponse::None => {
            // Nothing to do, but sending command back to the editor is required to handle case when
            // editor is blocked waiting for response via fifo.
            ctx.exec(meta.clone(), "nop".to_string());
            return;
        }
        TextEditResponse::Array(text_edits) => {
            ctx.exec(
                meta.clone(),
                apply_text_edits_to_buffer(None, &text_edits, &document.text, &ctx.offset_encoding),
            );
        }
    }
}
