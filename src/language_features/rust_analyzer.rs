use crate::context::Context;
use crate::position::lsp_range_to_kakoune;
use crate::types::{EditorMeta, EditorParams, KakounePosition, KakouneRange};
use crate::util::editor_quote;
use lsp_types::request::Request;
use lsp_types::{Range, TextDocumentIdentifier};
use serde::{Deserialize, Serialize};
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
                    let range = KakouneRange {
                        start: pos.clone(),
                        end: pos,
                    };
                    editor_quote(&format!("{}|{{default+di}}{{\\}}: {}", range, label))
                }
                InlayKind::ParameterHint => {
                    let range = KakouneRange {
                        start: range.start.clone(),
                        end: range.start,
                    };
                    editor_quote(&format!("{}|{{default+di}}{{\\}}{}: ", range, label))
                }
                InlayKind::ChainingHint => {
                    let pos = KakounePosition {
                        line: range.end.line,
                        column: range.end.column + 1,
                    };
                    let range = KakouneRange {
                        start: pos.clone(),
                        end: pos,
                    };
                    editor_quote(&format!("{}|{{default+di}}{{\\}} {}", range, label))
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
