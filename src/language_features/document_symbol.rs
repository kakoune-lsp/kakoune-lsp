use context::*;
use lsp_types::request::Request;
use lsp_types::*;
use serde_json::{self, Value};
use types::*;
use url::Url;
use util::*;

pub fn text_document_document_symbol(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let req_params = DocumentSymbolParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (
            meta.clone(),
            request::DocumentSymbolRequest::METHOD.into(),
            params,
        ),
    );
    ctx.call(
        id,
        request::DocumentSymbolRequest::METHOD.into(),
        req_params,
    );
}

pub fn editor_document_symbol(meta: &EditorMeta, result: Value, ctx: &mut Context) {
    let result: Option<Vec<SymbolInformation>> =
        serde_json::from_value(result).expect("Failed to parse document symbol response");
    if result.is_none() {
        return;
    }
    let result = result.unwrap();
    if result.is_empty() {
        return;
    }
    let content = format_symbol_information(result, ctx);
    let command = format!(
        "lsp-show-document-symbol {} {}",
        editor_quote(&ctx.root_path),
        editor_quote(&content),
    );
    ctx.exec(meta.clone(), command);
}
