use crate::context::*;
use crate::language_features::goto;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use jsonrpc_core::Params;
use lsp_types::request::Request;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

// Navigate

#[derive(Serialize, Deserialize, Debug, Clone)]
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

pub struct NavigateRequest {}

impl Request for NavigateRequest {
    type Params = NavigateParams;
    type Result = Option<GotoDefinitionResponse>;
    const METHOD: &'static str = "$ccls/navigate";
}

pub fn navigate(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = KakouneNavigateParams::deserialize(params).unwrap();

    let req_params = ctx
        .language_servers
        .iter()
        .map(|(server_name, server_settings)| {
            (
                server_name.clone(),
                vec![NavigateParams {
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
                    direction: params.direction.clone(),
                }],
            )
        })
        .collect();

    ctx.call::<NavigateRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| goto::goto(meta, results, ctx),
    );
}

// The following are more granular, c/c++ specific find-defintion style methods.
// Reference: https://github.com/MaskRay/ccls/wiki/LanguageClient-neovim#cross-reference-extensions

// $ccls/vars

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VarsParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

pub struct VarsRequest {}

impl Request for VarsRequest {
    type Params = VarsParams;
    type Result = Option<Vec<Location>>;
    const METHOD: &'static str = "$ccls/vars";
}

pub fn vars(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();

    let req_params = ctx
        .language_servers
        .iter()
        .map(|(server_name, server_settings)| {
            (
                server_name.clone(),
                vec![VarsParams {
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

    ctx.call::<VarsRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            let results = results
                .into_iter()
                .map(|(server_name, loc)| (server_name, loc.map(GotoDefinitionResponse::Array)))
                .collect();

            goto::goto(meta, results, ctx)
        },
    );
}

// $ccls/inheritance

#[derive(Serialize, Deserialize, Debug, Clone)]
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

pub struct InheritanceRequest {}

impl Request for InheritanceRequest {
    type Params = InheritanceParams;
    type Result = Option<Vec<Location>>;
    const METHOD: &'static str = "$ccls/inheritance";
}

pub fn inheritance(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = KakouneInheritanceParams::deserialize(params).unwrap();

    let req_params = ctx
        .language_servers
        .iter()
        .map(|(server_name, server_settings)| {
            (
                server_name.clone(),
                vec![InheritanceParams {
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
                    levels: params.levels,
                    derived: params.derived,
                }],
            )
        })
        .collect();

    ctx.call::<InheritanceRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            let results = results
                .into_iter()
                .map(|(server_name, loc)| (server_name, loc.map(GotoDefinitionResponse::Array)))
                .collect();

            goto::goto(meta, results, ctx)
        },
    );
}

// $ccls/call

#[derive(Serialize, Deserialize, Debug, Clone)]
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

pub struct CallRequest {}

impl Request for CallRequest {
    type Params = CallParams;
    type Result = Option<Vec<Location>>;
    const METHOD: &'static str = "$ccls/call";
}

pub fn call(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = KakouneCallParams::deserialize(params).unwrap();

    let req_params = ctx
        .language_servers
        .iter()
        .map(|(server_name, server_settings)| {
            (
                server_name.clone(),
                vec![CallParams {
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
                    callee: params.callee,
                }],
            )
        })
        .collect();

    ctx.call::<CallRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            let results = results
                .into_iter()
                .map(|(server_name, loc)| (server_name, loc.map(GotoDefinitionResponse::Array)))
                .collect();

            goto::goto(meta, results, ctx)
        },
    );
}

// $ccls/member

#[derive(Serialize, Deserialize, Debug, Clone)]
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

pub struct MemberRequest {}

impl Request for MemberRequest {
    type Params = MemberParams;
    type Result = Option<Vec<Location>>;
    const METHOD: &'static str = "$ccls/member";
}

pub fn member(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = KakouneMemberParams::deserialize(params).unwrap();

    let req_params = ctx
        .language_servers
        .iter()
        .map(|(server_name, server_settings)| {
            (
                server_name.clone(),
                vec![MemberParams {
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
                    kind: params.kind,
                }],
            )
        })
        .collect();

    ctx.call::<MemberRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            let results = results
                .into_iter()
                .map(|(server_name, loc)| (server_name, loc.map(GotoDefinitionResponse::Array)))
                .collect();

            goto::goto(meta, results, ctx)
        },
    );
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
    pub uri: Url,

    /// The symbols to highlight
    pub symbols: Vec<SemanticSymbol>,
}

pub fn publish_semantic_highlighting(server_name: &ServerName, params: Params, ctx: &mut Context) {
    let params: PublishSemanticHighlightingParams =
        params.parse().expect("Failed to parse semhl params");
    let path = params.uri.to_file_path().unwrap();
    let buffile = path.to_str().unwrap();
    let document = match ctx.documents.get(buffile) {
        Some(document) => document,
        None => return,
    };
    let meta = match ctx.meta_for_buffer(None, buffile) {
        Some(meta) => meta,
        None => return,
    };
    let server = &ctx.language_servers[server_name];
    let ranges = params
        .symbols
        .iter()
        .flat_map(|x| {
            let face = x.get_face();
            let offset_encoding = server.offset_encoding;
            x.ls_ranges.iter().filter_map(move |r| {
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
    let command = format!(
        "evaluate-commands -buffer {} -verbatim -- set-option buffer cquery_semhl {} {}",
        editor_quote(buffile),
        meta.version,
        ranges
    );
    ctx.exec(meta, command);
}
