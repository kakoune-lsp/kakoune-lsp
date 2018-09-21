use context::*;
use crossbeam_channel::{bounded, Receiver, Sender};
use diagnostics;
use editor_transport;
use fnv::FnvHashMap;
use general;
use jsonrpc_core::{Call, ErrorCode, Output, Params};
use language_features::*;
use language_server_transport;
use languageserver_types::notification::Notification;
use languageserver_types::request::Request;
use languageserver_types::*;
use project_root::find_project_root;
use serde::Deserialize;
use serde_json::{self, Value};
use std::io::{stderr, stdout, Write};
use std::path::Path;
use std::process;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use text_sync::*;
use toml;
use types::*;

type Controllers = FnvHashMap<Route, Sender<EditorRequest>>;

pub fn start(config: &Config, initial_request: Option<&str>) {
    info!("Starting Controller");

    let extensions = extension_to_language_id_map(&config);
    let languages = config.language.clone();

    let (editor_tx, editor_rx) = editor_transport::start(config, initial_request);

    let mut controllers: Controllers = FnvHashMap::default();
    let (controller_remove_tx, controller_remove_rx) = bounded(1);

    'event_loop: loop {
        select! {
            recv(editor_rx, request) => {
                if request.is_none() {
                    stop_session(&mut controllers);
                }

                let request = request.unwrap();

                if request.method == "stop" {
                    stop_session(&mut controllers);
                }

                if request.method == notification::Exit::METHOD {
                    exit_editor_session(&mut controllers, &request);
                    continue 'event_loop;
                }

                let language_id = path_to_language_id(&extensions, &request.meta.buffile);
                if language_id.is_none() {
                    debug!(
                        "Language server is not configured for extension `{}`",
                        ext_as_str(&request.meta.buffile)
                    );
                    continue 'event_loop;
                }
                let language_id = language_id.unwrap();

                let root_path = find_project_root(&languages[&language_id].roots, &request.meta.buffile);

                let route = Route {
                    session: request.meta.session.clone(),
                    language: language_id.clone(),
                    root: root_path.clone(),
                };

                debug!("Routing editor request to {:?}", route);

                match controllers.get(&route).cloned() {
                    Some(controller_tx) => {
                        controller_tx.send(request);
                    }
                    None => {
                        // because Kakoune triggers BufClose after KakEnd
                        // we don't want textDocument/didClose to start server
                        if request.method == notification::DidCloseTextDocument::METHOD {
                            continue 'event_loop;
                        }
                        spawn_controller(
                            &mut controllers,
                            config,
                            language_id,
                            root_path,
                            route,
                            request,
                            editor_tx.clone(),
                            controller_remove_tx.clone()
                        );
                    }
                }
            }

            recv(controller_remove_rx, route) => {
                if route.is_none() {
                    continue 'event_loop;
                }
                let route = route.unwrap();
                controllers.remove(&route);
                debug!("Controller {:?} removed", route);
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
        config: Config,
    ) -> Self {
        let (editor_reader_poison_tx, editor_reader_poison_rx) = bounded(1);
        let (lang_srv_reader_poison_tx, lang_srv_reader_poison_rx) = bounded(1);
        thread::spawn(move || {
            for msg in controller_poison_rx {
                editor_reader_poison_tx.send(msg);
                lang_srv_reader_poison_tx.send(msg);
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
            config.clone(),
            root_path.to_string(),
        )));

        let ctx = Arc::clone(&ctx_src);
        let editor_reader_handle = thread::spawn(move || {
            loop {
                select! {
                    recv(editor_rx, msg) =>{
                        if msg.is_none() {
                            debug!("Stopping editor dispatcher");
                            return;
                        }
                        let msg = msg.unwrap();
                        let mut ctx = ctx.lock().expect("Failed to lock context");
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
                                    // TODO if auto-hover is not enabled we might want warning about parking as well
                                    request::HoverRequest::METHOD => (),
                                    _ => ctx.exec(msg.meta.clone(), "lsp-show-error 'Language server is not initialized, parking request'".to_string())
                                }
                            }
                            ctx.pending_requests.push(msg);

                        }
                    }

                    recv(editor_reader_poison_rx) => {
                        debug!("Stopping editor dispatcher");
                        return;
                    }
                }
            }
        });

        let ctx = Arc::clone(&ctx_src);
        let lang_srv_handle = thread::spawn(move || loop {
            select! {
                recv(lang_srv_rx, msg) => {
                    if msg.is_none() {
                        debug!("Stopping language server dispatcher");
                        return;
                    }
                    let msg = msg.unwrap();
                    match msg {
                        ServerMessage::Request(call) => {
                            let mut ctx = ctx.lock().expect("Failed to lock context");
                            match call {
                                Call::MethodCall(request) => {
                                    debug!("Requests from language server are not supported yet: {:?}", request);
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
                                        error!("Id {:?} is not in waitlist!", success.id);
                                    }
                                }
                                Output::Failure(failure) => {
                                    error!("Error response from server: {:?}", failure);
                                    if let Some(request) = ctx.response_waitlist.remove(&failure.id) {
                                        let (meta, method, _) = request;
                                        let msg = match failure.error.code {
                                            ErrorCode::MethodNotFound => {
                                                format!("{} language server doesn't support method {}", ctx.language_id, method)
                                            }
                                            _ => {
                                                format!("{} language server error: {}", ctx.language_id, failure.error.message)
                                            }
                                        };
                                        ctx.exec(meta, format!("lsp-show-error %§{}§", msg));
                                    } else {
                                        error!("Id {:?} is not in waitlist!", failure.id);
                                    }
                                }
                            }
                        }
                    }
                }

                recv(lang_srv_reader_poison_rx) => {
                    debug!("Stopping language server dispatcher");
                    return;
                }
            }
        });

        {
            let mut ctx = ctx_src.lock().expect("Failed to lock context");
            // config for the current language should be defined at this point,
            // thus okay to just unwrap and panic if it's not because everything is broken then
            let options = config
                .language
                .get(language_id)
                .unwrap()
                .initialization_options
                .clone();
            general::initialize(root_path, options, &initial_request_meta, &mut ctx);
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
        notification::DidChangeConfiguration::METHOD => {
            warn!("Got DidChangeConfiguration: %{:?}", params);

            let maybe_settings = params
                .as_table()
                .and_then(|table| table.get("settings"))
                .and_then(|table| {
                     match table.clone().try_into() {
                         Ok(value) => Some(value),
                         Err(e) => {
                             warn!(
                                 "Could not convert settings {:?} to JSON: {}",
                                 table,
                                 e,
                             );
                             None
                         }
                     }
                });

            let settings = match maybe_settings {
                Some(table) => table,
                None => {
                    warn!(
                        "Got DidChangeConfiguration with no settings: %{:?}",
                        params,
                    );
                    return;
                }
            };

            let params = DidChangeConfigurationParams { settings };
            ctx.notify(
                notification::DidChangeConfiguration::METHOD.into(),
                params,
            );
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
        request::References::METHOD => {
            references::text_document_references(params, meta, &mut ctx);
        }
        notification::Exit::METHOD => {
            general::exit(params, meta, &mut ctx);
        }
        request::SignatureHelpRequest::METHOD => {
            signature_help::text_document_signature_help(params, meta, &mut ctx);
        }
        request::DocumentSymbolRequest::METHOD => {
            document_symbol::text_document_document_symbol(params, meta, &mut ctx);
        }
        request::Formatting::METHOD => {
            formatting::text_document_formatting(params, meta, &mut ctx);
        }
        "textDocument/diagnostics" => {
            diagnostics::editor_diagnostics(params, meta, &mut ctx);
        }
        "capabilities" => {
            general::capabilities(params, meta, &mut ctx);
        }
        _ => {
            warn!("Unsupported method: {}", method);
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
            debug!("Language server exited, poisoning controller");
            ctx.controller_poison_tx.send(());
        }
        "window/logMessage" => {
            debug!("{:?}", params);
        }
        "window/progress" => {
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
        request::References::METHOD => {
            references::editor_references(
                meta,
                &PositionParams::deserialize(params).expect("Failed to parse params"),
                serde_json::from_value(response).expect("Failed to parse references response"),
                &mut ctx,
            );
        }
        request::SignatureHelpRequest::METHOD => {
            signature_help::editor_signature_help(
                meta,
                &PositionParams::deserialize(params).expect("Failed to parse params"),
                serde_json::from_value(response).expect("Failed to parse signature help response"),
                &mut ctx,
            );
        }
        request::DocumentSymbolRequest::METHOD => {
            document_symbol::editor_document_symbol(
                meta,
                serde_json::from_value(response).expect("Failed to parse document symbol response"),
                &mut ctx,
            );
        }
        request::Formatting::METHOD => {
            formatting::editor_formatting(
                meta,
                &FormattingOptions::deserialize(params).expect("Failed to parse params"),
                serde_json::from_value(response).expect("Failed to parse formatting response"),
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
            error!("Don't know how to handle response for method: {}", method);
        }
    }
}

fn language_id_to_server_cmd(config: &Config, language_id: &str) -> Option<(String, Vec<String>)> {
    if let Some(language) = config.language.get(language_id) {
        return Some((language.command.clone(), language.args.clone()));
    }
    None
}

fn path_to_language_id(extensions: &FnvHashMap<String, String>, path: &str) -> Option<String> {
    extensions
        .get(Path::new(path).extension()?.to_str()?)
        .cloned()
}

fn extension_to_language_id_map(config: &Config) -> FnvHashMap<String, String> {
    let mut extensions = FnvHashMap::default();
    for (language_id, language) in &config.language {
        for extension in &language.extensions {
            extensions.insert(extension.clone(), language_id.clone());
        }
    }
    extensions
}

fn exit_editor_session(controllers: &mut Controllers, request: &EditorRequest) {
    info!(
        "Session `{}` closed, shutting down associated language servers",
        request.meta.session
    );
    for k in controllers.keys().cloned().collect::<Vec<_>>() {
        if k.session == request.meta.session {
            // should be safe to unwrap because we are iterating controllers' keys
            let controller_tx = controllers.remove(&k).unwrap();
            info!("Exit {} in project {}", k.language, k.root);
            controller_tx.send(request.clone());
        }
    }
}

fn stop_session(controllers: &mut Controllers) {
    let request = EditorRequest {
        meta: EditorMeta {
            session: "".to_string(),
            buffile: "".to_string(),
            client: None,
            version: 0,
        },
        method: notification::Exit::METHOD.to_string(),
        params: toml::Value::Table(toml::value::Table::default()),
    };
    info!("Shutting down language servers and exiting");
    for k in controllers.keys().cloned().collect::<Vec<_>>() {
        // should be safe to unwrap because we are iterating controllers' keys
        let controller_tx = controllers.remove(&k).unwrap();
        info!("Exit {} in project {}", k.language, k.root);
        controller_tx.send(request.clone())
    }
    stderr().flush().unwrap();
    stdout().flush().unwrap();
    thread::sleep(Duration::from_secs(1));
    process::exit(0);
}

fn ext_as_str(path: &str) -> &str {
    Path::new(path)
        .extension()
        .unwrap_or_default()
        .to_str()
        .unwrap_or_default()
}

fn spawn_controller(
    controllers: &mut Controllers,
    config: &Config,
    language_id: String,
    root_path: String,
    route: Route,
    request: EditorRequest,
    editor_tx: Sender<EditorResponse>,
    controller_remove_tx: Sender<Route>,
) {
    // should be fine to unwrap because request was already routed which means
    // language is configured with all mandatory fields in place
    let (lang_srv_cmd, lang_srv_args) = language_id_to_server_cmd(config, &language_id).unwrap();
    // NOTE 1024 is arbitrary
    let (controller_tx, controller_rx) = bounded(1024);
    controllers.insert(route.clone(), controller_tx);
    let editor_tx = editor_tx.clone();
    let (controller_poison_tx, controller_poison_rx) = bounded(1);
    let (controller_poison_tx_mult, controller_poison_rx_mult) = bounded(1);
    let controller_remove_tx = controller_remove_tx.clone();
    let route_mult = route.clone();
    thread::spawn(move || {
        for _ in controller_poison_rx_mult {
            controller_remove_tx.send(route_mult.clone());
            controller_poison_tx.send(());
        }
    });
    let config = (*config).clone();
    thread::spawn(move || {
        let (lang_srv_tx, lang_srv_rx, lang_srv_poison_tx) =
            language_server_transport::start(&lang_srv_cmd, &lang_srv_args);
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
            config,
        );
        controller.wait().expect("Failed to wait for controller");
        debug!("Controller {:?} exited", route);
    });
}
