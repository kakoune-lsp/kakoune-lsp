use crossbeam_channel::Sender;
use fnv::FnvHashMap;
use jsonrpc_core::{self, Call, Id, Version};
use languageserver_types::*;
use types::*;

pub struct Context {
    pub capabilities: Option<ServerCapabilities>,
    pub editor_tx: Sender<EditorResponse>,
    pub diagnostics: FnvHashMap<String, Vec<Diagnostic>>,
    pub lang_srv_tx: Sender<ServerMessage>,
    pub language_id: String,
    pub pending_requests: Vec<EditorRequest>,
    pub request_counter: u64,
    pub response_waitlist: FnvHashMap<Id, (EditorMeta, String, EditorParams)>,
    pub session: SessionId,
    pub versions: FnvHashMap<String, u64>,
}

impl Context {
    pub fn new(
        language_id: &str,
        initial_request: EditorRequest,
        lang_srv_tx: Sender<ServerMessage>,
        editor_tx: Sender<EditorResponse>,
    ) -> Self {
        let session = initial_request.meta.session.clone();
        Context {
            capabilities: None,
            diagnostics: FnvHashMap::default(),
            editor_tx,
            lang_srv_tx,
            language_id: language_id.to_string(),
            pending_requests: vec![initial_request],
            request_counter: 0,
            response_waitlist: FnvHashMap::default(),
            session,
            versions: FnvHashMap::default(),
        }
    }

    pub fn call(&mut self, id: Id, method: String, params: impl ToParams) {
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

    pub fn notify(&mut self, method: String, params: impl ToParams) {
        let notification = jsonrpc_core::Notification {
            jsonrpc: Some(Version::V2),
            method,
            params: Some(params.to_params().expect("Failed to convert params")),
        };
        self.lang_srv_tx
            .send(ServerMessage::Request(Call::Notification(notification)))
            .expect("Failed to send request to language server transport");
    }

    pub fn exec(&self, meta: EditorMeta, command: String) {
        self.editor_tx
            .send(EditorResponse { meta, command })
            .expect("Failed to send message to editor transport");
    }

    pub fn next_request_id(&mut self) -> Id {
        let id = Id::Num(self.request_counter);
        self.request_counter += 1;
        id
    }
}
