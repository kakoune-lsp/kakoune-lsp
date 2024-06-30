use std::collections::HashMap;
use std::collections::HashSet;

use crate::capabilities::attempt_server_capability;
use crate::capabilities::CAPABILITY_CODE_ACTIONS;
use crate::capabilities::CAPABILITY_CODE_ACTIONS_RESOLVE;
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
    let eligible_servers: Vec<_> = ctx
        .language_servers
        .iter()
        .filter(|srv| attempt_server_capability(*srv, &meta, CAPABILITY_CODE_ACTIONS))
        .collect();
    if eligible_servers.is_empty() {
        if meta.fifo.is_some() {
            ctx.exec(meta, "nop");
        }
        return;
    }

    let params = CodeActionsParams::deserialize(params)
        .expect("Params should follow CodeActionsParams structure");

    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => {
            let err = format!("Missing document for {}", &meta.buffile);
            error!("{}", err);
            if !meta.hook {
                ctx.exec(meta, format!("lsp-show-error '{}'", &editor_escape(&err)));
            }
            return;
        }
    };
    let ranges = eligible_servers
        .into_iter()
        .map(|(server_name, server_settings)| {
            (
                server_name.clone(),
                kakoune_range_to_lsp(
                    &parse_kakoune_range(&params.selection_desc).0,
                    &document.text,
                    server_settings.offset_encoding,
                ),
            )
        })
        .collect();
    code_actions_for_ranges(meta, params, ctx, ranges)
}

