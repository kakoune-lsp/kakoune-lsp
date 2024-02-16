use crate::context::meta_for_session;
use crate::controller;
use crate::editor_transport;
use crate::project_root::find_project_root;
use crate::thread_worker::Worker;
use crate::types::*;
use crate::util::*;
use crossbeam_channel::{after, never, select, Sender};
use indoc::formatdoc;
use lazy_static::lazy_static;
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use lsp_types::*;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

struct ControllerHandle {
    worker: Worker<EditorRequest, Void>,
}

type Controllers = HashMap<Vec<Route>, ControllerHandle>;

/// Start the main event loop.
///
/// This function starts editor transport and routes incoming editor requests to controllers.
/// One controller is spawned per unique route, which is essentially a product of editor session,
/// file type (represented as language id) and project (represented as project root path).
///
/// `initial_request` could be passed to avoid extra synchronization churn if event loop is started
/// as a result of request from editor.
pub fn start(
    config: &Config,
    log_path: &'static Option<PathBuf>,
    initial_request: Option<String>,
) -> i32 {
    info!("Starting main event loop");

    let editor = editor_transport::start(&config.server.session, initial_request);
    if let Err(code) = editor {
        return code;
    }
    let editor = editor.unwrap();

    let languages = config.language_server.clone();
    let filetypes = filetype_to_language_id_map(config);

    let mut controllers: Controllers = HashMap::default();

    let timeout = config.server.timeout;

    'event_loop: loop {
        let timeout_channel = if timeout > 0 {
            after(Duration::from_secs(timeout))
        } else {
            never()
        };

        select! {
            recv(timeout_channel) -> _ => {
                info!("Exiting session after {} seconds of inactivity", timeout);
                break 'event_loop
            }

            recv(editor.from_editor) -> request => {
                // editor.receiver was closed, either because of the unrecoverable error or timeout
                // nothing we can do except to gracefully exit by stopping session
                // luckily, next `kak-lsp --request` invocation would spin up fresh session
                let request = match request {
                    Ok(request) => request,
                    Err(_) => break 'event_loop,
                };
                let request: EditorRequest = match toml::from_str(&request) {
                    Ok(req) => req,
                    Err(err) => {
                        error!("Failed to parse editor request: {}", err);
                        handle_broken_editor_request(
                            editor.to_editor.sender(),
                            request,
                            &config.server.session,
                            err,
                        );
                        continue 'event_loop;
                    }
                };
                // editor explicitely asked us to stop kakoune-lsp session
                // (and we stop, even if other editor sessions are using this kakoune-lsp session)
                if request.method == "stop" {
                    break 'event_loop;
                }
                // editor exited, we need to cleanup associated controllers
                if request.method == notification::Exit::METHOD {
                    exit_editor_session(&mut controllers, &request);
                    continue 'event_loop;
                }

                let cfg = filetypes.get(&request.meta.filetype);
                if cfg.is_none() {
                    let msg = format!(
                        "Language server is not configured for filetype `{}`",
                        &request.meta.filetype
                    );
                    debug!("{}", msg);
                    return_request_error(editor.to_editor.sender(), &request, &msg);

                    continue 'event_loop;
                }

                let (language_id, servers) = cfg.unwrap();
                let routes: Vec<_> = servers
                    .iter()
                    .map(|server_name| {
                        let language  = &languages[server_name];
                        let root = find_project_root(language_id, &language.roots, &request.meta.buffile);
                        let route = Route {
                            session: request.meta.session.clone(),
                            server_name: server_name.clone(),
                            root,
                        };

                        debug!("Routing editor request to {:?}", route);
                        route
                    })
                    .collect();

                use std::collections::hash_map::Entry;
                match controllers.entry(routes.clone()) {
                    Entry::Occupied(controller_entry) => {
                        if let Err(err) = controller_entry.get().worker.sender().send(request.clone())  {
                            error!("Failed to send message to controller: {}", err);
                            return_request_error(
                                editor.to_editor.sender(),
                                &request,
                                "Language server is no longer running"
                            );
                            controller_entry.remove();
                            continue 'event_loop;
                        }
                    }
                    Entry::Vacant(controller_entry) => {
                        // As Kakoune triggers BufClose after KakEnd we don't want to spawn a
                        // new controller in that case. In normal situation it's unlikely to
                        // get didClose message without running controller, unless it crashed
                        // before. In that case didClose can be safely ignored as well.
                        if request.method != notification::DidCloseTextDocument::METHOD {
                            debug!("Spawning a new controller for {:#?}", routes);
                            controller_entry.insert(spawn_controller(
                                config.clone(),
                                log_path,
                                language_id.clone(),
                                routes,
                                request,
                                editor.to_editor.sender().clone(),
                            ));
                        }
                    }
                }
            }
        }
    }
    stop_session(&mut controllers);
    0
}

