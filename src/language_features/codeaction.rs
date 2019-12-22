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

    let buff_diags = ctx.diagnostics.get(&meta.buffile);

    let diagnostics: Vec<Diagnostic> = if buff_diags.is_some() {
        buff_diags
            .unwrap()
            .iter()
            .filter(|d| d.range.start.line <= position.line && position.line <= d.range.end.line )
            .cloned()
            .collect()
    }
    else {
        Vec::new()
    };

    let req_params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        range: Range {
            start: position,
            end: position,
        },
        context: CodeActionContext {
            diagnostics: diagnostics,
            only: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
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

    for cmd in &result {
        match cmd {
            CodeActionOrCommand::Command(cmd) => info!("Command: {:?}", cmd),
            CodeActionOrCommand::CodeAction(action) => info!("Action: {:?}", action),
        }
    }

    if result.is_empty() {
        ctx.exec(meta, format!("lsp-show-error 'No actions available'"));
        return;
    }

    let menu_args = result
        .iter()
        .map(|c| match c {
            CodeActionOrCommand::Command(_) => c.clone(),
            CodeActionOrCommand::CodeAction(action) => match &action.command {
                Some(cmd) => CodeActionOrCommand::Command(cmd.clone()),
                None => c.clone(),
            },
        })
        .map(|c| match c {
            CodeActionOrCommand::Command(command) => {
                let title = editor_quote(&command.title);
                let cmd = editor_quote(&command.command);
                // Double JSON serialization is performed to prevent parsing args as a TOML
                // structure when they are passed back via lsp-execute-command.
                let args = &serde_json::to_string(&command.arguments).unwrap();
                let args = editor_quote(&serde_json::to_string(&args).unwrap());
                let select_cmd = editor_quote(&format!("lsp-execute-command {} {}", cmd, args));
                format!("{} {}", title, select_cmd)
            }
            CodeActionOrCommand::CodeAction(action) => {
                let title = editor_quote(&action.title);
                // Double JSON serialization is performed to prevent parsing args as a TOML
                // structure when they are passed back via lsp-apply-workspace-edit.
                let edit = &serde_json::to_string(&action.edit.unwrap()).unwrap();
                let edit = editor_quote(&serde_json::to_string(&edit).unwrap());
                let select_cmd = editor_quote(&format!("lsp-apply-workspace-edit {}", edit));
                format!("{} {}", title, select_cmd)
            }
        })
        .join(" ");
    ctx.exec(meta, format!("menu {}", menu_args));
}
