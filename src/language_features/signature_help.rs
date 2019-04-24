use crate::context::*;
use crate::types::*;
use crate::util::*;
use lsp_types::request::Request;
use lsp_types::*;
use serde::Deserialize;
use serde_json::{self, Value};
use url::Url;

pub fn text_document_signature_help(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let req_params = PositionParams::deserialize(params.clone()).unwrap();
    let req_params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &req_params.position, ctx).unwrap(),
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (
            meta.clone(),
            request::SignatureHelpRequest::METHOD.into(),
            params,
        ),
    );
    ctx.call(id, request::SignatureHelpRequest::METHOD.into(), req_params);
}

pub fn editor_signature_help(
    meta: &EditorMeta,
    params: EditorParams,
    result: Value,
    ctx: &mut Context,
) {
    let params = PositionParams::deserialize(params).expect("Failed to parse params");
    let result: Option<SignatureHelp> =
        serde_json::from_value(result).expect("Failed to parse signature help response");
    if let Some(result) = result {
        let active_signature = result.active_signature.unwrap_or(0);
        if let Some(active_signature) = result.signatures.get(active_signature as usize) {
            // TODO decide how to use it
            // let active_parameter = result.active_parameter.unwrap_or(0);
            let contents = &active_signature.label;
            let command = format!(
                "lsp-show-signature-help {} {}",
                params.position,
                editor_quote(&contents)
            );
            ctx.exec(meta.clone(), command);
        }
    }
}