fn code_actions_for_ranges(
    meta: EditorMeta,
    params: CodeActionsParams,
    ctx: &mut Context,
    ranges: HashMap<ServerName, Range>,
) {
    let buff_diags = ctx.diagnostics.get(&meta.buffile);

    let mut diagnostics: HashMap<ServerName, Vec<Diagnostic>> = if let Some(buff_diags) = buff_diags
    {
        buff_diags
            .iter()
            .filter(|(server_name, d)| {
                ranges
                    .get(server_name)
                    .is_some_and(|r| ranges_overlap(d.range, *r))
            })
            .cloned()
            .fold(HashMap::new(), |mut m, v| {
                let (server_name, diagnostic) = v;
                m.entry(server_name).or_default().push(diagnostic);
                m
            })
    } else {
        HashMap::new()
    };

    let req_params = ranges
        .iter()
        .map(|(server_name, range)| {
            (
                server_name.clone(),
                vec![CodeActionParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    range: *range,
                    context: CodeActionContext {
                        diagnostics: diagnostics.remove(server_name).unwrap_or_default(),
                        only: params.only.as_ref().map(|only| {
                            only.split(' ')
                                .map(|s| CodeActionKind::from(s.to_string()))
                                .collect()
                        }),
                        trigger_kind: Some(if meta.hook {
                            CodeActionTriggerKind::AUTOMATIC
                        } else {
                            CodeActionTriggerKind::INVOKED
                        }),
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<CodeActionRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| editor_code_actions(meta, results, ctx, params, ranges),
    );
}

fn editor_code_actions(
    meta: EditorMeta,
    results: Vec<(ServerName, Option<CodeActionResponse>)>,
    ctx: &mut Context,
    params: CodeActionsParams,
    mut ranges: HashMap<ServerName, Range>,
) {
    if !meta.hook
        && results
            .iter()
            .all(|(server_name, result)| match ranges.get(server_name) {
                Some(range) => {
                    result == &Some(vec![])
                        && range.start.character != 0
                        && range.end.character != EOL_OFFSET
                }
                // Range is not registered for the language server,
                // so let's not let it influence in whether we should
                // reset the range and re-run code actions.
                None => true,
            })
    {
        // Some servers send code actions only if the requested range includes the affected
        // AST nodes.  Let's make them more convenient to access by requesting on whole lines.
        for range in ranges.values_mut() {
            range.start.character = 0;
            range.end.character = EOL_OFFSET;
        }
        code_actions_for_ranges(meta, params, ctx, ranges);
        return;
    }

    let mut actions: Vec<_> = results
        .into_iter()
        .flat_map(|(server_name, cmd)| {
            let cmd: Vec<_> = cmd
                .unwrap_or_default()
                .into_iter()
                .map(|cmd| (server_name.clone(), cmd))
                .collect();
            cmd
        })
        .collect();

    for (_, cmd) in &actions {
        match cmd {
            CodeActionOrCommand::Command(cmd) => info!("Command: {:?}", cmd),
            CodeActionOrCommand::CodeAction(action) => info!("Action: {:?}", action),
        }
    }

    let may_resolve: HashSet<_> = ranges
        .iter()
        .filter(|(server_name, _)| {
            let server_name = *server_name;
            let server_settings = &ctx.language_servers[server_name];

            attempt_server_capability(
                (server_name, server_settings),
                &meta,
                CAPABILITY_CODE_ACTIONS_RESOLVE,
            )
        })
        .map(|(server_name, _)| server_name)
        .collect();

    let sync = meta.fifo.is_some();
    if sync || params.code_action_pattern.is_some() {
        let actions = if let Some(pattern) = params.code_action_pattern.as_ref() {
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
            actions
                .into_iter()
                .filter(|(_, c)| {
                    let title = match c {
                        CodeActionOrCommand::Command(command) => &command.title,
                        CodeActionOrCommand::CodeAction(action) => &action.title,
                    };
                    regex.is_match(title)
                })
                .collect::<Vec<_>>()
        } else {
            actions
        };
        let fail = if sync {
            // We might be running from a hook, so let's allow silencing errors with a "try".
            // Also, prefix with the (presumable) function name, to reduce confusion.
            "fail lsp-code-actions:"
        } else {
            "lsp-show-error"
        }
        .to_string();
        let command = match actions.len() {
            0 => fail + " 'no matching action available'",
            1 => {
                let (server_name, cmd) = &actions[0];
                let may_resolve = may_resolve.contains(server_name);
                code_action_or_command_to_editor_command(cmd, sync, may_resolve)
            }
            _ => fail + " 'multiple matching actions'",
        };
        ctx.exec(
            meta,
            format!("evaluate-commands -- {}", &editor_quote(&command)),
        );
        return;
    }

    actions.sort_by_key(|(_server, ca)| {
        // TODO Group by server?
        let empty = CodeActionKind::EMPTY;
        let kind = match ca {
            CodeActionOrCommand::Command(_) => &empty,
            CodeActionOrCommand::CodeAction(action) => action.kind.as_ref().unwrap_or(&empty),
        };
        // TODO These loosely follow what VSCode does, we should be more accurate.
        match kind.as_str() {
            "quickfix" => 0,
            "refactor" => 1,
            "refactor.extract" => 2,
            "refactor.inline" => 3,
            "refactor.rewrite" => 4,
            "source" => 5,
            "source.organizeImports" => 6,
            _ => 7,
        }
    });
    let titles_and_commands = if params.auto_single {
        "-auto-single "
    } else {
        ""
    }
    .to_string()
        + &actions
            .iter()
            .map(|(server_name, c)| {
                let mut title: &str = match c {
                    CodeActionOrCommand::Command(command) => &command.title,
                    CodeActionOrCommand::CodeAction(action) => &action.title,
                };
                if let Some((head, _)) = title.split_once('\n') {
                    title = head
                }
                let may_resolve = may_resolve.contains(server_name);
                let select_cmd = code_action_or_command_to_editor_command(c, false, may_resolve);
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

fn code_action_or_command_to_editor_command(
    action: &CodeActionOrCommand,
    sync: bool,
    may_resolve: bool,
) -> String {
    match action {
        CodeActionOrCommand::Command(command) => execute_command_editor_command(command, sync),
        CodeActionOrCommand::CodeAction(action) => {
            code_action_to_editor_command(action, sync, may_resolve)
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
                command
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
    let req_params = serde_json::from_str(&params.code_action).unwrap();

    ctx.call::<CodeActionResolveRequest, _>(
        meta,
        RequestParams::All(vec![req_params]),
        move |ctx: &mut Context, meta, results| {
            if let Some((_, result)) = results.first() {
                let cmd = code_action_to_editor_command(result, false, false);
                ctx.exec(meta, format!("evaluate-commands -- {}", editor_quote(&cmd)))
            }
        },
    );
}
