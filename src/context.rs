use crate::types::*;
use crossbeam_channel::Sender;
use jsonrpc_core::{self, Call, Id, Params, Version};
use lsp_types::*;
use ropey;
use std::collections::HashMap;
use std::fs;

pub struct Document {
    pub version: u64,
    pub text: ropey::Rope,
}

pub struct Context {
    pub capabilities: Option<ServerCapabilities>,
    pub config: Config,
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
    pub editor_tx: Sender<EditorResponse>,
    pub lang_srv_tx: Sender<ServerMessage>,
    pub language_id: String,
    pub pending_requests: Vec<EditorRequest>,
    pub request_counter: u64,
    pub response_waitlist: HashMap<Id, (EditorMeta, String, EditorParams)>,
    pub root_path: String,
    pub session: SessionId,
    pub documents: HashMap<String, Document>,
    pub offset_encoding: String,
}

impl Context {
    pub fn new(
        language_id: &str,
        initial_request: EditorRequest,
        lang_srv_tx: Sender<ServerMessage>,
        editor_tx: Sender<EditorResponse>,
        config: Config,
        root_path: String,
        offset_encoding: String,
    ) -> Self {
        let session = initial_request.meta.session.clone();
        Context {
            capabilities: None,
            config,
            diagnostics: HashMap::default(),
            editor_tx,
            lang_srv_tx,
            language_id: language_id.to_string(),
            pending_requests: vec![initial_request],
            request_counter: 0,
            response_waitlist: HashMap::default(),
            root_path,
            session,
            documents: HashMap::default(),
            offset_encoding,
        }
    }

    pub fn call(&mut self, id: Id, method: String, params: impl ToParams) {
        let params = params.to_params();
        if params.is_err() {
            error!("Failed to convert params");
            return;
        }
        let call = jsonrpc_core::MethodCall {
            jsonrpc: Some(Version::V2),
            id,
            method,
            params: Some(params.unwrap()),
        };
        self.lang_srv_tx
            .send(ServerMessage::Request(Call::MethodCall(call)));
    }

    pub fn notify(&mut self, method: String, params: impl ToParams) {
        let params = params.to_params();
        if params.is_err() {
            error!("Failed to convert params");
            return;
        }
        let notification = jsonrpc_core::Notification {
            jsonrpc: Some(Version::V2),
            method,
            // NOTE this is required because jsonrpc serializer converts Some(None) into []
            params: match params.unwrap() {
                Params::None => None,
                params => Some(params),
            },
        };
        self.lang_srv_tx
            .send(ServerMessage::Request(Call::Notification(notification)))
    }

    pub fn exec(&self, meta: EditorMeta, command: String) {
        match meta.fifo.as_ref() {
            Some(fifo) => {
                debug!("To editor `{}`: {}", meta.session, command);
                fs::write(fifo, command).expect("Failed to write command to fifo")
            }
            None => self.editor_tx.send(EditorResponse { meta, command }),
        }
    }

    pub fn next_request_id(&mut self) -> Id {
        let id = Id::Num(self.request_counter);
        self.request_counter += 1;
        id
    }
}
