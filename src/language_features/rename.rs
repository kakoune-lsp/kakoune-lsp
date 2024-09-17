use crate::context::*;
use crate::position::*;
use crate::types::*;

use lsp_types::request::*;
use lsp_types::*;
use url::Url;

use super::super::workspace;

pub fn text_document_rename(meta: EditorMeta, params: TextDocumentRenameParams, ctx: &mut Context) {
    let req_params = ctx
        .servers(&meta)
        .map(|(server_id, server_settings)| {
            (
                server_id,
                vec![RenameParams {
                    text_document_position: TextDocumentPositionParams {
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
                    },
                    new_name: params.new_name.clone(),
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<Rename, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            let result = match results.into_iter().find(|(_, v)| v.is_some()) {
                Some(result) => result,
                None => (meta.servers[0], None),
            };

            editor_rename(meta, result, ctx)
        },
    );
}

// TODO handle version, so change is not applied if buffer is modified (and need to show a warning)
fn editor_rename(meta: EditorMeta, result: (ServerId, Option<WorkspaceEdit>), ctx: &mut Context) {
    let (server_id, result) = result;
    if result.is_none() {
        return;
    }
    let result = result.unwrap();
    workspace::apply_edit(server_id, &meta, result, ctx);
}
