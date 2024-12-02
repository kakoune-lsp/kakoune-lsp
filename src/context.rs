use crate::editor_transport::{self, ToEditor};
use crate::language_server_transport::LanguageServerTransport;
use crate::text_sync::CompiledFileSystemWatcher;
use crate::thread_worker::{ToEditorDispatcher, Worker};
use crate::{filetype_to_language_id_map, types::*};
use jsonrpc_core::{self, Call, Error, Failure, Id, Output, Success, Value, Version};
use lsp_types::notification::{Cancel, Notification};
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::path::PathBuf;
use std::time;

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
    Each(HashMap<ServerId, Vec<T>>),
}

pub type ResponsesCallback =
    Box<dyn FnOnce(&mut Context, EditorMeta, Vec<(ServerId, Value)>) -> ()>;
type BatchNumber = usize;
type BatchCount = BatchNumber;

pub struct OutstandingRequests {
    oldest: Option<Id>,
    youngest: Option<Id>,
}

pub struct ServerSettings {
    pub name: String,
    pub roots: Vec<RootPath>,
    pub offset_encoding: OffsetEncoding,
    pub preferred_offset_encoding: Option<OffsetEncoding>,
    pub transport: LanguageServerTransport,
    pub capabilities: Option<ServerCapabilities>,
    pub settings: Option<Value>,
    pub workaround_eslint: bool,
}

pub struct FileWatcher {
    pub pending_file_events: HashSet<FileEvent>,
    pub worker: Box<Worker<(), Vec<FileEvent>>>,
}

