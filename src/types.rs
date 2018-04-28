use fnv::FnvHashMap;
use jsonrpc_core::{Call, Output, Params};
use languageserver_types::*;
use serde::Serialize;
use serde_json::Value;
use std::io::Error;
use toml;

// Configuration

#[derive(Deserialize, Debug)]
pub struct Config {
    pub language: FnvHashMap<String, LanguageConfig>,
    #[serde(default)]
    pub server: ServerConfig,
}

#[derive(Deserialize, Debug)]
pub struct ServerConfig {
    #[serde(default = "default_ip")]
    pub ip: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[derive(Clone, Deserialize, Debug)]
pub struct LanguageConfig {
    pub extensions: Vec<String>,
    pub roots: Vec<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            ip: default_ip(),
            port: default_port(),
        }
    }
}

fn default_ip() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    31337
}

// Editor

#[derive(Clone, Debug, Deserialize)]
pub struct EditorMeta {
    pub session: String,
    pub client: Option<String>,
    pub buffile: String,
    pub version: u64,
}

pub type EditorParams = toml::Value;

#[derive(Clone, Debug, Deserialize)]
pub struct EditorRequest {
    #[serde(flatten)]
    pub meta: EditorMeta,
    pub method: String,
    pub params: EditorParams,
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

#[derive(Serialize, Deserialize, Debug)]
pub struct EditorCompletion {
    pub offset: u64,
}

#[derive(Deserialize, Debug)]
pub struct TextDocumentDidChangeParams {
    pub draft: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TextDocumentCompletionParams {
    pub position: Position,
    pub completion: EditorCompletion,
}

#[derive(Deserialize, Debug)]
pub struct PositionParams {
    pub position: Position,
}

// Language Server

// XXX serde(untagged) ?
#[derive(Debug)]
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GotoDefinitionResponse {
    None,
    Scalar(Location),
    Array(Vec<Location>),
}
