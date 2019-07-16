use crate::context::*;
use crate::diagnostics;
use crate::general;
use crate::language_features::*;
use crate::language_server_transport;
use crate::text_sync::*;
use crate::types::*;
use crate::util::*;
use crate::workspace;
use crossbeam_channel::{select, Receiver, Sender};
use jsonrpc_core::{Call, ErrorCode, MethodCall, Output, Params};
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use lsp_types::*;

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
    let offset_encoding;
    {
        // should be fine to unwrap because request was already routed which means language is configured
        let lang = &config.language[&route.language];
        options = lang.initialization_options.clone();
        offset_encoding = lang.offset_encoding.clone();
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
        offset_encoding,
    );

    general::initialize(&route.root, options, initial_request_meta, &mut ctx);

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
                              dispatch_server_request(request, &mut ctx);
                            }
                            Call::Notification(notification) => {
                                dispatch_server_notification(
                                    &notification.method,
                                    notification.params,
                                    &mut ctx,
                                );
                            }
                            Call::Invalid {id} => {
                                error!("Invalid call from language server: {:?}", id);
                            }
                        }
                    }
                    ServerMessage::Response(output) => {
                        match output {
                            Output::Success(success) => {
                                if let Some((meta, _, mut callback)) = ctx.response_waitlist.remove(&success.id) {
                                  callback(&mut ctx, meta, success.result);
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
                            Output::Notification(notification) => {
                                dispatch_server_notification(
                                    &notification.method,
                                    notification.params,
                                    &mut ctx,
                                );
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

pub fn dispatch_pending_editor_requests(mut ctx: &mut Context) {
    let mut requests = std::mem::replace(&mut ctx.pending_requests, vec![]);

    for msg in requests.drain(..) {
        dispatch_editor_request(msg, &mut ctx);
    }
}

fn dispatch_editor_request(request: EditorRequest, mut ctx: &mut Context) {
    ensure_did_open(&request, ctx);
    let meta = request.meta;
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
        request::CodeActionRequest::METHOD => {
            codeaction::text_document_codeaction(meta, params, &mut ctx);
        }
        request::ExecuteCommand::METHOD => {
            workspace::execute_command(meta, params, &mut ctx);
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
            document_symbol::text_document_document_symbol(meta, &mut ctx);
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
        ccls::NavigateRequest::METHOD => {
            ccls::navigate(meta, params, ctx);
        }
        ccls::VarsRequest::METHOD => {
            ccls::vars(meta, params, ctx);
        }
        ccls::InheritanceRequest::METHOD => {
            ccls::inheritance(meta, params, ctx);
        }
        ccls::CallRequest::METHOD => {
            ccls::call(meta, params, ctx);
        }
        ccls::MemberRequest::METHOD => {
            ccls::member(meta, params, ctx);
        }

        _ => {
            warn!("Unsupported method: {}", method);
        }
    }
}

fn dispatch_server_request(request: MethodCall, ctx: &mut Context) {
    let method: &str = &request.method;
    match method {
        request::ApplyWorkspaceEdit::METHOD => {
            workspace::apply_edit(request.id, request.params, ctx);
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
        notification::ShowMessage::METHOD => {
            let params: ShowMessageParams = params
                .parse()
                .expect("Failed to parse ShowMessageParams params");
            ctx.exec(
                ctx.meta_for_session(),
                format!(
                    "lsp-show-message {} {}",
                    params.typ as u8,
                    editor_quote(&params.message)
                ),
            );
        }
        "window/logMessage" => {
            let params: LogMessageParams = params
                .parse()
                .expect("Failed to parse LogMessageParams params");
            ctx.exec(
                ctx.meta_for_session(),
                format!("echo -debug LSP: {}", editor_quote(&params.message)),
            );
        }
        "window/progress" => {
            let params: WindowProgress = params
                .parse()
                .expect("Failed to parse WindowProgress params");
            ctx.exec(
                ctx.meta_for_session(),
                format!(
                    "lsp-handle-progress {} {} {} {}",
                    editor_quote(&params.title),
                    editor_quote(&params.message.unwrap_or_default()),
                    editor_quote(&params.percentage.unwrap_or_default()),
                    editor_quote(params.done.map_or("", |_| "done"))
                ),
            );
        }
        "telemetry/event" => {
            debug!("{:?}", params);
        }
        _ => {
            warn!("Unsupported method: {}", method);
        }
    }
}

/// Ensure that textDocument/didOpen is sent for the given buffer before any other request, if possible.
///
/// kak-lsp tries to not bother Kakoune side of the plugin with bookkeeping status of kak-lsp server
/// itself and lsp servers run by it. It is possible that kak-lsp server or lsp server dies at some
/// point while Kakoune session is still running. That session can send a request for some already
/// open (opened before kak-lsp/lsp exit) buffer. In this case, kak-lsp/lsp server will be restarted
/// by the incoming request. `ensure_did_open` tries to sneak in `textDocument/didOpen` request for
/// this buffer then as the specification requires to send such request before other requests for
/// the file.
///
/// In a normal situation, such extra request is not required, and `ensure_did_open` short-circuits
/// most of the time in `if buffile.is_empty() || ctx.documents.contains_key(buffile)` condition.
fn ensure_did_open(request: &EditorRequest, mut ctx: &mut Context) {
    let buffile = &request.meta.buffile;
    if buffile.is_empty() || ctx.documents.contains_key(buffile) {
        return;
    };
    if request.method == notification::DidChangeTextDocument::METHOD {
        return text_document_did_open(request.meta.clone(), request.params.clone(), &mut ctx);
    }
    match std::fs::read_to_string(buffile) {
        Ok(draft) => {
            let mut params = toml::value::Table::default();
            params.insert("draft".to_string(), toml::Value::String(draft));
            text_document_did_open(request.meta.clone(), toml::Value::Table(params), &mut ctx);
        }
        Err(_) => error!(
            "Failed to read file {} to simulate textDocument/didOpen",
            buffile
        ),
    };
}
