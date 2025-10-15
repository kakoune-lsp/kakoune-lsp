use jsonrpc_core::{Call, Output, Params};
use libc::{ENXIO, O_NONBLOCK};
use lsp_types::{
    CodeActionKind, DiagnosticSeverity, FormattingOptions, Position, SemanticTokenModifier,
};
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::any::Any;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Display;
use std::io::{Error, Write};
use std::ops::Deref;
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;
use std::{fs, io};

pub enum Void {}

// Configuration

const fn default_true() -> bool {
    true
}

#[derive(Clone, Default, Deserialize, Debug)]
pub struct Config {
    #[deprecated(note = "use EditorMeta::language_server")]
    #[serde(default)]
    pub language_server: HashMap<ServerName, LanguageServerConfig>,
    #[deprecated(note = "use language_server")]
    #[serde(default)]
    pub language: HashMap<LanguageId, LanguageServerConfig>,
    #[serde(default)]
    pub server: ServerConfig,
    #[deprecated(note = "use -v argument")]
    #[serde(default)]
    pub verbosity: u8,
    #[serde(default = "default_true")]
    pub snippet_support: bool,
    #[serde(default)]
    pub file_watch_support: bool,
    #[deprecated(note = "use EditorMeta::semantic_tokens")]
    #[serde(default)]
    pub semantic_tokens: SemanticTokenConfig,
    #[serde(default)]
    #[deprecated(note = "use EditorMeta::language_id")]
    pub language_ids: HashMap<String, LanguageId>,
}

#[derive(Clone, Default, Deserialize, Debug)]
pub struct DynamicConfig {
    #[serde(default, alias = "language")]
    pub language_server: HashMap<ServerName, DynamicLanguageServerConfig>,
}

#[derive(Clone, Default, Deserialize, Debug)]
pub struct ServerConfig {
    #[deprecated]
    #[allow(unused)]
    #[serde(default)]
    session: String,
    #[serde(default)]
    pub timeout: u64,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct LanguageServerConfig {
    #[deprecated]
    #[serde(default)]
    pub filetypes: Vec<String>,
    #[serde(default)]
    pub root: String,
    #[deprecated]
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub root_globs: Vec<String>,
    pub single_instance: Option<bool>,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub envs: HashMap<String, String>,
    pub settings_section: Option<String>,
    pub workspace_did_change_configuration_subsection: Option<String>,
    pub settings: Option<Value>,
    pub offset_encoding: Option<OffsetEncoding>,
    #[serde(default)]
    pub symbol_kinds: HashMap<String, String>,
    pub experimental: Option<Value>,
    // This does nothing, but is kept so we can still parse old configs.
    #[allow(dead_code)]
    workaround_server_sends_plaintext_labeled_as_markdown: Option<bool>,
    pub workaround_eslint: Option<bool>,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct DynamicLanguageServerConfig {
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
                formatter.write_str("A valid semantic-tokens configuration. See https://github.com/kakoune-lsp/kakoune-lsp#semantic-tokens for the new configuration syntax for semantic tokens")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut faces = None;
                while let Some(k) = map.next_key::<String>()? {
                    match k.as_str() {
                        "faces" => faces = Some(map.next_value()?),
                        "faces_str" => {
                            let s = map.next_value::<String>()?;
                            return toml::from_str(&format!("faces = {}", &s)).map_err(|err| {
                                SerdeError::custom(format!(
                                    "failed to parse %opt{{lsp_semantic_tokens}}: {}",
                                    err
                                ))
                            });
                        }
                        _ => return Err(A::Error::unknown_field(&k, &["faces"])),
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
    pub session: SessionId,
    pub client: Option<ClientId>,
    pub buffile: String,
    pub language_id: LanguageId,
    pub filetype: String,
    pub version: i32,
    #[serde(default)]
    pub hook: bool,
    #[serde(default)]
    pub sourcing: bool,
    #[serde(default)]
    pub language_server: HashMap<ServerName, LanguageServerConfig>,
    pub semantic_tokens: SemanticTokenConfig,
    pub server: Option<ServerName>,
    pub word_regex: Option<String>,
    #[serde(default)]
    pub servers: Vec<ServerId>,

    #[deprecated]
    #[serde(default)]
    pub legacy_dynamic_config: String,
    #[deprecated]
    #[serde(default)]
    pub legacy_server_initialization_options: Vec<String>,
}

impl EditorMeta {
    pub fn for_client(client: ClientId) -> Self {
        Self {
            client: Some(client),
            ..Default::default()
        }
    }
}

pub fn is_using_legacy_toml(config: &Config) -> bool {
    #[allow(deprecated)]
    !config.language_server.is_empty()
}

pub fn server_configs<'a>(
    config: &'a Config,
    meta: &'a EditorMeta,
) -> &'a HashMap<ServerName, LanguageServerConfig> {
    #[allow(deprecated)]
    if is_using_legacy_toml(config) {
        &config.language_server
    } else {
        &meta.language_server
    }
}

pub fn server_name_for_lookup<'a>(
    config: &Config,
    language_id: &LanguageId,
    server_name: &'a ServerName,
) -> Cow<'a, str> {
    #[allow(deprecated)]
    if config.language.is_empty() {
        return Cow::Borrowed(server_name);
    }
    Cow::Owned(format!("{}:{}", language_id, server_name))
}

