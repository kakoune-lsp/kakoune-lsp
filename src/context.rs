use crate::text_sync::CompiledFileSystemWatcher;
use crate::types::*;
use crossbeam_channel::Sender;
use jsonrpc_core::{self, Call, Error, Failure, Id, Output, Success, Value, Version};
use lsp_types::notification::{Cancel, Notification};
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::TryInto;
use std::path::PathBuf;
use std::{fs, time};

// Copy of Kakoune's timestamped buffer content.
pub struct Document {
    // Corresponds to Kakoune's timestamp.
    // It's passed to a language server as a version and is used to tag selections, highlighters and
    // other timestamp sensitive parameters in commands sent to kakoune.
    pub version: i32,
    // Buffer content.
    // It's used to translate between LSP and Kakoune coordinates.
    pub text: ropey::Rope,
}

/// Groups parameters for each request.
pub enum RequestParams<T> {
    /// Replicates the same list of parameters for all language servers in a context.
    All(Vec<T>),
    /// Uses different parameters for each language server in a context.
    Each(HashMap<ServerName, Vec<T>>),
}

pub type ResponsesCallback =
    Box<dyn FnOnce(&mut Context, EditorMeta, Vec<(ServerName, Value)>) -> ()>;
type BatchNumber = usize;
type BatchCount = BatchNumber;

pub struct OutstandingRequests {
    oldest: Option<Id>,
    youngest: Option<Id>,
}

pub struct ServerSettings {
    pub root_path: String,
    pub offset_encoding: OffsetEncoding,
    pub preferred_offset_encoding: Option<OffsetEncoding>,
    pub capabilities: Option<ServerCapabilities>,
    pub tx: Sender<ServerMessage>,
}

