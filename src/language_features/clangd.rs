use crate::context::*;
use crate::types::*;
use crate::util::*;
use lsp_types::request::Request;
use lsp_types::*;

pub struct SwitchSourceHeaderRequest {}

impl Request for SwitchSourceHeaderRequest {
    type Params = TextDocumentIdentifier;
    type Result = Option<Url>;
    const METHOD: &'static str = "textDocument/switchSourceHeader";
}

pub fn switch_source_header(meta: EditorMeta, ctx: &mut Context) {
    let req_params = TextDocumentIdentifier {
        uri: Url::from_file_path(&meta.buffile).unwrap(),
    };
    ctx.call::<SwitchSourceHeaderRequest, _>(
        meta,
        req_params,
        move |ctx: &mut Context, meta, response| {
            if let Some(response) = response {
                let command = format!(
                    "eval -try-client %opt{{jumpclient}} -verbatim -- edit -existing {}",
                    editor_quote(response.to_file_path().unwrap().to_str().unwrap()),
                );
                ctx.exec(meta, command);
            }
        },
    );
}
