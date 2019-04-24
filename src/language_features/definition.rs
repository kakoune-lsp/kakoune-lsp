use crate::context::*;
use crate::types::*;
use crate::util::*;
use lsp_types::request::Request;
use lsp_types::*;
use serde::Deserialize;
use serde_json::{self, Value};
use url::Url;

pub fn text_document_definition(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
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
        (meta.clone(), request::GotoDefinition::METHOD.into(), params),
    );
    ctx.call(id, request::GotoDefinition::METHOD.into(), req_params);
}

pub fn editor_definition(meta: &EditorMeta, result: Value, ctx: &mut Context) {
    let result = serde_json::from_value(result).expect("Failed to parse definition response");
    if let Some(location) = goto_definition_response_to_location(result) {
        let path = location.uri.to_file_path().unwrap();
        let filename = path.to_str().unwrap();
        let p = get_kakoune_position(filename, &location.range.start, ctx).unwrap();
        let command = format!("edit {} {} {}", editor_quote(filename), p.line, p.column);
        ctx.exec(meta.clone(), command);
    };
}
