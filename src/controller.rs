use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::{self};
use std::io::{self, Read};
use std::ops::ControlFlow;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use std::{iter, mem};

use crate::capabilities::{self, initialize};
use crate::context::Context;
use crate::editor_transport::{self, ToEditorSender};
use crate::language_features::{selection_range, *};
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
use crate::{diagnostics, do_cleanup};
use crate::{language_server_transport, LAST_CLIENT};
use ccls::{EditorCallParams, EditorInheritanceParams, EditorMemberParams, EditorNavigateParams};
use code_lens::{text_document_code_lens, CodeLensOptions};
use crossbeam_channel::{after, never, tick, Receiver, Select, Sender};
use indoc::formatdoc;
use inlay_hints::InlayHintsOptions;
use itertools::Itertools;
use jsonrpc_core::{Call, ErrorCode, MethodCall, Output, Params};
use libc::O_NONBLOCK;
use lsp_types::error_codes::CONTENT_MODIFIED;
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use lsp_types::*;
use serde::Deserialize;
use sloggers::types::Severity;

struct Fifo {
    file: fs::File,
    poll: mio::Poll,
    events: mio::Events,
}

impl Fifo {
    fn new(file: fs::File) -> Self {
        let source = file.as_raw_fd();
        let mut source = mio::unix::SourceFd(&source);
        let poll = mio::Poll::new().expect("failed to create poll");
        poll.registry()
            .register(&mut source, mio::Token(0), mio::Interest::READABLE)
            .unwrap();
        let events = mio::Events::with_capacity(1024);
        Self { file, poll, events }
    }
    fn wait_until_readable(&mut self) {
        loop {
            if let Err(err) = self.poll.poll(&mut self.events, None) {
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                panic!("poll error: {}", err);
            }
            let readable = self.events.iter().any(|evt| evt.is_readable());
            self.events.clear();
            if readable {
                break;
            }
        }
    }
}

struct ParserState {
    to_editor: ToEditorSender,
    fifo: Fifo,
    alt_fifo: Fifo,
    input: Vec<u8>,
    input_offset: usize,
    buffer_input: Vec<u8>,
    buffer_input_lines: usize,
    output: Vec<u8>,
    debug: bool,
    debug_output: String,
}

impl ParserState {
    fn new(to_editor: ToEditorSender, fifo: fs::File, alt_fifo: fs::File) -> Self {
        ParserState {
            to_editor,
            fifo: Fifo::new(fifo),
            alt_fifo: Fifo::new(alt_fifo),
            input: vec![],
            input_offset: 0,
            buffer_input: vec![],
            buffer_input_lines: 0,
            output: vec![],
            debug: false,
            debug_output: String::new(),
        }
    }
}

fn next_string(state: &mut ParserState) -> String {
    read_token(state);
    let token = String::from_utf8_lossy(&state.output).to_string();
    state.output.clear();
    if state.debug {
        state.debug_output.push_str(" {");
        state.debug_output.push_str(&token);
        state.debug_output.push('}');
    }
    token
}

fn read_token(state: &mut ParserState) {
    let mut escaped = false;
    let mut quoted = false;
    let mut offset = state.input_offset;
    loop {
        if offset == state.input.len() {
            let n = blocking_read(state);
            state.input.truncate(n);
            debug!(
                &state.to_editor,
                "From editor (raw): {{{}}}",
                &String::from_utf8_lossy(&state.input)
            );
            offset = 0;
        }
        let c = state.input[offset];
        offset += 1;
        if escaped {
            state.output.push(c);
            escaped = false;
        } else if quoted {
            if c == b'\'' {
                quoted = false;
            } else {
                state.output.push(c);
            }
        } else {
            match c {
                b' ' => break,
                b'\'' => quoted = true,
                b'\\' => escaped = true,
                _ => {
                    panic!(
                        "expected quote, backslash or space at offset {offset}, saw '{}'",
                        char::from(c)
                    )
                }
            }
        }
    }
    state.input_offset = offset;
}

