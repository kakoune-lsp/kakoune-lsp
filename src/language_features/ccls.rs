use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use jsonrpc_core::{Params, Value};
use lsp_types::{NumberOrString, Position, Range, TextDocumentIdentifier};
use serde;
use serde::Deserialize;
use serde_json;
use url::Url;
use url_serde;

// Navigate

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NavigateParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub direction: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct KakouneNavigateParams {
    pub position: KakounePosition,
    pub direction: String,
}

pub fn navigate(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let req_params = KakouneNavigateParams::deserialize(params.clone()).unwrap();
    let req_params = NavigateParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &req_params.position, ctx).unwrap(),
        direction: req_params.direction,
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist
        .insert(id.clone(), (meta.clone(), "$ccls/navigate".into(), params));
    ctx.call(id, "$ccls/navigate".into(), req_params);
}

pub fn navigate_response(meta: &EditorMeta, result: Value, ctx: &mut Context) {
    let result = serde_json::from_value(result).expect("Failed to parse definition response");
    if let Some(location) = goto_definition_response_to_location(result) {
        let path = location.uri.to_file_path().unwrap();
        let filename = path.to_str().unwrap();
        let p = get_kakoune_position(filename, &location.range.start, ctx).unwrap();
        let command = format!("edit %§{}§ {} {}", filename, p.line, p.column);
        ctx.exec(meta.clone(), command);
    };
}

// The following are more granular, c/c++ specific find-defintion style methods.
// Reference: https://github.com/MaskRay/ccls/wiki/LanguageClient-neovim#cross-reference-extensions

// $ccls/vars

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VarsParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

pub fn vars(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let req_params = PositionParams::deserialize(params.clone()).unwrap();
    let req_params = VarsParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &req_params.position, ctx).unwrap(),
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist
        .insert(id.clone(), (meta.clone(), "$ccls/vars".into(), params));
    ctx.call(id, "$ccls/vars".into(), req_params);
}

// $ccls/inheritance

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InheritanceParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub levels: usize,
    pub derived: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct KakouneInheritanceParams {
    pub position: KakounePosition,
    pub levels: usize,
    pub derived: bool,
}

pub fn inheritance(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let req_params = KakouneInheritanceParams::deserialize(params.clone()).unwrap();
    let req_params = InheritanceParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &req_params.position, ctx).unwrap(),
        levels: req_params.levels,
        derived: req_params.derived,
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), "$ccls/inheritance".into(), params),
    );
    ctx.call(id, "$ccls/inheritance".into(), req_params);
}

// $ccls/call

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CallParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub callee: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct KakouneCallParams {
    pub position: KakounePosition,
    pub callee: bool,
}

pub fn call(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let req_params = KakouneCallParams::deserialize(params.clone()).unwrap();
    let req_params = CallParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &req_params.position, ctx).unwrap(),
        callee: req_params.callee,
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist
        .insert(id.clone(), (meta.clone(), "$ccls/call".into(), params));
    ctx.call(id, "$ccls/call".into(), req_params);
}

// $ccls/member

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MemberParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub kind: u8, // 1: variable, 2: type, 3: function
}

#[derive(Serialize, Deserialize, Debug)]
pub struct KakouneMemberParams {
    pub position: KakounePosition,
    pub kind: u8, // 1: variable, 2: type, 3: function
}

pub fn member(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let req_params = KakouneMemberParams::deserialize(params.clone()).unwrap();
    let req_params = MemberParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &req_params.position, ctx).unwrap(),
        kind: req_params.kind,
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist
        .insert(id.clone(), (meta.clone(), "$ccls/member".into(), params));
    ctx.call(id, "$ccls/member".into(), req_params);
}

// Semantic Highlighting

enum_from_primitive! {
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum StorageClass {
    None = 0,
    Extern = 1,
    Static = 2,
    PrivateExtern = 3,
    Auto = 4,
    Register = 5
}
}

impl<'de> serde::Deserialize<'de> for StorageClass {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use enum_primitive::FromPrimitive;

        let i = u8::deserialize(deserializer)?;
        Ok(StorageClass::from_u8(i).unwrap_or(StorageClass::None))
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
    id: NumberOrString,
    parent_kind: SemanticSymbolKind,
    kind: SemanticSymbolKind,
    is_type_member: Option<bool>,
    storage: StorageClass,
    ls_ranges: Vec<Range>,
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
    #[serde(with = "url_serde")]
    pub uri: Url,

    /// The symbols to highlight
    pub symbols: Vec<SemanticSymbol>,
}

pub fn publish_semantic_highlighting(params: Params, ctx: &mut Context) {
    let params: PublishSemanticHighlightingParams =
        params.parse().expect("Failed to parse semhl params");
    let path = params.uri.to_file_path().unwrap();
    let buffile = path.to_str().unwrap();
    let document = match ctx.documents.get(buffile) {
        Some(document) => document,
        None => return,
    };
    let meta = match ctx.meta_for_buffer(buffile.to_string()) {
        Some(meta) => meta,
        None => return,
    };
    let ranges = params
        .symbols
        .iter()
        .flat_map(|x| {
            let face = x.get_face();
            let offset_encoding = ctx.offset_encoding.to_owned();
            x.ls_ranges.iter().filter_map(move |r| {
                if face.is_empty() {
                    warn!("No face found for {:?}", x);
                    Option::None
                } else {
                    Option::Some(format!(
                        "{}|{}",
                        lsp_range_to_kakoune(r, &document.text, &offset_encoding),
                        face
                    ))
                }
            })
        })
        .join(" ");
    let command = format!(
        "eval -buffer %§{}§ %§set buffer cquery_semhl {} {}§",
        buffile, meta.version, ranges
    );
    ctx.exec(meta, command.to_string());
}
