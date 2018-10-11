use context::*;
use languageserver_types::request::Request;
use languageserver_types::*;
use types::*;
use url::Url;
use util::*;

pub fn text_document_document_symbol(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
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

pub fn editor_document_symbol(
    meta: &EditorMeta,
    result: Option<Vec<SymbolInformation>>,
    ctx: &mut Context,
) {
    if result.is_none() {
        return;
    }
    let result = result.unwrap();
    if result.is_empty() {
        return;
    }
    let content = format_symbol_information(result, ctx);
    let command = format!(
        "lsp-show-document-symbol %§{}§ %§{}§",
        ctx.root_path, content,
    );
    ctx.exec(meta.clone(), command);
}
