use crate::context::*;
use crate::controller::can_serve;
use crate::language_features::{document_symbol, rust_analyzer};
use crate::settings::*;
use crate::text_edit::apply_text_edits_try_deferred;
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

#[derive(Clone, Deserialize, Debug)]
pub struct EditorDidChangeConfigurationParams {
    #[deprecated]
    pub config: String,
    #[deprecated]
    pub server_configuration: Vec<String>,
}

pub fn did_change_configuration(
    meta: EditorMeta,
    params: EditorDidChangeConfigurationParams,
    ctx: &mut Context,
) {
    #[allow(deprecated)]
    record_dynamic_config(&meta, ctx, &params.config);

    for &server_id in &meta.servers {
        let server_name = &ctx.server(server_id).name;
        let settings = ctx
            .dynamic_config
            .language_server
            .get(server_name)
            .and_then(|lang| lang.settings.as_ref());
        let settings =
            configured_section(&meta, ctx, true, server_id, settings).unwrap_or_else(|| {
                #[allow(deprecated)]
                if !params.server_configuration.is_empty() {
                    Value::Object(explode_str_to_str_map(
                        ctx.to_editor(),
                        &params.server_configuration,
                    ))
                } else {
                    let server = ctx.server_config(&meta, server_name).unwrap();
                    configured_section(&meta, ctx, true, server_id, server.settings.as_ref())
                        .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
                }
            });

        let req_params = DidChangeConfigurationParams { settings };
        ctx.notify::<DidChangeConfiguration>(server_id, req_params);
    }
}

pub fn configuration(
    meta: EditorMeta,
    params: Params,
    server_id: ServerId,
    ctx: &mut Context,
) -> Result<Value, jsonrpc_core::Error> {
    let params = params.parse::<ConfigurationParams>()?;

    let server_name = &ctx.server(server_id).name;

    let settings = ctx
        .dynamic_config
        .language_server
        .get(server_name)
        .and_then(|cfg| cfg.settings.as_ref().cloned())
        .or_else(|| {
            if is_using_legacy_toml(&ctx.config) {
                ctx.server_config(&meta, server_name)
                    .and_then(|conf| conf.settings.as_ref().cloned())
            } else {
                ctx.server(server_id).settings.as_ref().cloned()
            }
        });

    let items = params
        .items
        .iter()
        .map(|item| {
            // There's also a `scopeUri`, which lists the file/folder
            // that the config should apply to. But kakoune-lsp doesn't
            // have a concept of per-file configuration and workspaces
            // are separated by kak-lsp processes.
            item.section
                .as_ref()
                // The specification isn't clear about whether you should
                // reply with just the value or with `json!({ section: <value> })`.
                // Tests indicate the former.
                .map(|section| match &settings {
                    None => Value::Null,
                    Some(settings) => {
                        if if is_using_legacy_toml(&ctx.config) {
                            ctx.server_config(&meta, server_name)
                                .is_some_and(|cfg| cfg.workaround_eslint == Some(true))
                        } else {
                            ctx.server(server_id).workaround_eslint
                        } && section.is_empty()
                        {
                            return settings.clone();
                        }
                        settings.get(section).unwrap_or(&Value::Null).clone()
                    }
                })
                .unwrap_or(Value::Null)
        })
        .collect::<Vec<Value>>();

    Ok(Value::Array(items))
}

pub fn workspace_symbol(meta: EditorMeta, params: WorkspaceSymbolParams, ctx: &mut Context) {
    ctx.call::<WorkspaceSymbolRequest, _>(
        meta,
        RequestParams::All(vec![params]),
        move |ctx, meta, results| {
            let result = match results.into_iter().find(|(_, v)| v.is_some()) {
                Some(result) => result,
                None => (meta.servers[0], None),
            };

            editor_workspace_symbol(meta, result, ctx)
        },
    );
}

impl document_symbol::Symbol<WorkspaceSymbol> for WorkspaceSymbol {
    fn name(&self) -> &str {
        &self.name
    }
    fn kind(&self) -> SymbolKind {
        self.kind
    }
    fn uri(&self) -> Option<&Url> {
        None
    }
    fn range(&self) -> Range {
        match &self.location {
            OneOf::Left(location) => location.range,
            OneOf::Right(_workspace_location) => {
                Range::new(Position::new(0, 0), Position::new(0, 0))
            }
        }
    }
    fn selection_range(&self) -> Range {
        self.range()
    }
    fn children(&self) -> &[WorkspaceSymbol] {
        &[]
    }
    fn children_mut(&mut self) -> &mut [WorkspaceSymbol] {
        &mut []
    }
}

