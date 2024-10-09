use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::{self};
use std::io::Read;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use std::{iter, mem};

use crate::capabilities::{self, initialize};
use crate::context::meta_for_session;
use crate::context::Context;
use crate::diagnostics;
use crate::editor_transport;
use crate::language_features::{selection_range, *};
use crate::language_server_transport;
use crate::log::DEBUG;
use crate::progress;
use crate::project_root::find_project_root;
use crate::show_message::{self, MessageRequestResponse};
use crate::text_sync::*;
use crate::thread_worker::Worker;
use crate::types::*;
use crate::util::*;
use crate::workspace::{
    self, EditorApplyEdit, EditorDidChangeConfigurationParams, EditorExecuteCommand,
};
use crate::{context::*, set_logger};
use ccls::{EditorCallParams, EditorInheritanceParams, EditorMemberParams, EditorNavigateParams};
use code_lens::{text_document_code_lens, CodeLensOptions};
use crossbeam_channel::{after, never, tick, Receiver, Select, Sender};
use indoc::formatdoc;
use inlay_hints::InlayHintsOptions;
use itertools::Itertools;
use jsonrpc_core::{Call, ErrorCode, MethodCall, Output, Params};
use lsp_types::error_codes::CONTENT_MODIFIED;
use lsp_types::notification::DidChangeWorkspaceFolders;
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use lsp_types::*;
use serde::Deserialize;
use sloggers::types::Severity;

#[derive(Eq, PartialEq)]
enum QuoteState {
    OutsideArg,
    InsideArg,
    InsideArgSQ,
}

pub struct ParserState {
    session: SessionId,
    pub buf: Vec<u8>,
    offset: usize,
    state: QuoteState,
}

impl ParserState {
    pub fn new(session: SessionId) -> Self {
        ParserState {
            session,
            buf: vec![],
            offset: 0,
            state: QuoteState::OutsideArg,
        }
    }
}

