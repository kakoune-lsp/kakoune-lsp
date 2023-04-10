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
use std::collections::{HashMap, VecDeque};
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

pub type ResponsesCallback = Box<dyn FnOnce(&mut Context, EditorMeta, Vec<Value>) -> ()>;
type BatchNumber = usize;
type BatchCount = BatchNumber;

pub struct OutstandingRequests {
    oldest: Option<Id>,
    youngest: Option<Id>,
}

pub struct Context {
    batch_counter: BatchNumber,
    pub batches:
        HashMap<BatchNumber, (BatchCount, Vec<serde_json::value::Value>, ResponsesCallback)>,
    pub capabilities: Option<ServerCapabilities>,
    pub completion_items: Vec<CompletionItem>,
    pub completion_items_timestamp: i32,
    // We currently only track one client's completion items, to simplify cleanup (else we
    // might need to hook into ClientClose). Track the client name, so we can check if the
    // completions are valid.
    pub completion_last_client: Option<String>,
    pub config: Config,
    pub dynamic_config: DynamicConfig,
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
    pub code_lenses: HashMap<String, Vec<CodeLens>>,
    pub editor_tx: Sender<EditorResponse>,
    pub lang_srv_tx: Sender<ServerMessage>,
    pub language_id: String,
    pub outstanding_requests: HashMap<(&'static str, String, Option<String>), OutstandingRequests>,
    pub pending_requests: Vec<EditorRequest>,
    pub pending_message_requests: VecDeque<(Id, ShowMessageRequestParams)>,
    pub request_counter: u64,
    pub response_waitlist: HashMap<Id, (EditorMeta, &'static str, BatchNumber, bool)>,
    pub root_path: String,
    pub session: SessionId,
    pub documents: HashMap<String, Document>,
    pub offset_encoding: OffsetEncoding,
    pub preferred_offset_encoding: Option<OffsetEncoding>,
    pub work_done_progress: HashMap<NumberOrString, Option<WorkDoneProgressBegin>>,
    pub work_done_progress_report_timestamp: time::Instant,
    pub pending_file_watchers: HashMap<Option<PathBuf>, Vec<CompiledFileSystemWatcher>>,
}

pub struct ContextBuilder {
    pub language_id: String,
    pub initial_request: EditorRequest,
    pub lang_srv_tx: Sender<ServerMessage>,
    pub editor_tx: Sender<EditorResponse>,
    pub config: Config,
    pub root_path: String,
    pub offset_encoding: Option<OffsetEncoding>,
}

impl Context {
    pub fn new(params: ContextBuilder) -> Self {
        let session = params.initial_request.meta.session.clone();
        Context {
            batch_counter: 0,
            batches: HashMap::default(),
            capabilities: None,
            completion_items: vec![],
            completion_items_timestamp: i32::max_value(),
            completion_last_client: None,
            config: params.config,
            dynamic_config: DynamicConfig::default(),
            diagnostics: HashMap::default(),
            code_lenses: HashMap::default(),
            editor_tx: params.editor_tx,
            lang_srv_tx: params.lang_srv_tx,
            language_id: params.language_id,
            outstanding_requests: HashMap::default(),
            pending_requests: vec![params.initial_request],
            pending_message_requests: VecDeque::new(),
            request_counter: 0,
            response_waitlist: HashMap::default(),
            root_path: params.root_path,
            session,
            documents: HashMap::default(),
            offset_encoding: params.offset_encoding.unwrap_or(OffsetEncoding::Utf16),
            preferred_offset_encoding: params.offset_encoding,
            work_done_progress: HashMap::default(),
            work_done_progress_report_timestamp: time::Instant::now(),
            pending_file_watchers: HashMap::default(),
        }
    }

    pub fn call<
        R: Request,
        F: for<'a> FnOnce(&'a mut Context, EditorMeta, R::Result) -> () + 'static,
    >(
        &mut self,
        meta: EditorMeta,
        params: R::Params,
        callback: F,
    ) where
        R::Params: IntoParams,
        R::Result: for<'a> Deserialize<'a>,
    {
        let ops: Vec<R::Params> = vec![params];
        self.batch_call::<R, _>(
            meta,
            ops,
            Box::new(
                move |ctx: &mut Context, meta: EditorMeta, mut results: Vec<R::Result>| {
                    if let Some(result) = results.pop() {
                        callback(ctx, meta, result);
                    }
                },
            ),
        );
    }

    pub fn batch_call<
        R: Request,
        F: for<'a> FnOnce(&'a mut Context, EditorMeta, Vec<R::Result>) -> () + 'static,
    >(
        &mut self,
        meta: EditorMeta,
        ops: Vec<R::Params>,
        callback: F,
    ) where
        R::Params: IntoParams,
        R::Result: for<'a> Deserialize<'a>,
    {
        let batch_id = self.next_batch_id();
        self.batches.insert(
            batch_id,
            (
                ops.len(),
                Vec::with_capacity(ops.len()),
                Box::new(move |ctx, meta, vals| {
                    let results: Vec<R::Result> = vals
                        .into_iter()
                        .map(|val| serde_json::from_value(val).expect("Failed to parse response"))
                        .collect();
                    callback(ctx, meta, results)
                }),
            ),
        );
        for params in ops {
            let params = params.into_params();
            if params.is_err() {
                error!("Failed to convert params");
                return;
            }
            let id = self.next_request_id();
            self.response_waitlist
                .insert(id.clone(), (meta.clone(), R::METHOD, batch_id, false));
            add_outstanding_request(
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
            if self
                .lang_srv_tx
                .send(ServerMessage::Request(Call::MethodCall(call)))
                .is_err()
            {
                error!("Failed to call language server");
            };
        }
    }

    pub fn cancel(&mut self, id: Id) {
        match self.response_waitlist.get_mut(&id) {
            Some((_meta, method, _batch_id, canceled)) => {
                debug!("Canceling request {id:?} ({method})");
                *canceled = true;
            }
            None => {
                error!("Failed to cancel request {id:?}");
            }
        }
        let id = match id {
            Id::Num(id) => id,
            _ => panic!("expected numeric ID"),
        };
        self.notify::<Cancel>(CancelParams {
            id: NumberOrString::Number(id.try_into().unwrap()),
        });
    }

    pub fn reply(&mut self, id: Id, result: Result<Value, Error>) {
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
        if self
            .lang_srv_tx
            .send(ServerMessage::Response(output))
            .is_err()
        {
            error!("Failed to reply to language server");
        };
    }

    pub fn notify<N: Notification>(&mut self, params: N::Params)
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
        if self
            .lang_srv_tx
            .send(ServerMessage::Request(Call::Notification(notification)))
            .is_err()
        {
            error!("Failed to send notification to language server");
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
        let id = self.batch_counter;
        self.batch_counter += 1;
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
        write_response_to_fifo: false,
        hook: false,
    }
}

fn add_outstanding_request(
    ctx: &mut Context,
    method: &'static str,
    buffile: String,
    client: Option<String>,
    id: Id,
) {
    let to_cancel = match ctx.outstanding_requests.entry((method, buffile, client)) {
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
        ctx.cancel(id);
    }
}

pub fn remove_outstanding_request(
    ctx: &mut Context,
    method: &'static str,
    buffile: String,
    client: Option<String>,
    id: &Id,
) {
    let key = (method, buffile, client);
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
        "Not in outstanding requests: method {} buffile {} client {}",
        key.0,
        key.1,
        key.2.unwrap_or_default()
    );
}
