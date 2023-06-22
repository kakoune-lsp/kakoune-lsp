use crate::context::*;
use crate::position::*;
use crate::types::ServerName;
use crate::util::*;
use itertools::Itertools;
use jsonrpc_core::Params;
use lsp_types::{NumberOrString, Range};
use url::Url;

enum_from_primitive! {
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum StorageClass {
    Invalid = 0,
    No = 1,
    Extern = 2,
    Static = 3,
    PrivateExtern = 4,
    Auto = 5,
    Register = 6
}
}

impl<'de> serde::Deserialize<'de> for StorageClass {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use enum_primitive::FromPrimitive;

        let i = u8::deserialize(deserializer)?;
        Ok(StorageClass::from_u8(i).unwrap_or(StorageClass::Invalid))
    }
}

impl serde::Serialize for StorageClass {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

enum_from_primitive! {
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum SemanticSymbolKind {
    Unknown = 0,
    File = 1,
    Module = 2,
    Namespace = 3,
    Package = 4,

    Class = 5,
    Method = 6,
    Property = 7,
    Field = 8,
    Constructor = 9,

    Enum = 10,
    Interface = 11,
    Function = 12,
    Variable = 13,
    Constant = 14,

    String = 15,
    Number = 16,
    Boolean = 17,
    Array = 18,
    Object = 19,

    Key = 20,
    Null = 21,
    EnumMember = 22,
    Struct = 23,
    Event = 24,

    Operator = 25,
    TypeParameter = 26,

    TypeAlias = 252,
    Parameter = 253,
    StaticMethod = 254,
    Macro = 255,
}
}

impl<'de> serde::Deserialize<'de> for SemanticSymbolKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use enum_primitive::FromPrimitive;

        let i = u8::deserialize(deserializer)?;
        Ok(SemanticSymbolKind::from_u8(i).unwrap_or(SemanticSymbolKind::Unknown))
    }
}

impl serde::Serialize for SemanticSymbolKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticSymbol {
    stable_id: NumberOrString,
    parent_kind: SemanticSymbolKind,
    kind: SemanticSymbolKind,
    is_type_member: Option<bool>,
    storage: StorageClass,
    ranges: Vec<Range>,
}

impl SemanticSymbol {
    /// Get the face for this symbol
    pub fn get_face(&self) -> String {
        match self.kind {
            SemanticSymbolKind::Class | SemanticSymbolKind::Struct => "cqueryTypes",
            SemanticSymbolKind::Enum => "cqueryEnums",
            SemanticSymbolKind::TypeAlias => "cqueryTypeAliases",
            SemanticSymbolKind::TypeParameter => "cqueryTemplateParameters",
            SemanticSymbolKind::Function => "cqueryFreeStandingFunctions",
            SemanticSymbolKind::Method | SemanticSymbolKind::Constructor => "cqueryMemberFunctions",
            SemanticSymbolKind::StaticMethod => "cqueryStaticMemberFunctions",
            SemanticSymbolKind::Variable => match self.parent_kind {
                SemanticSymbolKind::Function => "cqueryFreeStandingVariables",
                _ => "cqueryGlobalVariables",
            },
            SemanticSymbolKind::Field => match self.storage {
                StorageClass::Static => "cqueryStaticMemberVariables",
                _ => "cqueryMemberVariables",
            },
            SemanticSymbolKind::Parameter => "cqueryParameters",
            SemanticSymbolKind::EnumMember => "cqueryEnumConstants",
            SemanticSymbolKind::Namespace => "cqueryNamespaces",
            SemanticSymbolKind::Macro => "cqueryMacros",
            _ => "",
        }
        .to_string()
    }
}

#[derive(Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishSemanticHighlightingParams {
    /// The URI for which diagnostic information is reported.
    pub uri: Url,

    /// The symbols to highlight
    pub symbols: Vec<SemanticSymbol>,
}

pub fn publish_semantic_highlighting(server_name: &ServerName, params: Params, ctx: &mut Context) {
    let params: PublishSemanticHighlightingParams =
        params.parse().expect("Failed to parse semhl params");
    let client = None;
    let path = params.uri.to_file_path().unwrap();
    let buffile = path.to_str().unwrap();
    let document = ctx.documents.get(buffile);
    if document.is_none() {
        return;
    }
    let document = document.unwrap();
    let version = document.version;
    let server = &ctx.language_servers[server_name];
    let ranges = params
        .symbols
        .iter()
        .flat_map(|x| {
            let face = x.get_face();
            let offset_encoding = server.offset_encoding;
            x.ranges.iter().filter_map(move |r| {
                if face.is_empty() {
                    warn!("No face found for {:?}", x);
                    Option::None
                } else {
                    Option::Some(format!(
                        "{}|{}",
                        lsp_range_to_kakoune(r, &document.text, offset_encoding),
                        face
                    ))
                }
            })
        })
        .join(" ");
    let command = format!("set-option buffer cquery_semhl {} {}", version, ranges);
    let command = format!(
        "evaluate-commands -buffer {} -verbatim -- {}",
        editor_quote(buffile),
        command
    );
    let meta = ctx.meta_for_buffer_version(client, buffile, version);
    ctx.exec(meta, command);
}
