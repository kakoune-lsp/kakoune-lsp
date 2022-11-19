use crate::capabilities::attempt_server_capability;
use crate::capabilities::CAPABILITY_CODE_ACTIONS;
use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use crate::wcwidth;
use indoc::formatdoc;
use itertools::Itertools;
use lazy_static::lazy_static;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_code_action(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    if meta.fifo.is_none() && !attempt_server_capability(ctx, CAPABILITY_CODE_ACTIONS) {
        return;
    }

    let params = CodeActionsParams::deserialize(params)
        .expect("Params should follow CodeActionsParams structure");

    let document = ctx.documents.get(&meta.buffile).unwrap();
    let range = kakoune_range_to_lsp(
        &parse_kakoune_range(&params.selection_desc).0,
        &document.text,
        ctx.offset_encoding,
    );
    code_actions_for_range(meta, params, ctx, range)
}

fn code_actions_for_range(
    meta: EditorMeta,
    params: CodeActionsParams,
    ctx: &mut Context,
    range: Range,
) {
    let buff_diags = ctx.diagnostics.get(&meta.buffile);

    let diagnostics: Vec<Diagnostic> = if let Some(buff_diags) = buff_diags {
        buff_diags
            .iter()
            .filter(|d| ranges_overlap(d.range, range))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    let req_params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        range,
        context: CodeActionContext {
            diagnostics,
            only: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    ctx.call::<CodeActionRequest, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_code_actions(meta, result, ctx, params, range)
    });
}

fn editor_code_actions(
    meta: EditorMeta,
    result: Option<CodeActionResponse>,
    ctx: &mut Context,
    params: CodeActionsParams,
    mut range: Range,
) {
    if !meta.hook
        && result == Some(vec![])
        && range.start.character != 0
        && range.end.character != EOL_OFFSET
    {
        // Some servers send code actions only if the requested range includes the affected
        // AST nodes.  Let's make them more convenient to access by requesting on whole lines.
        range.start.character = 0;
        range.end.character = EOL_OFFSET;
        code_actions_for_range(meta, params, ctx, range);
        return;
    }

    let result = result.unwrap_or_default();

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

    if let Some(pattern) = params.code_action_pattern.as_ref() {
        let regex = match regex::Regex::new(pattern) {
            Ok(regex) => regex,
            Err(error) => {
                let command = format!(
                    "lsp-show-error 'invalid pattern: {}'",
                    &editor_escape(&error.to_string())
                );
                ctx.exec(meta, command);
                return;
            }
        };
        let matches = actions
            .iter()
            .filter(|c| {
                let title = match c {
                    CodeActionOrCommand::Command(command) => &command.title,
                    CodeActionOrCommand::CodeAction(action) => &action.title,
                };
                regex.is_match(title)
            })
            .collect::<Vec<_>>();
        let sync = meta.fifo.is_some();
        let fail = if sync {
            // We might be running from a hook, so let's allow silencing errors with a "try".
            // Also, prefix with the (presumable) function name, to reduce confusion.
            "fail lsp-code-action:"
        } else {
            "lsp-show-error"
        }
        .to_string();
        let command = match matches.len() {
            0 => fail + " 'no matching action available'",
            1 => code_action_or_command_to_editor_command(matches[0], sync),
            _ => fail + " 'multiple matching actions'",
        };
        ctx.exec(meta, command);
        return;
    }

    let titles_and_commands = actions
        .iter()
        .map(|c| {
            let mut title: &str = match c {
                CodeActionOrCommand::Command(command) => &command.title,
                CodeActionOrCommand::CodeAction(action) => &action.title,
            };
            if let Some((head, _)) = title.split_once('\n') {
                title = head
            }
            let select_cmd = code_action_or_command_to_editor_command(c, false);
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
            lazy_static! {
                static ref CODE_ACTION_INDICATOR: &'static str =
                    wcwidth::expected_width_or_fallback("💡", 2, "[A]");
            }
            let commands = formatdoc!(
                "set-option global lsp_code_action_indicator {}
                 lsp-show-code-actions {}
                 ",
                *CODE_ACTION_INDICATOR,
                titles_and_commands
            );
            format!("evaluate-commands -- {}", editor_quote(&commands))
        }
    };
    ctx.exec(meta, command);
}

fn code_action_or_command_to_editor_command(action: &CodeActionOrCommand, sync: bool) -> String {
    match action {
        CodeActionOrCommand::Command(command) => execute_command_editor_command(command, sync),
        CodeActionOrCommand::CodeAction(action) => {
            code_action_to_editor_command(action, sync, true)
        }
    }
}

fn code_action_to_editor_command(action: &CodeAction, sync: bool, may_resolve: bool) -> String {
    let command = match &action.command {
        Some(command) => "\n".to_string() + &execute_command_editor_command(command, sync),
        None => "".to_string(),
    };
    match &action.edit {
        Some(edit) => apply_workspace_edit_editor_command(edit, sync) + &command,
        None => {
            if may_resolve {
                let args = &serde_json::to_string(&action).unwrap();
                format!("lsp-code-action-resolve-request {}", editor_quote(args))
            } else {
                "lsp-show-error 'unresolved code action'".to_string()
            }
        }
    }
}

pub fn apply_workspace_edit_editor_command(edit: &WorkspaceEdit, sync: bool) -> String {
    // Double JSON serialization is performed to prevent parsing args as a TOML
    // structure when they are passed back via lsp-apply-workspace-edit.
    let edit = &serde_json::to_string(edit).unwrap();
    let edit = editor_quote(&serde_json::to_string(&edit).unwrap());
    format!(
        "{} {}",
        if sync {
            "lsp-apply-workspace-edit-sync"
        } else {
            "lsp-apply-workspace-edit"
        },
        edit
    )
}

pub fn execute_command_editor_command(command: &Command, sync: bool) -> String {
    let cmd = editor_quote(&command.command);
    // Double JSON serialization is performed to prevent parsing args as a TOML
    // structure when they are passed back via lsp-execute-command.
    let args = &serde_json::to_string(&command.arguments).unwrap();
    let args = editor_quote(&serde_json::to_string(&args).unwrap());
    format!(
        "{} {} {}",
        if sync {
            "lsp-execute-command-sync"
        } else {
            "lsp-execute-command"
        },
        cmd,
        args
    )
}

pub fn text_document_code_action_resolve(
    meta: EditorMeta,
    params: EditorParams,
    ctx: &mut Context,
) {
    let params = CodeActionResolveParams::deserialize(params)
        .expect("Params should follow CodeActionResolveParams structure");

    ctx.call::<CodeActionResolveRequest, _>(
        meta,
        serde_json::from_str(&params.code_action).unwrap(),
        move |ctx: &mut Context, meta, result| {
            let cmd = code_action_to_editor_command(&result, false, false);
            ctx.exec(meta, cmd)
        },
    );
}
