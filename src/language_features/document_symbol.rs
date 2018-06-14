use context::*;
use languageserver_types::request::Request;
use languageserver_types::*;
use types::*;
use url::Url;

pub fn text_document_document_symbol(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let req_params = DocumentSymbolParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::DocumentSymbol::METHOD.into(), params),
    );
    ctx.call(id, request::DocumentSymbol::METHOD.into(), req_params);
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
    let content = result
        .into_iter()
        .map(|symbol| {
            let SymbolInformation {
                location,
                name,
                kind,
                ..
            } = symbol;
            let filename = location.uri.to_file_path().unwrap();
            let filename = filename
                .strip_prefix(&ctx.root_path)
                .ok()
                .and_then(|p| Some(p.to_str().unwrap()))
                .or_else(|| filename.to_str())
                .unwrap();

            let position = location.range.start;
            let description = format!("{:?} {}", kind, name);
            format!(
                "{}:{}:{}:{}",
                filename,
                position.line + 1,
                position.character + 1,
                description
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let command = format!(
        "lsp-show-document-symbol %§{}§ %§{}§",
        ctx.root_path, content,
    );
    ctx.exec(meta.clone(), command);
}
