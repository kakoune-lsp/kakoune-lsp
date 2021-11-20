use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_codeaction(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = CodeActionsParams::deserialize(params)
        .expect("Params should follow CodeActionsParams structure");
    let position = get_lsp_position(&meta.buffile, &params.position, ctx).unwrap();

    let buff_diags = ctx.diagnostics.get(&meta.buffile);

    let diagnostics: Vec<Diagnostic> = if let Some(buff_diags) = buff_diags {
        buff_diags
            .iter()
            .filter(|d| d.range.start.line <= position.line && position.line <= d.range.end.line)
            .cloned()
            .collect()
    } else {
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
            diagnostics,
            only: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    ctx.call::<CodeActionRequest, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_code_actions(meta, result, ctx, params)
    });
}

pub fn editor_code_actions(
    meta: EditorMeta,
    result: Option<CodeActionResponse>,
    ctx: &mut Context,
    params: CodeActionsParams,
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

    let actions = result
        .into_iter()
        .map(|c| match c {
            CodeActionOrCommand::Command(_) => c,
            CodeActionOrCommand::CodeAction(action) => match action.command {
                Some(cmd) => CodeActionOrCommand::Command(cmd),
                None => CodeActionOrCommand::CodeAction(action),
            },
        })
        .collect::<Vec<_>>();

    let titles_and_commands = actions
        .iter()
        .map(|c| {
            let title = match c {
                CodeActionOrCommand::Command(command) => &command.title,
                CodeActionOrCommand::CodeAction(action) => &action.title,
            };
            let select_cmd = code_action_to_editor_command(c);
            format!("{} {}", editor_quote(title), editor_quote(&select_cmd))
        })
        .join(" ");

    #[allow(clippy::collapsible_else_if)]
    let command = if params.perform_code_action {
        if actions.is_empty() {
            "lsp-show-error 'no actions available'".to_string()
        } else {
            format!("lsp-perform-code-action {}\n", titles_and_commands)
        }
    } else {
        if actions.is_empty() {
            "lsp-hide-code-actions\n".to_string()
        } else {
            format!("lsp-show-code-actions {}\n", titles_and_commands)
        }
    };
    ctx.exec(meta, command);
}

fn code_action_to_editor_command(action: &CodeActionOrCommand) -> String {
    match action {
        CodeActionOrCommand::Command(command) => {
            let cmd = editor_quote(&command.command);
            // Double JSON serialization is performed to prevent parsing args as a TOML
            // structure when they are passed back via lsp-execute-command.
            let args = &serde_json::to_string(&command.arguments).unwrap();
            let args = editor_quote(&serde_json::to_string(&args).unwrap());
            format!("lsp-execute-command {} {}", cmd, args)
        }
        CodeActionOrCommand::CodeAction(action) => {
            // Double JSON serialization is performed to prevent parsing args as a TOML
            // structure when they are passed back via lsp-apply-workspace-edit.
            let edit = &serde_json::to_string(&action.edit.as_ref().unwrap()).unwrap();
            let edit = editor_quote(&serde_json::to_string(&edit).unwrap());
            format!("lsp-apply-workspace-edit {}", edit)
        }
    }
}
