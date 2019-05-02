use crate::context::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_codeaction(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params =
        PositionParams::deserialize(params).expect("Params should follow PositionParams structure");
    let position = get_lsp_position(&meta.buffile, &params.position, ctx).unwrap();

    let req_params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        range: Range {
            start: position,
            end: position,
        },
        context: CodeActionContext {
            diagnostics: vec![], // TODO
            only: None,
        },
    };
    ctx.call::<CodeActionRequest, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_code_actions(meta, result, ctx)
    });
}

pub fn editor_code_actions(
    meta: EditorMeta,
    result: Option<CodeActionResponse>,
    ctx: &mut Context,
) {
    let result = match result {
        Some(result) => result,
        None => return,
    };
    match result {
        CodeActionResponse::Commands(cmds) => {
            if cmds.is_empty() {
                return;
            }
            for cmd in &cmds {
                debug!("Command: {:?}", cmd);
            }
            let menu_args = cmds
                .iter()
                .map(|command| {
                    let title = editor_quote(&command.title);
                    let cmd = editor_quote(&command.command);
                    let args = &serde_json::to_string(&command.arguments).unwrap();
                    let args = editor_quote(&serde_json::to_string(&args).unwrap());
                    let select_cmd = editor_quote(&format!("lsp-execute-command {} {}", cmd, args));
                    format!("{} {}", title, select_cmd)
                })
                .join(" ");
            ctx.exec(meta, format!("menu {}", menu_args));
        }
        CodeActionResponse::Actions(actions) => {
            for action in actions {
                debug!("Action: {:?}", action);
            }
        }
    }
}
