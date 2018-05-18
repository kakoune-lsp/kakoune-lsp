use context::*;
use languageserver_types::notification::Notification;
use languageserver_types::request::Request;
use languageserver_types::*;
use std::process;
use toml;
use types::*;
use util::*;

pub fn initialize(root_path: &str, meta: &EditorMeta, ctx: &mut Context) {
    let params = InitializeParams {
        capabilities: ClientCapabilities {
            workspace: Some(WorkspaceClientCapabilities::default()),
            text_document: Some(TextDocumentClientCapabilities::default()),
            experimental: None,
        },
        initialization_options: None,
        process_id: Some(process::id().into()),
        root_uri: Some(path_to_uri(root_path)),
        root_path: Some(root_path.to_string()),
        trace: Some(TraceOption::Off),
    };

    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (
            meta.clone(),
            request::Initialize::METHOD.into(),
            toml::Value::Table(toml::value::Table::default()),
        ),
    );
    ctx.call(id, request::Initialize::METHOD.into(), params);
}

pub fn exit(_params: EditorParams, _meta: &EditorMeta, ctx: &mut Context) {
    // NOTE we can't use Params::None because it's serialized as Value::Array([])
    let params: Option<u8> = None;
    ctx.notify(notification::Exit::METHOD.into(), params);
    ctx.lang_srv_poison_tx
        .send(())
        .expect("Failed to poison language server");
    ctx.controller_poison_tx
        .send(())
        .expect("Failed to poison controller");
}
