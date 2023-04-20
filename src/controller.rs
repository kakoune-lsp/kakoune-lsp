use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::mem;
use std::time::Duration;

use crate::capabilities;
use crate::capabilities::initialize;
use crate::context::*;
use crate::diagnostics;
use crate::language_features::{selection_range, *};
use crate::language_server_transport;
use crate::progress;
use crate::show_message;
use crate::text_sync::*;
use crate::thread_worker::Worker;
use crate::types::*;
use crate::util::*;
use crate::workspace;
use crossbeam_channel::{never, select, tick, Receiver, Sender};
use jsonrpc_core::{Call, ErrorCode, MethodCall, Output, Params};
use lsp_types::error_codes::CONTENT_MODIFIED;
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use lsp_types::*;
use serde::Serialize;

/// Start controller.
///
/// Controller spawns language server for the given language and project root (passed as `route`).
/// Then it takes care of dispatching editor requests to this language server and dispatching
/// responses back to editor.
pub fn start(
    to_editor: Sender<EditorResponse>,
    from_editor: Receiver<EditorRequest>,
    route: &Route,
    initial_request: EditorRequest,
    config: Config,
) {
    let lang_srv: language_server_transport::LanguageServerTransport;
    let offset_encoding;
    {
        // should be fine to unwrap because request was already routed which means language is configured
        let lang = &config.language[&route.language];
        offset_encoding = lang.offset_encoding;
        lang_srv = match language_server_transport::start(&lang.command, &lang.args, &lang.envs) {
            Ok(ls) => ls,
            Err(err) => {
                error!("failed to start language server: {}", err);
                // If the server command isn't from a hook (e.g. auto-hover),
                // then send a prominent error to the editor.
                if !initial_request.meta.hook {
                    let command = format!(
                        "lsp-show-error {}",
                        editor_quote(&format!("failed to start language server: {}", err)),
                    );
                    if to_editor
                        .send(EditorResponse {
                            meta: initial_request.meta,
                            command: Cow::from(command),
                        })
                        .is_err()
                    {
                        error!("Failed to send command to editor");
                    }
                }
                return;
            }
        }
    }

    let mut initial_request_meta = initial_request.meta.clone();
    initial_request_meta.buffile = "".to_string();
    initial_request_meta.fifo = None;
    initial_request_meta.write_response_to_fifo = false;

    let mut ctx = Context::new(ContextBuilder {
        language_id: route.language.clone(),
        initial_request,
        lang_srv_tx: lang_srv.to_lang_server.sender().clone(),
        editor_tx: to_editor,
        config,
        root_path: route.root.clone(),
        offset_encoding,
    });

    initialize(&route.root, initial_request_meta.clone(), &mut ctx);

    struct FileWatcher {
        pending_file_events: HashSet<FileEvent>,
        worker: Worker<(), Vec<FileEvent>>,
    }
    let mut file_watcher: Option<FileWatcher> = None;

    'event_loop: loop {
        select! {
            recv(from_editor) -> msg => {
                if msg.is_err() {
                    break 'event_loop;
                }
                let msg = msg.unwrap();
                // initialize request must be first request from client to language server
                // initialized response contains capabilities which we save for future use
                // capabilities also serve as a marker of completing initialization
                // we park all requests from editor before initialization is complete
                // and then dispatch them
                if ctx.capabilities.is_some() {
                    dispatch_incoming_editor_request(msg, &mut ctx);
                } else {
                    debug!("Language server is not initialized, parking request");
                    let err = "lsp-show-error 'language server is not initialized, parking request'";
                    match &*msg.method {
                        notification::DidOpenTextDocument::METHOD => (),
                        notification::DidChangeTextDocument::METHOD => (),
                        notification::DidCloseTextDocument::METHOD => (),
                        notification::DidSaveTextDocument::METHOD => (),
                        _ => if !msg.meta.hook {
                                ctx.exec(msg.meta.clone(), err.to_string());
                        }
                    }
                    ctx.pending_requests.push(msg);
                }
            }
            recv(lang_srv.from_lang_server.receiver()) -> msg => {
                if msg.is_err() {
                    break 'event_loop;
                }
                let msg = msg.unwrap();
                match msg {
                    ServerMessage::Request(call) => {
                        match call {
                            Call::MethodCall(request) => {
                              dispatch_server_request(initial_request_meta.clone(), request, &mut ctx);
                            }
                            Call::Notification(notification) => {
                                dispatch_server_notification(
                                    initial_request_meta.clone(),
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
                                if let Some((meta, method, batch_id, canceled)) = ctx.response_waitlist.remove(&success.id) {
                                    if canceled {
                                        continue;
                                    }
                                    remove_outstanding_request(&mut ctx, method, meta.buffile.clone(), meta.client.clone(), &success.id);
                                    if meta.write_response_to_fifo {
                                        write_response_to_fifo(meta, &success);
                                        continue;
                                    }
                                    if let Some((batch_amt, mut vals, callback)) = ctx.batches.remove(&batch_id) {
                                        vals.push(success.result);
                                        if batch_amt == 1 {
                                            callback(&mut ctx, meta, vals);
                                        } else {
                                            ctx.batches.insert(batch_id, (batch_amt - 1, vals, callback));
                                        }
                                    }
                                } else {
                                    error!("Id {:?} is not in waitlist!", success.id);
                                }
                            }
                            Output::Failure(failure) => {
                                if let Some(request) = ctx.response_waitlist.remove(&failure.id) {
                                    let (meta, method, _, canceled) = request;
                                    if canceled {
                                        continue;
                                    }
                                    remove_outstanding_request(&mut ctx, method, meta.buffile.clone(), meta.client.clone(), &failure.id);
                                    error!("Error response from server: {:?}", failure);
                                    if meta.write_response_to_fifo {
                                        write_response_to_fifo(meta, failure);
                                        continue;
                                    }
                                    match failure.error.code {
                                        code if code == ErrorCode::ServerError(CONTENT_MODIFIED) || method == request::CodeActionRequest::METHOD => {
                                            // Nothing to do, but sending command back to the editor is required to handle case when
                                            // editor is blocked waiting for response via fifo.
                                            ctx.exec(meta, "nop".to_string());
                                        },
                                        code => {
                                            let msg = match code {
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
                                        }
                                    }
                                } else {
                                    error!("Error response from server: {:?}", failure);
                                    error!("Id {:?} is not in waitlist!", failure.id);
                                }
                            }
                        }
                    }
                }
            }
            recv(file_watcher.as_ref().map(|fw| fw.worker.receiver()).unwrap_or(&never())) -> msg => {
                if msg.is_err() {
                    break 'event_loop;
                }
                let mut file_events = msg.unwrap();
                info!("received {} events from file watcher", file_events.len());
                // Enqueue the events from the file watcher.
                file_watcher.as_mut().unwrap().pending_file_events.extend(file_events.drain(..));
            }
            recv(file_watcher.as_ref().and_then(
                // If there are enqueud events, let's wait a bit for others to come in, to send
                // them in batch.
                |fw| if fw.pending_file_events.is_empty() { None } else {
                    Some(tick(Duration::from_millis(500)))
                })
                .unwrap_or_else(never)
            ) -> _ => {
                let fw = file_watcher.as_mut().unwrap();
                if !fw.pending_file_events.is_empty() {
                    let file_events = fw.pending_file_events.drain().collect();
                    workspace_did_change_watched_files(file_events, &mut ctx);
                    fw.pending_file_events.clear();
                }
            }
        }
        // Did the language server request us to watch for file changes?
        if !ctx.pending_file_watchers.is_empty() {
            let mut requested_watchers = HashMap::default();
            mem::swap(&mut ctx.pending_file_watchers, &mut requested_watchers);
            // If there's an existing watcher, ask nicely to terminate.
            if let Some(ref fw) = file_watcher.as_mut() {
                info!("stopping stale file watcher");
                if let Err(err) = fw.worker.sender().send(()) {
                    error!("{}", err);
                }
            }
            file_watcher = Some(FileWatcher {
                pending_file_events: HashSet::new(),
                worker: spawn_file_watcher(route.root.clone(), requested_watchers),
            });
        }
    }
}

pub fn write_response_to_fifo<T: Serialize>(meta: EditorMeta, response: T) {
    let json = serde_json::to_string_pretty(&response).unwrap();
    let fifo = meta.fifo.expect("Need fifo to write response to");
    std::fs::write(fifo, (json + "\n").as_bytes()).expect("Failed to write JSON response to fifo");
}

pub fn dispatch_pending_editor_requests(ctx: &mut Context) {
    let mut requests = std::mem::take(&mut ctx.pending_requests);

    for msg in requests.drain(..) {
        dispatch_editor_request(msg, ctx);
    }
}

fn dispatch_incoming_editor_request(request: EditorRequest, ctx: &mut Context) {
    let method: &str = &request.method;
    let document_version = {
        let buffile = &request.meta.buffile;
        ctx.documents
            .get(buffile)
            .map(|doc| doc.version)
            .unwrap_or(0)
    };
    if document_version > request.meta.version {
        debug!(
            "incoming request {} is stale, version {} but I already have {}",
            request.method, request.meta.version, document_version
        );
        // Keep it nevertheless because at least "completionItem/resolve" is useful.
    }
    if request.meta.fifo.is_none() {
        let notifications = &[
            notification::DidOpenTextDocument::METHOD,
            notification::DidChangeTextDocument::METHOD,
            notification::DidCloseTextDocument::METHOD,
            notification::DidSaveTextDocument::METHOD,
            notification::DidChangeConfiguration::METHOD,
            notification::Exit::METHOD,
            notification::WorkDoneProgressCancel::METHOD,
        ];

        if !request.meta.buffile.is_empty()
            && document_version < request.meta.version
            && !notifications.contains(&method)
            // InsertIdle is not triggered while the completion pager is active, so let's
            // smuggle completion-related requests through.
            && method != request::ResolveCompletionItem::METHOD
        {
            // Wait for buffer update.
            ctx.pending_requests.push(request);
            return;
        }
    };
    let version_bump = [
        notification::DidOpenTextDocument::METHOD,
        notification::DidChangeTextDocument::METHOD,
    ]
    .contains(&method);

    dispatch_editor_request(request, ctx);

    if !version_bump {
        return;
    }
    let mut requests = std::mem::take(&mut ctx.pending_requests);
    requests.retain_mut(|request| {
        let buffile = &request.meta.buffile;
        let document = match ctx.documents.get(buffile) {
            Some(document) => document,
            None => return true,
        };
        if document.version < request.meta.version {
            return true;
        }
        info!(
            "dispatching pending request {} because we have received matching version in didChange",
            request.method
        );
        if document.version > request.meta.version {
            debug!(
                "pending request {} is stale, version {} but I already have {}",
                request.method, request.meta.version, document.version
            );
            // Keep it nevertheless because at least "completionItem/resolve" is useful.
        }
        dispatch_editor_request(std::mem::take(request), ctx);
        false
    });
    assert!(ctx.pending_requests.is_empty());
    ctx.pending_requests = std::mem::take(&mut requests);
}

fn dispatch_editor_request(request: EditorRequest, ctx: &mut Context) {
    ensure_did_open(&request, ctx);
    let method: &str = &request.method;
    let meta = request.meta;
    let params = request.params;
    match method {
        notification::DidOpenTextDocument::METHOD => {
            text_document_did_open(meta, params, ctx);
        }
        notification::DidChangeTextDocument::METHOD => {
            text_document_did_change(meta, params, ctx);
        }
        notification::DidCloseTextDocument::METHOD => {
            text_document_did_close(meta, ctx);
        }
        notification::DidSaveTextDocument::METHOD => {
            text_document_did_save(meta, ctx);
        }
        notification::DidChangeConfiguration::METHOD => {
            workspace::did_change_configuration(meta, params, ctx);
        }
        request::CallHierarchyPrepare::METHOD => {
            call_hierarchy::call_hierarchy_prepare(meta, params, ctx);
        }
        request::Completion::METHOD => {
            completion::text_document_completion(meta, params, ctx);
        }
        request::ResolveCompletionItem::METHOD => {
            completion::completion_item_resolve(meta, params, ctx);
        }
        request::CodeActionRequest::METHOD => {
            code_action::text_document_code_action(meta, params, ctx);
        }
        request::CodeActionResolveRequest::METHOD => {
            code_action::text_document_code_action_resolve(meta, params, ctx);
        }
        request::ExecuteCommand::METHOD => {
            workspace::execute_command(meta, params, ctx);
        }
        request::HoverRequest::METHOD => {
            hover::text_document_hover(meta, params, ctx);
        }
        request::GotoDefinition::METHOD => {
            goto::text_document_definition(false, meta, params, ctx);
        }
        request::GotoDeclaration::METHOD => {
            goto::text_document_definition(true, meta, params, ctx);
        }
        request::GotoImplementation::METHOD => {
            goto::text_document_implementation(meta, params, ctx);
        }
        request::GotoTypeDefinition::METHOD => {
            goto::text_document_type_definition(meta, params, ctx);
        }
        request::References::METHOD => {
            goto::text_document_references(meta, params, ctx);
        }
        notification::Exit::METHOD => {
            ctx.notify::<notification::Exit>(());
        }
        notification::WorkDoneProgressCancel::METHOD => {
            progress::work_done_progress_cancel(meta, params, ctx);
        }
        request::SelectionRangeRequest::METHOD => {
            selection_range::text_document_selection_range(meta, params, ctx);
        }
        request::SignatureHelpRequest::METHOD => {
            signature_help::text_document_signature_help(meta, params, ctx);
        }
        request::DocumentHighlightRequest::METHOD => {
            highlight::text_document_highlight(meta, params, ctx);
        }
        request::DocumentSymbolRequest::METHOD => {
            document_symbol::text_document_document_symbol(meta, ctx);
        }
        "kak-lsp/next-or-previous-symbol" => {
            document_symbol::next_or_prev_symbol(meta, params, ctx);
        }
        "kak-lsp/object" => {
            document_symbol::object(meta, params, ctx);
        }
        "kak-lsp/goto-document-symbol" => {
            document_symbol::document_symbol_menu(meta, params, ctx);
        }
        "kak-lsp/textDocument/codeLens" => {
            code_lens::resolve_and_perform_code_lens(meta, params, ctx);
        }
        request::Formatting::METHOD => {
            formatting::text_document_formatting(meta, params, ctx);
        }
        request::RangeFormatting::METHOD => {
            range_formatting::text_document_range_formatting(meta, params, ctx);
        }
        request::WorkspaceSymbolRequest::METHOD => {
            workspace::workspace_symbol(meta, params, ctx);
        }
        request::Rename::METHOD => {
            rename::text_document_rename(meta, params, ctx);
        }
        "textDocument/diagnostics" => {
            diagnostics::editor_diagnostics(meta, ctx);
        }
        "capabilities" => {
            capabilities::capabilities(meta, ctx);
        }
        "apply-workspace-edit" => {
            workspace::apply_edit_from_editor(meta, params, ctx);
        }
        request::SemanticTokensFullRequest::METHOD => {
            semantic_tokens::tokens_request(meta, ctx);
        }

        request::InlayHintRequest::METHOD => {
            inlay_hints::inlay_hints(meta, params, ctx);
        }

        show_message::SHOW_MESSAGE_REQUEST_NEXT => {
            show_message::show_message_request_next(meta, ctx);
        }
        show_message::SHOW_MESSAGE_REQUEST_RESPOND => {
            show_message::show_message_request_respond(params, ctx);
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

        // clangd
        clangd::SwitchSourceHeaderRequest::METHOD => {
            clangd::switch_source_header(meta, ctx);
        }

        // eclipse.jdt.ls
        "eclipse.jdt.ls/organizeImports" => {
            eclipse_jdt_ls::organize_imports(meta, ctx);
        }

        // rust-analyzer
        rust_analyzer::ExpandMacroRequest::METHOD => {
            rust_analyzer::expand_macro(meta, params, ctx);
        }

        // texlab
        texlab::Build::METHOD => {
            texlab::build(meta, params, ctx);
        }
        texlab::ForwardSearch::METHOD => {
            texlab::forward_search(meta, params, ctx);
        }

        _ => {
            warn!("Unsupported method: {}", method);
        }
    }
}

fn dispatch_server_request(meta: EditorMeta, request: MethodCall, ctx: &mut Context) {
    let method: &str = &request.method;
    let result = match method {
        request::ApplyWorkspaceEdit::METHOD => {
            workspace::apply_edit_from_server(request.params, ctx)
        }
        request::RegisterCapability::METHOD => {
            let params: RegistrationParams = request
                .params
                .parse()
                .expect("Failed to parse RegistrationParams params");
            for registration in params.registrations {
                match registration.method.as_str() {
                    notification::DidChangeWatchedFiles::METHOD => {
                        register_workspace_did_change_watched_files(
                            registration.register_options,
                            ctx,
                        )
                    }
                    notification::DidChangeWorkspaceFolders::METHOD => {
                        // Since we only support one root path, we are never going to send
                        // "workspace/didChangeWorkspaceFolders" anyway, so let's not issue a warning.
                        continue;
                    }
                    _ => warn!("Unsupported registration: {}", registration.method),
                }
            }
            Ok(serde_json::Value::Null)
        }
        request::WorkspaceFoldersRequest::METHOD => {
            Ok(serde_json::to_value(vec![WorkspaceFolder {
                uri: Url::from_file_path(&ctx.root_path).unwrap(),
                name: ctx.root_path.to_string(),
            }])
            .ok()
            .unwrap())
        }
        request::WorkDoneProgressCreate::METHOD => {
            progress::work_done_progress_create(request.params, ctx)
        }
        request::WorkspaceConfiguration::METHOD => workspace::configuration(request.params, ctx),
        request::ShowMessageRequest::METHOD => {
            return show_message::show_message_request(meta, request, ctx);
        }
        _ => {
            warn!("Unsupported method: {}", method);
            Err(jsonrpc_core::Error::new(
                jsonrpc_core::ErrorCode::MethodNotFound,
            ))
        }
    };

    ctx.reply(request.id, result);
}

fn dispatch_server_notification(meta: EditorMeta, method: &str, params: Params, ctx: &mut Context) {
    match method {
        notification::Progress::METHOD => {
            progress::dollar_progress(meta, params, ctx);
        }
        notification::PublishDiagnostics::METHOD => {
            diagnostics::publish_diagnostics(params, ctx);
        }
        "$cquery/publishSemanticHighlighting" => {
            cquery::publish_semantic_highlighting(params, ctx);
        }
        "$ccls/publishSemanticHighlight" => {
            ccls::publish_semantic_highlighting(params, ctx);
        }
        notification::Exit::METHOD => {
            debug!("Language server exited");
        }
        notification::ShowMessage::METHOD => {
            let params: ShowMessageParams = params
                .parse()
                .expect("Failed to parse ShowMessageParams params");
            show_message::show_message(meta, params.typ, &params.message, ctx);
        }
        "window/logMessage" => {
            let params: LogMessageParams = params
                .parse()
                .expect("Failed to parse LogMessageParams params");
            ctx.exec(
                meta,
                format!("lsp-show-message-log {}", editor_quote(&params.message)),
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
fn ensure_did_open(request: &EditorRequest, ctx: &mut Context) {
    let buffile = &request.meta.buffile;
    if buffile.is_empty() || ctx.documents.contains_key(buffile) {
        return;
    };
    if request.method == notification::DidChangeTextDocument::METHOD {
        text_document_did_open(request.meta.clone(), request.params.clone(), ctx);
        return;
    }
    match read_document(buffile) {
        Ok(draft) => {
            let mut params = toml::value::Table::default();
            params.insert("draft".to_string(), toml::Value::String(draft));
            text_document_did_open(request.meta.clone(), toml::Value::Table(params), ctx);
        }
        Err(err) => error!(
            "Failed to read file {} to simulate textDocument/didOpen: {}",
            buffile, err
        ),
    };
}
