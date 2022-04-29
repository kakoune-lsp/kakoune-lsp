use super::code_action::apply_workspace_edit_editor_command;
use crate::context::*;
use crate::types::*;
use lsp_types::request::ExecuteCommand;
use lsp_types::*;

pub fn organize_imports(meta: EditorMeta, ctx: &mut Context) {
    let file_uri = Url::from_file_path(&meta.buffile).unwrap();

    let file_uri: String = file_uri.into();
    let req_params = ExecuteCommandParams {
        command: "java.edit.organizeImports".to_string(),
        arguments: vec![serde_json::json!(file_uri)],
        ..ExecuteCommandParams::default()
    };
    ctx.call::<ExecuteCommand, _>(
        meta,
        req_params,
        move |ctx: &mut Context, meta, response| {
            if let Some(response) = response {
                organize_imports_response(meta, serde_json::from_value(response).unwrap(), ctx);
            }
        },
    );
}

pub fn organize_imports_response(
    meta: EditorMeta,
    result: Option<WorkspaceEdit>,
    ctx: &mut Context,
) {
    let result = match result {
        Some(result) => result,
        None => return,
    };

    let select_cmd = apply_workspace_edit_editor_command(&result, false);

    ctx.exec(meta, select_cmd);
}
