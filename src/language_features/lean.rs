use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use indoc::formatdoc;
use lsp_types::request::Request;
use lsp_types::*;

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

pub struct PlainGoalRequest {}

impl Request for PlainGoalRequest {
    type Params = PlainGoalParams;
    type Result = Option<PlainGoalResponse>;
    const METHOD: &'static str = "$/lean/plainGoal";
}

pub fn plain_goal(meta: EditorMeta, params: EditorPlainGoalParams, ctx: &mut Context) {
    let req_params = ctx
        .servers(&meta)
        .map(|(server_id, server_settings)| {
            (
                server_id,
                vec![PlainGoalParams {
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

    ctx.call::<PlainGoalRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, _meta, results| {
            let rendered = results
                .into_iter()
                .filter_map(|(_, goals)| goals.map(|goals| goals.rendered))
                .collect::<String>();
            let command = formatdoc!(
                "edit -scratch {}
                 set-option buffer filetype lean-goals
                 set-register a {}
                 execute-keys -draft '%c<c-r>a<esc>'
                ",
                editor_quote(&params.buffer),
                editor_quote(&rendered)
            );
            ctx.exec(EditorMeta::default(), command)
        },
    );
}
