use context::*;
use crossbeam_channel::{bounded, Receiver, Sender};
use diagnostics;
use editor_transport;
use fnv::FnvHashMap;
use general;
use jsonrpc_core::{Call, Output, Params};
use language_features::*;
use language_server_transport;
use languageserver_types::notification::Notification;
use languageserver_types::request::Request;
use languageserver_types::*;
use project_root::find_project_root;
use serde::Deserialize;
use serde_json::{self, Value};
use slog::Logger;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use text_sync::*;
use toml;
use types::*;

fn get_server_cmd(config: &Config, language_id: &str) -> Option<(String, Vec<String>)> {
    if let Some(language) = config.language.get(language_id) {
        return Some((language.command.clone(), language.args.clone()));
    }
    None
}

fn get_language_id(extensions: &FnvHashMap<String, String>, path: &str) -> Option<String> {
    extensions
        .get(Path::new(path).extension()?.to_str()?)
        .cloned()
}

pub fn start(config: &Config, logger: Logger) {
    info!(logger, "Starting Controller");
    let (editor_tx, editor_rx) = editor_transport::start(config, logger.clone());
    let mut extensions = FnvHashMap::default();
    for (language_id, language) in &config.language {
        for extension in &language.extensions {
            extensions.insert(extension.clone(), language_id.clone());
        }
    }
    let extensions = extensions;
    let languages = config.language.clone();
    let mut controllers: FnvHashMap<Route, Sender<EditorRequest>> = FnvHashMap::default();
    let (controller_remove_tx, controller_remove_rx) = bounded(1);
    'event_loop: loop {
        select_loop! {
            recv(editor_rx, request) => {
                if request.method == notification::Exit::METHOD {
                    info!(
                        logger,
                        "Session `{}` closed, shutting down associated language servers",
                        request.meta.session
                    );
                    for k in controllers.keys().map(|k| k.clone()).collect::<Vec<_>>() {
                        if k.0 == request.meta.session {
                            let controller_tx = controllers.remove(&k).unwrap();
                            debug!(logger, "Exit {} in project {}", k.1, k.2);
                            controller_tx
                                .send(request.clone())
                                .expect("Failed to route editor request");
                        }
                    }
                    continue 'event_loop;
                }
                let language_id = get_language_id(&extensions, &request.meta.buffile);
                if language_id.is_none() {
                    debug!(
                        logger,
                        "Language server is not configured for extension `{}`",
                        Path::new(&request.meta.buffile)
                            .extension()
                            .unwrap_or_default()
                            .to_str()
                            .unwrap_or_default()
                    );
                    continue 'event_loop;
                }
                let language_id = language_id.unwrap();
                let root_path = find_project_root(&languages[&language_id].roots, &request.meta.buffile);
                if root_path.is_none() {
                    debug!(
                        logger,
                        "Unable to detect project root for file `{}`", request.meta.buffile
                    );
                    continue 'event_loop;
                }
                let root_path = root_path.unwrap();
                let route = (
                    request.meta.session.clone(),
                    language_id.clone(),
                    root_path.clone(),
                );
                debug!(logger, "Routing editor request to {:?}", route);
                let controller = controllers.get(&route).cloned();
                match controller {
                    Some(controller_tx) => {
                        controller_tx
                            .send(request)
                            .expect("Failed to route editor request");
                    }
                    None => {
                        // because Kakoune triggers BufClose after KakEnd
                        // we don't want textDocument/didClose to start server
                        if request.method == notification::DidCloseTextDocument::METHOD {
                            continue 'event_loop;
                        }
                        let (lang_srv_cmd, lang_srv_args) = get_server_cmd(config, &language_id).unwrap();
                        // NOTE 1024 is arbitrary
                        let (controller_tx, controller_rx) = bounded(1024);
                        controllers.insert(route.clone(), controller_tx);
                        let editor_tx = editor_tx.clone();
                        let logger = logger.clone();
                        let (controller_poison_tx, controller_poison_rx) = bounded(1);
                        let (controller_poison_tx_mult, controller_poison_rx_mult) = bounded(1);
                        let controller_remove_tx = controller_remove_tx.clone();
                        let route_mult = route.clone();
                        thread::spawn(move || {
                            for _ in controller_poison_rx_mult {
                                controller_remove_tx.send(route_mult.clone()).unwrap();
                                controller_poison_tx.send(()).unwrap();
                            }
                        });
                        thread::spawn(move || {
                            let (lang_srv_tx, lang_srv_rx, lang_srv_poison_tx) =
                                language_server_transport::start(
                                    &lang_srv_cmd,
                                    &lang_srv_args,
                                    logger.clone(),
                                );
                            let controller = Controller::start(
                                &language_id,
                                &root_path,
                                lang_srv_tx,
                                lang_srv_rx,
                                editor_tx,
                                controller_rx,
                                lang_srv_poison_tx,
                                controller_poison_tx_mult,
                                controller_poison_rx,
                                request,
                                logger.clone(),
                            );
                            controller.wait().expect("Failed to wait for controller");
                            debug!(logger, "Controller {:?} exited", route);
                        });
                    }
                }
            }
            recv(controller_remove_rx, route) => {
                controllers.remove(&route);
                debug!(logger, "Controller {:?} removed", route);
                continue 'event_loop;
            }
        }
    }
}