fn editor_workspace_symbol(
    meta: EditorMeta,
    result: (ServerId, Option<WorkspaceSymbolResponse>),
    ctx: &mut Context,
) {
    let (server_id, result) = result;
    let server = ctx.server(server_id);
    let (content, _) = match result {
        Some(WorkspaceSymbolResponse::Flat(result)) => {
            if result.is_empty() {
                return;
            }
            document_symbol::format_symbol(result, None, &meta, server, ctx)
        }
        Some(WorkspaceSymbolResponse::Nested(result)) => {
            if result.is_empty() {
                return;
            }
            document_symbol::format_symbol(result, None, &meta, server, ctx)
        }
        None => {
            return;
        }
    };
    let command = format!(
        "lsp-show-workspace-symbol {} {}",
        editor_quote(ctx.main_root(&meta)),
        editor_quote(&content),
    );
    ctx.exec(meta, command);
}

#[derive(Deserialize)]
pub struct EditorExecuteCommand {
    pub command: String,
    pub arguments: String,
    pub server_name: Option<ServerName>,
}

pub fn execute_command(
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    params: EditorExecuteCommand,
    ctx: &mut Context,
) {
    let req_params = ExecuteCommandParams {
        command: params.command,
        // arguments is quoted to avoid parsing issues
        arguments: if params.arguments.is_empty() {
            vec![]
        } else {
            serde_json::from_str(&params.arguments).unwrap()
        },
        work_done_progress_params: Default::default(),
    };
    match &*req_params.command {
        "rust-analyzer.applySourceChange" => {
            rust_analyzer::apply_source_change(meta, response_fifo, req_params, ctx);
        }
        "rust-analyzer.runSingle" => {
            rust_analyzer::run_single(meta, response_fifo, req_params, ctx);
        }
        _ => {
            let params = if let Some(server_name) = params.server_name.as_ref() {
                let Some((server_id, _)) = ctx.servers(&meta).find(|(server_id, _)| {
                    can_serve(
                        ctx,
                        *server_id,
                        server_name,
                        &ctx.server_config(&meta, server_name).unwrap().root,
                    )
                }) else {
                    error!(
                        ctx.to_editor(),
                        "cannot find server for with name: {}", server_name
                    );
                    return;
                };
                RequestParams::Each(vec![(server_id, vec![req_params])].into_iter().collect())
            } else {
                RequestParams::All(vec![req_params])
            };
            ctx.call::<ExecuteCommand, _>(meta, params, move |_: &mut Context, _, _| {
                let _response_fifo = response_fifo;
            });
        }
    }
}

pub fn apply_document_resource_op(op: ResourceOp) -> io::Result<()> {
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
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
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
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(from, &to)
            }
        }
    }
}

// TODO handle version, so change is not applied if buffer is modified (and need to show a warning)
pub fn apply_edit(
    server_id: ServerId,
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    edit: WorkspaceEdit,
    ctx: &mut Context,
) -> ApplyWorkspaceEditResponse {
    let mut command = String::new();
    if let Some(document_changes) = edit.document_changes {
        match document_changes {
            DocumentChanges::Edits(edits) => {
                for edit in edits {
                    apply_text_edits_try_deferred(
                        &mut command,
                        server_id,
                        &meta,
                        edit.text_document.uri,
                        edit.edits,
                        ctx,
                    );
                }
            }
            DocumentChanges::Operations(ops) => {
                for op in ops {
                    match op {
                        DocumentChangeOperation::Edit(edit) => {
                            apply_text_edits_try_deferred(
                                &mut command,
                                server_id,
                                &meta,
                                edit.text_document.uri,
                                edit.edits,
                                ctx,
                            );
                        }
                        DocumentChangeOperation::Op(op) => {
                            if let Err(e) = apply_document_resource_op(op) {
                                error!(
                                    ctx.to_editor(),
                                    "failed to apply document change operation: {}", e
                                );
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
            apply_text_edits_try_deferred(&mut command, server_id, &meta, uri, change, ctx);
        }
    }
    if !command.is_empty() {
        ctx.exec_fifo(meta, response_fifo, command);
    }
    ApplyWorkspaceEditResponse {
        applied: true,
        failure_reason: None,
        failed_change: None,
    }
}

#[derive(Deserialize)]
pub struct EditorApplyEdit {
    pub edit: String,
}

pub fn apply_edit_from_editor(
    server_id: ServerId,
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    params: EditorApplyEdit,
    ctx: &mut Context,
) {
    let edit = WorkspaceEdit::deserialize(serde_json::from_str::<Value>(&params.edit).unwrap())
        .expect("Failed to parse edit");

    apply_edit(server_id, meta, response_fifo, edit, ctx);
}

pub fn apply_edit_from_server(
    meta: EditorMeta,
    server_id: ServerId,
    params: Params,
    ctx: &mut Context,
) -> Result<Value, jsonrpc_core::Error> {
    let params: ApplyWorkspaceEditParams = params.parse()?;
    let response = apply_edit(server_id, meta, None, params.edit, ctx);
    Ok(serde_json::to_value(response).unwrap())
}
