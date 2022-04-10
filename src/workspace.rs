use crate::context::*;
use crate::language_features::{document_symbol, rust_analyzer};
use crate::settings::*;
use crate::text_edit::{apply_annotated_text_edits, apply_text_edits};
use crate::types::*;
use crate::util::*;
use jsonrpc_core::Params;
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use serde_json::{self, Value};
use std::fs;
use std::io;

pub fn did_change_configuration(meta: EditorMeta, mut params: EditorParams, ctx: &mut Context) {
    let mut default_settings = toml::value::Table::new();

    let raw_settings = params
        .as_table_mut()
        .and_then(|t| t.get_mut("settings"))
        .and_then(|t| t.as_table_mut())
        .unwrap_or(&mut default_settings);

    let config_param = raw_settings.remove("lsp_config");
    let config = config_param
        .as_ref()
        .map(|config| {
            config
                .as_str()
                .expect("Parameter \"lsp_config\" must be a string")
        })
        .unwrap_or("");

    record_dynamic_config(&meta, ctx, config);

    let settings = ctx
        .dynamic_config
        .language
        .get(&ctx.language_id)
        .and_then(|lang| lang.settings.as_ref());
    let settings = configured_section(ctx, settings).unwrap_or_else(|| {
        if !raw_settings.is_empty() {
            Value::Object(explode_string_table(raw_settings))
        } else {
            let language = ctx.config.language.get(&ctx.language_id).unwrap();
            configured_section(ctx, language.settings.as_ref()).unwrap_or_default()
        }
    });

    let params = DidChangeConfigurationParams { settings };
    ctx.notify::<DidChangeConfiguration>(params);
}

pub fn configuration(params: Params, ctx: &mut Context) -> Result<Value, jsonrpc_core::Error> {
    let params = params.parse::<ConfigurationParams>()?;

    let settings = ctx
        .dynamic_config
        .language
        .get(&ctx.language_id)
        .and_then(|cfg| cfg.settings.as_ref().cloned())
        .or_else(|| {
            ctx.config
                .language
                .get(&ctx.language_id)
                .and_then(|conf| conf.settings.as_ref().cloned())
        });

    let items = params
        .items
        .iter()
        .map(|item| {
            // There's also a `scopeUri`, which lists the file/folder
            // that the config should apply to. But kak-lsp doesn't
            // have a concept of per-file configuration and workspaces
            // are separated by kak-lsp processes.
            item.section
                .as_ref()
                // The specification isn't clear about whether you should
                // reply with just the value or with `json!({ section: <value> })`.
                // Tests indicate the former.
                .map(|section| match &settings {
                    None => Value::Null,
                    Some(settings) => settings.get(section).unwrap_or(&Value::Null).clone(),
                })
                .unwrap_or(Value::Null)
        })
        .collect::<Vec<Value>>();

    Ok(Value::Array(items))
}

pub fn workspace_symbol(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = WorkspaceSymbolParams::deserialize(params)
        .expect("Params should follow WorkspaceSymbolParams structure");
    ctx.call::<WorkspaceSymbol, _>(meta, params, move |ctx: &mut Context, meta, result| {
        editor_workspace_symbol(meta, result, ctx)
    });
}

pub fn editor_workspace_symbol(
    meta: EditorMeta,
    result: Option<Vec<SymbolInformation>>,
    ctx: &mut Context,
) {
    if result.is_none() {
        return;
    }
    let result = result.unwrap();
    let content = document_symbol::format_symbol(result, &meta, ctx);
    let command = format!(
        "lsp-show-workspace-symbol {} {}",
        editor_quote(&ctx.root_path),
        editor_quote(&content),
    );
    ctx.exec(meta, command);
}

#[derive(Deserialize)]
struct EditorExecuteCommand {
    command: String,
    arguments: String,
}

