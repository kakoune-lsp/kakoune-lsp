use context::*;
use languageserver_types::request::Request;
use languageserver_types::*;
use serde::Deserialize;
use types::*;
use url::Url;

pub fn text_document_signature_help(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let req_params = PositionParams::deserialize(params.clone());
    if req_params.is_err() {
        error!("Params should follow PositionParams structure");
        return;
    }
    let req_params = req_params.unwrap();
    let position = req_params.position;
    let req_params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position,
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
    params: &PositionParams,
    result: Option<SignatureHelp>,
    ctx: &mut Context,
) {
    if result.is_none() {
        return;
    }
    let result = result.unwrap();
    if result.signatures.is_empty() {
        return;
    }
    let active_signature = result.active_signature.unwrap_or(0);
    let active_signature = &result.signatures[active_signature as usize];
    // TODO decide how to use it
    // let active_parameter = result.active_parameter.unwrap_or(0);
    let contents = &active_signature.label;
    let position = params.position;
    let position = format!("{}.{}", position.line + 1, position.character + 1);
    let command = format!("lsp-show-signature-help {} %§{}§", position, contents);
    ctx.exec(meta.clone(), command);
}
