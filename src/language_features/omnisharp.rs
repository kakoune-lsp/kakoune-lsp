use crate::{
    context::Context,
    types::{EditorMeta, EditorParams, PositionParams},
    util::{self, get_lsp_position},
};
use lsp_types::{request::Request, Url};
use serde::Deserialize;
use std::{io::Write, path::PathBuf};

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Location {
    pub file_name: String,
    pub range: Range,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Range {
    pub start: Point,
    pub end: Point,
}

impl From<Range> for lsp_types::Range {
    fn from(Range { start, end }: Range) -> Self {
        Self {
            start: start.into(),
            end: end.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Point {
    pub line: u32,
    pub column: u32,
}

impl From<Point> for lsp_types::Position {
    fn from(Point { line, column }: Point) -> Self {
        Self {
            line,
            character: column,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct LinePositionSpanTextChange {
    pub new_text: String,
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Definition {
    pub location: Location,
    pub metadata_source: Option<MetadataSource>,
    pub source_generated_file_info: Option<SourceGeneratedFileInfo>,
}

pub enum GoToDefinition {}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GoToDefinitionParams {
    pub file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<Vec<LinePositionSpanTextChange>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply_changes_together: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub want_metadata: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct GoToDefinitionResponse {
    pub definitions: Option<Vec<Definition>>,
}

impl Request for GoToDefinition {
    type Params = GoToDefinitionParams;
    type Result = GoToDefinitionResponse;
    const METHOD: &'static str = "o#/v2/gotodefinition";
}

pub fn convert_definition<F>(
    meta: EditorMeta,
    definition: Definition,
    ctx: &mut Context,
    callback: F,
) where
    F: for<'a> FnOnce(&'a mut Context, EditorMeta, lsp_types::Location) -> () + 'static,
{
    if let Some(metadata_source) = definition.metadata_source {
        let req_params = MetadataParams {
            metadata_source,
            timeout: Some(5000),
        };
        let range = definition.location.range;
        ctx.call::<Metadata, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
            let mut path = metadata_dir(&meta);
            path.push(result.source_name.strip_prefix("$metadata$/").unwrap());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open(&path)
                .unwrap();
            file.write_all(result.source.as_bytes()).unwrap();
            file.flush().unwrap();
            callback(
                ctx,
                meta,
                lsp_types::Location {
                    uri: Url::from_file_path(path).unwrap(),
                    range: range.into(),
                },
            );
        });
    } else if let Some(rest) = definition.location.file_name.strip_prefix("$metadata$") {
        let mut path = metadata_dir(&meta);
        path.push(rest);
        callback(
            ctx,
            meta,
            lsp_types::Location {
                uri: Url::from_file_path(path).unwrap(),
                range: definition.location.range.into(),
            },
        );
    } else {
        callback(
            ctx,
            meta,
            lsp_types::Location {
                uri: Url::from_file_path(definition.location.file_name).unwrap(),
                range: definition.location.range.into(),
            },
        );
    }
}

fn convert_definitions<F>(
    meta: EditorMeta,
    mut definitions: Vec<Definition>,
    mut locations: Vec<lsp_types::Location>,
    ctx: &mut Context,
    callback: F,
) where
    F: for<'a> FnOnce(&'a mut Context, EditorMeta, Vec<lsp_types::Location>) -> () + 'static,
{
    convert_definition(
        meta.clone(),
        definitions.pop().unwrap(),
        ctx,
        move |ctx: &mut Context, meta, location| {
            locations.push(location);
            if !definitions.is_empty() {
                convert_definitions(meta, definitions, locations, ctx, callback);
            } else {
                callback(ctx, meta, locations);
            }
        },
    );
}

fn metadata_dir(meta: &EditorMeta) -> PathBuf {
    util::temp_dir().join(format!("omnisharp-metadata-{}", meta.session))
}

pub fn go_to_definition(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();
    let position = get_lsp_position(&meta.buffile, &params.position, ctx).unwrap();
    let file_name = {
        if let Ok(rel_path) = PathBuf::from(&meta.buffile).strip_prefix(metadata_dir(&meta)) {
            format!(
                "{}/[metadata] {}",
                rel_path.parent().unwrap().to_str().unwrap(),
                rel_path.file_name().unwrap().to_str().unwrap()
            )
        } else {
            meta.buffile.clone()
        }
    };
    let req_params = GoToDefinitionParams {
        file_name,
        line: Some(position.line),
        column: Some(position.character),
        buffer: None,
        changes: None,
        apply_changes_together: None,
        want_metadata: Some(true),
    };
    ctx.call::<GoToDefinition, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        goto(meta, result, ctx);
    });
}

fn goto(meta: EditorMeta, result: GoToDefinitionResponse, ctx: &mut Context) {
    let mut definitions = result.definitions.unwrap_or_default();
    match definitions.len() {
        0 => {}
        1 => {
            convert_definition(
                meta.clone(),
                definitions.pop().unwrap(),
                ctx,
                move |ctx: &mut Context, meta, location| {
                    crate::language_features::goto::goto_location(meta, &location, ctx);
                },
            );
        }
        _ => {
            convert_definitions(
                meta,
                definitions,
                Vec::new(),
                ctx,
                move |ctx: &mut Context, meta, locations| {
                    crate::language_features::goto::goto_locations(meta, &locations, ctx);
                },
            );
        }
    }
}

pub enum Metadata {}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct MetadataSource {
    pub assembly_name: String,
    pub project_name: String,
    pub version_number: Option<String>,
    pub language: Option<String>,
    pub type_name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct MetadataParams {
    #[serde(flatten)]
    metadata_source: MetadataSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct MetadataResponse {
    pub source_name: String,
    pub source: String,
}

impl Request for Metadata {
    type Params = MetadataParams;
    type Result = MetadataResponse;
    const METHOD: &'static str = "o#/metadata";
}

pub enum SourceGeneratedFile {}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct SourceGeneratedFileInfo {
    pub project_guid: String,
    pub document_guid: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct SourceGeneratedFileParams {
    #[serde(flatten)]
    metadata_source: SourceGeneratedFileInfo,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct SourceGeneratedFileResponse {
    pub source: String,
    pub source_name: String,
}

impl Request for SourceGeneratedFile {
    type Params = SourceGeneratedFileParams;
    type Result = SourceGeneratedFileResponse;
    const METHOD: &'static str = "o#/metadata";
}
