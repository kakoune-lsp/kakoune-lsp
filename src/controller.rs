use std::borrow::Cow;
use std::collections::HashSet;
use std::mem;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::time::Duration;

use crate::capabilities::{self, initialize};
use crate::context::meta_for_session;
use crate::context::Context;
use crate::context::*;
use crate::diagnostics;
use crate::editor_transport;
use crate::language_features::{selection_range, *};
use crate::language_server_transport;
use crate::progress;
use crate::project_root::find_project_root;
use crate::show_message;
use crate::text_sync::*;
use crate::types::*;
use crate::util::*;
use crate::workspace;
use crossbeam_channel::{after, never, tick, Receiver, Select, Sender};
use indoc::formatdoc;
use itertools::Itertools;
use jsonrpc_core::{Call, ErrorCode, MethodCall, Output, Params};
use lazy_static::lazy_static;
use lsp_types::error_codes::CONTENT_MODIFIED;
use lsp_types::notification::DidChangeWorkspaceFolders;
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use lsp_types::*;
use regex::Regex;

/// Start the main event loop.
///
/// This function starts editor transport and processes incoming editor requests.
///
/// `initial_request` could be passed to avoid extra synchronization churn if event loop is started
/// as a result of request from editor.
pub fn start(
    session: SessionId,
    lsp_session: &LspSessionId,
    config: Config,
    log_path: &'static Option<PathBuf>,
    initial_request: Option<String>,
) -> i32 {
    info!(session, "Starting main event loop");

    let editor = editor_transport::start(&session, lsp_session, initial_request);
    if let Err(code) = editor {
        return code;
    }
    let editor = editor.unwrap();

    let mut ctx = Context::new(session, editor.to_editor.sender().clone(), config);
    let ctx = &mut ctx;

    let timeout = ctx.config.server.timeout;

    'event_loop: loop {
        let server_rxs: Vec<&Receiver<ServerMessage>> = ctx
            .language_servers
            .values()
            .map(|settings| settings.transport.from_lang_server.receiver())
            .collect();
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
        let from_editor_op = sel.recv(&editor.from_editor);
        let from_file_watcher_op = sel.recv(from_file_watcher);
        let from_pending_file_watcher_op = sel.recv(from_pending_file_watcher);

        let timeout_channel = if timeout > 0 {
            after(Duration::from_secs(timeout))
        } else {
            never()
        };
        let timeout_op = sel.recv(&timeout_channel);

        let op = sel.select();
        match op.index() {
            idx if idx == timeout_op => {
                info!(
                    ctx.last_session(),
                    "Exiting session after {} seconds of inactivity", timeout
                );
                break 'event_loop;
            }
            idx if idx == from_editor_op => {
                let request = op.recv(&editor.from_editor);
                let Ok(request) = request else {
                    break 'event_loop;
                };
                match process_raw_editor_request(ctx, request) {
                    ControlFlow::Continue(()) => (),
                    ControlFlow::Break(()) => break 'event_loop,
                }
            }
            i if i == from_file_watcher_op => {
                let msg = op.recv(from_file_watcher);

                if msg.is_err() {
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
                let _msg = op.recv(from_pending_file_watcher);

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

                if msg.is_err() {
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

pub fn process_raw_editor_request(ctx: &mut Context, request: String) -> ControlFlow<()> {
    let request: EditorRequest = match toml::from_str(&request) {
        Ok(req) => req,
        Err(err) => {
            error!(
                ctx.last_session(),
                "Failed to parse editor request: {}", err
            );
            handle_broken_editor_request(&ctx.editor_tx, request, ctx.last_session(), err);
            return ControlFlow::Continue(());
        }
    };
    process_editor_request(ctx, request)
}

pub fn process_editor_request(ctx: &mut Context, mut request: EditorRequest) -> ControlFlow<()> {
    if let Some(pos) = ctx.sessions.iter().position(|c| c == &request.meta.session) {
        let last_pos = ctx.sessions.len() - 1;
        ctx.sessions.swap(pos, last_pos);
    } else {
        ctx.sessions.push(request.meta.session.clone());
    }
    if !route_request(ctx, &mut request.meta, &request.method) {
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
        dispatch_incoming_editor_request(request, ctx);
    } else {
        let servers = parked
            .into_iter()
            .map(|server_id| &ctx.server(*server_id).name)
            .join(", ");
        debug!(
            ctx.last_session(),
            "Language servers {} are still not initialized, parking request {:?}", servers, request
        );
        let err = format!(
            "lsp-show-error 'language servers {} are still not initialized, parking request'",
            servers
        );
        match &*request.method {
            notification::DidOpenTextDocument::METHOD => (),
            notification::DidChangeTextDocument::METHOD => (),
            notification::DidChangeConfiguration::METHOD => (),
            notification::DidCloseTextDocument::METHOD => (),
            notification::DidSaveTextDocument::METHOD => (),
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
    request: String,
    session: &SessionId,
    err: toml::de::Error,
) {
    // Try to parse enough of the broken toml to send the error to the editor.
    lazy_static! {
        static ref CLIENT_RE: Regex = Regex::new(r#"(?m)^client *= *"([a-zA-Z0-9_-]*)""#)
            .expect("Failed to parse client name regex");
        static ref HOOK_RE: Regex =
            Regex::new(r"(?m)^hook *= *true").expect("Failed to parse hook regex");
    }
    if let Some(client_name) = CLIENT_RE
        .captures(&request)
        .and_then(|cap| cap.get(1))
        .map(|cap| cap.as_str())
    {
        // We still don't want to spam the user if a hook triggered the error.
        if !HOOK_RE.is_match(&request) {
            let msg = format!("Failed to parse editor request: {err}");
            let meta = meta_for_session(session.clone(), Some(client_name.to_string()));
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
            params: toml::Value::Table(toml::value::Table::default()),
        };
        process_editor_request(ctx, request);
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
    if request_method == notification::Exit::METHOD {
        info!(
            meta.session,
            "Editor session `{}` closed, shutting down associated language servers", meta.session
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
        for server_config in ctx.config.language_server.values_mut() {
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
        for (server_name, server) in servers {
            if server.root.is_empty() {
                let msg = format!(
                    "missing project root path for {server_name}, please set the root option"
                );
                error!(meta.session, "{}", msg);
                report_error(&ctx.editor_tx, meta, &msg);
                return false;
            }
        }
        server_addresses = servers
            .iter()
            .map(|(server_name, server_settings)| {
                (server_name.clone(), server_settings.root.clone())
            })
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

fn dispatch_incoming_editor_request(request: EditorRequest, ctx: &mut Context) {
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
            return;
        }
    };
    let version_bump = [
        notification::DidOpenTextDocument::METHOD,
        notification::DidChangeTextDocument::METHOD,
    ]
    .contains(&method);

    dispatch_editor_request(request, ctx);

    if !version_bump {
        return;
    }
    let mut requests = mem::take(&mut ctx.pending_requests);
    requests.retain_mut(|request| {
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
        dispatch_editor_request(mem::take(request), ctx);
        false
    });
    assert!(ctx.pending_requests.is_empty());
    ctx.pending_requests = mem::take(&mut requests);
}

fn dispatch_editor_request(request: EditorRequest, ctx: &mut Context) {
    ensure_did_open(&request, ctx);
    let method: &str = &request.method;
    let meta = request.meta;
    let params = request.params;
    match method {
        notification::DidOpenTextDocument::METHOD => {
            text_document_did_open(meta, params, ctx);
        }
        notification::DidChangeTextDocument::METHOD => {
            text_document_did_change(meta, params, ctx);
        }
        notification::DidCloseTextDocument::METHOD => {
            text_document_did_close(meta, ctx);
        }
        notification::DidSaveTextDocument::METHOD => {
            text_document_did_save(meta, ctx);
        }
        notification::DidChangeConfiguration::METHOD => {
            workspace::did_change_configuration(meta, params, ctx);
        }
        request::CallHierarchyPrepare::METHOD => {
            call_hierarchy::call_hierarchy_prepare(meta, params, ctx);
        }
        request::Completion::METHOD => {
            completion::text_document_completion(meta, params, ctx);
        }
        request::ResolveCompletionItem::METHOD => {
            completion::completion_item_resolve(meta, params, ctx);
        }
        request::CodeActionRequest::METHOD => {
            code_action::text_document_code_action(meta, params, ctx);
        }
        request::CodeActionResolveRequest::METHOD => {
            code_action::text_document_code_action_resolve(meta, params, ctx);
        }
        request::ExecuteCommand::METHOD => {
            workspace::execute_command(meta, params, ctx);
        }
        request::HoverRequest::METHOD => {
            hover::text_document_hover(meta, params, ctx);
        }
        request::GotoDefinition::METHOD => {
            goto::text_document_definition(false, meta, params, ctx);
        }
        request::GotoDeclaration::METHOD => {
            goto::text_document_definition(true, meta, params, ctx);
        }
        request::GotoImplementation::METHOD => {
            goto::text_document_implementation(meta, params, ctx);
        }
        request::GotoTypeDefinition::METHOD => {
            goto::text_document_type_definition(meta, params, ctx);
        }
        request::References::METHOD => {
            goto::text_document_references(meta, params, ctx);
        }
        notification::Exit::METHOD => {
            let mut redundant_servers = vec![];
            for (server_id, server) in ctx.language_servers.iter_mut() {
                if let Some(pos) = server.users.iter().position(|s| s == &meta.session) {
                    server.users.swap_remove(pos);
                    if server.users.is_empty() {
                        redundant_servers.push(*server_id);
                    }
                }
            }
            for server_id in redundant_servers {
                ctx.notify::<notification::Exit>(server_id, ());
            }
        }

        notification::WorkDoneProgressCancel::METHOD => {
            progress::work_done_progress_cancel(meta, params, ctx);
        }
        request::SelectionRangeRequest::METHOD => {
            selection_range::text_document_selection_range(meta, params, ctx);
        }
        request::SignatureHelpRequest::METHOD => {
            signature_help::text_document_signature_help(meta, params, ctx);
        }
        request::DocumentHighlightRequest::METHOD => {
            highlight::text_document_highlight(meta, params, ctx);
        }
        request::DocumentSymbolRequest::METHOD => {
            document_symbol::text_document_document_symbol(meta, ctx);
        }
        "kakoune/breadcrumbs" => {
            document_symbol::breadcrumbs(meta, params, ctx);
        }
        "kakoune/next-or-previous-symbol" => {
            document_symbol::next_or_prev_symbol(meta, params, ctx);
        }
        "kakoune/object" => {
            document_symbol::object(meta, params, ctx);
        }
        "kakoune/goto-document-symbol" => {
            document_symbol::document_symbol_menu(meta, params, ctx);
        }
        "kakoune/textDocument/codeLens" => {
            code_lens::resolve_and_perform_code_lens(meta, params, ctx);
        }
        request::Formatting::METHOD => {
            formatting::text_document_formatting(meta, params, ctx);
        }
        request::RangeFormatting::METHOD => {
            range_formatting::text_document_range_formatting(meta, params, ctx);
        }
        request::WorkspaceSymbolRequest::METHOD => {
            workspace::workspace_symbol(meta, params, ctx);
        }
        request::Rename::METHOD => {
            rename::text_document_rename(meta, params, ctx);
        }
        "textDocument/diagnostics" => {
            diagnostics::editor_diagnostics(meta, ctx);
        }
        "capabilities" => {
            capabilities::capabilities(meta, ctx);
        }
        "apply-workspace-edit" => {
            if let Some(&server_id) = meta.servers.first() {
                workspace::apply_edit_from_editor(server_id, &meta, params, ctx);
            }
        }
        request::SemanticTokensFullRequest::METHOD => {
            semantic_tokens::tokens_request(meta, ctx);
        }

        request::InlayHintRequest::METHOD => {
            inlay_hints::inlay_hints(meta, params, ctx);
        }

        show_message::SHOW_MESSAGE_REQUEST_NEXT => {
            show_message::show_message_request_next(meta, ctx);
        }
        show_message::SHOW_MESSAGE_REQUEST_RESPOND => {
            show_message::show_message_request_respond(meta, params, ctx);
        }

        // CCLS
        ccls::NavigateRequest::METHOD => {
            ccls::navigate(meta, params, ctx);
        }
        ccls::VarsRequest::METHOD => {
            ccls::vars(meta, params, ctx);
        }
        ccls::InheritanceRequest::METHOD => {
            ccls::inheritance(meta, params, ctx);
        }
        ccls::CallRequest::METHOD => {
            ccls::call(meta, params, ctx);
        }
        ccls::MemberRequest::METHOD => {
            ccls::member(meta, params, ctx);
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
            rust_analyzer::expand_macro(meta, params, ctx);
        }

        // texlab
        texlab::Build::METHOD => {
            texlab::build(meta, params, ctx);
        }
        texlab::ForwardSearch::METHOD => {
            texlab::forward_search(meta, params, ctx);
        }

        _ => {
            warn!(meta.session, "Unsupported method: {}", method);
        }
    }
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
        text_document_did_open(request.meta.clone(), request.params.clone(), ctx);
        return;
    }
    match read_document(buffile) {
        Ok(draft) => {
            let mut params = toml::value::Table::default();
            params.insert("draft".to_string(), toml::Value::String(draft));
            text_document_did_open(request.meta.clone(), toml::Value::Table(params), ctx);
        }
        Err(err) => error!(
            request.meta.session,
            "Failed to read file {} to simulate textDocument/didOpen: {}", buffile, err
        ),
    };
}
