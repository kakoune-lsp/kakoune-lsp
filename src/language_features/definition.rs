use crate::context::*;
use crate::types::*;
use crate::util::*;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_definition(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();
    let req_params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
    };
    ctx.call::<GotoDefinition, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_definition(meta, result, ctx)
    });
}

pub fn editor_definition(
    meta: EditorMeta,
    result: Option<GotoDefinitionResponse>,
    ctx: &mut Context,
) {
    if let Some(location) = goto_definition_response_to_location(result) {
        let path = location.uri.to_file_path().unwrap();
        let filename = path.to_str().unwrap();
        let p = get_kakoune_position(filename, &location.range.start, ctx).unwrap();
        let command = format!(
            "evaluate-commands -try-client %opt{{jumpclient}} %{{edit {} {} {}}}",
            editor_quote(filename),
            p.line,
            p.column
        );
        ctx.exec(meta, command);
    };
}