pub fn semantic_tokens_config<'a>(
    config: &'a Config,
    meta: &'a EditorMeta,
) -> &'a [SemanticTokenFace] {
    #[allow(deprecated)]
    if is_using_legacy_toml(config) {
        &config.semantic_tokens.faces
    } else {
        &meta.semantic_tokens.faces
    }
}

#[derive(Debug)]
pub struct EditorParams(pub Box<dyn Any + Send>);

impl EditorParams {
    pub fn unbox<T: 'static>(self) -> T {
        *self.0.downcast().unwrap()
    }
    pub fn downcast_ref<T: 'static>(&self) -> &T {
        self.0.downcast_ref().unwrap()
    }
}

#[derive(Debug)]
pub struct ResponseFifo(Option<String>);

impl ResponseFifo {
    pub fn new(fifo: String) -> Self {
        Self(Some(fifo))
    }
    pub fn write(&mut self, command: &str) {
        let fifo = self.0.take().unwrap();
        let mut opts = fs::OpenOptions::new();
        opts.write(true).custom_flags(O_NONBLOCK);
        loop {
            match opts.open(&fifo) {
                Ok(mut file) => {
                    file.write_all(command.as_bytes())
                        .expect("Failed to write command to fifo");
                    break;
                }
                Err(err) => {
                    if err.raw_os_error() == Some(ENXIO) {
                        std::thread::sleep(Duration::from_millis(1));
                    } else if err.kind() == io::ErrorKind::NotFound {
                        break;
                    } else {
                        panic!("Failed to open fifo '{}': {}", &fifo, err);
                    }
                }
            }
        }
    }
}

impl Drop for ResponseFifo {
    fn drop(&mut self) {
        if self.0.is_some() {
            // Nothing to do, but sending command back to the editor is required to handle case
            // when editor is blocked waiting for response via fifo.
            self.write("nop");
        }
    }
}

#[derive(Debug)]
pub struct EditorRequest {
    pub meta: EditorMeta,
    pub response_fifo: Option<ResponseFifo>,
    pub method: String,
    pub params: EditorParams,
}

impl Default for EditorRequest {
    fn default() -> Self {
        Self {
            meta: Default::default(),
            response_fifo: None,
            method: Default::default(),
            params: EditorParams(Box::new(())),
        }
    }
}

#[derive(Deserialize)]
pub struct EditorResponse {
    pub meta: EditorMeta,
    pub command: Cow<'static, str>,
    // Set for the commands needed to transport a log statement, to stop recursion.
    pub suppress_logging: bool,
}

impl EditorResponse {
    pub fn new(meta: EditorMeta, command: Cow<'static, str>) -> Self {
        Self {
            meta,
            command,
            suppress_logging: false,
        }
    }
}