/// Tries to send an error to the client about a request that failed to parse.
fn handle_broken_editor_request(
    to_editor: &Sender<EditorResponse>,
    request: String,
    session: &str,
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
            let meta = meta_for_session(session.to_string(), Some(client_name.to_string()));
            let command = format!("lsp-show-error {}", editor_quote(&msg));
            let response = EditorResponse {
                meta,
                command: command.into(),
            };
            if let Err(err) = to_editor.send(response) {
                error!("Failed to send error message to editor: {err}");
            };
        }
    }
}

/// Sends an error back to the editor.
///
/// This will cancel any blocking requests and also print an error if the
/// request was not triggered by an editor hook.
fn return_request_error(to_editor: &Sender<EditorResponse>, request: &EditorRequest, msg: &str) {
    let word_regex = request.meta.word_regex.as_ref();
    let command = if let Some(multi_cmds) = match request.method.as_str() {
        _ if request.meta.hook => None,
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

    // If editor is expecting a fifo response, give it one, so it won't hang.
    if let Some(ref fifo) = request.meta.fifo {
        std::fs::write(fifo, &command).expect("Failed to write command to fifo");
    }

    if !request.meta.hook {
        let response = EditorResponse {
            meta: request.meta.clone(),
            command: command.into(),
        };
        if let Err(err) = to_editor.send(response) {
            error!("Failed to send error message to editor: {err}");
        };
    }
}

/// Reap controllers associated with editor session.
fn exit_editor_session(controllers: &mut Controllers, request: &EditorRequest) {
    info!(
        "Editor session `{}` closed, shutting down associated language servers",
        request.meta.session
    );
    controllers.retain(|routes, controller| {
        let all_from_session = routes
            .iter()
            .all(|route| route.session == request.meta.session);
        let any_from_session = routes
            .iter()
            .all(|route| route.session == request.meta.session);
        assert_eq!(all_from_session, any_from_session);
        if all_from_session {
            for route in routes {
                info!("Exit {} in project {}", route.server_name, route.root);
            }
            // to notify kakoune-lsp about editor session end we use the same `exit` notification
            // as used in LSP spec to notify language server to exit, thus we can just clone
            // request and pass it along
            if controller.worker.sender().send(request.clone()).is_err() {
                error!("Failed to send stop message to language server");
            }
            false
        } else {
            true
        }
    });
}

/// Shut down all language servers and exit.
fn stop_session(controllers: &mut Controllers) {
    let request = EditorRequest {
        meta: EditorMeta::default(),
        method: notification::Exit::METHOD.to_string(),
        params: toml::Value::Table(toml::value::Table::default()),
    };
    info!("Shutting down language servers and exiting");
    for (routes, controller) in controllers.drain() {
        if controller.worker.sender().send(request.clone()).is_err() {
            error!("Failed to send stop message to language server");
        }
        for route in &routes {
            info!("Exit {} in project {}", route.server_name, route.root);
        }
    }
}

fn spawn_controller(
    config: Config,
    log_path: &'static Option<PathBuf>,
    language_id: LanguageId,
    routes: Vec<Route>,
    request: EditorRequest,
    to_editor: Sender<EditorResponse>,
) -> ControllerHandle {
    // NOTE 1024 is arbitrary
    let channel_capacity = 1024;

    let worker = Worker::spawn("Controller", channel_capacity, move |receiver, _| {
        controller::start(
            to_editor,
            receiver,
            &language_id,
            &routes,
            request,
            config,
            log_path,
        );
    });

    ControllerHandle { worker }
}
