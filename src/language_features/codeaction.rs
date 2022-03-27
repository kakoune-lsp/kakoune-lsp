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

    let document = ctx.documents.get(&meta.buffile).unwrap();
    let range = kakoune_range_to_lsp(
        &parse_kakoune_range(&params.selection_desc).0,
        &document.text,
        ctx.offset_encoding,
    );

    let buff_diags = ctx.diagnostics.get(&meta.buffile);

    let diagnostics: Vec<Diagnostic> = if let Some(buff_diags) = buff_diags {
        buff_diags
            .iter()
            .filter(|d| ranges_lines_overlap(d.range, range))
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
        editor_code_actions(meta, result, ctx, params)
    });
}

pub fn editor_code_actions(
    meta: EditorMeta,
    result: Option<CodeActionResponse>,
    ctx: &mut Context,
    params: CodeActionsParams,
) {
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
            1 => code_action_to_editor_command(matches[0], sync),
            _ => fail + " 'multiple matching actions'",
        };
        ctx.exec(meta, command);
        return;
    }

    let titles_and_commands = actions
        .iter()
        .map(|c| {
            let title = match c {
                CodeActionOrCommand::Command(command) => &command.title,
                CodeActionOrCommand::CodeAction(action) => &action.title,
            };
            let select_cmd = code_action_to_editor_command(c, false);
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

fn code_action_to_editor_command(action: &CodeActionOrCommand, sync: bool) -> String {
    match action {
        CodeActionOrCommand::Command(command) => {
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
        CodeActionOrCommand::CodeAction(action) => {
            // Double JSON serialization is performed to prevent parsing args as a TOML
            // structure when they are passed back via lsp-apply-workspace-edit.
            let edit = &serde_json::to_string(&action.edit.as_ref().unwrap()).unwrap();
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
    }
}