pub trait ToEditor {
    fn dispatch(&self, response: EditorResponse);
}

pub struct NotToEditor {}
impl ToEditor for NotToEditor {
    fn dispatch(&self, _response: EditorResponse) {}
}

/// Kakoune session ID.
#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionId(pub String);

impl Deref for SessionId {
    type Target = String;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Kakoune client ID.
#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClientId(pub String);

impl Deref for ClientId {
    type Target = String;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

pub type LanguageId = String;
pub type ServerName = String;
pub type RootPath = String;
pub type ServerId = usize;

#[derive(Debug)]
pub struct EditorCompletion {
    pub offset: u32,
}

#[derive(Debug)]
pub struct TextDocumentDidOpenParams {
    pub draft: String,
}

#[derive(Debug)]
pub struct TextDocumentDidChangeParams {
    pub draft: String,
}

#[derive(Debug)]
pub struct TextDocumentCompletionParams {
    pub position: KakounePosition,
    pub completion: EditorCompletion,
}

#[derive(Debug)]
pub struct CompletionItemResolveParams {
    pub completion_item_timestamp: i32,
    pub completion_item_index: isize,
    pub pager_active: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct PositionParams {
    pub position: KakounePosition,
}

#[derive(Clone, Debug)]
pub struct EditorHoverParams {
    pub selection_desc: String,
    pub tabstop: usize,
    pub hover_client: Option<ClientId>,
}

#[derive(Clone, Debug)]
pub struct CallHierarchyParams {
    pub position: KakounePosition,
    pub incoming_or_outgoing: bool,
}

#[derive(Clone, Debug)]
pub enum CodeActionFilter {
    ByKind(Vec<CodeActionKind>),
    ByRegex(String),
}

#[derive(Clone, Debug)]
pub struct CodeActionsParams {
    pub selection_desc: String,
    pub perform_code_action: bool,
    pub auto_single: bool,
    pub filters: Option<CodeActionFilter>,
}

#[derive(Clone, Debug)]
pub struct CodeActionResolveParams {
    pub code_action: String,
}

#[derive(Clone, Debug)]
pub struct RangeFormattingParams {
    pub formatting_options: FormattingOptions,
    pub ranges: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct NextOrPrevSymbolParams {
    pub position: KakounePosition,
    /// If true then searches forward ("next")
    /// otherwise searches backward ("previous")
    pub search_next: bool,
    /// If true, don't navigate to the next/previous symbol but show its hover
    /// otherwise goto the next/previous symbol
    pub hover: bool,
    /// Match any of these kinds of symbols, or any symbol if empty.
    pub symbol_kinds: Vec<String>,
}

#[derive(Clone, Default, Debug)]
pub struct BreadcrumbsParams {
    pub position_line: u32,
}

#[derive(Clone, Debug)]
pub struct GotoSymbolParams {
    pub goto_symbol: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ObjectParams {
    pub count: u32,
    pub mode: String,
    pub selections_desc: Vec<String>,
    pub symbol_kinds: Vec<String>,
}

#[derive(Debug)]
pub struct TextDocumentRenameParams {
    pub position: KakounePosition,
    pub new_name: String,
}

#[derive(Clone, Debug)]
pub struct SelectionRangePositionParams {
    // The cursor position.
    pub position: KakounePosition,
    // The ranges of all Kakoune selections.
    pub selections_desc: Vec<String>,
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

#[derive(PartialEq, Eq)]
pub enum HoverType {
    InfoBox,
    Modal {
        modal_heading: String,
        do_after: String,
    },
    HoverBuffer {
        client: ClientId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        write!(f, "{},{}", self.end, self.start)
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

impl Default for OffsetEncoding {
    fn default() -> Self {
        Self::Utf16
    }
}

// An intermediate representation of the diagnostics on a line, for use with inlay diagnostics
pub struct LineDiagnostics<'a> {
    pub range_end: Position,
    pub symbols: String,
    pub text: &'a str,
    pub text_face: &'static str,
    pub text_severity: Option<DiagnosticSeverity>,
}
