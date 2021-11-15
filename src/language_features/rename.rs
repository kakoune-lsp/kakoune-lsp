use crate::context::*;
use crate::position::*;
use crate::types::*;

use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

use super::super::workspace;

pub fn text_document_rename(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = TextDocumentRenameParams::deserialize(params).unwrap();
    let req_params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        new_name: params.new_name,
        work_done_progress_params: Default::default(),
    };
    ctx.call::<Rename, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_rename(meta, result, ctx)
    });
}

// TODO handle version, so change is not applied if buffer is modified (and need to show a warning)
pub fn editor_rename(meta: EditorMeta, result: Option<WorkspaceEdit>, ctx: &mut Context) {
    if result.is_none() {
        return;
    }
    let result = result.unwrap();
    workspace::apply_edit(meta, result, ctx);
}
