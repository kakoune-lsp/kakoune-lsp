use jsonrpc_core::{Call, Output, Params};
use lsp_types::{DiagnosticSeverity, Position, Range, SemanticTokenModifier};
use serde::de::{MapAccess, SeqAccess, Visitor};
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
    #[serde(default)]
    pub semantic_tokens: SemanticTokenConfig,
}

#[derive(Clone, Default, Deserialize, Debug)]
pub struct DynamicConfig {
    #[serde(default)]
    pub language: HashMap<String, DynamicLanguageConfig>,
}

#[derive(Clone, Default, Deserialize, Debug)]
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
    #[serde(default)]
    pub envs: HashMap<String, String>,
    pub settings_section: Option<String>,
    pub settings: Option<Value>,
    pub offset_encoding: Option<OffsetEncoding>,
    // This does nothing, but is kept so we can still parse old configs.
    pub workaround_server_sends_plaintext_labeled_as_markdown: Option<bool>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct DynamicLanguageConfig {
    pub settings: Option<Value>,
}

#[derive(Clone, Default, Debug)]
pub struct SemanticTokenConfig {
    pub faces: Vec<SemanticTokenFace>,
}

impl<'de> Deserialize<'de> for SemanticTokenConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SemanticTokenConfigVisitor;

        impl<'de> Visitor<'de> for SemanticTokenConfigVisitor {
            type Value = SemanticTokenConfig;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("A valid semantic-tokens configuration. See https://github.com/kak-lsp/kak-lsp#semantic-tokens for the new configuration syntax for semantic tokens")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut faces = None;
                while let Some(k) = map.next_key()? {
                    match k {
                        "faces" => faces = Some(map.next_value()?),
                        _ => return Err(A::Error::unknown_field(k, &["faces"])),
                    }
                }
                let faces = faces.ok_or_else(|| A::Error::missing_field("faces"))?;
                Ok(SemanticTokenConfig { faces })
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut faces = vec![];
                while let Some(face) = seq.next_element::<SemanticTokenFace>()? {
                    faces.push(face)
                }
                Ok(SemanticTokenConfig { faces })
            }
        }

        deserializer.deserialize_any(SemanticTokenConfigVisitor)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SemanticTokenFace {
    pub face: String,
    pub token: String,
    #[serde(default)]
    pub modifiers: Vec<SemanticTokenModifier>,
}

// Editor

#[derive(Clone, Debug, Default, Deserialize)]
pub struct EditorMeta {
    pub session: String,
    pub client: Option<String>,
    pub buffile: String,
    pub filetype: String,
    pub version: i32,
    pub fifo: Option<String>,
    pub command_fifo: Option<String>,
    #[serde(default)]
    pub write_response_to_fifo: bool,
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
    pub have_kakoune_feature_filtertext: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CompletionItemResolveParams {
    pub completion_item_index: isize,
}

#[derive(Clone, Copy, Deserialize, Debug)]
pub struct PositionParams {
    pub position: KakounePosition,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HoverDetails {
    pub hover_fifo: Option<String>,
    pub hover_client: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHierarchyParams {
    pub position: KakounePosition,
    pub incoming_or_outgoing: bool,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CodeActionsParams {
    pub selection_desc: String,
    pub perform_code_action: bool,
    pub code_action_pattern: Option<String>,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NextOrPrevSymbolParams {
    pub position: KakounePosition,
    /// Match any of these kinds of symbols, or any symbol if empty.
    pub symbol_kinds: Vec<String>,
    /// If true then searches forward ("next")
    /// otherwise searches backward ("previous")
    pub search_next: bool,
    /// If true, don't navigate to the next/previous symbol but show its hover
    /// otherwise goto the next/previous symbol
    #[serde(default)]
    pub hover: bool,
}

#[derive(Clone, Deserialize, Debug)]
pub struct ObjectParams {
    pub count: u32,
    pub mode: String,
    pub position: KakounePosition,
    pub selections_desc: String,
    pub symbol_kinds: Vec<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentRenameParams {
    pub position: KakounePosition,
    pub new_name: String,
}

#[derive(Clone, Deserialize, Debug)]
pub struct SelectionRangePositionParams {
    // The cursor position.
    pub position: KakounePosition,
    // The ranges of all Kakoune selections.
    pub selections_desc: String,
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

#[derive(PartialEq)]
pub enum HoverType {
    InfoBox,
    Modal {
        modal_heading: String,
        do_after: String,
    },
    HoverBuffer {
        fifo: String,
        client: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
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