fn blocking_read(state: &mut ParserState) -> usize {
    state.input.clear();
    state.input.resize(4096, 0_u8);
    let n = match state.fifo.file.read(&mut state.input) {
        Ok(n) => n,
        Err(err) => {
            if err.kind() == io::ErrorKind::WouldBlock {
                0
            } else {
                panic!("read error: {}", err);
            }
        }
    };

    if n != 0 {
        return n;
    }
    state.fifo.wait_until_readable();
    match state.fifo.file.read(&mut state.input) {
        Ok(n) => n,
        Err(err) => {
            panic!("read error: {}", err);
        }
    }
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

trait Deserializable {
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
        let buf_line_count: usize = self.next();
        let mut offset = self.buffer_input.len();
        let count_lines = |s: &[u8]| s.iter().filter(|&&c| c == b'\n').count();
        while self.buffer_input_lines < buf_line_count {
            self.alt_fifo.wait_until_readable();
            if let Err(err) = self.alt_fifo.file.read_to_end(&mut self.buffer_input) {
                if err.kind() != io::ErrorKind::WouldBlock {
                    panic!("error reading buffer contents: {}", err);
                }
                continue;
            }
            self.buffer_input_lines += count_lines(&self.buffer_input[offset..]);
            offset = self.buffer_input.len();
        }
        let excess_newlines = self.buffer_input_lines - buf_line_count;
        let last_newline_offset = self
            .buffer_input
            .iter()
            .enumerate()
            .rev()
            .filter(|(_i, &c)| c == b'\n')
            .skip(excess_newlines)
            .map(|(i, _c)| i)
            .next()
            .unwrap();
        let mut tmp = self.buffer_input.split_off(last_newline_offset + 1);
        mem::swap(&mut tmp, &mut self.buffer_input);
        self.buffer_input_lines -= buf_line_count;
        assert!(self.buffer_input_lines == count_lines(&self.buffer_input));
        let result = String::from_utf8_lossy(&tmp).to_string();
        debug!(
            &self.to_editor,
            "Buffer contents from editor: {{{}}}", &result
        );
        result
    }
}

