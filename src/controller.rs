use crossbeam_channel::{bounded, Receiver, Sender};
use editor_transport;
use fnv::FnvHashMap;
use jsonrpc_core::{self, Call, Id, Output, Params, Version};
use language_server_transport;
use languageserver_types::*;
use languageserver_types::notification::Notification;
use languageserver_types::request::Request;
use regex::Regex;
use serde_json::{self, Value};
use serde::Deserialize;
use std::fs::{remove_file, File};
use std::io::Read;
use std::process;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use toml;
use types::*;
use url::Url;

fn get_server_cmd(config: &Config, language_id: &str) -> Option<(String, Vec<String>)> {
    if let Some(language) = config.language.get(language_id) {
        return Some((language.command.clone(), language.args.clone()));
    }
    None
}

pub fn start(config: &Config) {
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
                    let (lang_srv_tx, lang_srv_rx): (
                        Sender<ServerMessage>,
                        Receiver<ServerMessage>,
                    ) = language_server_transport::start(&lang_srv_cmd, &lang_srv_args);
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

struct Context {
    capabilities: Option<ServerCapabilities>,
    editor_tx: Sender<EditorResponse>,
    diagnostics: FnvHashMap<String, Vec<Diagnostic>>,
    lang_srv_tx: Sender<ServerMessage>,
    language_id: String,
    pending_requests: Vec<EditorRequest>,
    request_counter: u64,
    response_waitlist: FnvHashMap<Id, (EditorMeta, String, EditorParams)>,
    session: SessionId,
    versions: FnvHashMap<String, u64>,
}

impl Context {
    fn call(&mut self, id: Id, method: String, params: impl ToParams) {
        let call = jsonrpc_core::MethodCall {
            jsonrpc: Some(Version::V2),
            id,
            method,
            params: Some(params.to_params().expect("Failed to convert params")),
        };
        self.lang_srv_tx
            .send(ServerMessage::Request(Call::MethodCall(call)))
            .expect("Failed to send request to language server transport");
    }

    fn notify(&mut self, method: String, params: impl ToParams) {
        let notification = jsonrpc_core::Notification {
            jsonrpc: Some(Version::V2),
            method,
            params: Some(params.to_params().expect("Failed to convert params")),
        };
        self.lang_srv_tx
            .send(ServerMessage::Request(Call::Notification(notification)))
            .expect("Failed to send request to language server transport");
    }

    fn exec(&self, meta: EditorMeta, command: String) {
        self.editor_tx
            .send(EditorResponse { meta, command })
            .expect("Failed to send message to editor transport");
    }
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
        let ctx_src = Arc::new(Mutex::new(Context {
            capabilities: None,
            diagnostics: FnvHashMap::default(),
            editor_tx,
            lang_srv_tx,
            language_id: language_id.to_string(),
            pending_requests: vec![initial_request],
            request_counter: 0,
            response_waitlist: FnvHashMap::default(),
            session: initial_request_meta.session.clone(),
            versions: FnvHashMap::default(),
        }));

        let ctx = Arc::clone(&ctx_src);
        let editor_reader_handle = thread::spawn(move || {
            for msg in editor_rx {
                let mut ctx = ctx.lock().expect("Failed to lock context");
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
        let req_id = Id::Num(ctx.request_counter);
        let req = jsonrpc_core::MethodCall {
            jsonrpc: Some(Version::V2),
            id: req_id.clone(),
            method: request::Initialize::METHOD.into(),
            params: Some(initialize(root_path)),
        };
        ctx.response_waitlist.insert(
            req_id,
            (
                initial_request_meta,
                req.method.clone(),
                toml::Value::Table(toml::value::Table::default()),
            ),
        );
        ctx.lang_srv_tx
            .send(ServerMessage::Request(Call::MethodCall(req)))
            .expect("Failed to send request to language server transport");

        Controller {
            editor_reader_handle,
        }
    }

    pub fn wait(self) -> thread::Result<()> {
        self.editor_reader_handle.join()
        // TODO lang_srv_reader_handle
    }
}

fn initialize(root_path: &str) -> Params {
    let params = InitializeParams {
        capabilities: ClientCapabilities {
            workspace: None,
            text_document: Some(TextDocumentClientCapabilities {
                synchronization: None,
                completion: Some(CompletionCapability {
                    dynamic_registration: None,
                    completion_item: Some(CompletionItemCapability {
                        snippet_support: None,
                        commit_characters_support: None,
                        documentation_format: None,
                    }),
                }),
                hover: None,
                signature_help: None,
                references: None,
                document_highlight: None,
                document_symbol: None,
                formatting: None,
                range_formatting: None,
                on_type_formatting: None,
                definition: None,
                code_action: None,
                code_lens: None,
                document_link: None,
                rename: None,
            }),
            experimental: None,
        },
        initialization_options: None,
        process_id: Some(process::id().into()),
        root_uri: Some(Url::parse(&format!("file://{}", root_path)).unwrap()),
        root_path: Some(root_path.to_string()),
        trace: Some(TraceOption::Off),
    };

    params.to_params().unwrap()
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
            text_document_completion(params, meta, &mut ctx);
        }
        request::HoverRequest::METHOD => {
            text_document_hover(params, meta, &mut ctx);
        }
        request::GotoDefinition::METHOD => {
            text_document_definition(params, meta, &mut ctx);
        }
        _ => {
            println!("Unsupported method: {}", request.method);
        }
    }
}

fn dispatch_server_notification(method: &str, params: Params, mut ctx: &mut Context) {
    match method {
        notification::PublishDiagnostics::METHOD => {
            publish_diagnostics(params.parse().expect("Failed to parse params"), &mut ctx);
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
            editor_completion(
                meta,
                &TextDocumentCompletionParams::deserialize(params).expect("Failed to parse params"),
                serde_json::from_value(response).expect("Failed to parse completion response"),
                &mut ctx,
            );
        }
        request::HoverRequest::METHOD => {
            editor_hover(
                meta,
                &PositionParams::deserialize(params).expect("Failed to parse params"),
                serde_json::from_value(response).expect("Failed to parse hover response"),
                &mut ctx,
            );
        }
        request::GotoDefinition::METHOD => {
            editor_definition(
                meta,
                &PositionParams::deserialize(params).expect("Failed to parse params"),
                serde_json::from_value(response).expect("Failed to parse definition response"),
                &mut ctx,
            );
        }
        request::Initialize::METHOD => {
            initialized(
                meta,
                &toml::Value::Table(toml::value::Table::default()),
                serde_json::from_value(response).expect("Failed to parse initialized response"),
                &mut ctx,
            );
        }
        _ => {
            println!("Don't know how to handle response for method: {}", method);
        }
    }
}

fn text_document_did_open(_params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let language_id = ctx.language_id.clone();
    let mut file = File::open(&meta.buffile).expect("Failed to open file");
    let mut text = String::new();
    file.read_to_string(&mut text)
        .expect("Failed to read from file");
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: Url::parse(&format!("file://{}", &meta.buffile)).unwrap(),
            language_id,
            version: meta.version,
            text,
        },
    };
    ctx.versions.insert(meta.buffile.clone(), meta.version);
    ctx.notify(notification::DidOpenTextDocument::METHOD.into(), params);
}

