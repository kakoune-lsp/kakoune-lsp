use context::*;
use crossbeam_channel::{Receiver, Sender};
use diagnostics;
use general;
use jsonrpc_core::{Call, ErrorCode, Output, Params};
use language_features::*;
use language_server_transport;
use languageserver_types::notification::Notification;
use languageserver_types::request::Request;
use languageserver_types::*;
use serde_json::{self, Value};
use text_sync::*;
use types::*;
use util::*;
use workspace;

/// Start controller.
///
/// Controller spawns language server for the given language and project root (passed as `route`).
/// Then it takes care of dispatching editor requests to this language server and dispatching      
/// responses back to editor.
pub fn start(
    editor_tx: Sender<EditorResponse>,
    editor_rx: &Receiver<EditorRequest>,
    is_alive: Sender<Void>,
    route: &Route,
    initial_request: EditorRequest,
    config: Config,
) {
    let lang_srv: language_server_transport::LanguageServerTransport;
    let options;
    {
        // should be fine to unwrap because request was already routed which means language is configured
        let lang = &config.language[&route.language];
        options = lang.initialization_options.clone();
        lang_srv = language_server_transport::start(&lang.command, &lang.args);
    }

    let initial_request_meta = initial_request.meta.clone();

    let mut ctx = Context::new(
        &route.language,
        initial_request,
        lang_srv.sender,
        editor_tx,
        config,
        route.root.clone(),
    );

    general::initialize(&route.root, options, &initial_request_meta, &mut ctx);

    'event_loop: loop {
        select! {
            recv(editor_rx, msg) => {
                if msg.is_none() {
                    break 'event_loop;
                }
                let msg = msg.unwrap();
                // initialize request must be first request from client to language server
                // initialized response contains capabilities which we save for future use
                // capabilities also serve as a marker of completing initialization
                // we park all requests from editor before initialization is complete
                // and then dispatch them
                if ctx.capabilities.is_some() {
                    dispatch_editor_request(msg, &mut ctx);
                } else {
                    debug!("Language server is not initialized, parking request");
                    {
                        let method: &str = &msg.method;
                        match method {
                            notification::DidOpenTextDocument::METHOD => (),
                            notification::DidChangeTextDocument::METHOD => (),
                            notification::DidCloseTextDocument::METHOD => (),
                            notification::DidSaveTextDocument::METHOD => (),
                            // TODO if auto-hover or auto-hl-references is not enabled we might want warning about parking as well
                            request::HoverRequest::METHOD => (),
                            "textDocument/referencesHighlight" => (),
                            _ => ctx.exec(
                                msg.meta.clone(),
                                "lsp-show-error 'Language server is not initialized, parking request'"
                                    .to_string(),
                            ),
                        }
                    }
                    ctx.pending_requests.push(msg);
                }
            }
            recv(lang_srv.receiver, msg) => {
                if msg.is_none() {
                    break 'event_loop;
                }
                let msg = msg.unwrap();
                match msg {
                    ServerMessage::Request(call) => {
                        match call {
                            Call::MethodCall(request) => {
                                debug!(
                                    "Requests from language server are not supported yet: {:?}",
                                    request
                                );
                            }
                            Call::Notification(notification) => {
                                if notification.params.is_none() {
                                    error!("Missing notification params");
                                    return;
                                }
                                dispatch_server_notification(
                                    &notification.method,
                                    notification.params.unwrap(),
                                    &mut ctx,
                                );
                            }
                            Call::Invalid(m) => {
                                error!("Invalid call from language server: {:?}", m);
                            }
                        }
                    }
                    ServerMessage::Response(output) => {
                        match output {
                            Output::Success(success) => {
                                if let Some(request) = ctx.response_waitlist.remove(&success.id) {
                                    let (meta, method, params) = request;
                                    dispatch_server_response(
                                        &meta,
                                        &method,
                                        params,
                                        success.result,
                                        &mut ctx,
                                    );
                                } else {
                                    error!("Id {:?} is not in waitlist!", success.id);
                                }
                            }
                            Output::Failure(failure) => {
                                error!("Error response from server: {:?}", failure);
                                if let Some(request) = ctx.response_waitlist.remove(&failure.id) {
                                    let (meta, method, _) = request;
                                    let msg = match failure.error.code {
                                        ErrorCode::MethodNotFound => format!(
                                            "{} language server doesn't support method {}",
                                            ctx.language_id, method
                                        ),
                                        _ => format!(
                                            "{} language server error: {}",
                                            ctx.language_id, editor_quote(&failure.error.message)
                                        ),
                                    };
                                    ctx.exec(meta, format!("lsp-show-error {}", editor_quote(&msg)));
                                } else {
                                    error!("Id {:?} is not in waitlist!", failure.id);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // signal to session that it's okay to join controller thread
    drop(is_alive);
    // signal to language server transport to stop writer thread
    drop(ctx.lang_srv_tx);
    if lang_srv.thread.join().is_err() {
        error!("Language thread panicked");
    };
}

fn dispatch_editor_request(request: EditorRequest, mut ctx: &mut Context) {
    ensure_did_open(&request, ctx);
    let meta = &request.meta;
    let params = request.params;
    let method: &str = &request.method;
    match method {
        notification::DidOpenTextDocument::METHOD => {
            text_document_did_open(meta, params, &mut ctx);
        }
        notification::DidChangeTextDocument::METHOD => {
            text_document_did_change(meta, params, &mut ctx);
        }
        notification::DidCloseTextDocument::METHOD => {
            text_document_did_close(meta, &mut ctx);
        }
        notification::DidSaveTextDocument::METHOD => {
            text_document_did_save(meta, &mut ctx);
        }
        notification::DidChangeConfiguration::METHOD => {
            workspace::did_change_configuration(params, &mut ctx);
        }
        request::Completion::METHOD => {
            completion::text_document_completion(meta, params, &mut ctx);
        }
        request::HoverRequest::METHOD => {
            hover::text_document_hover(meta, params, &mut ctx);
        }
        request::GotoDefinition::METHOD => {
            definition::text_document_definition(meta, params, &mut ctx);
        }
        request::References::METHOD => {
            references::text_document_references(meta, params, &mut ctx);
        }
        notification::Exit::METHOD => {
            general::exit(&mut ctx);
        }
        request::SignatureHelpRequest::METHOD => {
            signature_help::text_document_signature_help(meta, params, &mut ctx);
        }
        request::DocumentSymbolRequest::METHOD => {
            document_symbol::text_document_document_symbol(meta, params, &mut ctx);
        }
        request::Formatting::METHOD => {
            formatting::text_document_formatting(meta, params, &mut ctx);
        }
        request::WorkspaceSymbol::METHOD => {
            workspace::workspace_symbol(meta, params, &mut ctx);
        }
        request::Rename::METHOD => {
            rename::text_document_rename(meta, params, &mut ctx);
        }
        "textDocument/diagnostics" => {
            diagnostics::editor_diagnostics(meta, &mut ctx);
        }
        "capabilities" => {
            general::capabilities(meta, &mut ctx);
        }
        "textDocument/referencesHighlight" => {
            references::text_document_references_highlight(meta, params, &mut ctx);
        }

        // CCLS
        "$ccls/navigate" => {
            ccls::navigate(meta, params, ctx);
        }
        "$ccls/vars" => {
            ccls::vars(meta, params, ctx);
        }
        "$ccls/inheritance" => {
            ccls::inheritance(meta, params, ctx);
        }
        "$ccls/call" => {
            ccls::call(meta, params, ctx);
        }
        "$ccls/member" => {
            ccls::member(meta, params, ctx);
        }

        _ => {
            warn!("Unsupported method: {}", method);
        }
    }
}

fn dispatch_server_notification(method: &str, params: Params, mut ctx: &mut Context) {
    match method {
        notification::PublishDiagnostics::METHOD => {
            diagnostics::publish_diagnostics(params, &mut ctx);
        }
        "$cquery/publishSemanticHighlighting" => {
            cquery::publish_semantic_highlighting(params, &mut ctx);
        }
        "$ccls/publishSemanticHighlight" => {
            ccls::publish_semantic_highlighting(params, &mut ctx);
        }
        notification::Exit::METHOD => {
            debug!("Language server exited");
        }
        "window/logMessage" => {
            debug!("{:?}", params);
        }
        "window/progress" => {
            debug!("{:?}", params);
        }
        "telemetry/event" => {
            debug!("{:?}", params);
        }
        _ => {
            warn!("Unsupported method: {}", method);
        }
    }
}

fn dispatch_server_response(
    meta: &EditorMeta,
    method: &str,
    params: EditorParams,
    response: Value,
    mut ctx: &mut Context,
) {
    match method {
        request::Completion::METHOD => {
            completion::editor_completion(meta, params, response, &mut ctx);
        }
        request::HoverRequest::METHOD => {
            hover::editor_hover(meta, params, response, &mut ctx);
        }
        request::GotoDefinition::METHOD => {
            definition::editor_definition(meta, response, &mut ctx);
        }
        request::References::METHOD => {
            references::editor_references(meta, response, &mut ctx);
        }
        request::SignatureHelpRequest::METHOD => {
            signature_help::editor_signature_help(meta, params, response, &mut ctx);
        }
        request::DocumentSymbolRequest::METHOD => {
            document_symbol::editor_document_symbol(meta, response, &mut ctx);
        }
        request::Formatting::METHOD => {
            formatting::editor_formatting(meta, params, response, &mut ctx);
        }
        request::WorkspaceSymbol::METHOD => {
            workspace::editor_workspace_symbol(meta, response, &mut ctx);
        }
        request::Rename::METHOD => {
            rename::editor_rename(meta, params, response, &mut ctx);
        }
        "textDocument/referencesHighlight" => {
            references::editor_references_highlight(meta, response, &mut ctx);
        }
        request::Initialize::METHOD => {
            ctx.capabilities = Some(
                serde_json::from_value::<InitializeResult>(response)
                    .expect("Failed to parse initialized response")
                    .capabilities,
            );
            ctx.notify(
                notification::Initialized::METHOD.into(),
                InitializedParams {},
            );
            let mut requests = Vec::with_capacity(ctx.pending_requests.len());
            for msg in ctx.pending_requests.drain(..) {
                requests.push(msg);
            }

            for msg in requests.drain(..) {
                dispatch_editor_request(msg, &mut ctx);
            }
        }
        "$ccls/navigate" => {
            ccls::navigate_response(meta, response, &mut ctx);
        }
        "$ccls/vars" => {
            references::editor_references(meta, response, &mut ctx);
        }
        "$ccls/inheritance" => {
            references::editor_references(meta, response, &mut ctx);
        }
        "$ccls/call" => {
            references::editor_references(meta, response, &mut ctx);
        }
        "$ccls/member" => {
            references::editor_references(meta, response, &mut ctx);
        }
        _ => {
            error!("Don't know how to handle response for method: {}", method);
        }
    }
}

fn ensure_did_open(request: &EditorRequest, mut ctx: &mut Context) {
    let buffile = &request.meta.buffile;
    if buffile.is_empty() || ctx.versions.contains_key(buffile) {
        return;
    };
    if request.method == notification::DidChangeTextDocument::METHOD {
        return text_document_did_open(&request.meta, request.params.clone(), &mut ctx);
    }
    match std::fs::read_to_string(buffile) {
        Ok(draft) => {
            let mut params = toml::value::Table::default();
            params.insert("draft".to_string(), toml::Value::String(draft));
            text_document_did_open(&request.meta, toml::Value::Table(params), &mut ctx);
        }
        Err(_) => error!(
            "Failed to read file {} to simulate textDocument/didOpen",
            buffile
        ),
    };
}
