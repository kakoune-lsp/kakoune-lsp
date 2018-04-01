use fnv::FnvHashMap;
use jsonrpc_core::{Call, Output, Params};
use languageserver_types::*;
use serde::Serialize;
use serde_json::Value;
use std::io::Error;
use url::Url;
use url_serde;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub language: FnvHashMap<String, LanguageConfig>,
    pub server: ServerConfig,
}

#[derive(Deserialize, Debug)]
pub struct ServerConfig {
    pub ip: String,
    pub port: u16,
}

#[derive(Clone, Deserialize, Debug)]
pub struct LanguageConfig {
    pub extensions: Vec<String>,
    pub roots: Vec<String>,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Deserialize, Debug)]
pub struct FileConfig {
    pub language: FnvHashMap<String, FileLanguageConfig>,
    pub server: Option<FileServerConfig>,
}

#[derive(Deserialize, Debug)]
pub struct FileServerConfig {
    pub ip: Option<String>,
    pub port: Option<u16>,
}

#[derive(Deserialize, Debug)]
pub struct FileLanguageConfig {
    pub extensions: Vec<String>,
    pub roots: Vec<String>,
    pub command: String,
    pub args: Option<Vec<String>>,
}

#[derive(Clone, Deserialize)]
pub struct EditorMeta {
    pub session: String,
    pub client: String,
    pub buffile: String,
    pub version: u64,
}

#[derive(Deserialize)]
pub struct EditorRequest {
    pub meta: EditorMeta,
    pub call: Call,
}

#[derive(Deserialize)]
pub struct EditorResponse {
    pub meta: EditorMeta,
    pub command: String,
}

pub type SessionId = String;
pub type LanguageId = String;
pub type RootPath = String;
pub type Route = (SessionId, LanguageId, RootPath);

pub struct RoutedEditorRequest {
    pub request: EditorRequest,
    pub route: Route,
}

// XXX serde(untagged) ?
pub enum ServerMessage {
    Request(Call),
    Response(Output),
}

pub trait ToParams {
    fn to_params(self) -> Result<Params, Error>;
}

impl<T> ToParams for T
where
    T: Serialize,
{
    fn to_params(self) -> Result<Params, Error> {
        use serde_json;

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

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Completion {
    pub offset: u64,
}

#[derive(Deserialize, Debug)]
pub struct TextDraft {
    #[serde(with = "url_serde")]
    pub uri: Url,
    pub version: Option<u64>,
    pub draft: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TextDocumentDidOpenParams {
    #[serde(rename = "textDocument")]
    pub text_document: VersionedTextDocumentIdentifier,
}

#[derive(Deserialize, Debug)]
pub struct TextDocumentDidChangeParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDraft,
}

#[derive(Deserialize, Debug)]
pub struct TextDocumentDidCloseParams {
    #[serde(rename = "textDocument")]
    pub text_document: VersionedTextDocumentIdentifier,
}

#[derive(Deserialize, Debug)]
pub struct TextDocumentDidSaveParams {
    #[serde(rename = "textDocument")]
    pub text_document: VersionedTextDocumentIdentifier,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TextDocumentCompletionParams {
    #[serde(rename = "textDocument")]
    pub text_document: VersionedTextDocumentIdentifier,
    pub position: Position,
    pub completion: Completion,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GotoDefinitionResponse {
    None,
    Scalar(Location),
    Array(Vec<Location>),
}
