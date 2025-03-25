use crate::context::*;
use crate::types::*;
use crate::util::editor_quote;
use jsonrpc_core::Value;
use lsp_types::request::Request;

pub struct SignIn {}

impl Request for SignIn {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = "signIn";
}

pub fn sign_in(meta: EditorMeta, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| srv.1.workaround_copilot)
        .collect();
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, _server_settings)| (server_id, vec![Value::Object(Default::default())]))
        .collect();
    ctx.call::<SignIn, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            editor_sign_in(ctx, meta, results);
        },
    );
}

fn editor_sign_in(ctx: &mut Context, meta: EditorMeta, results: Vec<(ServerId, Value)>) {
    for (_server_id, result) in results {
        let Some(object) = result.as_object() else {
            error!(ctx.to_editor(), "Not an object");
            continue;
        };
        let Some(status) = object.get("status").and_then(|status| status.as_str()) else {
            error!(ctx.to_editor(), "Missing status");
            continue;
        };
        if status == "AlreadySignedIn" {
            continue;
        }
        if status != "PromptUserDeviceFlow" {
            error!(ctx.to_editor(), "Unknown status");
            continue;
        }
        let Some(verification_uri) = object.get("verificationUri").and_then(|s| s.as_str()) else {
            error!(ctx.to_editor(), "Missing verificationUri");
            continue;
        };
        let Some(user_code) = object.get("userCode").and_then(|s| s.as_str()) else {
            error!(ctx.to_editor(), "Missing userCode");
            continue;
        };
        let command = format!(
            "Please authenticate at {} using code {}",
            verification_uri, user_code
        );
        let command = format!("lsp-show-error {}", editor_quote(&command));
        ctx.exec(meta.clone(), command);
    }
}

pub struct SignOut {}

impl Request for SignOut {
    type Params = Value;
    type Result = Value;
    const METHOD: &'static str = "signOut";
}

pub fn sign_out(meta: EditorMeta, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| srv.1.workaround_copilot)
        .collect();
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, _server_settings)| (server_id, vec![Value::Object(Default::default())]))
        .collect();
    ctx.call::<SignOut, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            editor_sign_in(ctx, meta, results);
        },
    );
}