pub struct Context {
    batch_count: BatchCount,
    pub batch_sizes: HashMap<BatchNumber, HashMap<ServerId, usize>>,
    pub batches:
        HashMap<BatchNumber, (Vec<(ServerId, serde_json::value::Value)>, ResponsesCallback)>,
    pub buffer_tombstones: HashSet<String>,
    pub server_tombstones: HashSet<String>,
    pub code_lenses: HashMap<String, Vec<(ServerId, CodeLens)>>,
    pub completion_items: Vec<(ServerId, CompletionItem)>,
    pub completion_items_timestamp: i32,
    // We currently only track one client's completion items, to simplify cleanup (else we
    // might need to hook into ClientClose). Track the client name, so we can check if the
    // completions are valid.
    pub completion_last_client: Option<ClientId>,
    pub config: Config,
    pub diagnostics: HashMap<String, Vec<(ServerId, Diagnostic)>>,
    pub documents: HashMap<String, Document>,
    pub dynamic_config: DynamicConfig,
    pub language_servers: BTreeMap<ServerId, ServerSettings>,
    pub route_cache: HashMap<(ServerName, RootPath), ServerId>,
    pub outstanding_requests:
        HashMap<(ServerId, &'static str, String, Option<ClientId>), OutstandingRequests>,
    pub pending_requests: Vec<EditorRequest>,
    pub pending_requests_from_future: Vec<EditorRequest>,
    pub pending_message_requests: VecDeque<(Id, ServerId, ShowMessageRequestParams)>,
    pub request_counter: u64,
    pub response_waitlist: HashMap<Id, (EditorMeta, &'static str, BatchNumber, bool)>,
    pub session: SessionId,
    pub to_editor: ToEditor,
    pub to_editor_dispatcher: ToEditorDispatcher,
    pub work_done_progress: HashMap<NumberOrString, Option<WorkDoneProgressBegin>>,
    pub work_done_progress_report_timestamp: time::Instant,
    pub pending_file_watchers:
        HashMap<(ServerId, String, Option<PathBuf>), Vec<CompiledFileSystemWatcher>>,
    pub file_watcher: Option<FileWatcher>,
    #[deprecated]
    pub legacy_filetypes: HashMap<String, (LanguageId, Vec<ServerName>)>,
    pub is_exiting: bool,
}

impl Context {
    pub fn new(session: SessionId, to_editor: ToEditor, config: Config) -> Self {
        let legacy_filetypes = filetype_to_language_id_map(&config);
        #[allow(deprecated)]
        Context {
            batch_count: 0,
            batch_sizes: Default::default(),
            batches: Default::default(),
            buffer_tombstones: Default::default(),
            server_tombstones: Default::default(),
            code_lenses: Default::default(),
            completion_items: vec![],
            completion_items_timestamp: i32::MAX,
            completion_last_client: None,
            config,
            diagnostics: Default::default(),
            documents: Default::default(),
            dynamic_config: DynamicConfig::default(),
            language_servers: BTreeMap::new(),
            route_cache: HashMap::new(),
            outstanding_requests: HashMap::default(),
            pending_requests: vec![],
            pending_requests_from_future: vec![],
            pending_message_requests: VecDeque::new(),
            request_counter: 0,
            response_waitlist: HashMap::default(),
            session,
            to_editor: to_editor.clone(),
            to_editor_dispatcher: ToEditorDispatcher::OtherThread(to_editor),
            work_done_progress: HashMap::default(),
            work_done_progress_report_timestamp: time::Instant::now(),
            pending_file_watchers: HashMap::default(),
            file_watcher: None,
            legacy_filetypes,
            is_exiting: false,
        }
    }

    pub fn session(&self) -> &SessionId {
        &self.session
    }

    pub fn to_editor(&self) -> &ToEditor {
        &self.to_editor
    }

    pub fn main_root<'a>(&'a self, meta: &'a EditorMeta) -> &'a RootPath {
        let first_server = &self.servers(meta).next().unwrap().1;
        &self.server_config(meta, &first_server.name).unwrap().root
    }

    pub fn servers<'a>(
        &'a self,
        meta: &'a EditorMeta,
    ) -> impl Iterator<Item = (ServerId, &'a ServerSettings)> {
        meta.servers
            .iter()
            .map(move |&server_id| (server_id, self.server(server_id)))
    }
    pub fn server(&self, server_id: ServerId) -> &ServerSettings {
        &self.language_servers[&server_id]
    }
    pub fn server_config<'a>(
        &'a self,
        meta: &'a EditorMeta,
        server_name: &ServerName,
    ) -> Option<&'a LanguageServerConfig> {
        server_configs(&self.config, meta)
            .get(server_name_for_lookup(&self.config, &meta.language_id, server_name).as_ref())
    }

    pub fn call<
        R: Request,
        F: for<'a> FnOnce(&'a mut Context, EditorMeta, Vec<(ServerId, R::Result)>) -> () + 'static,
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
                let mut ops = Vec::with_capacity(params.len() * meta.servers.len());
                for &server_id in &meta.servers {
                    let params: Vec<_> = params.to_vec();
                    for params in params {
                        ops.push((server_id, params));
                    }
                }
                ops
            }
            RequestParams::Each(params) => params
                .into_iter()
                .flat_map(|(key, ops)| {
                    let ops: Vec<(ServerId, <R as Request>::Params)> =
                        ops.into_iter().map(|op| (key, op)).collect();
                    ops
                })
                .collect(),
        };
        self.batch_call::<R, _>(
            meta,
            ops,
            Box::new(
                move |ctx: &mut Context, meta: EditorMeta, results: Vec<(ServerId, R::Result)>| {
                    callback(ctx, meta, results)
                },
            ),
        );
    }

    fn batch_call<
        R: Request,
        F: for<'a> FnOnce(&'a mut Context, EditorMeta, Vec<(ServerId, R::Result)>) -> () + 'static,
    >(
        &mut self,
        meta: EditorMeta,
        ops: Vec<(ServerId, R::Params)>,
        callback: F,
    ) where
        R::Params: IntoParams,
        R::Result: for<'a> Deserialize<'a>,
    {
        let batch_id = self.next_batch_id();

        self.batch_sizes.insert(
            batch_id,
            ops.iter().fold(HashMap::new(), |mut m, (server_id, _)| {
                let count = m.entry(*server_id).or_default();
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
                        .map(|(server_id, val)| {
                            (
                                server_id,
                                serde_json::from_value(val).expect("Failed to parse response"),
                            )
                        })
                        .collect();
                    callback(ctx, meta, results)
                }),
            ),
        );
        for (server_id, params) in ops {
            let params = params.into_params();
            if params.is_err() {
                error!(self.to_editor(), "Failed to convert params");
                return;
            }
            let id = self.next_request_id();
            self.response_waitlist
                .insert(id.clone(), (meta.clone(), R::METHOD, batch_id, false));

            add_outstanding_request(
                server_id,
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
            let server = self.server(server_id);
            if server
                .transport
                .to_lang_server
                .sender()
                .send(ServerMessage::Request(Call::MethodCall(call)))
                .is_err()
            {
                error!(self.to_editor(), "Failed to call language server");
            };
        }
    }

    pub fn cancel(&mut self, server_id: ServerId, id: Id) {
        if let Some((_meta, method, _batch_id, _canceled)) = self.response_waitlist.get(&id) {
            debug!(
                self.to_editor(),
                "Canceling request to server {}: {:?} ({})",
                &self.server(server_id).name,
                id,
                method
            );
        }
        match self.response_waitlist.get_mut(&id) {
            Some((_meta, _method, _batch_id, canceled)) => {
                *canceled = true;
            }
            None => {
                error!(
                    self.to_editor(),
                    "Failed to cancel request {id:?} to server {}",
                    &self.server(server_id).name,
                );
            }
        }
        let id = match id {
            Id::Num(id) => id,
            _ => panic!(
                "expected numeric ID for {} server",
                &self.server(server_id).name
            ),
        };
        self.notify::<Cancel>(
            server_id,
            CancelParams {
                id: NumberOrString::Number(id.try_into().unwrap()),
            },
        );
    }

    pub fn reply(&mut self, server_id: ServerId, id: Id, result: Result<Value, Error>) {
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
        let server = &self.server(server_id);
        if server
            .transport
            .to_lang_server
            .sender()
            .send(ServerMessage::Response(output))
            .is_err()
        {
            error!(
                self.to_editor(),
                "Failed to reply to language server {}", &server.name
            );
        };
    }

    pub fn notify<N: Notification>(&mut self, server_id: ServerId, params: N::Params)
    where
        N::Params: IntoParams,
    {
        let params = params.into_params();
        if params.is_err() {
            error!(self.to_editor(), "Failed to convert params");
            return;
        }
        let notification = jsonrpc_core::Notification {
            jsonrpc: Some(Version::V2),
            method: N::METHOD.into(),
            params: params.unwrap(),
        };
        let server = &self.server(server_id);
        if server
            .transport
            .to_lang_server
            .sender()
            .send(ServerMessage::Request(Call::Notification(notification)))
            .is_err()
        {
            error!(
                self.to_editor(),
                "Failed to send notification to language server {}", &server.name,
            );
        }
    }

    pub fn exec<S>(&self, meta: EditorMeta, command: S)
    where
        S: Into<Cow<'static, str>>,
    {
        self.exec_fifo(meta, None, command);
    }

    pub fn exec_fifo<S>(&self, meta: EditorMeta, response_fifo: Option<ResponseFifo>, command: S)
    where
        S: Into<Cow<'static, str>>,
    {
        editor_transport::exec_fifo(&self.to_editor_dispatcher, meta, response_fifo, command);
    }

    pub fn show_error(&mut self, meta: EditorMeta, message: impl AsRef<str>) {
        self.show_error_fifo(meta, None, message);
    }

    pub fn show_error_fifo(
        &mut self,
        meta: EditorMeta,
        response_fifo: Option<ResponseFifo>,
        message: impl AsRef<str>,
    ) {
        editor_transport::show_error(&self.to_editor_dispatcher, meta, response_fifo, message);
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
}

fn add_outstanding_request(
    server_id: ServerId,
    ctx: &mut Context,
    method: &'static str,
    buffile: String,
    client: Option<ClientId>,
    id: Id,
) {
    let to_cancel = match ctx
        .outstanding_requests
        .entry((server_id, method, buffile, client))
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
        ctx.cancel(server_id, id);
    }
}

pub fn remove_outstanding_request(
    server_id: ServerId,
    ctx: &mut Context,
    method: &'static str,
    buffile: String,
    client: Option<ClientId>,
    id: &Id,
) {
    let key = (server_id, method, buffile, client);
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
        ctx.to_editor(),
        "[{}] Not in outstanding requests: method {} buffile {} client {}",
        key.0,
        key.1,
        key.2,
        key.3.unwrap_or_default()
    );
}