fn next_string(state: &mut ParserState) -> String {
    let mut out = vec![];
    assert!(state.offset < state.buf.len());
    while state.offset < state.buf.len() {
        if process(state, &mut out) {
            break;
        }
    }
    if state.offset == state.buf.len() {
        assert!(state.state == QuoteState::InsideArgSQ);
        state.state = QuoteState::OutsideArg;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn process(state: &mut ParserState, out: &mut Vec<u8>) -> bool {
    let c = state.buf[state.offset];
    state.offset += 1;
    if state.state == QuoteState::OutsideArg {
        if c == b'\'' {
            state.state = QuoteState::InsideArg;
        } else {
            assert!(c == b' ', "expected space before quote, saw {}", c);
        }
    } else if state.state == QuoteState::InsideArg {
        if c == b'\'' {
            state.state = QuoteState::InsideArgSQ;
        } else {
            out.push(c);
        }
    } else if state.state == QuoteState::InsideArgSQ {
        if c == b'\'' {
            out.push(b'\'');
            state.state = QuoteState::InsideArg;
        } else {
            state.state = QuoteState::OutsideArg;
            assert!(c == b' ', "expected space after quote, saw {}", c);
            return true;
        }
    }
    false
}

trait FromString: Sized {
    type Err;
    fn from_string(s: String) -> Result<Self, Self::Err>;
}

impl FromString for String {
    type Err = ();
    fn from_string(s: String) -> Result<Self, Self::Err> {
        Ok(s)
    }
}

impl FromString for Option<String> {
    type Err = ();
    fn from_string(s: String) -> Result<Self, Self::Err> {
        let maybe_string = (!s.is_empty()).then_some(s);
        Ok(maybe_string)
    }
}

impl FromString for CodeActionKind {
    type Err = ();
    fn from_string(s: String) -> Result<Self, Self::Err> {
        Ok(s.into())
    }
}

impl FromString for ProgressToken {
    type Err = ();
    fn from_string(s: String) -> Result<Self, Self::Err> {
        Ok(ProgressToken::String(s))
    }
}

trait UseFromStr: FromStr {}

impl<T: UseFromStr> FromString for T {
    type Err = T::Err;
    fn from_string(s: String) -> Result<Self, Self::Err> {
        T::from_str(&s)
    }
}

impl UseFromStr for bool {}
impl UseFromStr for i8 {}
impl UseFromStr for u8 {}
impl UseFromStr for i32 {}
impl UseFromStr for u32 {}
impl UseFromStr for isize {}
impl UseFromStr for usize {}

pub trait Deserializable {
    fn deserialize(state: &mut ParserState) -> Self;
}
impl<T: FromString> Deserializable for T
where
    <T as FromString>::Err: std::fmt::Debug,
{
    fn deserialize(state: &mut ParserState) -> Self {
        T::from_string(next_string(state)).unwrap()
    }
}
impl Deserializable for KakounePosition {
    fn deserialize(state: &mut ParserState) -> Self {
        KakounePosition {
            line: state.next(),
            column: state.next(),
        }
    }
}
impl Deserializable for FormattingOptions {
    fn deserialize(state: &mut ParserState) -> Self {
        FormattingOptions {
            tab_size: state.next(),
            insert_spaces: state.next(),
            properties: HashMap::new(),
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        }
    }
}

impl ParserState {
    pub fn next<T: Deserializable>(&mut self) -> T {
        T::deserialize(self)
    }

    pub fn next_vec<T: Deserializable>(&mut self, n: usize) -> Vec<T> {
        iter::from_fn(|| Some(T::deserialize(self)))
            .take(n)
            .collect()
    }

    pub fn buffer_contents(&mut self) -> String {
        let path: String = self.next();
        let mut buf = vec![];
        fs::File::open(path.clone())
            .unwrap()
            .read_to_end(&mut buf)
            .unwrap();
        let _ = fs::remove_file(path);
        String::from_utf8_lossy(&buf).to_string()
    }
}

fn dispatch_fifo_request(
    state: &mut ParserState,
    to_editor: &Sender<EditorResponse>,
    from_editor: &Sender<EditorRequest>,
) -> ControlFlow<()> {
    let session = SessionId(state.next());
    if session.as_str() == "$exit" {
        return ControlFlow::Break(());
    }
    let client: String = state.next();
    let hook = state.next();
    let buffile = state.next();
    let version = state.next();
    let filetype = state.next();
    let language_id = state.next();
    let lsp_servers: String = state.next();
    let lsp_semantic_tokens: String = state.next();

    let parse_error = |what, err| {
        handle_broken_editor_request(to_editor, &session, &client, hook, what, err);
        ControlFlow::Continue(())
    };

    let language_server: toml::Value = match toml::from_str(&lsp_servers) {
        Ok(ls) => ls,
        Err(err) => return parse_error("%opt{lsp_servers}", err),
    };
    let language_server =
        match HashMap::<ServerName, LanguageServerConfig>::deserialize(language_server) {
            Ok(ls) => ls,
            Err(err) => return parse_error("%opt{lsp_servers}", err),
        };
    let semantic_tokens: toml::Value =
        match toml::from_str(&format!("faces = {}", lsp_semantic_tokens.trim_start())) {
            Ok(st) => st,
            Err(err) => return parse_error("%opt{lsp_semantic_tokens}", err),
        };
    let semantic_tokens = match SemanticTokenConfig::deserialize(semantic_tokens) {
        Ok(st) => st,
        Err(err) => return parse_error("%opt{lsp_semantic_tokens}", err),
    };
    let mut meta = EditorMeta {
        session: session.clone(),
        client: (!client.is_empty()).then_some(client),
        buffile,
        language_id,
        filetype,
        version,
        fifo: None,
        command_fifo: None,
        hook,
        language_server,
        semantic_tokens,
        server: None,
        word_regex: None,
        servers: Default::default(),
    };

    fn sync_trailer(meta: &mut EditorMeta, state: &mut ParserState, is_sync: bool) {
        if is_sync {
            meta.command_fifo = Some(state.next());
            meta.fifo = Some(state.next());
        }
    }

    let method: String = state.next();
    let params = EditorParams(match method.as_str() {
        "$ccls/call" => Box::new(EditorCallParams {
            position: state.next(),
            callee: state.next(),
        }),
        "$ccls/inheritance" => Box::new(EditorInheritanceParams {
            position: state.next(),
            levels: state.next(),
            derived: state.next(),
        }),
        "$ccls/member" => Box::new(EditorMemberParams {
            position: state.next(),
            kind: state.next(),
        }),
        "$ccls/navigate" => Box::new(EditorNavigateParams {
            position: state.next(),
            direction: state.next(),
        }),
        "$ccls/vars" => Box::new(PositionParams {
            position: state.next(),
        }),
        "apply-workspace-edit" => {
            let is_sync = state.next::<String>() == "is-sync";
            let params = Box::new(EditorApplyEdit { edit: state.next() });
            sync_trailer(&mut meta, state, is_sync);
            params
        }
        "capabilities" => Box::new(()),
        "codeAction/resolve" => Box::new(CodeActionResolveParams {
            code_action: state.next(),
        }),
        "completionItem/resolve" => {
            let params = Box::new(CompletionItemResolveParams {
                completion_item_timestamp: state.next(),
                completion_item_index: state.next(),
                pager_active: state.next(),
            });
            if params.completion_item_index == -1 {
                return ControlFlow::Continue(());
            }
            params
        }
        "eclipse.jdt.ls/organizeImports" => Box::new(()),
        "exit" => Box::new(()),
        "kakoune/breadcrumbs" => Box::new(BreadcrumbsParams {
            position_line: state.next(),
        }),
        "kakoune/goto-document-symbol" => Box::new(GotoSymbolParams {
            goto_symbol: state.next(),
        }),
        "kakoune/next-or-previous-symbol" => {
            let num_symbol_kinds = state.next();
            Box::new(NextOrPrevSymbolParams {
                position: state.next(),
                search_next: match state.next::<String>().as_str() {
                    "next" => true,
                    "previous" => false,
                    _ => panic!("invalid request"),
                },
                hover: match state.next::<String>().as_str() {
                    "hover" => true,
                    "goto" => false,
                    _ => panic!("invalid request"),
                },
                symbol_kinds: state.next_vec(num_symbol_kinds),
            })
        }
        "kakoune/object" => Box::new(ObjectParams {
            count: state.next(),
            mode: state.next(),
            selections_desc: {
                let selection_count = state.next();
                state.next_vec(selection_count)
            },
            symbol_kinds: {
                let num_symbol_kinds = state.next();
                state.next_vec(num_symbol_kinds)
            },
        }),
        "kakoune/textDocument/codeLens" => Box::new(CodeLensOptions {
            selection_desc: state.next(),
        }),
        "kakoune/did-change-option" => {
            let hook_param = state.next::<String>();
            let debug = match hook_param.as_str() {
                "lsp_debug=true" => true,
                "lsp_debug=false" => false,
                _ => panic!("invalid request"),
            };
            DEBUG.store(debug, Relaxed);
            set_logger(if debug {
                Severity::Debug
            } else {
                Severity::Info
            });
            info!(state.session, "Applied option change {}", hook_param);
            return ControlFlow::Continue(());
        }
        "rust-analyzer/expandMacro" => Box::new(PositionParams {
            position: state.next(),
        }),
        "textDocument/build" => Box::new(()),
        "textDocument/codeAction" => {
            let selection_desc = state.next();
            let num_filters = state.next();
            let perform_code_action = state.next();
            let is_sync = state.next::<String>() == "is-sync";
            let params = Box::new(CodeActionsParams {
                selection_desc,
                perform_code_action,
                auto_single: false,
                filters: match state.next::<String>().as_str() {
                    "only" => (num_filters != 0)
                        .then(|| CodeActionFilter::ByKind(state.next_vec(num_filters))),
                    "matching" => Some(CodeActionFilter::ByRegex(state.next())),
                    _ => panic!("invalid request"),
                },
            });
            sync_trailer(&mut meta, state, is_sync);
            params
        }
        "textDocument/codeLens" => Box::new(()),
        "textDocument/completion" => Box::new(TextDocumentCompletionParams {
            position: state.next(),
            completion: EditorCompletion {
                offset: state.next(),
            },
        }),
        "textDocument/definition" => {
            meta.word_regex = Some(state.next());
            Box::new(PositionParams {
                position: state.next(),
            })
        }
        "textDocument/declaration"
        | "textDocument/implementation"
        | "textDocument/typeDefinition" => Box::new(PositionParams {
            position: state.next(),
        }),
        "textDocument/diagnostics" | "textDocument/documentSymbol" => Box::new(()),
        "textDocument/didChange" => Box::new(TextDocumentDidChangeParams {
            draft: state.buffer_contents(),
        }),
        "textDocument/didClose" => Box::new(()),
        "textDocument/didOpen" => Box::new(TextDocumentDidOpenParams {
            draft: state.buffer_contents(),
        }),
        "textDocument/didSave" => Box::new(()),
        "textDocument/documentHighlight" => Box::new(PositionParams {
            position: state.next(),
        }),
        "textDocument/formatting" => {
            let params = Box::new(<FormattingOptions as Deserializable>::deserialize(state));
            let is_sync = state.next::<String>() == "is-sync";
            if let Some(server_override) = state.next() {
                meta.server = Some(server_override);
            }
            sync_trailer(&mut meta, state, is_sync);
            params
        }
        "textDocument/forwardSearch" => Box::new(PositionParams {
            position: state.next(),
        }),
        "textDocument/hover" => Box::new(EditorHoverParams {
            selection_desc: state.next(),
            tabstop: state.next(),
            hover_client: state.next(),
        }),
        "textDocument/inlayHint" => Box::new(InlayHintsOptions {
            buf_line_count: state.next(),
        }),
        "textDocument/prepareCallHierarchy" => Box::new(CallHierarchyParams {
            position: state.next(),
            incoming_or_outgoing: state.next(),
        }),
        "textDocument/rangeFormatting" => {
            let params = Box::new(RangeFormattingParams {
                formatting_options: state.next(),
                ranges: {
                    let selection_count: usize = state.next();
                    iter::from_fn(|| state.next())
                        .take(selection_count)
                        .collect()
                },
            });
            let is_sync = state.next::<String>() == "is-sync";
            if let Some(server_override) = state.next() {
                meta.server = Some(server_override);
            }
            sync_trailer(&mut meta, state, is_sync);
            params
        }
        "textDocument/references" => {
            let params = Box::new(PositionParams {
                position: state.next(),
            });
            meta.word_regex = Some(state.next());
            params
        }
        "textDocument/rename" => Box::new(TextDocumentRenameParams {
            position: state.next(),
            new_name: state.next(),
        }),
        "textDocument/selectionRange" => Box::new(SelectionRangePositionParams {
            position: state.next(),
            selections_desc: {
                let selection_count = state.next();
                state.next_vec(selection_count)
            },
        }),
        "textDocument/signatureHelp" => Box::new(PositionParams {
            position: state.next(),
        }),
        "textDocument/semanticTokens/full" => Box::new(()),
        "textDocument/switchSourceHeader" => Box::new(()),
        "window/showMessageRequest/showNext" => Box::new(()),
        "window/showMessageRequest/respond" => {
            let id: toml::Value = toml::from_str(&state.next::<String>()).unwrap();
            Box::new(MessageRequestResponse {
                message_request_id: jsonrpc_core::Id::deserialize(id).unwrap(),
                item: state
                    .next::<Option<String>>()
                    .map(|s| toml::from_str(&s).unwrap()),
            })
        }
        "window/workDoneProgress/cancel" => Box::new(WorkDoneProgressCancelParams {
            token: state.next(),
        }),
        "workspace/didChangeConfiguration" =>
        {
            #[allow(deprecated)]
            Box::new(EditorDidChangeConfigurationParams {
                config: state.next(),
                server_configuration: iter::from_fn(|| state.next())
                    .take_while(|s| s != "map-end")
                    .collect(),
            })
        }
        "workspace/executeCommand" => {
            let is_sync = state.next::<String>() == "is-sync";
            let params = Box::new(EditorExecuteCommand {
                command: state.next(),
                arguments: state.next(),
            });
            sync_trailer(&mut meta, state, is_sync);
            params
        }
        "workspace/symbol" => {
            meta.buffile = state.next();
            meta.language_id = state.next();
            meta.filetype.clear();
            meta.version = state.next();
            let params = Box::new(WorkspaceSymbolParams {
                partial_result_params: PartialResultParams {
                    partial_result_token: None,
                },
                work_done_progress_params: WorkDoneProgressParams {
                    work_done_token: None,
                },
                query: state.next(),
            });
            if params.query.is_empty() {
                return ControlFlow::Continue(());
            }
            params
        }
        method => {
            panic!("unexpected method {}", method);
        }
    });
    assert!(state.offset == state.buf.len());
    let flow = if method == "exit" {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    };
    from_editor
        .send(EditorRequest {
            meta,
            method,
            params,
        })
        .unwrap();
    flow
}

/// Start the main event loop.
///
/// This function starts editor transport.
pub fn start(
    session: SessionId,
    config: Config,
    log_path: &'static Option<PathBuf>,
    fifo: PathBuf,
) -> i32 {
    info!(
        session,
        "kak-lsp server starting. To increase log verbosity, run 'set g lsp_debug true'"
    );

    let editor = editor_transport::start(&session);
    if let Err(code) = editor {
        return code;
    }
    let editor = editor.unwrap();

    let mut ctx = Context::new(session, editor.to_editor.sender().clone(), config);
    let ctx = &mut ctx;

    let timeout = ctx.config.server.timeout;

    let session = ctx.last_session().clone();
    let fifo_worker = {
        let mut state = ParserState::new(session.clone());
        let session = session.clone();
        let to_editor = editor.to_editor.sender().clone();
        let fifo = fifo.clone();
        Worker::spawn(
            ctx.last_session().clone(),
            "Messages from editor",
            1024, // arbitrary
            move |_receiver: Receiver<()>, from_editor: Sender<EditorRequest>| loop {
                state.buf.clear();
                {
                    let mut file = match fs::File::open(fifo.clone()) {
                        Ok(file) => file,
                        Err(err) => {
                            panic!("failed to open fifo '{}', {}", fifo.display(), err);
                        }
                    };
                    file.read_to_end(&mut state.buf).unwrap();
                }
                debug!(
                    session,
                    "From editor: <{}>",
                    String::from_utf8_lossy(&state.buf)
                );
                state.offset = 0;
                state.state = QuoteState::OutsideArg;
                if dispatch_fifo_request(&mut state, &to_editor, &from_editor).is_break() {
                    break;
                }
            },
        )
    };

    'event_loop: loop {
        let server_rxs: Vec<&Receiver<ServerMessage>> = ctx
            .language_servers
            .values()
            .map(|settings| settings.transport.from_lang_server.receiver())
            .collect();
        let from_editor = fifo_worker.receiver();
        let never_rx = never();
        let from_file_watcher = ctx
            .file_watcher
            .as_ref()
            .map(|fw| fw.worker.receiver())
            .unwrap_or(&never_rx);
        let from_pending_file_watcher = &ctx
            .file_watcher
            .as_ref()
            .and_then(
                // If there are enqueued events, let's wait a bit for others to come in, to send
                // them in batch.
                |fw| {
                    if fw.pending_file_events.is_empty() {
                        None
                    } else {
                        Some(tick(Duration::from_secs(1)))
                    }
                },
            )
            .unwrap_or_else(never);

        let mut sel = Select::new();
        // Server receivers are registered first so we can match their order
        // with servers in the context.
        for rx in &server_rxs {
            sel.recv(rx);
        }
        let from_editor_op = sel.recv(from_editor);
        let from_file_watcher_op = sel.recv(from_file_watcher);
        let from_pending_file_watcher_op = sel.recv(from_pending_file_watcher);

        let timeout_channel = if timeout > 0 {
            after(Duration::from_secs(timeout))
        } else {
            never()
        };
        let timeout_op = sel.recv(&timeout_channel);

        let force_exit = |ctx: &mut Context| {
            if let Err(err) = fs::write(fifo.clone(), "'$exit'") {
                error!(ctx.last_session(), "Error writing to fifo: {}", err);
            }
        };

        let op = sel.select();
        match op.index() {
            idx if idx == timeout_op => {
                info!(
                    ctx.last_session(),
                    "Exiting session after {} seconds of inactivity", timeout
                );
                op.recv(&timeout_channel).unwrap();
                force_exit(ctx);
                break 'event_loop;
            }
            idx if idx == from_editor_op => {
                debug!(ctx.last_session(), "Received editor request via fifo");
                let editor_request = op.recv(from_editor).unwrap();
                if process_editor_request(ctx, editor_request).is_break() {
                    break 'event_loop;
                }
            }
            i if i == from_file_watcher_op => {
                let msg = op.recv(from_file_watcher);

                if let Err(err) = msg {
                    debug!(
                        ctx.last_session(),
                        "received error from file watcher: {err}"
                    );
                    force_exit(ctx);
                    break 'event_loop;
                }
                let mut file_events = msg.unwrap();
                debug!(
                    ctx.last_session(),
                    "received {} events from file watcher",
                    file_events.len()
                );
                // Enqueue the events from the file watcher.
                ctx.file_watcher
                    .as_mut()
                    .unwrap()
                    .pending_file_events
                    .extend(file_events.drain(..));
            }
            i if i == from_pending_file_watcher_op => {
                let _msg = op.recv(from_pending_file_watcher).unwrap();

                let fw = ctx.file_watcher.as_mut().unwrap();
                if !fw.pending_file_events.is_empty() {
                    let file_events: Vec<_> = fw.pending_file_events.drain().collect();
                    let servers: Vec<_> = ctx.language_servers.keys().cloned().collect();
                    for server_id in servers {
                        workspace_did_change_watched_files(server_id, file_events.clone(), ctx);
                    }
                    assert!(ctx
                        .file_watcher
                        .as_mut()
                        .unwrap()
                        .pending_file_events
                        .is_empty());
                }
            }
            i => {
                let msg = op.recv(server_rxs[i]);
                let server_id = ctx.language_servers.iter().nth(i).map(|(s, _)| *s).unwrap();

                if let Err(err) = msg {
                    debug!(ctx.last_session(), "received error from server: {err}");
                    force_exit(ctx);
                    break 'event_loop;
                }
                let msg = msg.unwrap();
                match msg {
                    ServerMessage::Request(call) => match call {
                        Call::MethodCall(request) => {
                            dispatch_server_request(
                                server_id,
                                meta_for_session(ctx.last_session().clone(), None),
                                request,
                                ctx,
                            );
                        }
                        Call::Notification(notification) => {
                            dispatch_server_notification(
                                server_id,
                                meta_for_session(ctx.last_session().clone(), None),
                                &notification.method,
                                notification.params,
                                ctx,
                            );
                        }
                        Call::Invalid { id } => {
                            error!(
                                ctx.last_session(),
                                "Invalid call from language server: {:?}", id
                            );
                        }
                    },
                    ServerMessage::Response(output) => {
                        match output {
                            Output::Success(success) => {
                                if let Some((meta, method, batch_id, canceled)) =
                                    ctx.response_waitlist.remove(&success.id)
                                {
                                    if canceled {
                                        continue;
                                    }
                                    remove_outstanding_request(
                                        server_id,
                                        ctx,
                                        method,
                                        &meta.session,
                                        meta.buffile.clone(),
                                        meta.client.clone(),
                                        &success.id,
                                    );
                                    if let Some((mut vals, callback)) =
                                        ctx.batches.remove(&batch_id)
                                    {
                                        if let Some(batch_seq) = ctx.batch_sizes.remove(&batch_id) {
                                            vals.push((server_id, success.result));
                                            let batch_size = batch_seq.values().sum();

                                            if vals.len() >= batch_size {
                                                callback(ctx, meta, vals);
                                                if ctx.is_exiting {
                                                    break 'event_loop;
                                                }
                                            } else {
                                                ctx.batch_sizes.insert(batch_id, batch_seq);
                                                ctx.batches.insert(batch_id, (vals, callback));
                                            }
                                        }
                                    }
                                } else {
                                    error!(
                                        ctx.last_session(),
                                        "Id {:?} is not in waitlist!", success.id
                                    );
                                }
                            }
                            Output::Failure(failure) => {
                                if let Some(request) = ctx.response_waitlist.remove(&failure.id) {
                                    let (meta, method, batch_id, canceled) = request;
                                    if canceled {
                                        continue;
                                    }
                                    remove_outstanding_request(
                                        server_id,
                                        ctx,
                                        method,
                                        &meta.session,
                                        meta.buffile.clone(),
                                        meta.client.clone(),
                                        &failure.id,
                                    );
                                    error!(
                                        meta.session,
                                        "Error response from server {}: {:?}",
                                        &ctx.server(server_id).name,
                                        failure
                                    );
                                    if let Some((vals, callback)) = ctx.batches.remove(&batch_id) {
                                        if let Some(mut batch_seq) =
                                            ctx.batch_sizes.remove(&batch_id)
                                        {
                                            batch_seq.remove(&server_id);

                                            // We con only keep going if there are still other servers to respond.
                                            // Otherwise, skip the following block and handle failure.
                                            if !batch_seq.is_empty() {
                                                // Remove this failing language server from the batch, allowing
                                                // working ones to still be handled.
                                                let vals: Vec<_> = vals
                                                    .into_iter()
                                                    .filter(|(s, _)| *s != server_id)
                                                    .collect();

                                                // Scenario: this failing server is holding back the response handling
                                                // for all other servers, which already responded successfully.
                                                if vals.len() >= batch_seq.values().sum() {
                                                    callback(ctx, meta, vals);
                                                    if ctx.is_exiting {
                                                        break 'event_loop;
                                                    }
                                                } else {
                                                    // Re-insert the batch, as we have no business with it at the moment,
                                                    // since not all servers have completely responded.
                                                    ctx.batch_sizes.insert(batch_id, batch_seq);
                                                    ctx.batches.insert(batch_id, (vals, callback));
                                                }

                                                continue;
                                            }
                                        }
                                    }
                                    match failure.error.code {
                                        code if code
                                            == ErrorCode::ServerError(CONTENT_MODIFIED)
                                            || method == request::CodeActionRequest::METHOD =>
                                        {
                                            // Nothing to do, but sending command back to the editor is required to handle case when
                                            // editor is blocked waiting for response via fifo.
                                            ctx.exec(meta, "nop".to_string());
                                        }
                                        code => {
                                            let msg = match code {
                                                ErrorCode::MethodNotFound => format!(
                                                    "language server {} doesn't support method {}",
                                                    &ctx.server(server_id).name,
                                                    method
                                                ),
                                                _ => format!(
                                                    "language server {} error: {}",
                                                    &ctx.server(server_id).name,
                                                    editor_quote(&failure.error.message)
                                                ),
                                            };
                                            ctx.exec(
                                                meta,
                                                format!("lsp-show-error {}", editor_quote(&msg)),
                                            );
                                        }
                                    }
                                } else {
                                    error!(
                                        ctx.last_session(),
                                        "Error response from server {}: {:?}",
                                        &ctx.server(server_id).name,
                                        failure
                                    );
                                    error!(
                                        ctx.last_session(),
                                        "Id {:?} is not in waitlist!", failure.id
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Did a language server request us to watch for file changes?
        if !ctx.pending_file_watchers.is_empty() {
            let requested_watchers = mem::take(&mut ctx.pending_file_watchers);
            // If there's an existing watcher, ask nicely to terminate.
            let session = ctx.last_session().clone();
            if let Some(ref fw) = ctx.file_watcher.as_mut() {
                info!(session, "stopping stale file watcher");
                if let Err(err) = fw.worker.sender().send(()) {
                    error!(session, "{}", err);
                }
            }
            ctx.file_watcher = Some(FileWatcher {
                pending_file_events: HashSet::new(),
                worker: Box::new(spawn_file_watcher(
                    ctx.last_session().clone(),
                    log_path,
                    requested_watchers,
                )),
            });
        }
    }
    stop_session(ctx);
    0
}

pub fn process_editor_request(ctx: &mut Context, mut request: EditorRequest) -> ControlFlow<()> {
    if let Some(pos) = ctx.sessions.iter().position(|c| c == &request.meta.session) {
        let last_pos = ctx.sessions.len() - 1;
        ctx.sessions.swap(pos, last_pos);
    } else {
        ctx.sessions.push(request.meta.session.clone());
    }
    if !route_request(ctx, &mut request.meta, &request.method) {
        debug!(request.meta.session, "Failed to route {}", &request.method);
        return ControlFlow::Continue(());
    }
    // initialize request must be first request from client to language server
    // initialized response contains capabilities which we save for future use
    // capabilities also serve as a marker of completing initialization
    // we park all requests from editor before initialization is complete
    // and then dispatch them
    let parked: Vec<_> = request
        .meta
        .servers
        .iter()
        .filter(|server_id| ctx.language_servers[server_id].capabilities.is_none())
        .collect();
    if parked.is_empty() {
        dispatch_incoming_editor_request(request, ctx)?;
    } else {
        let servers = parked
            .into_iter()
            .map(|server_id| &ctx.server(*server_id).name)
            .join(", ");
        debug!(
            ctx.last_session(),
            "Language server(s) {} are still not initialized, parking request {:?}",
            servers,
            request
        );
        let err = format!(
            "lsp-show-error 'language servers {} are still not initialized, parking request {}'",
            servers, &request.method
        );
        match &*request.method {
            notification::DidOpenTextDocument::METHOD => (),
            notification::DidChangeTextDocument::METHOD => (),
            notification::DidChangeConfiguration::METHOD => (),
            notification::DidCloseTextDocument::METHOD => (),
            notification::DidSaveTextDocument::METHOD => (),
            request::CodeLensRequest::METHOD => (),
            _ => {
                if !request.meta.hook {
                    ctx.exec(request.meta.clone(), err);
                }
            }
        }
        ctx.pending_requests.push(request);
    }

    ControlFlow::Continue(())
}

/// Tries to send an error to the client about a request that failed to parse.
fn handle_broken_editor_request(
    to_editor: &Sender<EditorResponse>,
    session: &SessionId,
    client: &str,
    hook: bool,
    what: &str,
    err: toml::de::Error,
) {
    let msg = format!("Failed to parse {what}: {err}");
    error!(session, "{msg}");
    // We don't want to spam the user if a hook triggered the error.
    if !hook {
        let meta = meta_for_session(session.clone(), Some(client.to_string()));
        let command = format!("lsp-show-error {}", editor_quote(&msg));
        let response = EditorResponse {
            meta,
            command: command.into(),
        };
        if let Err(err) = to_editor.send(response) {
            error!(session, "Failed to send error message to editor: {err}");
        };
    }
}

/// Shut down all language servers and exit.
fn stop_session(ctx: &mut Context) {
    info!(
        ctx.last_session(),
        "Shutting down language servers and exiting"
    );
    for session in ctx.sessions.clone().into_iter() {
        let request = EditorRequest {
            meta: meta_for_session(session.clone(), None),
            method: notification::Exit::METHOD.to_string(),
            params: EditorParams(Box::new(())),
        };
        if process_editor_request(ctx, request).is_break() {
            break;
        }
    }
    info!(ctx.last_session(), "Exit all servers");
}

pub fn can_serve(
    ctx: &Context,
    candidate_id: ServerId,
    requested_server_name: &ServerName,
    requested_root_path: &RootPath,
) -> bool {
    let candidate = ctx.server(candidate_id);
    let workspace_folder_support = candidate.capabilities.as_ref().is_some_and(|caps| {
        caps.workspace.as_ref().is_some_and(|ws| {
            ws.workspace_folders
                .as_ref()
                .is_some_and(|wsf| wsf.supported == Some(true))
        })
    });
    requested_server_name == &candidate.name
        && candidate.capabilities.is_some()
        && (candidate.roots.contains(requested_root_path) || workspace_folder_support)
}

fn route_request(ctx: &mut Context, meta: &mut EditorMeta, request_method: &str) -> bool {
    if request_method == "exit" {
        info!(
            meta.session,
            "Editor session `{}` closed, shutting down language servers", meta.session
        );
        return true;
    }
    if !meta.buffile.starts_with('/') {
        debug!(
            meta.session,
            "Unsupported scratch buffer, ignoring request from buffer '{}'", meta.buffile
        );
        let command = if meta.hook {
            "nop"
        } else {
            "lsp-show-error 'scratch buffers are not supported, refusing to forward request'"
        };
        ctx.exec(meta.clone(), command);
        return false;
    }

    #[allow(deprecated)]
    if !is_using_legacy_toml(&ctx.config)
        && meta
            .language_server
            .values()
            .any(|server| !server.roots.is_empty())
    {
        let msg = "Error: new server configuration does not support roots parameter";
        debug!(meta.session, "{}", msg);
        report_error(&ctx.editor_tx, meta, msg);
        return false;
    }

    #[allow(deprecated)]
    let legacy_cfg = ctx.legacy_filetypes.get(&meta.filetype);
    let server_addresses: Vec<(ServerName, RootPath)>;
    if is_using_legacy_toml(&ctx.config) {
        #[allow(deprecated)]
        let Some((language_id, servers)) = legacy_cfg
        else {
            let msg = format!(
                "language server is not configured for filetype `{}`",
                &meta.filetype
            );
            debug!(meta.session, "{}", msg);
            report_error_no_server_configured(&ctx.editor_tx, meta, request_method, &msg);

            return false;
        };
        #[allow(deprecated)]
        for server_name in servers {
            let server_config = &mut ctx.config.language_server.get_mut(server_name).unwrap();
            server_config.root = find_project_root(
                &meta.session,
                language_id,
                &server_config.roots,
                &meta.buffile,
            );
        }

        #[allow(deprecated)]
        {
            server_addresses = servers
                .iter()
                .map(|server_name| {
                    (
                        server_name.clone(),
                        ctx.config.language_server[server_name].root.clone(),
                    )
                })
                .collect();
        }
    } else {
        let language_id = &meta.language_id;
        if language_id.is_empty() {
            let msg = "lsp_language_id is empty, did you forget to run lsp-enable?";
            debug!(meta.session, "{}", msg);
            report_error_no_server_configured(&ctx.editor_tx, meta, request_method, msg);
            return false;
        }
        let servers = server_configs(&ctx.config, meta);
        if servers.is_empty() {
            let msg = format!(
                "language server is not configured for filetype '{}'{}, please set the lsp_servers option",
                &meta.filetype,
                if meta.filetype != *language_id {
                    format!(" (language ID '{}')", language_id)
                } else {
                    "".to_string()
                }
            );
            debug!(meta.session, "{}", msg);
            report_error_no_server_configured(&ctx.editor_tx, meta, request_method, &msg);
            return false;
        };
        for (server_name, server) in &mut meta.language_server {
            if !server.root.is_empty() && !server.root_globs.is_empty() {
                let msg = "cannot specify both root and root_globs";
                error!(meta.session, "{}", msg);
                report_error(&ctx.editor_tx, meta, msg);
                return false;
            }
            if server.root.is_empty() {
                if !server.root_globs.is_empty() {
                    server.root = find_project_root(
                        &meta.session,
                        language_id,
                        &server.root_globs,
                        &meta.buffile,
                    );
                } else {
                    let msg = format!(
                        "missing project root path for {server_name}, please set the root option"
                    );
                    error!(meta.session, "{}", msg);
                    report_error(&ctx.editor_tx, meta, &msg);
                    return false;
                }
            }
        }
        server_addresses = meta
            .language_server
            .iter()
            .map(|(server_name, server)| (server_name.clone(), server.root.clone()))
            .collect();
    };

    let mut to_initialize = vec![];
    'server: for (server_name, root) in server_addresses {
        if let Some(&server_id) = ctx.route_cache.get(&(server_name.clone(), root.clone())) {
            meta.servers.push(server_id);
            continue;
        }
        for &server_id in ctx.language_servers.keys() {
            if !can_serve(ctx, server_id, &server_name, &root) {
                continue;
            }
            ctx.language_servers
                .get_mut(&server_id)
                .unwrap()
                .roots
                .push(root.clone());
            ctx.route_cache
                .insert((server_name.clone(), root.clone()), server_id);
            meta.servers.push(server_id);
            let params = DidChangeWorkspaceFoldersParams {
                event: WorkspaceFoldersChangeEvent {
                    added: vec![WorkspaceFolder {
                        uri: Url::from_file_path(&root).unwrap(),
                        name: root,
                    }],
                    removed: vec![],
                },
            };
            ctx.notify::<DidChangeWorkspaceFolders>(server_id, params);
            continue 'server;
        }

        let server_id = ctx.language_servers.len();
        meta.servers.push(server_id);

        // should be fine to unwrap because request was already routed which means language is configured
        let server_config = &server_configs(&ctx.config, meta)[&server_name];
        let server_transport = match language_server_transport::start(
            meta.session.clone(),
            server_name.clone(),
            server_config.command.as_ref().unwrap_or(&server_name),
            &server_config.args,
            &server_config.envs,
        ) {
            Ok(ls) => ls,
            Err(err) => {
                error!(meta.session, "failed to start language server: {}", err);
                if meta.hook && !meta.buffile.is_empty() {
                    let command = format!(
                        "evaluate-commands -buffer {} lsp-block-in-buffer",
                        editor_quote(&meta.buffile),
                    );
                    error!(meta.session, "disabling LSP for buffer: {}", &command);
                    if ctx
                        .editor_tx
                        .send(EditorResponse {
                            meta: meta.clone(),
                            command: Cow::from(command),
                        })
                        .is_err()
                    {
                        error!(meta.session, "Failed to send command to editor");
                    }
                }
                // If the server command isn't from a hook (e.g. auto-hover),
                // then send a prominent error to the editor.
                if !meta.hook {
                    let command = format!(
                        "lsp-show-error {}",
                        editor_quote(&format!("failed to start language server: {}", err)),
                    );
                    if ctx
                        .editor_tx
                        .send(EditorResponse {
                            meta: meta.clone(),
                            command: Cow::from(command),
                        })
                        .is_err()
                    {
                        error!(meta.session, "Failed to send command to editor");
                    }
                }
                return false;
            }
        };

        let offset_encoding = server_config.offset_encoding;
        let server_settings = ServerSettings {
            name: server_name.clone(),
            roots: vec![root.clone()],
            offset_encoding: offset_encoding.unwrap_or_default(),
            transport: server_transport,
            preferred_offset_encoding: offset_encoding,
            capabilities: None,
            settings: None,
            users: vec![meta.session.clone()],
        };
        ctx.language_servers.insert(server_id, server_settings);
        ctx.route_cache.insert((server_name, root), server_id);
        to_initialize.push(server_id);
    }
    if !to_initialize.is_empty() {
        initialize(meta.clone(), ctx, to_initialize);
    }
    true
}

fn report_error_no_server_configured(
    to_editor: &Sender<EditorResponse>,
    meta: &EditorMeta,
    request_method: &str,
    msg: &str,
) {
    let word_regex = meta.word_regex.as_ref();
    let command = if let Some(multi_cmds) = match request_method {
        _ if meta.hook => None,
        request::GotoDefinition::METHOD | request::References::METHOD => Some(formatdoc!(
            "grep {}
             lsp-show-error {}",
            editor_quote(word_regex.unwrap()),
            editor_quote(msg),
        )),
        request::DocumentHighlightRequest::METHOD => Some(formatdoc!(
            "evaluate-commands -save-regs a/^ %|
                 execute-keys -save-regs '' %[\"aZ]
                 set-register / {}
                 execute-keys -save-regs '' <percent>s<ret>Z
                 execute-keys %[\"az<a-z>a]
             |
             lsp-show-error {}",
            editor_quote(word_regex.unwrap()).replace('|', "||"),
            editor_quote(&format!(
                "{msg}, falling_back to %s{}<ret>",
                word_regex.unwrap()
            ))
        )),
        _ => None,
    } {
        format!("evaluate-commands {}", &editor_quote(&multi_cmds))
    } else {
        format!("lsp-show-error {}", editor_quote(msg))
    };
    report_error_impl(to_editor, meta, command)
}

/// Sends an error back to the editor.
///
/// This will cancel any blocking requests and also print an error if the
/// request was not triggered by an editor hook.
pub fn report_error(to_editor: &Sender<EditorResponse>, meta: &EditorMeta, msg: &str) {
    report_error_impl(
        to_editor,
        meta,
        format!("lsp-show-error {}", editor_quote(msg)),
    )
}

fn report_error_impl(to_editor: &Sender<EditorResponse>, meta: &EditorMeta, command: String) {
    // If editor is expecting a fifo response, give it one, so it won't hang.
    if let Some(ref fifo) = meta.fifo {
        std::fs::write(fifo, &command).expect("Failed to write command to fifo");
    }

    if !meta.hook {
        let response = EditorResponse {
            meta: meta.clone(),
            command: command.into(),
        };
        if let Err(err) = to_editor.send(response) {
            error!(
                meta.session,
                "Failed to send error message to editor: {err}"
            );
        };
    }
}

pub fn dispatch_pending_editor_requests(ctx: &mut Context) {
    let mut requests = mem::take(&mut ctx.pending_requests);

    for msg in requests.drain(..) {
        dispatch_editor_request(msg, ctx);
    }
}

fn dispatch_incoming_editor_request(request: EditorRequest, ctx: &mut Context) -> ControlFlow<()> {
    let method: &str = &request.method;
    let document_version = {
        let buffile = &request.meta.buffile;
        ctx.documents
            .get(buffile)
            .map(|doc| doc.version)
            .unwrap_or(0)
    };
    if document_version > request.meta.version {
        debug!(
            request.meta.session,
            "incoming request {} is stale, version {} but I already have {}",
            request.method,
            request.meta.version,
            document_version
        );
        // Keep it nevertheless because at least "completionItem/resolve" is useful.
    }
    if request.meta.fifo.is_none() {
        let notifications = &[
            notification::DidOpenTextDocument::METHOD,
            notification::DidChangeTextDocument::METHOD,
            notification::DidCloseTextDocument::METHOD,
            notification::DidSaveTextDocument::METHOD,
            notification::DidChangeConfiguration::METHOD,
            notification::Exit::METHOD,
            notification::WorkDoneProgressCancel::METHOD,
        ];

        if !request.meta.buffile.is_empty()
            && document_version < request.meta.version
            && !notifications.contains(&method)
            // InsertIdle is not triggered while the completion pager is active, so let's
            // smuggle completion-related requests through.
            && method != request::ResolveCompletionItem::METHOD
        {
            // Wait for buffer update.
            ctx.pending_requests.push(request);
            return ControlFlow::Continue(());
        }
    };
    let version_bump = [
        notification::DidOpenTextDocument::METHOD,
        notification::DidChangeTextDocument::METHOD,
    ]
    .contains(&method);

    dispatch_editor_request(request, ctx)?;

    if !version_bump {
        return ControlFlow::Continue(());
    }
    let mut requests = mem::take(&mut ctx.pending_requests);
    let mut all_exited = false;
    requests.retain_mut(|request| {
        if all_exited {
            return false;
        }
        let buffile = &request.meta.buffile;
        let document = match ctx.documents.get(buffile) {
            Some(document) => document,
            None => return true,
        };
        if document.version < request.meta.version {
            return true;
        }
        debug!(
            request.meta.session,
            "dispatching pending request {} because we have received matching version in didChange",
            request.method
        );
        if document.version > request.meta.version {
            debug!(
                request.meta.session,
                "pending request {} is stale, version {} but I already have {}",
                request.method,
                request.meta.version,
                document.version
            );
            // Keep it nevertheless because at least "completionItem/resolve" is useful.
        }
        if dispatch_editor_request(mem::take(request), ctx).is_break() {
            all_exited = true;
        }
        false
    });
    assert!(ctx.pending_requests.is_empty());
    ctx.pending_requests = mem::take(&mut requests);
    ControlFlow::Continue(())
}

fn dispatch_editor_request(request: EditorRequest, ctx: &mut Context) -> ControlFlow<()> {
    ensure_did_open(&request, ctx);
    let method: &str = &request.method;
    let meta = request.meta;
    let params = request.params;
    match method {
        notification::DidOpenTextDocument::METHOD => {
            text_document_did_open(meta, params.unbox(), ctx);
        }
        notification::DidChangeTextDocument::METHOD => {
            text_document_did_change(meta, params.unbox(), ctx);
        }
        notification::DidCloseTextDocument::METHOD => {
            text_document_did_close(meta, ctx);
        }
        notification::DidSaveTextDocument::METHOD => {
            text_document_did_save(meta, ctx);
        }
        notification::DidChangeConfiguration::METHOD => {
            workspace::did_change_configuration(meta, params.unbox(), ctx);
        }
        request::CallHierarchyPrepare::METHOD => {
            call_hierarchy::call_hierarchy_prepare(meta, params.unbox(), ctx);
        }
        request::CodeLensRequest::METHOD => {
            text_document_code_lens(meta, ctx);
        }
        request::Completion::METHOD => {
            completion::text_document_completion(meta, params.unbox(), ctx);
        }
        request::ResolveCompletionItem::METHOD => {
            completion::completion_item_resolve(meta, params.unbox(), ctx);
        }
        request::CodeActionRequest::METHOD => {
            code_action::text_document_code_action(meta, params.unbox(), ctx);
        }
        request::CodeActionResolveRequest::METHOD => {
            code_action::text_document_code_action_resolve(meta, params.unbox(), ctx);
        }
        request::ExecuteCommand::METHOD => {
            workspace::execute_command(meta, params.unbox(), ctx);
        }
        request::HoverRequest::METHOD => {
            hover::text_document_hover(meta, params.unbox(), ctx);
        }
        request::GotoDefinition::METHOD => {
            goto::text_document_definition(false, meta, params.unbox(), ctx);
        }
        request::GotoDeclaration::METHOD => {
            goto::text_document_definition(true, meta, params.unbox(), ctx);
        }
        request::GotoImplementation::METHOD => {
            goto::text_document_implementation(meta, params.unbox(), ctx);
        }
        request::GotoTypeDefinition::METHOD => {
            goto::text_document_type_definition(meta, params.unbox(), ctx);
        }
        request::References::METHOD => {
            goto::text_document_references(meta, params.unbox(), ctx);
        }
        notification::Exit::METHOD => {
            let mut redundant_servers = vec![];
            for (server_id, server) in ctx.language_servers.iter_mut() {
                if let Some(pos) = server.users.iter().position(|s| s == &meta.session) {
                    debug!(
                        meta.session,
                        "Sending exit notification to server {} with users: {}",
                        &server.name,
                        server.users.iter().join(", ")
                    );
                    server.users.swap_remove(pos);
                    if server.users.is_empty() {
                        redundant_servers.push(*server_id);
                    }
                }
            }
            for server_id in redundant_servers {
                ctx.notify::<notification::Exit>(server_id, ());
            }
            if ctx.language_servers.values().all(|v| v.users.is_empty()) {
                return ControlFlow::Break(());
            }
        }

        notification::WorkDoneProgressCancel::METHOD => {
            progress::work_done_progress_cancel(meta, params.unbox(), ctx);
        }
        request::SelectionRangeRequest::METHOD => {
            selection_range::text_document_selection_range(meta, params.unbox(), ctx);
        }
        request::SignatureHelpRequest::METHOD => {
            signature_help::text_document_signature_help(meta, params.unbox(), ctx);
        }
        request::DocumentHighlightRequest::METHOD => {
            highlight::text_document_highlight(meta, params.unbox(), ctx);
        }
        request::DocumentSymbolRequest::METHOD => {
            document_symbol::text_document_document_symbol(meta, ctx);
        }
        "kakoune/breadcrumbs" => {
            document_symbol::breadcrumbs(meta, params.unbox(), ctx);
        }
        "kakoune/next-or-previous-symbol" => {
            document_symbol::next_or_prev_symbol(meta, params.unbox(), ctx);
        }
        "kakoune/object" => {
            document_symbol::object(meta, params.unbox(), ctx);
        }
        "kakoune/goto-document-symbol" => {
            document_symbol::document_symbol_menu(meta, params.unbox(), ctx);
        }
        "kakoune/textDocument/codeLens" => {
            code_lens::resolve_and_perform_code_lens(meta, params.unbox(), ctx);
        }
        request::Formatting::METHOD => {
            formatting::text_document_formatting(meta, params.unbox(), ctx);
        }
        request::RangeFormatting::METHOD => {
            range_formatting::text_document_range_formatting(meta, params.unbox(), ctx);
        }
        request::WorkspaceSymbolRequest::METHOD => {
            workspace::workspace_symbol(meta, params.unbox(), ctx);
        }
        request::Rename::METHOD => {
            rename::text_document_rename(meta, params.unbox(), ctx);
        }
        "textDocument/diagnostics" => {
            diagnostics::editor_diagnostics(meta, ctx);
        }
        "capabilities" => {
            capabilities::capabilities(meta, ctx);
        }
        "apply-workspace-edit" => {
            if let Some(&server_id) = meta.servers.first() {
                workspace::apply_edit_from_editor(server_id, &meta, params.unbox(), ctx);
            }
        }
        request::SemanticTokensFullRequest::METHOD => {
            semantic_tokens::tokens_request(meta, ctx);
        }

        request::InlayHintRequest::METHOD => {
            inlay_hints::inlay_hints(meta, params.unbox(), ctx);
        }

        show_message::SHOW_MESSAGE_REQUEST_NEXT => {
            show_message::show_message_request_next(meta, ctx);
        }
        show_message::SHOW_MESSAGE_REQUEST_RESPOND => {
            show_message::show_message_request_respond(meta, params.unbox(), ctx);
        }

        // CCLS
        ccls::NavigateRequest::METHOD => {
            ccls::navigate(meta, params.unbox(), ctx);
        }
        ccls::VarsRequest::METHOD => {
            ccls::vars(meta, params.unbox(), ctx);
        }
        ccls::InheritanceRequest::METHOD => {
            ccls::inheritance(meta, params.unbox(), ctx);
        }
        ccls::CallRequest::METHOD => {
            ccls::call(meta, params.unbox(), ctx);
        }
        ccls::MemberRequest::METHOD => {
            ccls::member(meta, params.unbox(), ctx);
        }

        // clangd
        clangd::SwitchSourceHeaderRequest::METHOD => {
            clangd::switch_source_header(meta, ctx);
        }

        // eclipse.jdt.ls
        "eclipse.jdt.ls/organizeImports" => {
            eclipse_jdt_ls::organize_imports(meta, ctx);
        }

        // rust-analyzer
        rust_analyzer::ExpandMacroRequest::METHOD => {
            rust_analyzer::expand_macro(meta, params.unbox(), ctx);
        }

        // texlab
        texlab::Build::METHOD => {
            texlab::build(meta, ctx);
        }
        texlab::ForwardSearch::METHOD => {
            texlab::forward_search(meta, params.unbox(), ctx);
        }

        _ => {
            warn!(meta.session, "Unsupported method: {}", method);
        }
    }
    ControlFlow::Continue(())
}

fn dispatch_server_request(
    server_id: ServerId,
    meta: EditorMeta,
    request: MethodCall,
    ctx: &mut Context,
) {
    let method: &str = &request.method;
    let result = match method {
        request::ApplyWorkspaceEdit::METHOD => {
            workspace::apply_edit_from_server(meta, server_id, request.params, ctx)
        }
        request::RegisterCapability::METHOD => {
            let params: RegistrationParams = request
                .params
                .parse()
                .expect("Failed to parse RegistrationParams params");
            for registration in params.registrations {
                match registration.method.as_str() {
                    notification::DidChangeWatchedFiles::METHOD => {
                        register_workspace_did_change_watched_files(
                            server_id,
                            registration.register_options,
                            ctx,
                        )
                    }
                    notification::DidChangeWorkspaceFolders::METHOD => {
                        // Since we only support one root path, we are never going to send
                        // "workspace/didChangeWorkspaceFolders" anyway, so let's not issue a warning.
                        continue;
                    }
                    "textDocument/semanticTokens" => {
                        let Some(options) = registration.register_options else {
                            warn!(meta.session, "semantic tokens registration without options");
                            continue;
                        };
                        let semantic_tokens_options: SemanticTokensOptions =
                            serde_json::from_value(options).unwrap();
                        let semantic_tokens_server_capabilities =
                            SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(
                                SemanticTokensRegistrationOptions {
                                    text_document_registration_options:
                                        TextDocumentRegistrationOptions {
                                            document_selector: None,
                                        },
                                    semantic_tokens_options,
                                    static_registration_options: StaticRegistrationOptions {
                                        id: Some(registration.id),
                                    },
                                },
                            );
                        ctx.language_servers
                            .get_mut(&server_id)
                            .unwrap()
                            .capabilities
                            .as_mut()
                            .unwrap()
                            .semantic_tokens_provider = Some(semantic_tokens_server_capabilities);
                    }
                    _ => warn!(
                        meta.session,
                        "Unsupported registration: {}", registration.method
                    ),
                }
            }
            Ok(serde_json::Value::Null)
        }
        request::WorkspaceFoldersRequest::METHOD => Ok(serde_json::to_value(
            ctx.server(server_id)
                .roots
                .iter()
                .map(|root| WorkspaceFolder {
                    uri: Url::from_file_path(root).unwrap(),
                    name: root.clone(),
                })
                .collect::<Vec<_>>(),
        )
        .ok()
        .unwrap()),
        request::WorkDoneProgressCreate::METHOD => {
            progress::work_done_progress_create(meta, request.params, ctx)
        }
        request::WorkspaceConfiguration::METHOD => {
            workspace::configuration(meta, request.params, server_id, ctx)
        }
        request::ShowMessageRequest::METHOD => {
            return show_message::show_message_request(meta, server_id, request, ctx);
        }
        request::CodeLensRefresh::METHOD => {
            ctx.exec(
                meta,
                "evaluate-commands -buffer * unset-option buffer lsp_code_lens_timestamp",
            );
            Ok(serde_json::Value::Null)
        }
        request::InlayHintRefreshRequest::METHOD => {
            ctx.exec(
                meta,
                "evaluate-commands -buffer * unset-option buffer lsp_inlay_hints_timestamp",
            );
            Ok(serde_json::Value::Null)
        }
        request::SemanticTokensRefresh::METHOD => {
            ctx.exec(
                meta,
                "evaluate-commands -buffer * unset-option buffer lsp_semantic_tokens_timestamp",
            );
            Ok(serde_json::Value::Null)
        }
        _ => {
            warn!(meta.session, "Unsupported method: {}", method);
            Err(jsonrpc_core::Error::new(
                jsonrpc_core::ErrorCode::MethodNotFound,
            ))
        }
    };

    ctx.reply(server_id, request.id, result);
}

fn dispatch_server_notification(
    server_id: ServerId,
    meta: EditorMeta,
    method: &str,
    params: Params,
    ctx: &mut Context,
) {
    match method {
        notification::Progress::METHOD => {
            progress::dollar_progress(meta, params, ctx);
        }
        notification::PublishDiagnostics::METHOD => {
            diagnostics::publish_diagnostics(server_id, params, ctx);
        }
        "$cquery/publishSemanticHighlighting" => {
            cquery::publish_semantic_highlighting(server_id, params, ctx);
        }
        "$ccls/publishSemanticHighlight" => {
            ccls::publish_semantic_highlighting(server_id, params, ctx);
        }
        notification::Exit::METHOD => {
            debug!(
                meta.session,
                "language server {} exited",
                &ctx.server(server_id).name
            );
        }
        notification::ShowMessage::METHOD => {
            let params: ShowMessageParams = params
                .parse()
                .expect("Failed to parse ShowMessageParams params");
            show_message::show_message(meta, server_id, params.typ, &params.message, ctx);
        }
        "window/logMessage" => {
            let params: LogMessageParams = params
                .parse()
                .expect("Failed to parse LogMessageParams params");
            ctx.exec(
                meta,
                format!(
                    "lsp-show-message-log {} {}",
                    editor_quote(&ctx.server(server_id).name),
                    editor_quote(&params.message)
                ),
            );
        }
        "telemetry/event" => {
            debug!(meta.session, "{:?}", params);
        }
        _ => {
            warn!(meta.session, "Unsupported method: {}", method);
        }
    }
}

/// Ensure that textDocument/didOpen is sent for the given buffer before any other request, if possible.
///
/// kakoune-lsp tries to not bother Kakoune side of the plugin with bookkeeping status of
/// kakoune-lsp server itself and lsp servers run by it. It is possible that kakoune-lsp server
/// or lsp server dies at some point while Kakoune session is still running. That session can
/// send a request for some already open (opened before kakoune-lsp/lsp exit) buffer. In this
/// case, kakoune-lsp/lsp server will be restarted by the incoming request. `ensure_did_open`
/// tries to sneak in `textDocument/didOpen` request for this buffer then as the specification
/// requires to send such request before other requests for the file.
///
/// In a normal situation, such extra request is not required, and `ensure_did_open` short-circuits
/// most of the time in `if buffile.is_empty() || ctx.documents.contains_key(buffile)` condition.
fn ensure_did_open(request: &EditorRequest, ctx: &mut Context) {
    let buffile = &request.meta.buffile;
    if buffile.is_empty() || ctx.documents.contains_key(buffile) {
        return;
    };
    if request.method == notification::DidChangeTextDocument::METHOD {
        let params: &TextDocumentDidChangeParams = request.params.downcast_ref();
        text_document_did_open(
            request.meta.clone(),
            TextDocumentDidOpenParams {
                draft: params.draft.clone(),
            },
            ctx,
        );
        return;
    }
    match read_document(buffile) {
        Ok(draft) => {
            text_document_did_open(
                request.meta.clone(),
                TextDocumentDidOpenParams { draft },
                ctx,
            );
        }
        Err(err) => error!(
            request.meta.session,
            "Failed to read file {} to simulate textDocument/didOpen: {}", buffile, err
        ),
    };
}
