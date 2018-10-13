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
use workspace;

type Controllers = Arc<Mutex<FnvHashMap<Route, Sender<EditorRequest>>>>;

pub fn start(config: &Config, initial_request: Option<&str>) {
    info!("Starting Controller");

    let extensions = extension_to_language_id_map(&config);
    let languages = config.language.clone();

    let (editor_tx, editor_rx) = editor_transport::start(config, initial_request);

    let controllers: Controllers = Arc::new(Mutex::new(FnvHashMap::default()));
    let (controller_remove_tx, controller_remove_rx) = bounded(1);

    {
        let controllers = Arc::clone(&controllers);
        thread::spawn(move || {
            for route in controller_remove_rx {
                controllers.lock().unwrap().remove(&route);
                debug!("Controller {:?} removed", route);
            }
        });
    }

    for request in editor_rx {
        if request.method == "stop" {
            stop_session(&controllers);
        }

        if request.method == notification::Exit::METHOD {
            exit_editor_session(&controllers, &request);
            continue;
        }

        let language_id = path_to_language_id(&extensions, &request.meta.buffile);
        if language_id.is_none() {
            debug!(
                "Language server is not configured for extension `{}`",
                ext_as_str(&request.meta.buffile)
            );
            continue;
        }
        let language_id = language_id.unwrap();

        let root_path = find_project_root(&languages[&language_id].roots, &request.meta.buffile);

        let route = Route {
            session: request.meta.session.clone(),
            language: language_id.clone(),
            root: root_path.clone(),
        };

        debug!("Routing editor request to {:?}", route);

        let controller_tx = controllers.lock().unwrap().get(&route).cloned();

        match controller_tx {
            Some(controller_tx) => {
                debug!("Controller found, sending request");
                controller_tx.send(request);
            }
            None => {
                // because Kakoune triggers BufClose after KakEnd
                // we don't want textDocument/didClose to start server
                if request.method == notification::DidCloseTextDocument::METHOD {
                    continue;
                }
                debug!("Controller not found, spawning a new one");
                spawn_controller(
                    &controllers,
                    &config,
                    language_id,
                    root_path,
                    route,
                    request,
                    editor_tx.clone(),
                    controller_remove_tx.clone(),
                );
            }
        }
    }
    stop_session(&controllers);
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
        route: Route,
        controller_remove_tx: Sender<Route>,
        initial_request: EditorRequest,
        config: Config,
    ) -> Self {
        let initial_request_meta = initial_request.meta.clone();

        let ctx_src = Arc::new(Mutex::new(Context::new(
            language_id,
            initial_request,
            lang_srv_tx,
            editor_tx,
            config.clone(),
            root_path.to_string(),
            route,
            controller_remove_tx,
        )));

        let ctx = Arc::clone(&ctx_src);
        let editor_reader_handle = thread::spawn(move || {
            for msg in editor_rx {
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
                                    // TODO if auto-hover or auto-hl-references is not enabled we might want warning about parking as well
                                    request::HoverRequest::METHOD => (),
                                    "textDocument/highlightReferences" => (),
                                    _ => ctx.exec(msg.meta.clone(), "lsp-show-error 'Language server is not initialized, parking request'".to_string())
                                }
                    }
                    ctx.pending_requests.push(msg);
                }
            }
            debug!("Stopping editor dispatcher");
        });

        let ctx = Arc::clone(&ctx_src);
        let lang_srv_handle = thread::spawn(move || {
            for msg in lang_srv_rx {
                match msg {
                    ServerMessage::Request(call) => {
                        let mut ctx = ctx.lock().expect("Failed to lock context");
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
                                        ErrorCode::MethodNotFound => format!(
                                            "{} language server doesn't support method {}",
                                            ctx.language_id, method
                                        ),
                                        _ => format!(
                                            "{} language server error: {}",
                                            ctx.language_id, failure.error.message
                                        ),
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
            debug!("Stopping language server dispatcher");
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
        text_document_did_open(&request.meta, &mut ctx);
    }
    let meta = &request.meta;
    let params = request.params;
    let method: &str = &request.method;
    match method {
        notification::DidOpenTextDocument::METHOD => {
            text_document_did_open(meta, &mut ctx);
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
        "textDocument/diagnostics" => {
            diagnostics::editor_diagnostics(meta, &mut ctx);
        }
        "capabilities" => {
            general::capabilities(meta, &mut ctx);
        }
        "textDocument/referencesHighlight" => {
            references::text_document_references_highlight(meta, params, &mut ctx);
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
        notification::Exit::METHOD => {
            debug!("Language server exited, poisoning context");
            ctx.poison();
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
            formatting::editor_formatting(meta, response, &mut ctx);
        }
        request::WorkspaceSymbol::METHOD => {
            workspace::editor_workspace_symbol(meta, response, &mut ctx);
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

fn exit_editor_session(controllers: &Controllers, request: &EditorRequest) {
    info!(
        "Session `{}` closed, shutting down associated language servers",
        request.meta.session
    );
    let mut controllers = controllers.lock().unwrap();
    for k in controllers.keys().cloned().collect::<Vec<_>>() {
        if k.session == request.meta.session {
            // should be safe to unwrap because we are iterating controllers' keys
            let controller_tx = controllers.remove(&k).unwrap();
            info!("Exit {} in project {}", k.language, k.root);
            controller_tx.send(request.clone());
        }
    }
}

fn stop_session(controllers: &Controllers) {
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
    let mut controllers = controllers.lock().unwrap();
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
    controllers: &Controllers,
    config: &Config,
    language_id: String,
    root_path: String,
    route: Route,
    request: EditorRequest,
    editor_tx: Sender<EditorResponse>,
    controller_remove_tx: Sender<Route>,
) {
    // should be fine to unwrap because request was already routed which means
    // language is configured with all mandatory fields in
    let (lang_srv_cmd, lang_srv_args) = language_id_to_server_cmd(config, &language_id).unwrap();
    // NOTE 1024 is arbitrary
    let (controller_tx, controller_rx) = bounded(1024);
    controllers
        .lock()
        .unwrap()
        .insert(route.clone(), controller_tx);
    let editor_tx = editor_tx.clone();

    let config = (*config).clone();
    thread::spawn(move || {
        let (lang_srv_tx, lang_srv_rx) =
            language_server_transport::start(&lang_srv_cmd, &lang_srv_args);
        let controller = Controller::start(
            &language_id,
            &root_path,
            lang_srv_tx,
            lang_srv_rx,
            editor_tx,
            controller_rx,
            route.clone(),
            controller_remove_tx,
            request,
            config,
        );
        controller.wait().expect("Failed  wait for controller");
        debug!("Controller {:?} exited", route);
    });
}
