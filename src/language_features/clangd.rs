use crate::context::*;
use crate::types::*;
use crate::util::*;
use lsp_types::request::Request;
use lsp_types::*;

pub struct SwitchSourceHeaderRequest {}

impl Request for SwitchSourceHeaderRequest {
    type Params = TextDocumentIdentifier;
    type Result = Option<Uri>;
    const METHOD: &'static str = "textDocument/switchSourceHeader";
}

pub fn switch_source_header(meta: EditorMeta, ctx: &mut Context) {
    let req_params = meta
        .servers
        .iter()
        .map(|&server_id| {
            (
                server_id,
                vec![TextDocumentIdentifier {
                    uri: file_path_to_uri(&meta.buffile),
                }],
            )
        })
        .collect();

    ctx.call::<SwitchSourceHeaderRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            let response = match results.into_iter().find(|(_, v)| v.is_some()) {
                Some((_, result)) => result,
                None => None,
            };

            if let Some(response) = response {
                let command = format!(
                    "evaluate-commands -try-client %opt{{jumpclient}} -verbatim -- edit -existing {}",
                    editor_quote(response.as_str()),
                );
                ctx.exec(meta, command);
            }
        },
    );
}