pub struct Context {
    batch_count: BatchCount,
    pub batch_sizes: HashMap<BatchNumber, HashMap<ServerName, usize>>,
    pub batches: HashMap<
        BatchNumber,
        (
            Vec<(ServerName, serde_json::value::Value)>,
            ResponsesCallback,
        ),
    >,
    pub code_lenses: HashMap<String, Vec<(ServerName, CodeLens)>>,
    pub completion_items: Vec<(ServerName, CompletionItem)>,
    pub completion_items_timestamp: i32,
    // We currently only track one client's completion items, to simplify cleanup (else we
    // might need to hook into ClientClose). Track the client name, so we can check if the
    // completions are valid.
    pub completion_last_client: Option<String>,
    pub config: Config,
    pub diagnostics: HashMap<String, Vec<(ServerName, Diagnostic)>>,
    pub documents: HashMap<String, Document>,
    pub dynamic_config: DynamicConfig,
    pub editor_tx: Sender<EditorResponse>,
    pub language_id: LanguageId,
    pub language_servers: BTreeMap<ServerName, ServerSettings>,
    pub outstanding_requests:
        HashMap<(ServerName, &'static str, String, Option<String>), OutstandingRequests>,
    pub pending_requests: Vec<EditorRequest>,
    pub pending_message_requests: VecDeque<(Id, ServerName, ShowMessageRequestParams)>,
    pub request_counter: u64,
    pub response_waitlist: HashMap<Id, (EditorMeta, &'static str, BatchNumber, bool)>,
    pub session: SessionId,
    pub work_done_progress: HashMap<NumberOrString, Option<WorkDoneProgressBegin>>,
    pub work_done_progress_report_timestamp: time::Instant,
    pub pending_file_watchers:
        HashMap<(ServerName, String, Option<PathBuf>), Vec<CompiledFileSystemWatcher>>,
}

pub struct ContextBuilder {
    pub language_id: LanguageId,
    pub language_servers: BTreeMap<ServerName, ServerSettings>,
    pub initial_request: EditorRequest,
    pub editor_tx: Sender<EditorResponse>,
    pub config: Config,
}

impl Context {
    pub fn new(params: ContextBuilder) -> Self {
        let session = params.initial_request.meta.session.clone();
        Context {
            batch_count: 0,
            batch_sizes: Default::default(),
            batches: Default::default(),
            code_lenses: Default::default(),
            completion_items: vec![],
            completion_items_timestamp: i32::max_value(),
            completion_last_client: None,
            config: params.config,
            diagnostics: Default::default(),
            documents: Default::default(),
            dynamic_config: DynamicConfig::default(),
            editor_tx: params.editor_tx,
            language_id: params.language_id,
            language_servers: params.language_servers,
            outstanding_requests: HashMap::default(),
            pending_requests: vec![params.initial_request],
            pending_message_requests: VecDeque::new(),
            request_counter: 0,
            response_waitlist: HashMap::default(),
            session,
            work_done_progress: HashMap::default(),
            work_done_progress_report_timestamp: time::Instant::now(),
            pending_file_watchers: HashMap::default(),
        }
    }

    pub fn call<
        R: Request,
        F: for<'a> FnOnce(&'a mut Context, EditorMeta, Vec<(ServerName, R::Result)>) -> () + 'static,
    >(
        &mut self,
        meta: EditorMeta,
        params: RequestParams<R::Params>,
        callback: F,
    ) where
        R::Params: IntoParams + Clone,
        R::Result: for<'a> Deserialize<'a>,
    {
        let ops = match params {
            RequestParams::All(params) => {
                let mut ops = Vec::with_capacity(params.len() * self.language_servers.len());
                for server_name in self.language_servers.keys() {
                    let params: Vec<_> = params.to_vec();
                    for params in params {
                        ops.push((server_name.clone(), params));
                    }
                }
                ops
            }
            RequestParams::Each(params) => params
                .into_iter()
                .flat_map(|(key, ops)| {
                    let ops: Vec<(ServerName, <R as Request>::Params)> =
                        ops.into_iter().map(|op| (key.clone(), op)).collect();
                    ops
                })
                .collect(),
        };
        self.batch_call::<R, _>(
            meta,
            ops,
            Box::new(
                move |ctx: &mut Context,
                      meta: EditorMeta,
                      results: Vec<(ServerName, R::Result)>| {
                    callback(ctx, meta, results)
                },
            ),
        );
    }

    fn batch_call<
        R: Request,
        F: for<'a> FnOnce(&'a mut Context, EditorMeta, Vec<(ServerName, R::Result)>) -> () + 'static,
    >(
        &mut self,
        meta: EditorMeta,
        ops: Vec<(ServerName, R::Params)>,
        callback: F,
    ) where
        R::Params: IntoParams,
        R::Result: for<'a> Deserialize<'a>,
    {
        let batch_id = self.next_batch_id();

        self.batch_sizes.insert(
            batch_id,
            ops.iter().fold(HashMap::new(), |mut m, (server_name, _)| {
                let count = m.entry(server_name.clone()).or_default();
                *count += 1;
                m
            }),
        );
        self.batches.insert(
            batch_id,
            (
                Vec::with_capacity(ops.len()),
                Box::new(move |ctx, meta, vals| {
                    // Only get the last response of each server.
                    let results = vals
                        .into_iter()
                        .map(|(server_name, val)| {
                            (
                                server_name,
                                serde_json::from_value(val).expect("Failed to parse response"),
                            )
                        })
                        .collect();
                    callback(ctx, meta, results)
                }),
            ),
        );
        for (server_name, params) in ops {
            let params = params.into_params();
            if params.is_err() {
                error!("Failed to convert params");
                return;
            }
            let id = self.next_request_id();
            self.response_waitlist
                .insert(id.clone(), (meta.clone(), R::METHOD, batch_id, false));

            add_outstanding_request(
                &server_name,
                self,
                R::METHOD,
                meta.buffile.clone(),
                meta.client.clone(),
                id.clone(),
            );

            let call = jsonrpc_core::MethodCall {
                jsonrpc: Some(Version::V2),
                id,
                method: R::METHOD.into(),
                params: params.unwrap(),
            };
            let server = &self.language_servers[&server_name];
            if server
                .tx
                .send(ServerMessage::Request(Call::MethodCall(call)))
                .is_err()
            {
                error!("Failed to call language server");
            };
        }
    }

    pub fn cancel(&mut self, server_name: &ServerName, id: Id) {
        match self.response_waitlist.get_mut(&id) {
            Some((_meta, method, _batch_id, canceled)) => {
                debug!("Canceling request to {server_name} server {id:?} ({method})");
                *canceled = true;
            }
            None => {
                error!("Failed to cancel request {id:?} to {server_name} server");
            }
        }
        let id = match id {
            Id::Num(id) => id,
            _ => panic!("expected numeric ID for {} server", server_name),
        };
        self.notify::<Cancel>(
            server_name,
            CancelParams {
                id: NumberOrString::Number(id.try_into().unwrap()),
            },
        );
    }

    pub fn reply(&mut self, server_name: &ServerName, id: Id, result: Result<Value, Error>) {
        let output = match result {
            Ok(result) => Output::Success(Success {
                jsonrpc: Some(Version::V2),
                id,
                result,
            }),
            Err(error) => Output::Failure(Failure {
                jsonrpc: Some(Version::V2),
                id,
                error,
            }),
        };
        let server = &self.language_servers[server_name];
        if server.tx.send(ServerMessage::Response(output)).is_err() {
            error!("Failed to reply to language server {server_name}");
        };
    }

