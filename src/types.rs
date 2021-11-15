use jsonrpc_core::{Call, Output, Params};
use lsp_types::{DiagnosticSeverity, Position, Range, SemanticTokenModifier};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Display;
use std::io::Error;

pub enum Void {}

// Configuration

#[derive(Clone, Deserialize, Debug)]
pub struct Config {
    pub language: HashMap<String, LanguageConfig>,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub verbosity: u8,
    #[serde(default)]
    pub snippet_support: bool,
    #[serde(default, deserialize_with = "deserialize_semantic_tokens")]
    pub semantic_tokens: Vec<SemanticTokenConfig>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct DynamicConfig {
    #[serde(default)]
    pub language: HashMap<String, DynamicLanguageConfig>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct ServerConfig {
    #[serde(default)]
    pub session: String,
    #[serde(default)]
    pub timeout: u64,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct LanguageConfig {
    pub filetypes: Vec<String>,
    pub roots: Vec<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub settings_section: Option<String>,
    pub settings: Option<Value>,
    #[serde(default = "default_offset_encoding")]
    pub offset_encoding: OffsetEncoding,
    pub workaround_server_sends_plaintext_labeled_as_markdown: Option<bool>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct DynamicLanguageConfig {
    pub settings: Option<Value>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            session: String::new(),
            timeout: 0,
        }
    }
}

fn default_offset_encoding() -> OffsetEncoding {
    OffsetEncoding::Utf16
}

#[derive(Clone, Deserialize, Debug)]
pub struct SemanticTokenConfig {
    pub token: String,
    pub face: String,
    #[serde(default)]
    pub modifiers: Vec<SemanticTokenModifier>,
}

fn deserialize_semantic_tokens<'de, D>(
    deserializer: D,
) -> Result<Vec<SemanticTokenConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::deserialize(deserializer).map_err(|e| {
        D::Error::custom(e.to_string() + "\nSee https://github.com/kak-lsp/kak-lsp#semantic-tokens for the new configuration syntax for semantic tokens\n")
    })
}

// Editor

#[derive(Clone, Debug, Deserialize)]
pub struct EditorMeta {
    pub session: String,
    pub client: Option<String>,
    pub buffile: String,
    pub filetype: String,
    pub version: i32,
    pub fifo: Option<String>,
}

pub type EditorParams = toml::Value;

#[derive(Clone, Debug, Deserialize)]
pub struct EditorRequest {
    #[serde(flatten)]
    pub meta: EditorMeta,
    pub method: String,
    pub params: EditorParams,
    pub ranges: Option<Vec<Range>>,
}

#[derive(Deserialize)]
pub struct EditorResponse {
    pub meta: EditorMeta,
    pub command: Cow<'static, str>,
}

pub type SessionId = String;
pub type LanguageId = String;
pub type RootPath = String;

#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub struct Route {
    pub session: SessionId,
    pub language: LanguageId,
    pub root: RootPath,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EditorCompletion {
    pub offset: u32,
}

#[derive(Deserialize, Debug)]
pub struct TextDocumentDidOpenParams {
    pub draft: String,
}

#[derive(Deserialize, Debug)]
pub struct TextDocumentDidChangeParams {
    pub draft: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TextDocumentCompletionParams {
    pub position: KakounePosition,
    pub completion: EditorCompletion,
}

#[derive(Clone, Copy, Deserialize, Debug)]
pub struct PositionParams {
    pub position: KakounePosition,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHierarchyParams {
    pub position: KakounePosition,
    pub incoming_or_outgoing: bool,
}

#[derive(Clone, Copy, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CodeActionsParams {
    pub position: KakounePosition,
    pub perform_code_action: bool,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NextOrPrevSymbolParams {
    pub position: KakounePosition,
    pub symbol_kind: String,
    /// If true then searches forward ("next")
    /// otherwise searches backward ("previous")
    pub search_next: bool,
    /// If true, don't navigate to the next/previous symbol but show its hover
    /// otherwise goto the next/previous symbol
    #[serde(default)]
    pub hover: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentRenameParams {
    pub position: KakounePosition,
    pub new_name: String,
}

#[derive(Deserialize, Debug)]
pub struct WindowProgress {
    pub title: String,
    pub message: Option<String>,
    pub percentage: Option<String>,
    pub done: Option<bool>,
}

// Language Server

// XXX serde(untagged) ?
#[derive(Debug)]
pub enum ServerMessage {
    Request(Call),
    Response(Output),
}

pub trait IntoParams {
    fn into_params(self) -> Result<Params, Error>;
}

impl<T> IntoParams for T
where
    T: Serialize,
{
    fn into_params(self) -> Result<Params, Error> {
        let json_value = serde_json::to_value(self)?;

        let params = match json_value {
            Value::Null => Params::None,
            Value::Bool(_) | Value::Number(_) | Value::String(_) => Params::Array(vec![json_value]),
            Value::Array(vec) => Params::Array(vec),
            Value::Object(map) => Params::Map(map),
        };

        Ok(params)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct KakounePosition {
    pub line: u32,
    pub column: u32, // in bytes, not chars!!!
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoverModal {
    pub context: String,
    pub do_after: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoverModalParams {
    pub hover_modal: Option<HoverModal>,
}

#[derive(Debug, PartialEq)]
pub struct KakouneRange {
    pub start: KakounePosition,
    pub end: KakounePosition,
}

impl Display for KakounePosition {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}.{}", self.line, self.column)
    }
}

impl Display for KakouneRange {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{},{}", self.start, self.end)
    }
}

/// Represents how language server interprets LSP's `Position.character`
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum OffsetEncoding {
    /// UTF-8 code units aka bytes
    #[serde(rename = "utf-8")]
    Utf8,
    /// UTF-16 code units
    #[serde(rename = "utf-16")]
    Utf16,
}

// An intermediate representation of the diagnostics on a line, for use with inlay diagnostics
pub struct LineDiagnostics {
    pub range_end: Position,
    pub symbols: String,
    pub text: String,
    pub text_face: &'static str,
    pub text_severity: Option<DiagnosticSeverity>,
}
