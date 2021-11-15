use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_signature_help(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();
    let req_params = SignatureHelpParams {
        context: None,
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        work_done_progress_params: Default::default(),
    };
    ctx.call::<SignatureHelpRequest, _>(
        meta,
        req_params,
        move |ctx: &mut Context, meta, result| editor_signature_help(meta, params, result, ctx),
    );
}

pub fn editor_signature_help(
    meta: EditorMeta,
    params: PositionParams,
    result: Option<SignatureHelp>,
    ctx: &mut Context,
) {
    if let Some(result) = result {
        let active_signature = result.active_signature.unwrap_or(0);
        if let Some(active_signature) = result.signatures.get(active_signature as usize) {
            // TODO decide how to use it
            // let active_parameter = result.active_parameter.unwrap_or(0);
            let contents = &active_signature.label;
            let command = format!(
                "lsp-show-signature-help {} {}",
                params.position,
                editor_quote(contents)
            );
            ctx.exec(meta, command);
        }
    }
}
