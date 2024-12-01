use crate::context::{Context, RequestParams};
use crate::position::{get_lsp_position, lsp_position_to_kakoune};
use crate::text_edit::apply_text_edits_try_deferred;
use crate::types::{EditorMeta, KakounePosition, PositionParams};
use crate::util::{editor_escape, editor_quote};
use crate::{workspace, ResponseFifo};
use itertools::Itertools;
use lsp_types::request::Request;
use lsp_types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SourceChange {
    pub label: String,
    pub workspace_edit: SnippetWorkspaceEdit,
    pub cursor_position: Option<lsp_types::TextDocumentPositionParams>,
}

#[derive(Debug, Eq, PartialEq, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetWorkspaceEdit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<HashMap<Url, Vec<TextEdit>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_changes: Option<Vec<SnippetDocumentChangeOperation>>,
}

#[derive(Debug, Eq, PartialEq, Clone, Deserialize, Serialize)]
#[serde(untagged, rename_all = "lowercase")]
pub enum SnippetDocumentChangeOperation {
    Op(ResourceOp),
    Edit(SnippetTextDocumentEdit),
}

#[derive(Debug, Eq, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetTextDocumentEdit {
    pub text_document: VersionedTextDocumentIdentifier,
    pub edits: Vec<SnippetTextEdit>,
}

#[derive(Debug, Eq, PartialEq, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetTextEdit {
    pub range: Range,
    pub new_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text_format: Option<InsertTextFormat>,
}

pub fn apply_source_change(
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    params: ExecuteCommandParams,
    ctx: &mut Context,
) {
    let arg = params
        .arguments
        .into_iter()
        .next()
        .expect("Missing source change");
    let SourceChange {
        workspace_edit:
            SnippetWorkspaceEdit {
                changes,
                document_changes,
            },
        cursor_position,
        ..
    } = serde_json::from_value(arg).expect("Invalid source change");

    let server_id = meta.servers[0];
    let mut command = String::new();
    if let Some(document_changes) = document_changes {
        for op in document_changes {
            match op {
                SnippetDocumentChangeOperation::Op(resource_op) => {
                    if let Err(e) = workspace::apply_document_resource_op(resource_op) {
                        error!(ctx.to_editor(), "failed to apply document change: {}", e);
                    }
                }
                SnippetDocumentChangeOperation::Edit(SnippetTextDocumentEdit {
                    text_document: VersionedTextDocumentIdentifier { uri, .. },
                    edits,
                }) => {
                    let edits: Vec<TextEdit> = edits
                        .into_iter()
                        .map(
                            |SnippetTextEdit {
                                 range,
                                 new_text,
                                 insert_text_format: _, // TODO
                             }| TextEdit { range, new_text },
                        )
                        .collect();
                    apply_text_edits_try_deferred(&mut command, server_id, &meta, uri, edits, ctx);
                }
            }
        }
    } else if let Some(changes) = changes {
        for (uri, change) in changes {
            apply_text_edits_try_deferred(&mut command, server_id, &meta, uri, change, ctx);
        }
    }
    let (
        Some(client),
        Some(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        }),
    ) = (&meta.client, &cursor_position)
    else {
        if !command.is_empty() {
            ctx.exec_fifo(meta, response_fifo, command);
        }
        return;
    };
    let buffile = uri.to_file_path().unwrap();
    let buffile = buffile.to_str().unwrap();
    let position = match ctx.documents.get(buffile) {
        Some(document) => {
            let server = ctx.server(server_id);
            lsp_position_to_kakoune(position, &document.text, server.offset_encoding)
        }
        _ => KakounePosition {
            line: position.line + 1,
            column: position.character + 1,
        },
    };
    let goto_command = format!(
        "evaluate-commands -try-client %opt{{jumpclient}} -verbatim -- edit -existing {} {} {}",
        editor_quote(buffile),
        position.line,
        position.column - 1
    );
    let command = format!(
        "{command}; evaluate-commands -client {} -verbatim -- {}",
        editor_quote(client),
        goto_command
    );
    ctx.exec_fifo(meta, response_fifo, command);
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExpandMacroParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExpandMacroResponse {
    pub name: String,
    pub expansion: String,
}

pub struct ExpandMacroRequest {}

impl Request for ExpandMacroRequest {
    type Params = ExpandMacroParams;
    type Result = ExpandMacroResponse;
    const METHOD: &'static str = "rust-analyzer/expandMacro";
}

pub fn expand_macro(meta: EditorMeta, params: PositionParams, ctx: &mut Context) {
    let req_params = ctx
        .servers(&meta)
        .map(|(server_id, server_settings)| {
            (
                server_id,
                vec![ExpandMacroParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    position: get_lsp_position(
                        server_settings,
                        &meta.buffile,
                        &params.position,
                        ctx,
                    )
                    .unwrap(),
                }],
            )
        })
        .collect();

    ctx.call::<ExpandMacroRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            if let Some((_, result)) = results.first() {
                editor_expand_macro(meta, result, ctx);
            }
        },
    );
}

fn editor_expand_macro(meta: EditorMeta, result: &ExpandMacroResponse, ctx: &mut Context) {
    let command = format!(
        "info 'expansion of {}!\n\n{}'",
        editor_escape(&result.name),
        editor_escape(&result.expansion)
    );
    ctx.exec(meta, command);
}

#[derive(Debug, Eq, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunSingleArgument {
    args: RunSingleArgs,
    kind: String,
    label: String,
}

#[derive(Debug, Eq, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunSingleArgs {
    cargo_args: Vec<String>,
    #[serde(default)]
    cargo_extra_args: Vec<String>,
    executable_args: Vec<String>,
    override_cargo: Option<bool>,
    workspace_root: String,
}

pub fn run_single(
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    mut params: ExecuteCommandParams,
    ctx: &mut Context,
) {
    if params.arguments.len() != 1 {
        error!(
            ctx.to_editor(),
            "Unsupported number of runSingle arguments: {}",
            params.arguments.len()
        );
        return;
    }
    let argument = params.arguments.drain(..).next().unwrap();
    let argument: RunSingleArgument = serde_json::from_value(argument).unwrap();

    if argument.kind != "cargo" {
        error!(
            ctx.to_editor(),
            "Unsupported runSingle kind: {}", argument.kind
        );
        return;
    }

    let mut args = vec!["cargo".to_string()];
    args.extend(argument.args.cargo_args);
    args.extend(argument.args.cargo_extra_args);
    args.push("--".to_string());
    args.extend(argument.args.executable_args);

    let args = args.into_iter().map(|arg| editor_quote(&arg)).join(" ");
    let cmd = format!(
        "set-register : {}; execute-keys -client {} :<c-p>",
        editor_quote(&args),
        meta.client.as_ref().unwrap()
    );
    ctx.exec_fifo(meta, response_fifo, cmd);
}