fn dispatch_fifo_request(
    state: &mut ParserState,
    from_editor: &Sender<EditorRequest>,
) -> ControlFlow<()> {
    let session = SessionId(state.next());
    if session.as_str() == "$exit" {
        return ControlFlow::Break(());
    }
    let client = ClientId(state.next());
    let hook = state.next();
    let sourcing = state.next();
    let buffile = state.next();
    let version = state.next();
    let filetype = state.next();
    let language_id = state.next();
    let lsp_servers: String = state.next();
    let lsp_semantic_tokens: String = state.next();
    let lsp_config: String = state.next();
    let lsp_server_initialization_options: Vec<String> = iter::from_fn(|| state.next())
        .take_while(|s| s != "map-end")
        .collect();

    let parse_error = |what, err| {
        handle_broken_editor_request(&state.to_editor, &client, hook, what, err);
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
    #[allow(deprecated)]
    let mut meta = EditorMeta {
        session,
        client: (!client.is_empty()).then_some(client),
        buffile,
        language_id,
        filetype,
        version,
        hook,
        sourcing,
        language_server,
        semantic_tokens,
        server: None,
        word_regex: None,
        servers: Default::default(),
        legacy_dynamic_config: lsp_config,
        legacy_server_initialization_options: lsp_server_initialization_options,
    };

    let mut response_fifo = None;

    let mut sync_trailer = |state: &mut ParserState, is_sync: bool| {
        if is_sync {
            response_fifo = Some(ResponseFifo::new(state.next()));
        }
    };

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
            sync_trailer(state, is_sync);
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
        "kakoune/breadcrumbs" => Box::new(BreadcrumbsParams {
            position_line: state.next(),
        }),
        "kakoune/exit" => Box::new(()),
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
            debug!(&state.to_editor, "Applied option change {}", hook_param);
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
            sync_trailer(state, is_sync);
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
        "textDocument/documentHighlight" => {
            meta.word_regex = Some(state.next());
            Box::new(PositionParams {
                position: state.next(),
            })
        }
        "textDocument/formatting" => {
            let params = Box::new(<FormattingOptions as Deserializable>::deserialize(state));
            let is_sync = state.next::<String>() == "is-sync";
            if let Some(server_override) = state.next() {
                meta.server = Some(server_override);
            }
            sync_trailer(state, is_sync);
            params
        }
        "textDocument/forwardSearch" => Box::new(PositionParams {
            position: state.next(),
        }),
        "textDocument/hover" => Box::new(EditorHoverParams {
            selection_desc: state.next(),
            tabstop: state.next(),
            hover_client: state.next::<Option<String>>().map(ClientId),
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
            sync_trailer(state, is_sync);
            params
        }
        "textDocument/references" => {
            meta.word_regex = Some(state.next());
            Box::new(PositionParams {
                position: state.next(),
            })
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
        "window/showMessageRequest/respond" => Box::new(MessageRequestResponse {
            message_request_id: serde_json::from_str(&state.next::<String>()).unwrap(),
            item: state
                .next::<Option<String>>()
                .map(|s| toml::from_str(&s).unwrap()),
        }),
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
            sync_trailer(state, is_sync);
            params
        }
        "workspace/symbol" => {
            meta.buffile = state.next();
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
    let flow = if method == "kakoune/exit" {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    };
    from_editor
        .send(EditorRequest {
            meta,
            response_fifo,
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
    to_editor: &ToEditorSender,
    log_path: &'static Option<PathBuf>,
    fifo: PathBuf,
    alt_fifo: PathBuf,
) {
    info!(
        to_editor,
        "kak-lsp server starting (PID={}). To control log verbosity, set the 'lsp_debug' option",
        unsafe { libc::getpid() }
    );

    let mut ctx = Context::new(session, to_editor.clone(), config);
    let ctx = &mut ctx;

    let timeout = ctx.config.server.timeout;

    let fifo_worker = {
        let mut opts = fs::OpenOptions::new();
        opts.read(true).custom_flags(O_NONBLOCK);
        let fifo = opts.open(fifo.clone()).unwrap();
        let alt_fifo = opts.open(alt_fifo.clone()).unwrap();
        Worker::spawn(
            to_editor.clone(),
            "Messages from editor",
            1024, // arbitrary
            move |to_editor, _receiver: Receiver<()>, from_editor: Sender<EditorRequest>| {
                let mut state = ParserState::new(to_editor, fifo, alt_fifo);
                loop {
                    state.debug = DEBUG.load(Relaxed);
                    let done = dispatch_fifo_request(&mut state, &from_editor).is_break();
                    if state.debug {
                        debug!(
                            &state.to_editor,
                            "From editor: {{{}}}",
                            &state.debug_output[1..]
                        );
                        state.debug_output.clear();
                    }
                    if done {
                        break;
                    }
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
                error!(ctx.to_editor(), "Error writing to fifo: {}", err);
            }
        };

        let op = sel.select();
        match op.index() {
            idx if idx == timeout_op => {
                info!(
                    ctx.to_editor(),
                    "Exiting session after {} seconds of inactivity", timeout
                );
                op.recv(&timeout_channel).unwrap();
                force_exit(ctx);
                break 'event_loop;
            }
            idx if idx == from_editor_op => {
                debug!(ctx.to_editor(), "Received editor request via fifo");
                let editor_request = match op.recv(from_editor) {
                    Ok(r) => r,
                    Err(err) => {
                        warn!(ctx.to_editor(), "Error receiving editor request: {err}");
                        break 'event_loop;
                    }
                };
                if process_editor_request(ctx, editor_request).is_break() {
                    debug!(ctx.to_editor(), "Processed exit request");
                    break 'event_loop;
                }
            }
            i if i == from_file_watcher_op => {
                let msg = op.recv(from_file_watcher);

                if let Err(err) = msg {
                    warn!(ctx.to_editor(), "received error from file watcher: {err}");
                    force_exit(ctx);
                    break 'event_loop;
                }
                let mut file_events = msg.unwrap();
                debug!(
                    ctx.to_editor(),
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
                    warn!(ctx.to_editor(), "received error from server: {err}");
                    force_exit(ctx);
                    break 'event_loop;
                }
                let msg = msg.unwrap();
                match msg {
                    ServerMessage::Request(call) => match call {
                        Call::MethodCall(request) => {
                            dispatch_server_request(server_id, EditorMeta::default(), request, ctx);
                        }
                        Call::Notification(notification) => {
                            dispatch_server_notification(
                                server_id,
                                EditorMeta::default(),
                                &notification.method,
                                notification.params,
                                ctx,
                            );
                        }
                        Call::Invalid { id } => {
                            error!(
                                ctx.to_editor(),
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
                                        ctx.to_editor(),
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
                                        meta.buffile.clone(),
                                        meta.client.clone(),
                                        &failure.id,
                                    );
                                    if failure.error.code
                                        == ErrorCode::ServerError(CONTENT_MODIFIED)
                                    {
                                        debug!(
                                            ctx.to_editor(),
                                            "Error response from server {}: {:?}",
                                            &ctx.server(server_id).name,
                                            failure
                                        );
                                    } else {
                                        error!(
                                            ctx.to_editor(),
                                            "Error response from server {}: {:?}",
                                            &ctx.server(server_id).name,
                                            failure
                                        );
                                    }
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
                                            || method == request::CodeActionRequest::METHOD => {}
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
                                            ctx.show_error(meta, msg);
                                        }
                                    }
                                } else {
                                    error!(
                                        ctx.to_editor(),
                                        "Error response from server {}: {:?}",
                                        &ctx.server(server_id).name,
                                        failure
                                    );
                                    error!(
                                        ctx.to_editor(),
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
            let to_editor = ctx.to_editor().clone();
            if let Some(ref fw) = ctx.file_watcher.as_mut() {
                debug!(&to_editor, "stopping stale file watcher");
                if let Err(err) = fw.worker.sender().send(()) {
                    error!(&to_editor, "{}", err);
                }
            }
            ctx.file_watcher = Some(FileWatcher {
                pending_file_events: HashSet::new(),
                worker: Box::new(spawn_file_watcher(to_editor, log_path, requested_watchers)),
            });
        }
    }
    stop_session(ctx);
}

pub fn process_editor_request(ctx: &mut Context, mut request: EditorRequest) -> ControlFlow<()> {
    if let Some(flow) = route_request(ctx, &mut request.meta, &request.method) {
        return flow;
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
            ctx.to_editor(),
            "Language server(s) {} are still not initialized, parking request {:?}",
            servers,
            request
        );
        if request.response_fifo.is_none()
            && !matches!(
                &*request.method,
                notification::DidOpenTextDocument::METHOD
                    | notification::DidChangeTextDocument::METHOD
                    | notification::DidChangeConfiguration::METHOD
                    | notification::DidCloseTextDocument::METHOD
                    | notification::DidSaveTextDocument::METHOD
                    | request::CodeLensRequest::METHOD
            )
        {
            ctx.show_error(
                request.meta.clone(),
                format!(
                    "language servers {} are still not initialized, parking request {}",
                    servers, &request.method
                ),
            );
        }
        ctx.pending_requests.push(request);
    }

    ControlFlow::Continue(())
}

/// Tries to send an error to the client about a request that failed to parse.
fn handle_broken_editor_request(
    to_editor: &ToEditorSender,
    client: &ClientId,
    hook: bool,
    what: &str,
    err: toml::de::Error,
) {
    let msg = format!("Failed to parse {what}: {err}");
    let mut meta = EditorMeta::for_client(client.clone());
    meta.hook = hook;
    editor_transport::show_error(to_editor, meta, None, msg);
}

/// Shut down all language servers and exit.
fn stop_session(ctx: &mut Context) {
    debug!(
        ctx.to_editor(),
        "Shutting down language servers and exiting"
    );
    do_cleanup();
    let request = EditorRequest {
        method: notification::Exit::METHOD.to_string(),
        ..Default::default()
    };
    let flow = process_editor_request(ctx, request);
    assert!(flow.is_break());
    debug!(ctx.to_editor(), "Exit all servers");
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

fn route_request(
    ctx: &mut Context,
    meta: &mut EditorMeta,
    request_method: &str,
) -> Option<ControlFlow<()>> {
    if request_method == "kakoune/exit" {
        debug!(
            ctx.to_editor(),
            "Editor session `{}` closed, shutting down language servers",
            ctx.session()
        );
        return Some(ControlFlow::Break(()));
    }
    if request_method == notification::Exit::METHOD {
        return None;
    }
    if !meta.session.is_empty() && &meta.session != ctx.session() {
        info!(
            ctx.to_editor(),
            "Request session ID '{}' does not match original session '{}', shutting down",
            meta.session,
            ctx.session(),
        );
        return Some(ControlFlow::Break(()));
    }
    if !meta.buffile.starts_with('/') {
        report_error_no_server_configured(
            ctx,
            meta,
            request_method,
            "not supported in scratch buffers",
        );
        return Some(ControlFlow::Continue(()));
    }
    if ctx.buffer_tombstones.contains(&meta.buffile) {
        report_error_no_server_configured(
            ctx,
            meta,
            request_method,
            &format!(
                "blocked in {}, see the *debug* buffer",
                std::env::current_dir()
                    .map(|cwd| short_file_path(&meta.buffile, cwd))
                    .unwrap_or(&meta.buffile),
            ),
        );
        return Some(ControlFlow::Continue(()));
    }

    #[allow(deprecated)]
    if !is_using_legacy_toml(&ctx.config)
        && meta
            .language_server
            .values()
            .any(|server| !server.roots.is_empty())
    {
        let msg = "Error: the lsp_servers configuration does not support the roots parameter, please use root_globs or root";
        ctx.show_error(mem::take(meta), msg);
        return Some(ControlFlow::Continue(()));
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
            report_error_no_server_configured(ctx, meta, request_method, &msg);

            return Some(ControlFlow::Continue(()));
        };
        let to_editor = ctx.to_editor().clone();
        #[allow(deprecated)]
        for server_name in servers {
            let server_config = &mut ctx
                .config
                .language_server
                .get_mut(server_name_for_lookup(&ctx.config, language_id, server_name).as_ref())
                .unwrap();
            server_config.root =
                find_project_root(&to_editor, language_id, &server_config.roots, &meta.buffile);
        }

        #[allow(deprecated)]
        {
            server_addresses = servers
                .iter()
                .map(|server_name| {
                    (
                        server_name.clone(),
                        ctx.config.language_server[server_name_for_lookup(
                            &ctx.config,
                            language_id,
                            server_name,
                        )
                        .as_ref()]
                        .root
                        .clone(),
                    )
                })
                .collect();
        }
    } else {
        let language_id = &meta.language_id;
        if language_id.is_empty() {
            let msg = "the 'lsp_language_id' option is empty, cannot route request";
            report_error_no_server_configured(ctx, meta, request_method, msg);
            return Some(ControlFlow::Continue(()));
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
            report_error_no_server_configured(ctx, meta, request_method, &msg);
            return Some(ControlFlow::Continue(()));
        };
        for (server_name, server) in &mut meta.language_server {
            if !server.root.is_empty() && !server.root_globs.is_empty() {
                let msg = "cannot specify both root and root_globs";
                ctx.show_error(mem::take(meta), msg);
                return Some(ControlFlow::Continue(()));
            }
            if !server.root.is_empty() {
                if !server.root.starts_with('/') {
                    let msg = format!(
                        "root path for '{server_name}' is not an absolute path: {}",
                        &server.root
                    );
                    ctx.show_error(mem::take(meta), msg);
                    return Some(ControlFlow::Continue(()));
                }
            } else if !server.root_globs.is_empty() {
                server.root = find_project_root(
                    ctx.to_editor(),
                    language_id,
                    &server.root_globs,
                    &meta.buffile,
                );
            } else {
                let msg = format!(
                    "missing project root path for '{server_name}', please set the root option"
                );
                ctx.show_error(mem::take(meta), msg);
                return Some(ControlFlow::Continue(()));
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
            continue 'server;
        }

        let server_id = ctx.language_servers.len();
        meta.servers.push(server_id);

        fn disable_in_buffer(ctx: &mut Context, meta: &EditorMeta) {
            ctx.buffer_tombstones.insert(meta.buffile.clone());
            let command = format!(
                "evaluate-commands -buffer {} lsp-block-in-buffer",
                editor_quote(&meta.buffile),
            );
            ctx.exec(meta.clone(), command);
        }

        // should be fine to unwrap because request was already routed which means language is configured
        let server_config = ctx.server_config(meta, &server_name).unwrap();
        let server_command = server_config.command.as_ref().unwrap_or(&server_name);
        if ctx.server_tombstones.contains(server_command) {
            debug!(
                ctx.to_editor(),
                "Ignoring request for disabled server {}", server_command
            );
            disable_in_buffer(ctx, meta);
            return Some(ControlFlow::Continue(()));
        }

        let server_transport = match language_server_transport::start(
            ctx.to_editor(),
            server_name.clone(),
            server_command,
            &server_config.args,
            &server_config.envs,
        ) {
            Ok(ls) => ls,
            Err(err) => {
                ctx.server_tombstones.insert(server_command.to_string());
                if !meta.buffile.is_empty() {
                    disable_in_buffer(ctx, meta);
                }
                ctx.show_error(
                    mem::take(meta),
                    format!(
                        "failed to start language server '{}', disabling it for this session: '{}'",
                        server_name, err
                    ),
                );
                return Some(ControlFlow::Continue(()));
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
            workaround_eslint: server_config.workaround_eslint.unwrap_or_default(),
        };
        ctx.language_servers.insert(server_id, server_settings);
        ctx.route_cache.insert((server_name, root), server_id);
        to_initialize.push(server_id);
    }
    if !to_initialize.is_empty() {
        initialize(meta.clone(), ctx, to_initialize);
    }
    None
}

fn report_error_no_server_configured(
    ctx: &mut Context,
    meta: &EditorMeta,
    request_method: &str,
    msg: &str,
) {
    if meta.sourcing {
        debug!(ctx.to_editor(), "{}", msg);
        return;
    }
    let word_regex = meta.word_regex.as_ref();
    let mut msg = Cow::Borrowed(msg);
    if let Some(fallback_cmd) = match request_method {
        _ if meta.hook => None,
        request::GotoDefinition::METHOD | request::References::METHOD => {
            let word_regex = word_regex.unwrap();
            msg = Cow::Owned(format!("{}. Falling back to: grep {}", msg, word_regex));
            Some(format!("grep {}", editor_quote(word_regex)))
        }
        request::DocumentHighlightRequest::METHOD => {
            let word_regex = word_regex.unwrap();
            msg = Cow::Owned(format!("{msg}. Falling_back to %s{}<ret>", word_regex));
            Some(formatdoc!(
                "evaluate-commands -save-regs a/^ %|
                 execute-keys -save-regs '' %[\"aZ]
                 set-register / {}
                 execute-keys -save-regs '' <percent>s<ret>Z
                 execute-keys %[\"az<a-z>a]
             |",
                editor_quote(word_regex).replace('|', "||"),
            ))
        }
        _ => None,
    } {
        assert!(!meta.hook);
        let command = format!("evaluate-commands {}", &editor_quote(&fallback_cmd));
        ctx.exec(meta.clone(), command);
    }
    ctx.show_error(meta.clone(), msg);
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
            ctx.to_editor(),
            "incoming request {} is stale, version {} but I already have {}",
            request.method,
            request.meta.version,
            document_version
        );
        // Keep it nevertheless because at least "completionItem/resolve" is useful.
    }
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
        assert!(request.response_fifo.is_none());
        // Wait for buffer update.
        ctx.pending_requests_from_future.push(request);
        return ControlFlow::Continue(());
    }

    let version_bump = [
        notification::DidOpenTextDocument::METHOD,
        notification::DidChangeTextDocument::METHOD,
    ]
    .contains(&method);

    dispatch_editor_request(request, ctx)?;

    if !version_bump {
        return ControlFlow::Continue(());
    }
    let mut requests = mem::take(&mut ctx.pending_requests_from_future);
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
            ctx.to_editor(),
            "dispatching pending request {} because we have received matching version in didChange",
            request.method
        );
        if document.version > request.meta.version {
            debug!(
                ctx.to_editor(),
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
    assert!(ctx.pending_requests_from_future.is_empty());
    ctx.pending_requests_from_future = mem::take(&mut requests);
    ControlFlow::Continue(())
}

fn dispatch_editor_request(request: EditorRequest, ctx: &mut Context) -> ControlFlow<()> {
    let method: &str = &request.method;
    if method != notification::DidOpenTextDocument::METHOD {
        ensure_did_open(&request, ctx);
    }
    let meta = request.meta;
    let response_fifo = request.response_fifo;
    let params = request.params;
    if let Some(client) = &meta.client {
        *LAST_CLIENT.lock().unwrap() = Some(client.clone());
    }
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
            code_action::text_document_code_action(meta, response_fifo, params.unbox(), ctx);
        }
        request::CodeActionResolveRequest::METHOD => {
            code_action::text_document_code_action_resolve(meta, params.unbox(), ctx);
        }
        request::ExecuteCommand::METHOD => {
            workspace::execute_command(meta, response_fifo, params.unbox(), ctx);
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
            for server in ctx.language_servers.values() {
                debug!(
                    ctx.to_editor(),
                    "Sending exit notification to server {}", server.name
                );
            }
            for server_id in ctx.language_servers.keys().cloned().collect_vec() {
                ctx.notify::<notification::Exit>(server_id, ());
            }
            return ControlFlow::Break(());
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
            formatting::text_document_formatting(meta, response_fifo, params.unbox(), ctx);
        }
        request::RangeFormatting::METHOD => {
            range_formatting::text_document_range_formatting(
                meta,
                response_fifo,
                params.unbox(),
                ctx,
            );
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
                workspace::apply_edit_from_editor(
                    server_id,
                    meta,
                    response_fifo,
                    params.unbox(),
                    ctx,
                );
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
            warn!(ctx.to_editor(), "Unsupported method: {}", method);
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
                            warn!(
                                ctx.to_editor(),
                                "semantic tokens registration without options"
                            );
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
                        ctx.to_editor(),
                        "Unsupported registration: {}", registration.method
                    ),
                }
            }
            Ok(serde_json::Value::Null)
        }
        request::WorkDoneProgressCreate::METHOD => {
            progress::work_done_progress_create(request.params, ctx)
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
            warn!(ctx.to_editor(), "Unsupported method: {}", method);
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
                ctx.to_editor(),
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
                    "evaluate-commands -verbatim -try-client '{}' lsp-show-message-log {} {}",
                    LAST_CLIENT
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|client| client.as_str())
                        .unwrap_or_default(),
                    editor_quote(&ctx.server(server_id).name),
                    editor_quote(&params.message)
                ),
            );
        }
        "telemetry/event" => {
            debug!(ctx.to_editor(), "{:?}", params);
        }
        _ => {
            warn!(ctx.to_editor(), "Unsupported method: {}", method);
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
    if buffile.is_empty() {
        return;
    }
    if !buffile.starts_with('/') {
        assert_eq!(&request.method, notification::Exit::METHOD);
        return;
    }
    let document = ctx.documents.get_mut(buffile);
    if document.is_none() && request.method == notification::DidChangeConfiguration::METHOD {
        return;
    }
    let document = document.unwrap();
    let unaware_servers: Vec<ServerId> = request
        .meta
        .servers
        .iter()
        .filter(|server_id| !document.opened_in_servers.contains(server_id))
        .copied()
        .collect();
    if unaware_servers.is_empty() {
        return;
    }
    document
        .opened_in_servers
        .extend(unaware_servers.iter().copied());
    let mut meta = request.meta.clone();
    meta.servers = unaware_servers;

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
    text_document_did_open_assume_cached(meta, document.text.to_string(), ctx);
}
