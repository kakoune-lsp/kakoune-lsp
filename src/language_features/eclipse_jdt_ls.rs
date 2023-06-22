use super::code_action::apply_workspace_edit_editor_command;
use crate::context::*;
use crate::types::*;
use lsp_types::request::ExecuteCommand;
use lsp_types::*;

pub fn organize_imports(meta: EditorMeta, ctx: &mut Context) {
    let file_uri = Url::from_file_path(&meta.buffile).unwrap();
    let file_uri: String = file_uri.into();

    let req_params = ctx
        .language_servers
        .keys()
        .map(|server_name| {
            (
                server_name.clone(),
                vec![ExecuteCommandParams {
                    command: "java.edit.organizeImports".to_string(),
                    arguments: vec![serde_json::json!(file_uri)],
                    ..ExecuteCommandParams::default()
                }],
            )
        })
        .collect();

    ctx.call::<ExecuteCommand, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            let response = match results.into_iter().find(|(_, v)| v.is_some()) {
                Some((_, result)) => result,
                None => None,
            };

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
