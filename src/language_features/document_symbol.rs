use crate::context::*;
use crate::types::*;
use crate::util::*;
use lsp_types::request::*;
use lsp_types::*;
use url::Url;

pub fn text_document_document_symbol(meta: EditorMeta, ctx: &mut Context) {
    let req_params = DocumentSymbolParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
    };
    ctx.call::<DocumentSymbolRequest, _>(
        meta,
        req_params,
        move |ctx: &mut Context, meta, result| editor_document_symbol(meta, result, ctx),
    );
}

pub fn editor_document_symbol(
    meta: EditorMeta,
    result: Option<DocumentSymbolResponse>,
    ctx: &mut Context,
) {
    let content = match result {
        Some(DocumentSymbolResponse::Flat(result)) => {
            if result.is_empty() {
                return;
            }
            format_symbol_information(result, ctx)
        }
        Some(DocumentSymbolResponse::Nested(result)) => {
            if result.is_empty() {
                return;
            }
            format_document_symbol(result, &meta, ctx)
        }
        None => {
            return;
        }
    };
    let command = format!(
        "lsp-show-document-symbol {} {}",
        editor_quote(&ctx.root_path),
        editor_quote(&content),
    );
    ctx.exec(meta, command);
}
