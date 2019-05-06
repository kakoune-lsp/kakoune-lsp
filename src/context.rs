use crate::types::*;
use crossbeam_channel::Sender;
use jsonrpc_core::{self, Call, Id, Params, Value, Version};
use lsp_types::notification::Notification;
use lsp_types::request::*;
use lsp_types::*;
use ropey;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

// Copy of Kakoune's timestamped buffer content.
pub struct Document {
    // Corresponds to Kakoune's timestamp.
    // It's passed to a language server as a version and is used to tag selections, highlighters and
    // other timestamp sensitive parameters in commands sent to kakoune.
    pub version: u64,
    // Buffer content.
    // It's used to translate between LSP and Kakoune coordinates.
    pub text: ropey::Rope,
}

// FnOnce doesn't work yet, so we use FnMut for now
// https://github.com/rust-lang/rust/issues/28796
pub type ResponseCallback = Box<FnMut(&mut Context, EditorMeta, Value) -> ()>;

pub struct Context {
    pub capabilities: Option<ServerCapabilities>,
    pub config: Config,
    pub diagnostics: HashMap<String, Vec<Diagnostic>>,
    pub editor_tx: Sender<EditorResponse>,
    pub lang_srv_tx: Sender<ServerMessage>,
    pub language_id: String,
    pub pending_requests: Vec<EditorRequest>,
    pub request_counter: u64,
    pub response_waitlist: HashMap<Id, (EditorMeta, &'static str, ResponseCallback)>,
    pub root_path: String,
    pub session: SessionId,
    pub documents: HashMap<String, Document>,
    pub offset_encoding: OffsetEncoding,
}

impl Context {
    pub fn new(
        language_id: &str,
        initial_request: EditorRequest,
        lang_srv_tx: Sender<ServerMessage>,
        editor_tx: Sender<EditorResponse>,
        config: Config,
        root_path: String,
        offset_encoding: OffsetEncoding,
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

    pub fn call<
        R: Request,
        F: for<'a> FnOnce(&'a mut Context, EditorMeta, R::Result) -> () + 'static,
    >(
        &mut self,
        meta: EditorMeta,
        params: R::Params,
        callback: F,
    ) where
        R::Params: ToParams,
        R::Result: for<'a> Deserialize<'a>,
    {
        let params = params.to_params();
        if params.is_err() {
            error!("Failed to convert params");
            return;
        }
        let id = self.next_request_id();
        let mut callback = Some(callback);
        self.response_waitlist.insert(
            id.clone(),
            (
                meta,
                R::METHOD,
                Box::new(move |ctx, meta, val| {
                    let result = serde_json::from_value(val).expect("Failed to parse response");
                    // This is a hack because Box<FnOnce> doesn't work yet
                    callback.take().unwrap()(ctx, meta, result)
                }),
            ),
        );
        let call = jsonrpc_core::MethodCall {
            jsonrpc: Some(Version::V2),
            id,
            method: R::METHOD.into(),
            params: Some(params.unwrap()),
        };
        self.lang_srv_tx
            .send(ServerMessage::Request(Call::MethodCall(call)));
    }

    pub fn notify<N: Notification>(&mut self, params: N::Params)
    where
        N::Params: ToParams,
    {
        let params = params.to_params();
        if params.is_err() {
            error!("Failed to convert params");
            return;
        }
        let notification = jsonrpc_core::Notification {
            jsonrpc: Some(Version::V2),
            method: N::METHOD.into(),
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

    fn next_request_id(&mut self) -> Id {
        let id = Id::Num(self.request_counter);
        self.request_counter += 1;
        id
    }

    pub fn meta_for_session(&self) -> EditorMeta {
        EditorMeta {
            session: self.session.clone(),
            client: None,
            buffile: "".to_string(),
            filetype: "".to_string(), // filetype is not used by ctx.exec, but it's definitely a code smell
            version: 0,
            fifo: None,
        }
    }

    pub fn meta_for_buffer(&self, buffile: String) -> Option<EditorMeta> {
        let document = self.documents.get(&buffile)?;
        Some(EditorMeta {
            session: self.session.clone(),
            client: None,
            buffile: buffile,
            filetype: "".to_string(), // filetype is not used by ctx.exec, but it's definitely a code smell
            version: document.version,
            fifo: None,
        })
    }
}