struct Controller {
    editor_reader_handle: JoinHandle<()>,
    lang_srv_handle: JoinHandle<()>,
}

impl Controller {
    fn start(
        language_id: &str,
        root_path: &str,
        lang_srv_tx: Sender<ServerMessage>,
        lang_srv_rx: Receiver<ServerMessage>,
        editor_tx: Sender<EditorResponse>,
        editor_rx: Receiver<EditorRequest>,
        lang_srv_poison_tx: Sender<()>,
        controller_poison_tx: Sender<()>,
        controller_poison_rx: Receiver<()>,
        initial_request: EditorRequest,
        logger: Logger,
    ) -> Self {
        let (editor_reader_poison_tx, editor_reader_poison_rx) = bounded(1);
        let (lang_srv_reader_poison_tx, lang_srv_reader_poison_rx) = bounded(1);
        thread::spawn(move || {
            for msg in controller_poison_rx {
                editor_reader_poison_tx.send(msg).unwrap();
                lang_srv_reader_poison_tx.send(msg).unwrap();
            }
        });
        let initial_request_meta = initial_request.meta.clone();
        let ctx_src = Arc::new(Mutex::new(Context::new(
            language_id,
            initial_request,
            lang_srv_tx,
            editor_tx,
            lang_srv_poison_tx,
            controller_poison_tx,
            logger.clone(),
        )));

        let ctx = Arc::clone(&ctx_src);
        let editor_reader_logger = logger.clone();
        let editor_reader_handle = thread::spawn(move || {
            loop {
                select_loop! {
                    recv(editor_rx, msg) =>{
                        let mut ctx = ctx.lock().expect("Failed to lock context");
                        // initialize request must be first requst from client to language server
                        // initialized response contains capabilities which we save for future use
                        // capabilities also serve as a marker of completing initialization
                        // we park all requests from editor before initialization is complete
                        // and then dispatch them
                        if ctx.capabilities.is_some() {
                            dispatch_editor_request(msg, &mut ctx);
                        } else {
                        debug!(editor_reader_logger, "Language server is not initialized, parking request");
                            ctx.pending_requests.push(msg);
                        }
                    }
                    recv(editor_reader_poison_rx, _) => {
                        debug!(editor_reader_logger, "Stopping editor dispatcher");
                        return;
                    }
                }
            }
        });

        let ctx = Arc::clone(&ctx_src);
        let lang_srv_logger = logger.clone();
        let lang_srv_handle = thread::spawn(move || loop {
            select_loop! {
                recv(lang_srv_rx, msg) => {
                    match msg {
                        ServerMessage::Request(call) => {
                            let mut ctx = ctx.lock().expect("Failed to lock context");
                            match call {
                                Call::MethodCall(request) => {
                                    debug!(lang_srv_logger, "Requests from language server are not supported yet: {:?}", request);
                                }
                                Call::Notification(notification) => {
                                    dispatch_server_notification(
                                        &notification.method,
                                        notification.params.unwrap(),
                                        &mut ctx,
                                    );
                                }
                                Call::Invalid(m) => {
                                    error!(lang_srv_logger, "Invalid call from language server: {:?}", m);
                                }
                            }
                        }
                        ServerMessage::Response(output) => {
                            let mut ctx = ctx.lock().expect("Failed to lock context");
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
                                        error!(lang_srv_logger, "Id {:?} is not in waitlist!", success.id);
                                    }
                                }
                                Output::Failure(failure) => {
                                    error!(lang_srv_logger, "Error response from server: {:?}", failure);
                                    ctx.response_waitlist.remove(&failure.id);
                                }
                            }
                        }
                    }
                }
                recv(lang_srv_reader_poison_rx, _) => {
                    debug!(lang_srv_logger, "Stopping language server dispatcher");
                    return;
                }
            }
        });

        {
            let mut ctx = ctx_src.lock().expect("Failed to lock context");
            general::initialize(root_path, &initial_request_meta, &mut ctx);
        }

        Controller {
            editor_reader_handle,
            lang_srv_handle,
        }
    }

    pub fn wait(self) -> thread::Result<()> {
        self.editor_reader_handle.join()?;
        self.lang_srv_handle.join()
    }
}

