use crate::context::*;
use crate::types::*;
use crate::util::*;
use jsonrpc_core::{Id, Params};
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use serde_json::{self, Value};
use std::fs;
use toml;

fn insert_value<'a, 'b, P>(
    target: &'b mut serde_json::map::Map<String, Value>,
    mut path: P,
    local_key: String,
    value: Value,
) -> Result<(), String>
where
    P: Iterator<Item = &'a str>,
    P: 'a,
{
    match path.next() {
        Some(key) => {
            let maybe_new_target = target
                .entry(key)
                .or_insert_with(|| Value::Object(serde_json::Map::new()))
                .as_object_mut();

            if maybe_new_target.is_none() {
                return Err(format!(
                    "Expected path {:?} to be object, found {:?}",
                    key, &maybe_new_target,
                ));
            }

            insert_value(maybe_new_target.unwrap(), path, local_key, value)
        }
        None => match target.insert(local_key, value) {
            Some(old_value) => Err(format!("Replaced old value: {:?}", old_value)),
            None => Ok(()),
        },
    }
}

pub fn did_change_configuration(params: EditorParams, ctx: &mut Context) {
    let default_settings = toml::value::Table::new();

    let raw_settings = params
        .as_table()
        .and_then(|t| t.get("settings"))
        .and_then(|val| val.as_table())
        .unwrap_or_else(|| &default_settings);

    let mut settings = serde_json::Map::new();

    for (raw_key, raw_value) in raw_settings.iter() {
        let mut key_parts = raw_key.split('.');
        let local_key = match key_parts.next_back() {
            Some(name) => name,
            None => {
                warn!("Got a setting with an empty local name: {:?}", raw_key);
                continue;
            }
        };

        let value: Value = match raw_value.clone().try_into() {
            Ok(value) => value,
            Err(e) => {
                warn!("Could not convert setting {:?} to JSON: {}", raw_value, e,);
                continue;
            }
        };

        match insert_value(&mut settings, key_parts, local_key.into(), value) {
            Ok(_) => (),
            Err(e) => {
                warn!("Could not set {:?} to {:?}: {}", raw_key, raw_value, e);
                continue;
            }
        }
    }

    let params = DidChangeConfigurationParams {
        settings: Value::Object(settings),
    };
    ctx.notify::<DidChangeConfiguration>(params);
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
    let content = format_symbol_information(result, ctx);
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
    };
    ctx.call::<ExecuteCommand, _>(meta, req_params, move |_: &mut Context, _, _| ());
}


// TODO handle version, so change is not applied if buffer is modified (and need to show a warning)
pub fn apply_edit(meta: EditorMeta, edit: WorkspaceEdit, ctx: &mut Context) -> ApplyWorkspaceEditResponse {
    let mut applied = true;
    if let Some(document_changes) = edit.document_changes {
        match document_changes {
            DocumentChanges::Edits(edits) => {
                for edit in edits {
                    apply_text_edits(&meta, &edit.text_document.uri, &edit.edits, ctx);
                }
            }
            DocumentChanges::Operations(ops) => {
                for op in ops {
                    match op {
                        DocumentChangeOperation::Edit(edit) => {
                            apply_text_edits(&meta, &edit.text_document.uri, &edit.edits, ctx);
                        }
                        DocumentChangeOperation::Op(op) => match op {
                            ResourceOp::Create(op) => {
                                let path = op.uri.to_file_path().unwrap();
                                let ignore_if_exists = if let Some(options) = op.options {
                                    !options.overwrite.unwrap_or(false)
                                        && options.ignore_if_exists.unwrap_or(false)
                                } else {
                                    false
                                };
                                if !(ignore_if_exists && path.exists())
                                    && fs::write(&path, []).is_err()
                                {
                                    error!(
                                        "Failed to create file: {}",
                                        path.to_str().unwrap_or("")
                                    );
                                    applied = true;
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
                                        if fs::remove_dir_all(&path).is_err() {
                                            error!(
                                                "Failed to delete directory: {}",
                                                path.to_str().unwrap_or("")
                                            );
                                    applied = true;
                                        }
                                    } else if fs::remove_dir(&path).is_err() {
                                        error!(
                                            "Failed to delete directory: {}",
                                            path.to_str().unwrap_or("")
                                        );
                                    applied = true;
                                    }
                                } else if path.is_file() && fs::remove_file(&path).is_err() {
                                    error!(
                                        "Failed to delete file: {}",
                                        path.to_str().unwrap_or("")
                                    );
                                    applied = true;
                                }
                            }
                            ResourceOp::Rename(op) => {
                                let from = op.old_uri.to_file_path().unwrap();
                                let to = op.new_uri.to_file_path().unwrap();
                                let ignore_if_exists = if let Some(options) = op.options {
                                    !options.overwrite.unwrap_or(false)
                                        && options.ignore_if_exists.unwrap_or(false)
                                } else {
                                    false
                                };
                                if !(ignore_if_exists && to.exists())
                                    && fs::rename(&from, &to).is_err()
                                {
                                    error!(
                                        "Failed to rename file: {} -> {}",
                                        from.to_str().unwrap_or(""),
                                        to.to_str().unwrap_or("")
                                    );
                                    applied = true;
                                }
                            }
                        },
                    }
                }
            }
        }
    } else if let Some(changes) = edit.changes {
        for (uri, change) in &changes {
            apply_text_edits(&meta, uri, change, ctx);
        }
    }
    return ApplyWorkspaceEditResponse { applied };
}

#[derive(Deserialize)]
struct EditorApplyEdit {
    edit: String,
}

pub fn apply_edit_from_editor(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = EditorApplyEdit::deserialize(params).expect("Failed to parse params");
    let edit = WorkspaceEdit::deserialize(serde_json::from_str::<Value>(&params.edit).unwrap()).expect("Failed to parse edit");

    apply_edit(meta, edit, ctx);
}

pub fn apply_edit_from_server(id: Id, params: Params, ctx: &mut Context) {
    let params: ApplyWorkspaceEditParams = params.parse().expect("Failed to parse params");
    let meta = ctx.meta_for_session();
    let response = apply_edit(meta, params.edit, ctx);
    ctx.reply(
        id,
        Ok(serde_json::to_value(response).unwrap()),
    );
}
