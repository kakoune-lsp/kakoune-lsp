use crate::context::{Context, RequestParams};
use crate::position::get_lsp_position;
use crate::types::EditorMeta;
use crate::PositionParams;
use lsp_types::request::Request;
use lsp_types::TextDocumentIdentifier;
use lsp_types::TextDocumentPositionParams;
use lsp_types::Url;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::fmt;

pub enum ForwardSearch {}

impl Request for ForwardSearch {
    type Params = TextDocumentPositionParams;
    type Result = ForwardSearchResult;
    const METHOD: &'static str = "textDocument/forwardSearch";
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ForwardSearchResult {
    status: ForwardSearchStatus,
}

#[derive(Serialize_repr, Deserialize_repr, Debug)]
#[repr(i32)]
pub enum ForwardSearchStatus {
    Success = 0,
    Error = 1,
    Failure = 2,
    Unconfigured = 3,
}

impl fmt::Display for ForwardSearchResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Forward Search {:?} (texlab)", self.status)
    }
}

pub fn forward_search(meta: EditorMeta, params: PositionParams, ctx: &mut Context) {
    let req_params = ctx
        .servers(&meta)
        .map(|(server_id, server_settings)| {
            (
                server_id,
                vec![TextDocumentPositionParams {
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

    ctx.call::<ForwardSearch, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            if let Some((_, response)) = results.first() {
                forward_search_response(meta, response, ctx)
            }
        },
    );
}

pub fn forward_search_response(meta: EditorMeta, result: &ForwardSearchResult, ctx: &mut Context) {
    let command = format!("echo {}", result);
    ctx.exec(meta, command);
}

pub enum Build {}

impl Request for Build {
    type Params = BuildTextDocumentParams;
    type Result = BuildResult;
    const METHOD: &'static str = "textDocument/build";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BuildTextDocumentParams {
    text_document: TextDocumentIdentifier,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BuildResult {
    status: BuildStatus,
}

#[derive(Serialize_repr, Deserialize_repr, Debug)]
#[repr(i32)]
pub enum BuildStatus {
    Success = 0,
    Error = 1,
    Failure = 2,
    Cancelled = 3,
}

impl fmt::Display for BuildResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Build {:?} (texlab)", self.status)
    }
}

pub fn build(meta: EditorMeta, ctx: &mut Context) {
    let req_params = vec![BuildTextDocumentParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
    }];
    ctx.call::<Build, _>(
        meta,
        RequestParams::All(req_params),
        move |ctx, meta, results| {
            if let Some((_, response)) = results.first() {
                build_response(meta, response, ctx)
            }
        },
    );
}

pub fn build_response(meta: EditorMeta, result: &BuildResult, ctx: &mut Context) {
    let command = format!("echo {}", result);
    ctx.exec(meta, command);
}
