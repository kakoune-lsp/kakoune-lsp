use crate::context::Context;
use crate::position::lsp_position_to_kakoune;
use crate::text_edit::apply_text_edits;
use crate::types::{EditorMeta, KakounePosition};
use crate::util::editor_quote;
use crate::workspace;
use lsp_types::ExecuteCommandParams;
use lsp_types::InsertTextFormat;
use lsp_types::TextEdit;
use lsp_types::VersionedTextDocumentIdentifier;
use lsp_types::{Range, ResourceOp, TextDocumentIdentifier, TextDocumentPositionParams};
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

pub fn apply_source_change(meta: EditorMeta, params: ExecuteCommandParams, ctx: &mut Context) {
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
    if let Some(document_changes) = document_changes {
        for op in document_changes {
            match op {
                SnippetDocumentChangeOperation::Op(resource_op) => {
                    if let Err(e) = workspace::apply_document_resource_op(&meta, resource_op, ctx) {
                        error!("failed to apply document change: {}", e);
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
                    apply_text_edits(&meta, &uri, edits, ctx);
                }
            }
        }
    } else if let Some(changes) = changes {
        for (uri, change) in changes {
            apply_text_edits(&meta, &uri, change, ctx);
        }
    }
    if let (
        Some(client),
        Some(TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        }),
    ) = (&meta.client, &cursor_position)
    {
        let buffile = uri.to_file_path().unwrap();
        let buffile = buffile.to_str().unwrap();
        let position = match ctx.documents.get(buffile) {
            Some(document) => {
                lsp_position_to_kakoune(position, &document.text, ctx.offset_encoding)
            }
            _ => KakounePosition {
                line: position.line + 1,
                column: position.character + 1,
            },
        };
        let command = format!(
            "eval -try-client %opt{{jumpclient}} -verbatim -- edit -existing {} {} {}",
            editor_quote(buffile),
            position.line,
            position.column - 1
        );
        let command = format!(
            "eval -client {} -verbatim -- {}",
            editor_quote(client),
            command
        );
        ctx.exec(meta, command);
    }
}
