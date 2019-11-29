use lsp_types::request::ExecuteCommand;
use crate::context::*;
use crate::types::*;
use crate::util::*;
use lsp_types::*;

pub fn organize_imports(meta: EditorMeta, ctx: &mut Context) {
    let file_uri = Url::from_file_path(&meta.buffile).unwrap();

    let req_params = ExecuteCommandParams {
        command: "java.edit.organizeImports".to_string(),
        arguments: vec![ serde_json::json!(file_uri.into_string()) ],
    };
    ctx.call::<ExecuteCommand, _>(
        meta,
        req_params,
        move |ctx: &mut Context, meta, response| match response {
            Some(response) => organize_imports_response(meta, serde_json::from_value(response).unwrap(), ctx),
            None => return
        }
    );
}

pub fn organize_imports_response(meta: EditorMeta, result: Option<WorkspaceEdit>, ctx: &mut Context) {
    let result = match result {
        Some(result) => result,
        None => return,
    };

    // Double JSON serialization is performed to prevent parsing args as a TOML
    // structure when they are passed back via lsp-apply-workspace-edit.
    let edit = &serde_json::to_string(&result).unwrap();
    let edit = editor_quote(&serde_json::to_string(&edit).unwrap());
    let select_cmd = format!("lsp-apply-workspace-edit {}", edit);

    ctx.exec(meta, select_cmd);
}