fn text_document_did_change(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let params = TextDocumentDidChangeParams::deserialize(params)
        .expect("Params should follow TextDocumentDidChangeParams structure");
    let uri = Url::parse(&format!("file://{}", &meta.buffile)).unwrap();
    let version = meta.version;
    let old_version = ctx.versions.get(&meta.buffile).cloned().unwrap_or(0);
    if old_version >= version {
        return;
    }
    ctx.versions.insert(meta.buffile.clone(), version);
    ctx.diagnostics.insert(meta.buffile.clone(), Vec::new());
    let file_path = params.draft;
    let mut text = String::new();
    {
        let mut file = File::open(&file_path).expect("Failed to open file");
        file.read_to_string(&mut text)
            .expect("Failed to read from file");
    }
    remove_file(file_path).expect("Failed to remove temporary file");
    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri,
            version: Some(meta.version),
        },
        content_changes: vec![
            TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text,
            },
        ],
    };
    ctx.notify(notification::DidChangeTextDocument::METHOD.into(), params);
}

fn text_document_did_close(_params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let uri = Url::parse(&format!("file://{}", &meta.buffile)).unwrap();
    let params = DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    };
    ctx.notify(notification::DidCloseTextDocument::METHOD.into(), params);
}

fn text_document_did_save(_params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let uri = Url::parse(&format!("file://{}", &meta.buffile)).unwrap();
    let params = DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    };
    ctx.notify(notification::DidSaveTextDocument::METHOD.into(), params);
}

fn text_document_completion(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let req_params = TextDocumentCompletionParams::deserialize(params.clone())
        .expect("Params should follow TextDocumentCompletionParams structure");
    let position = req_params.position;
    let req_params = CompletionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::parse(&format!("file://{}", &meta.buffile)).unwrap(),
        },
        position,
        context: None,
    };
    let id = Id::Num(ctx.request_counter);
    ctx.request_counter += 1;
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::Completion::METHOD.into(), params),
    );
    ctx.call(id, request::Completion::METHOD.into(), req_params);
}

fn text_document_hover(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let req_params = PositionParams::deserialize(params.clone())
        .expect("Params should follow PositionParams structure");
    let position = req_params.position;
    let req_params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::parse(&format!("file://{}", &meta.buffile)).unwrap(),
        },
        position,
    };
    // TODO DRY
    let id = Id::Num(ctx.request_counter);
    ctx.request_counter += 1;
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::HoverRequest::METHOD.into(), params),
    );
    ctx.call(id, request::HoverRequest::METHOD.into(), req_params);
}

fn text_document_definition(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let req_params = PositionParams::deserialize(params.clone())
        .expect("Params should follow PositionParams structure");
    let position = req_params.position;
    let req_params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::parse(&format!("file://{}", &meta.buffile)).unwrap(),
        },
        position,
    };
    // TODO DRY
    let id = Id::Num(ctx.request_counter);
    ctx.request_counter += 1;
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::GotoDefinition::METHOD.into(), params),
    );
    ctx.call(id, request::GotoDefinition::METHOD.into(), req_params);
}