fn dispatch_editor_request(request: EditorRequest, mut ctx: &mut Context) {
    let buffile = &request.meta.buffile;
    if !buffile.is_empty() && !ctx.versions.contains_key(buffile) {
        text_document_did_open(
            toml::Value::Table(toml::value::Table::default()),
            &request.meta,
            &mut ctx,
        );
    }
    let meta = &request.meta;
    let params = request.params;
    let method: &str = &request.method;
    match method {
        notification::DidOpenTextDocument::METHOD => {
            text_document_did_open(params, meta, &mut ctx);
        }
        notification::DidChangeTextDocument::METHOD => {
            text_document_did_change(params, meta, &mut ctx);
        }
        notification::DidCloseTextDocument::METHOD => {
            text_document_did_close(params, meta, &mut ctx);
        }
        notification::DidSaveTextDocument::METHOD => {
            text_document_did_save(params, meta, &mut ctx);
        }
        request::Completion::METHOD => {
            completion::text_document_completion(params, meta, &mut ctx);
        }
        request::HoverRequest::METHOD => {
            hover::text_document_hover(params, meta, &mut ctx);
        }
        request::GotoDefinition::METHOD => {
            definition::text_document_definition(params, meta, &mut ctx);
        }
        notification::Exit::METHOD => {
            general::exit(params, meta, &mut ctx);
        }
        _ => {
            println!("Unsupported method: {}", request.method);
        }
    }
}

fn dispatch_server_notification(method: &str, params: Params, mut ctx: &mut Context) {
    match method {
        notification::PublishDiagnostics::METHOD => {
            diagnostics::publish_diagnostics(
                params.parse().expect("Failed to parse params"),
                &mut ctx,
            );
        }
        "$cquery/publishSemanticHighlighting" => {
            cquery::publish_semantic_highlighting(
                params.parse().expect("Failed to parse semhl params"),
                &mut ctx,
            );
        }
        notification::Exit::METHOD => {
            debug!(ctx.logger, "Language server exited, poisoning controller");
            ctx.controller_poison_tx.send(()).unwrap();
        }
        // to not litter logs with "unsupported method"
        "window/progress" => {}
        _ => {
            println!("Unsupported method: {}", method);
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
            completion::editor_completion(
                meta,
                &TextDocumentCompletionParams::deserialize(params).expect("Failed to parse params"),
                serde_json::from_value(response).expect("Failed to parse completion response"),
                &mut ctx,
            );
        }
        request::HoverRequest::METHOD => {
            let response = if response.is_null() {
                None
            } else {
                Some(serde_json::from_value(response).expect("Failed to parse hover response"))
            };
            hover::editor_hover(
                meta,
                &PositionParams::deserialize(params).expect("Failed to parse params"),
                response,
                &mut ctx,
            );
        }
        request::GotoDefinition::METHOD => {
            definition::editor_definition(
                meta,
                &PositionParams::deserialize(params).expect("Failed to parse params"),
                serde_json::from_value(response).expect("Failed to parse definition response"),
                &mut ctx,
            );
        }
        request::Initialize::METHOD => {
            ctx.capabilities = Some(
                serde_json::from_value::<InitializeResult>(response)
                    .expect("Failed to parse initialized response")
                    .capabilities,
            );
            let mut requests = Vec::with_capacity(ctx.pending_requests.len());
            for msg in ctx.pending_requests.drain(..) {
                requests.push(msg);
            }

            for msg in requests.drain(..) {
                dispatch_editor_request(msg, &mut ctx);
            }
        }
        _ => {
            println!("Don't know how to handle response for method: {}", method);
        }
    }
}
