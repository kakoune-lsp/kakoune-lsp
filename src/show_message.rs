use std::borrow::Cow;

use itertools::Itertools;
use jsonrpc_core::{Id, MethodCall};
use lsp_types::{MessageActionItem, MessageType, ShowMessageRequestParams};
use serde::{Deserialize, Serialize};

use crate::{
    context::Context,
    types::{EditorMeta, ServerName},
    util::editor_quote,
};

// commands to be handled
pub const SHOW_MESSAGE_REQUEST_NEXT: &str = "window/showMessageRequest/showNext";
pub const SHOW_MESSAGE_REQUEST_RESPOND: &str = "window/showMessageRequest/respond";

/// Queues the message request from the LSP server.
pub fn show_message_request(
    meta: EditorMeta,
    server_name: &ServerName,
    request: MethodCall,
    ctx: &mut Context,
) {
    let request_id = request.id;
    let params: ShowMessageRequestParams = request
        .params
        .parse()
        .expect("Failed to parse ShowMessageRequest params");
    ctx.pending_message_requests
        .push_back((request_id, server_name.clone(), params));
    update_modeline(meta, ctx)
}

#[derive(Deserialize)]
struct MessageRequestResponse {
    pub message_request_id: Id,
    pub item: Option<toml::Value>,
}

/// Handles an user's response to a message request (or the user's request to display the next message request).
pub fn show_message_request_respond(params: toml::Value, ctx: &mut Context) {
    let resp =
        MessageRequestResponse::deserialize(params).expect("Cannot parse message request response");

    let servers: Vec<_> = ctx.language_servers.keys().cloned().collect();
    for server_name in &servers {
        let item = resp
            .item
            .clone()
            .and_then(|v| MessageActionItem::deserialize(v).ok())
            .map(|v| jsonrpc_core::to_value(v).expect("Cannot serialize item"))
            .unwrap_or(jsonrpc_core::Value::Null);
        ctx.reply(server_name, resp.message_request_id.clone(), Ok(item));
    }
}

pub fn show_message_request_next(meta: EditorMeta, ctx: &mut Context) {
    let (id, server_name, params) = match ctx.pending_message_requests.pop_front() {
        Some(v) => v,
        None => {
            return ctx.exec(meta, "lsp-show-error 'No pending message requests.'");
        }
    };

    let options = match &params.actions {
        Some(opts) if !opts.is_empty() => &opts[..],
        _ => {
            // a ShowMessageRequest with no actions is just a ShowMessage notification.
            show_message(meta, &server_name, params.typ, &params.message, ctx);

            ctx.reply(&server_name, id.clone(), Ok(serde_json::Value::Null));
            return;
        }
    };

    let mut request_id = String::new();
    id.serialize(toml::ser::ValueSerializer::new(&mut request_id))
        .expect("cannot convert request id to toml");
    let request_id = editor_quote(&request_id);

    let option_menu_opts = options
        .iter()
        .flat_map(|item| {
            let cmd = editor_quote(&format!(
                "lsp-show-message-request-respond {} {}",
                request_id,
                editor_quote(
                    &toml::to_string(item).expect("cannot convert message action to toml")
                )
            ));
            let mut title: &str = &item.title;
            if let Some((head, _)) = title.split_once('\n') {
                title = head;
            }
            [editor_quote(title), cmd]
        })
        .map(|v| editor_quote(&v)) // double quoting for request passing
        .join(" ");
    // send the command to the editor
    ctx.exec(
        meta.clone(),
        format!(
            "lsp-show-message-request {} {} {}",
            editor_quote(&params.message),
            editor_quote(&format!(
                "%{{lsp-show-message-request-respond {}}}",
                request_id
            )),
            option_menu_opts
        ),
    );
    update_modeline(meta, ctx);
}

/// Implements ShowMessage notification.
pub fn show_message(
    meta: EditorMeta,
    server_name: &ServerName,
    typ: MessageType,
    msg: &str,
    ctx: &Context,
) {
    let command = message_type(typ).unwrap_or("nop");
    ctx.exec(
        meta,
        format!(
            "{} {} {}",
            command,
            editor_quote(server_name),
            editor_quote(msg)
        ),
    );
}

fn update_modeline(meta: EditorMeta, ctx: &Context) {
    let modeline = if ctx.pending_message_requests.is_empty() {
        Cow::from("")
    } else {
        Cow::from(format!("🔔{}", ctx.pending_message_requests.len()))
    };

    ctx.exec(
        meta,
        format!(
            "set-option global lsp_modeline_message_requests {}",
            editor_quote(&modeline),
        ),
    );
}

fn message_type(typ: MessageType) -> Option<&'static str> {
    Some(match typ {
        MessageType::ERROR => "lsp-show-message-error",
        MessageType::WARNING => "lsp-show-message-warning",
        MessageType::INFO => "lsp-show-message-info",
        MessageType::LOG => "lsp-show-message-log",
        _ => {
            warn!("Unexpected ShowMessageParams type: {:?}", typ);
            return None;
        }
    })
}