fn editor_completion(
    meta: &EditorMeta,
    params: &TextDocumentCompletionParams,
    result: CompletionResponse,
    ctx: &mut Context,
) {
    let items = match result {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    let re = Regex::new(r"(?P<c>[:|$])").unwrap();
    let items = items
        .into_iter()
        .map(|x| {
            format!(
                "{}|{}|{}",
                re.replace_all(&x.label, r"\$c"),
                re.replace_all(&x.detail.unwrap_or_else(|| "".to_string()), r"\$c"),
                re.replace_all(&x.label, r"\$c"),
            )
        })
        .collect::<Vec<String>>()
        .join(":");
    let p = params.position;
    let command = format!(
        "set %{{buffer={}}} lsp_completions %§{}.{}@{}:{}§\n",
        meta.buffile,
        p.line + 1,
        p.character + 1 - params.completion.offset,
        meta.version,
        items
    );
    ctx.exec(meta.clone(), command);
}

fn editor_hover(meta: &EditorMeta, params: &PositionParams, result: Hover, ctx: &mut Context) {
    let diagnostics = ctx.diagnostics.get(&meta.buffile);
    let pos = params.position;
    let diagnostics = diagnostics
        .and_then(|x| {
            Some(
                x.iter()
                    .filter(|x| {
                        let start = x.range.start;
                        let end = x.range.end;
                        (start.line < pos.line && pos.line < end.line)
                            || (start.line == pos.line && pos.line == end.line
                                && start.character <= pos.character
                                && pos.character <= end.character)
                            || (start.line == pos.line && pos.line <= end.line
                                && start.character <= pos.character)
                            || (start.line <= pos.line && end.line == pos.line
                                && pos.character <= end.character)
                    })
                    .map(|x| x.message.to_string())
                    .collect::<Vec<String>>()
                    .join("\n"),
            )
        })
        .unwrap_or_else(String::new);
    let contents = match result.contents {
        HoverContents::Scalar(contents) => contents.plaintext(),
        HoverContents::Array(contents) => contents
            .into_iter()
            .map(|x| x.plaintext())
            .collect::<Vec<String>>()
            .join("\n"),
        HoverContents::Markup(contents) => contents.value,
    };
    if contents.is_empty() && diagnostics.is_empty() {
        return;
    }
    let command;
    if diagnostics.is_empty() {
        command = format!("info %§{}§", contents);
    } else if contents.is_empty() {
        command = format!("info %§{}§", diagnostics);
    } else {
        command = format!("info %§{}\n\n{}§", contents, diagnostics);
    }

    ctx.exec(meta.clone(), command);
}

fn editor_definition(
    meta: &EditorMeta,
    _params: &PositionParams,
    result: GotoDefinitionResponse,
    ctx: &mut Context,
) {
    if let Some(location) = match result {
        GotoDefinitionResponse::Scalar(location) => Some(location),
        GotoDefinitionResponse::Array(mut locations) => Some(locations.remove(0)),
        GotoDefinitionResponse::None => None,
    } {
        let filename = location.uri.path();
        let p = location.range.start;
        let command = format!("edit %§{}§ {} {}", filename, p.line + 1, p.character + 1);
        ctx.exec(meta.clone(), command);
    };
}

fn initialized(
    _meta: &EditorMeta,
    _params: &EditorParams,
    result: InitializeResult,
    mut ctx: &mut Context,
) {
    ctx.capabilities = Some(result.capabilities);
    let mut requests = Vec::with_capacity(ctx.pending_requests.len());
    for msg in ctx.pending_requests.drain(..) {
        requests.push(msg);
    }

    for msg in requests.drain(..) {
        dispatch_editor_request(msg, &mut ctx);
    }
}

fn publish_diagnostics(params: PublishDiagnosticsParams, ctx: &mut Context) {
    let session = ctx.session.clone();
    let client = None;
    let buffile = params.uri.path().to_string();
    let version = ctx.versions.get(&buffile);
    if version.is_none() {
        return;
    }
    let version = *version.unwrap();
    let ranges = params
        .diagnostics
        .iter()
        .map(|x| {
            format!(
                "{}.{},{}.{}|Error",
                x.range.start.line + 1,
                x.range.start.character + 1,
                x.range.end.line + 1,
                // LSP ranges are exclusive, but Kakoune's are inclusive
                x.range.end.character
            )
        })
        .collect::<Vec<String>>()
        .join(":");
    let command = format!(
        "eval -buffer %§{}§ %§set buffer lsp_errors \"{}:{}\"§",
        buffile, version, ranges
    );
    ctx.diagnostics.insert(buffile.clone(), params.diagnostics);
    let meta = EditorMeta {
        session,
        client,
        buffile,
        version,
    };
    ctx.exec(meta, command.to_string());
}

trait PlainText {
    fn plaintext(self) -> String;
}

impl PlainText for MarkedString {
    fn plaintext(self) -> String {
        match self {
            MarkedString::String(contents) => contents,
            MarkedString::LanguageString(contents) => contents.value,
        }
    }
}
