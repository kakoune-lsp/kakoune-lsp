use context::*;
use languageserver_types::request::Request;
use languageserver_types::*;
use serde::Deserialize;
use types::*;
use url::Url;

pub fn text_document_definition(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let req_params = PositionParams::deserialize(params.clone())
        .expect("Params should follow PositionParams structure");
    let position = req_params.position;
    let req_params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::parse(&format!("file://{}", &meta.buffile)).unwrap(),
        },
        position,
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::GotoDefinition::METHOD.into(), params),
    );
    ctx.call(id, request::GotoDefinition::METHOD.into(), req_params);
}

pub fn editor_definition(
    meta: &EditorMeta,
    _params: &PositionParams,
    result: GotoDefinitionResponse,
    ctx: &mut Context,
) {
    if let Some(location) = match result {
        GotoDefinitionResponse::Scalar(location) => Some(location),
        GotoDefinitionResponse::Array(mut locations) => if locations.is_empty() {
            None
        } else {
            Some(locations.remove(0))
        },
        GotoDefinitionResponse::None => None,
    } {
        let filename = location.uri.path();
        let p = location.range.start;
        let command = format!("edit %§{}§ {} {}", filename, p.line + 1, p.character + 1);
        ctx.exec(meta.clone(), command);
    };
}
