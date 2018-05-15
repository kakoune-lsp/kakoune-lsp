use crossbeam_channel::Sender;
use fnv::FnvHashMap;
use jsonrpc_core::{self, Call, Id, Params, Version};
use languageserver_types::*;
use types::*;

pub struct Context {
    pub capabilities: Option<ServerCapabilities>,
    pub config: Config,
    pub controller_poison_tx: Sender<()>,
    pub diagnostics: FnvHashMap<String, Vec<Diagnostic>>,
    pub editor_tx: Sender<EditorResponse>,
    pub lang_srv_poison_tx: Sender<()>,
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
        lang_srv_poison_tx: Sender<()>,
        controller_poison_tx: Sender<()>,
        config: Config,
    ) -> Self {
        let session = initial_request.meta.session.clone();
        Context {
            capabilities: None,
            config,
            controller_poison_tx,
            diagnostics: FnvHashMap::default(),
            editor_tx,
            lang_srv_poison_tx,
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
        let params = params.to_params().expect("Failed to convert params");
        let notification = jsonrpc_core::Notification {
            jsonrpc: Some(Version::V2),
            method,
            // NOTE this is required because jsonrpc serializer converts Some(None) into []
            params: match params {
                Params::None => None,
                params => Some(params),
            },
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