    pub fn notify<N: Notification>(&mut self, server_name: &ServerName, params: N::Params)
    where
        N::Params: IntoParams,
    {
        let params = params.into_params();
        if params.is_err() {
            error!("Failed to convert params");
            return;
        }
        let notification = jsonrpc_core::Notification {
            jsonrpc: Some(Version::V2),
            method: N::METHOD.into(),
            params: params.unwrap(),
        };
        let server = &self.language_servers[server_name];
        if server
            .tx
            .send(ServerMessage::Request(Call::Notification(notification)))
            .is_err()
        {
            error!("Failed to send notification to language server {server_name}");
        }
    }

    pub fn exec<S>(&self, meta: EditorMeta, command: S)
    where
        S: Into<Cow<'static, str>>,
    {
        let command = command.into();
        if let Some((fifo, which)) = meta
            .fifo
            .as_ref()
            .map(|f| (f, "fifo"))
            .or_else(|| meta.command_fifo.as_ref().map(|f| (f, "kak_command_fifo")))
        {
            debug!("To editor `{}` via {}: {}", meta.session, which, command);
            fs::write(fifo, command.as_bytes()).expect("Failed to write command to fifo");
            return;
        }
        if self
            .editor_tx
            .send(EditorResponse { meta, command })
            .is_err()
        {
            error!("Failed to send command to editor");
        }
    }

    fn next_batch_id(&mut self) -> BatchNumber {
        let id = self.batch_count;
        self.batch_count += 1;
        id
    }

    fn next_request_id(&mut self) -> Id {
        let id = Id::Num(self.request_counter);
        self.request_counter += 1;
        id
    }

    pub fn meta_for_buffer(&self, client: Option<String>, buffile: &str) -> Option<EditorMeta> {
        let document = self.documents.get(buffile)?;
        let mut meta = meta_for_session(self.session.clone(), client);
        meta.buffile = buffile.to_string();
        meta.version = document.version;
        Some(meta)
    }

    pub fn meta_for_buffer_version(
        &self,
        client: Option<String>,
        buffile: &str,
        version: i32,
    ) -> EditorMeta {
        let mut meta = meta_for_session(self.session.clone(), client);
        meta.buffile = buffile.to_string();
        meta.version = version;
        meta
    }
}

pub fn meta_for_session(session: String, client: Option<String>) -> EditorMeta {
    EditorMeta {
        session,
        client,
        buffile: "".to_string(),
        filetype: "".to_string(), // filetype is not used by ctx.exec, but it's definitely a code smell
        version: 0,
        fifo: None,
        command_fifo: None,
        hook: false,
        server: None,
        word_regex: None,
    }
}

fn add_outstanding_request(
    server_name: &ServerName,
    ctx: &mut Context,
    method: &'static str,
    buffile: String,
    client: Option<String>,
    id: Id,
) {
    let to_cancel =
        match ctx
            .outstanding_requests
            .entry((server_name.clone(), method, buffile, client))
        {
            Entry::Occupied(mut e) => {
                let OutstandingRequests { oldest, youngest } = e.get_mut();
                if oldest.is_none() {
                    *oldest = Some(id);
                    None
                } else {
                    let mut tmp = Some(id);
                    std::mem::swap(youngest, &mut tmp);
                    tmp
                }
            }
            Entry::Vacant(e) => {
                e.insert(OutstandingRequests {
                    oldest: Some(id),
                    youngest: None,
                });
                None
            }
        };
    if let Some(id) = to_cancel {
        ctx.cancel(server_name, id);
    }
}

pub fn remove_outstanding_request(
    server_name: &ServerName,
    ctx: &mut Context,
    method: &'static str,
    buffile: String,
    client: Option<String>,
    id: &Id,
) {
    let key = (server_name.clone(), method, buffile, client);
    if let Some(outstanding) = ctx.outstanding_requests.get_mut(&key) {
        if outstanding.youngest.as_ref() == Some(id) {
            outstanding.youngest = None;
            return;
        } else if outstanding.oldest.as_ref() == Some(id) {
            outstanding.oldest = std::mem::take(&mut outstanding.youngest);
            assert!(outstanding.youngest.is_none());
            return;
        }
    }
    error!(
        "[{}] Not in outstanding requests: method {} buffile {} client {}",
        key.0,
        key.1,
        key.2,
        key.3.unwrap_or_default()
    );
}
