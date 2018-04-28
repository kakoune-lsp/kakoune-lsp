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
use serde::Deserialize;
use serde_json::{self, Value};
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

pub fn start(config: &Config) {
    println!("Starting Controller");
    let (editor_tx, editor_rx) = editor_transport::start(config);
    let mut controllers: FnvHashMap<Route, Sender<EditorRequest>> = FnvHashMap::default();
    for request in editor_rx {
        let route = request.route.clone();
        let (_, language_id, root_path) = route.clone();
        let controller = controllers.get(&route).cloned();
        match controller {
            Some(controller_tx) => {
                controller_tx
                    .send(request.request)
                    .expect("Failed to route editor request");
            }
            None => {
                let (lang_srv_cmd, lang_srv_args) = get_server_cmd(config, &language_id).unwrap();
                // NOTE 1024 is arbitrary
                let (controller_tx, controller_rx) = bounded(1024);
                controllers.insert(route, controller_tx);
                let editor_tx = editor_tx.clone();
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
                        request.request,
                    );
                    controller.wait().expect("Failed to wait for controller");
                });
            }
        }
    }
}

struct Controller {
    editor_reader_handle: JoinHandle<()>,
}

impl Controller {
    fn start(
        language_id: &str,
        root_path: &str,
        lang_srv_tx: Sender<ServerMessage>,
        lang_srv_rx: Receiver<ServerMessage>,
        editor_tx: Sender<EditorResponse>,
        editor_rx: Receiver<EditorRequest>,
        initial_request: EditorRequest,
    ) -> Self {
        let initial_request_meta = initial_request.meta.clone();
        let ctx_src = Arc::new(Mutex::new(Context::new(
            language_id,
            initial_request,
            lang_srv_tx,
            editor_tx,
        )));

        let ctx = Arc::clone(&ctx_src);
        let editor_reader_handle = thread::spawn(move || {
            for msg in editor_rx {
                let mut ctx = ctx.lock().expect("Failed to lock context");
                // initialize request must be first requst from client to language server
                // initialized response contains capabilities which we save for future use
                // capabilities also serve as a marker of completing initialization
                // we park all requests from editor before initialization is complete
                // and then dispatch them
                if ctx.capabilities.is_some() {
                    dispatch_editor_request(msg, &mut ctx);
                } else {
                    ctx.pending_requests.push(msg);
                }
            }
        });

        let ctx = Arc::clone(&ctx_src);
        thread::spawn(move || {
            for msg in lang_srv_rx {
                match msg {
                    ServerMessage::Request(call) => {
                        let mut ctx = ctx.lock().expect("Failed to lock context");
                        match call {
                            Call::MethodCall(_request) => {
                                //println!("Requests from language server are not supported yet");
                                //println!("{:?}", request);
                            }
                            Call::Notification(notification) => {
                                dispatch_server_notification(
                                    &notification.method,
                                    notification.params.unwrap(),
                                    &mut ctx,
                                );
                            }
                            Call::Invalid(m) => {
                                println!("Invalid call from language server: {:?}", m);
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
                                    println!("Id {:?} is not in waitlist!", success.id);
                                }
                            }
                            Output::Failure(failure) => {
                                println!("Error response from server: {:?}", failure);
                                ctx.response_waitlist.remove(&failure.id);
                            }
                        }
                    }
                }
            }
        });

        let mut ctx = ctx_src.lock().expect("Failed to lock context");
        general::initialize(root_path, initial_request_meta, &mut ctx);

        Controller {
            editor_reader_handle,
        }
    }

    pub fn wait(self) -> thread::Result<()> {
        self.editor_reader_handle.join()
        // TODO lang_srv_reader_handle
    }
}

fn dispatch_editor_request(request: EditorRequest, mut ctx: &mut Context) {
    let buffile = &request.meta.buffile;
    if !ctx.versions.contains_key(buffile) {
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