pub fn execute_command(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = EditorExecuteCommand::deserialize(params)
        .expect("Params should follow ExecuteCommand structure");
    let req_params = ExecuteCommandParams {
        command: params.command,
        // arguments is quoted to avoid parsing issues
        arguments: serde_json::from_str(&params.arguments).unwrap(),
        work_done_progress_params: Default::default(),
    };
    match &*req_params.command {
        "rust-analyzer.applySourceChange" => {
            rust_analyzer::apply_source_change(meta, req_params, ctx);
        }
        _ => {
            ctx.call::<ExecuteCommand, _>(meta, req_params, move |_: &mut Context, _, _| ());
        }
    }
}

pub fn apply_document_resource_op(
    _meta: &EditorMeta,
    op: ResourceOp,
    _ctx: &mut Context,
) -> io::Result<()> {
    match op {
        ResourceOp::Create(op) => {
            let path = op.uri.to_file_path().unwrap();
            let ignore_if_exists = if let Some(options) = op.options {
                !options.overwrite.unwrap_or(false) && options.ignore_if_exists.unwrap_or(false)
            } else {
                false
            };
            if ignore_if_exists && path.exists() {
                Ok(())
            } else {
                fs::write(&path, [])
            }
        }
        ResourceOp::Delete(op) => {
            let path = op.uri.to_file_path().unwrap();
            if path.is_dir() {
                let recursive = if let Some(options) = op.options {
                    options.recursive.unwrap_or(false)
                } else {
                    false
                };
                if recursive {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_dir(&path)
                }
            } else if path.is_file() {
                fs::remove_file(&path)
            } else {
                Ok(())
            }
        }
        ResourceOp::Rename(op) => {
            let from = op.old_uri.to_file_path().unwrap();
            let to = op.new_uri.to_file_path().unwrap();
            let ignore_if_exists = if let Some(options) = op.options {
                !options.overwrite.unwrap_or(false) && options.ignore_if_exists.unwrap_or(false)
            } else {
                false
            };
            if ignore_if_exists && to.exists() {
                Ok(())
            } else {
                fs::rename(&from, &to)
            }
        }
    }
}

// TODO handle version, so change is not applied if buffer is modified (and need to show a warning)
pub fn apply_edit(
    meta: EditorMeta,
    edit: WorkspaceEdit,
    ctx: &mut Context,
) -> ApplyWorkspaceEditResponse {
    if let Some(document_changes) = edit.document_changes {
        match document_changes {
            DocumentChanges::Edits(edits) => {
                for edit in edits {
                    apply_annotated_text_edits(&meta, &edit.text_document.uri, edit.edits, ctx);
                }
            }
            DocumentChanges::Operations(ops) => {
                for op in ops {
                    match op {
                        DocumentChangeOperation::Edit(edit) => {
                            apply_annotated_text_edits(
                                &meta,
                                &edit.text_document.uri,
                                edit.edits,
                                ctx,
                            );
                        }
                        DocumentChangeOperation::Op(op) => {
                            if let Err(e) = apply_document_resource_op(&meta, op, ctx) {
                                error!("failed to apply document change operation: {}", e);
                                return ApplyWorkspaceEditResponse {
                                    applied: false,
                                    failure_reason: None,
                                    failed_change: None,
                                };
                            }
                        }
                    }
                }
            }
        }
    } else if let Some(changes) = edit.changes {
        for (uri, change) in changes {
            apply_text_edits(&meta, &uri, change, ctx);
        }
    }
    ApplyWorkspaceEditResponse {
        applied: true,
        failure_reason: None,
        failed_change: None,
    }
}

#[derive(Deserialize)]
struct EditorApplyEdit {
    edit: String,
}

pub fn apply_edit_from_editor(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = EditorApplyEdit::deserialize(params).expect("Failed to parse params");
    let edit = WorkspaceEdit::deserialize(serde_json::from_str::<Value>(&params.edit).unwrap())
        .expect("Failed to parse edit");

    apply_edit(meta, edit, ctx);
}

pub fn apply_edit_from_server(
    params: Params,
    ctx: &mut Context,
) -> Result<Value, jsonrpc_core::Error> {
    let params: ApplyWorkspaceEditParams = params.parse()?;
    let meta = ctx.meta_for_session(None);
    let response = apply_edit(meta, params.edit, ctx);
    Ok(serde_json::to_value(response).unwrap())
}
