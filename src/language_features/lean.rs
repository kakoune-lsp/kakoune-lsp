use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use indoc::formatdoc;
use lsp_types::request::Request;
use lsp_types::*;
use serde::Deserialize;
use serde::Serialize;

pub trait LeanPlainGoalResponse: Serialize + for<'de> Deserialize<'de> {
    fn get_rendered(self) -> String;
}

#[derive(Debug, Clone)]
pub struct EditorPlainGoalParams {
    pub position: KakounePosition,
    pub buffer: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlainGoalParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlainGoalResponse {
    pub goals: Vec<String>,
    pub rendered: String,
}
impl LeanPlainGoalResponse for PlainGoalResponse {
    fn get_rendered(self) -> String {
        self.rendered
    }
}

pub struct PlainGoalRequest {}

impl Request for PlainGoalRequest {
    type Params = PlainGoalParams;
    type Result = Option<PlainGoalResponse>;
    const METHOD: &'static str = "$/lean/plainGoal";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PlainTermGoalResponse {
    pub goal: String,
    pub range: Range,
}

impl LeanPlainGoalResponse for PlainTermGoalResponse {
    fn get_rendered(self) -> String {
        self.goal
    }
}

pub struct PlainTermGoalRequest {}

impl Request for PlainTermGoalRequest {
    type Params = PlainGoalParams;
    type Result = Option<PlainTermGoalResponse>;
    const METHOD: &'static str = "$/lean/plainTermGoal";
}

pub fn plain_goal_impl<
    R: Request<Params = PlainGoalParams, Result = Option<T>>,
    T: LeanPlainGoalResponse,
>( meta: EditorMeta, params: EditorPlainGoalParams, ctx: &mut Context, ) {
    let req_params = ctx
        .servers(&meta)
        .map(|(server_id, server_settings)| {
            (
                server_id,
                vec![R::Params {
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

    ctx.call::<R, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, _meta, results| {
            let rendered = results
                .into_iter()
                .filter_map(|(_, goals)| goals.map(|goals| goals.get_rendered()))
                .collect::<String>();
            let command = formatdoc!(
                "edit -scratch -- {}
                 set-option buffer filetype lean-goals
                 set-register a {}
                 execute-keys -draft '%c<c-r>a<esc>gg'
                ",
                editor_quote(&params.buffer),
                editor_quote(&rendered)
            );
            let command = format!("evaluate-commands -save-regs a {}", editor_quote(&command));
            ctx.exec(EditorMeta::default(), command)
        },
    );
}
