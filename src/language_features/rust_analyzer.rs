use crate::context::Context;
use crate::position::{lsp_position_to_kakoune, lsp_range_to_kakoune};
use crate::types::{EditorMeta, EditorParams, KakounePosition};
use crate::util::{apply_text_edits, editor_quote};
use crate::workspace;
use lsp_types::request::Request;
use lsp_types::ExecuteCommandParams;
use lsp_types::InsertTextFormat;
use lsp_types::TextEdit;
use lsp_types::VersionedTextDocumentIdentifier;
use lsp_types::{Range, ResourceOp, TextDocumentIdentifier, TextDocumentPositionParams};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

pub enum InlayHints {}

impl Request for InlayHints {
    type Params = InlayHintsParams;
    type Result = Vec<InlayHint>;
    const METHOD: &'static str = "rust-analyzer/inlayHints";
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsParams {
    pub text_document: TextDocumentIdentifier,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum InlayKind {
    TypeHint,
    ParameterHint,
    ChainingHint,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InlayHint {
    pub range: Range,
    pub kind: InlayKind,
    pub label: String,
}

pub fn inlay_hints(meta: EditorMeta, _params: EditorParams, ctx: &mut Context) {
    let req_params = InlayHintsParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
    };
    ctx.call::<InlayHints, _>(meta, req_params, move |ctx, meta, response| {
        inlay_hints_response(meta, response, ctx)
    });
}

pub fn inlay_hints_response(meta: EditorMeta, inlay_hints: Vec<InlayHint>, ctx: &mut Context) {
    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => return,
    };
    let ranges = inlay_hints
        .into_iter()
        .map(|InlayHint { range, kind, label }| {
            let range = lsp_range_to_kakoune(&range, &document.text, &ctx.offset_encoding);
            let label = label.replace("|", "\\|");
            match kind {
                InlayKind::TypeHint => {
                    let pos = KakounePosition {
                        line: range.end.line,
                        column: range.end.column + 1,
                    };
                    editor_quote(&format!("{}+0|{{InlayHint}}{{\\}}: {}", pos, label))
                }
                InlayKind::ParameterHint => {
                    editor_quote(&format!("{}+0|{{InlayHint}}{{\\}}{}: ", range.start, label))
                }
                InlayKind::ChainingHint => {
                    let pos = KakounePosition {
                        line: range.end.line,
                        column: range.end.column + 1,
                    };
                    editor_quote(&format!("{}+0|{{InlayHint}}{{\\}} {}", pos, label))
                }
            }
        })
        .collect::<Vec<String>>()
        .join(" ");
    let command = format!(
        "set buffer rust_analyzer_inlay_hints {} {}",
        meta.version, &ranges
    );
    let command = format!(
        "eval -buffer {} {}",
        editor_quote(&meta.buffile),
        editor_quote(&command)
    );
    ctx.exec(meta, command.to_string())
}

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
        .nth(0)
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
                    workspace::apply_document_resource_op(&meta, resource_op, ctx);
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
                    apply_text_edits(&meta, &uri, &edits, ctx);
                }
            }
        }
    } else if let Some(changes) = changes {
        for (uri, change) in &changes {
            apply_text_edits(&meta, uri, change, ctx);
        }
    }
    match (&meta.client, &cursor_position) {
        (
            Some(client),
            Some(TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            }),
        ) => {
            let buffile = uri.to_file_path().unwrap();
            let buffile = buffile.to_str().unwrap();
            let position = match ctx.documents.get(buffile) {
                Some(document) => {
                    lsp_position_to_kakoune(position, &document.text, &ctx.offset_encoding)
                }
                _ => KakounePosition {
                    line: position.line + 1,
                    column: position.character + 1,
                },
            };
            let command = format!(
                "evaluate-commands -try-client %opt{{jumpclient}} %{{edit {} {} {}}}",
                editor_quote(buffile),
                position.line,
                position.column - 1
            );
            let command = format!(
                "eval -client {} {}",
                editor_quote(client),
                editor_quote(&command)
            );
            ctx.exec(meta, command.to_string());
        }
        _ => {}
    }
}
