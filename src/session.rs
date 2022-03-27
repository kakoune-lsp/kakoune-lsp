use crate::controller;
use crate::editor_transport;
use crate::project_root::find_project_root;
use crate::thread_worker::Worker;
use crate::types::*;
use crate::util::*;
use crossbeam_channel::{after, never, select, Sender};
use lsp_types::notification::Notification;
use lsp_types::*;
use std::collections::HashMap;
use std::time::Duration;

struct ControllerHandle {
    worker: Worker<EditorRequest, Void>,
}

type Controllers = HashMap<Route, ControllerHandle>;

/// Start the main event loop.
///
/// This function starts editor transport and routes incoming editor requests to controllers.
/// One controller is spawned per unique route, which is essentially a product of editor session,
/// file type (represented as language id) and project (represented as project root path).
///
/// `initial_request` could be passed to avoid extra synchronization churn if event loop is started
/// as a result of request from editor.
pub fn start(config: &Config, initial_request: Option<String>) -> i32 {
    info!("Starting main event loop");

    let editor = editor_transport::start(&config.server.session, initial_request);
    if let Err(code) = editor {
        return code;
    }
    let editor = editor.unwrap();

    let languages = config.language.clone();
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

            recv(editor.from_editor) -> request  => {
                // editor.receiver was closed, either because of the unrecoverable error or timeout
                // nothing we can do except to gracefully exit by stopping session
                // luckily, next `kak-lsp --request` invocation would spin up fresh session
                if request.is_err() {
                    break 'event_loop;
                }
                // should be safe to unwrap as we just checked request for being None
                // done this way instead of `match` to reduce nesting
                let request = request.unwrap();
                // editor explicitely asked us to stop kak-lsp session
                // (and we stop, even if other editor sessions are using this kak-lsp session)
                if request.method == "stop" {
                    break 'event_loop;
                }
                // editor exited, we need to cleanup associated controllers
                if request.method == notification::Exit::METHOD {
                    exit_editor_session(&mut controllers, &request);
                    continue 'event_loop;
                }

                let language_id = filetypes.get(&request.meta.filetype);
                if language_id.is_none() {
                    debug!(
                        "Language server is not configured for filetype `{}`",
                        &request.meta.filetype
                    );
                    continue 'event_loop;
                }
                let language_id = language_id.unwrap();

                let root_path = find_project_root(language_id, &languages[language_id].roots, &request.meta.buffile);
                let route = Route {
                    session: request.meta.session.clone(),
                    language: language_id.clone(),
                    root: root_path.clone(),
                };

                debug!("Routing editor request to {:?}", route);

                use std::collections::hash_map::Entry;
                match controllers.entry(route.clone()) {
                    Entry::Occupied(controller_entry) => {
                        if let Err(err) = controller_entry.get().worker.sender().send(request.clone())  {
                            if let Some(fifo) = request.meta.fifo {
                                cancel_blocking_request(fifo);
                            }
                            controller_entry.remove();
                            error!("Failed to send message to controller: {}", err);
                            continue 'event_loop;
                        }
                    }
                    Entry::Vacant(controller_entry) => {
                        // As Kakoune triggers BufClose after KakEnd we don't want to spawn a
                        // new controller in that case. In normal situation it's unlikely to
                        // get didClose message without running controller, unless it crashed
                        // before. In that case didClose can be safely ignored as well.
                        if request.method != notification::DidCloseTextDocument::METHOD {
                            debug!("Spawning a new controller for {:?}", route);
                            controller_entry.insert(spawn_controller(
                                config.clone(),
                                route,
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

/// When server is not running it's better to cancel blocking request.
/// Because server can take a long time to initialize or can fail to start.
/// We assume that it's less annoying for user to just repeat command later
/// than to wait, cancel, and repeat.
fn cancel_blocking_request(fifo: String) {
    debug!("Blocking request but LSP server is not running");
    let command = "lsp-show-error 'language server is not running, cancelling blocking request'";
    std::fs::write(fifo, command).expect("Failed to write command to fifo");
}

/// Reap controllers associated with editor session.
fn exit_editor_session(controllers: &mut Controllers, request: &EditorRequest) {
    info!(
        "Editor session `{}` closed, shutting down associated language servers",
        request.meta.session
    );
    controllers.retain(|route, controller| {
        if route.session == request.meta.session {
            info!("Exit {} in project {}", route.language, route.root);
            // to notify kak-lsp about editor session end we use the same `exit` notification as
            // used in LSP spec to notify language server to exit, thus we can just clone request
            // and pass it along
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
        ranges: None,
    };
    info!("Shutting down language servers and exiting");
    for (route, controller) in controllers.drain() {
        if controller.worker.sender().send(request.clone()).is_err() {
            error!("Failed to send stop message to language server");
        }
        info!("Exit {} in project {}", route.language, route.root);
    }
}

fn spawn_controller(
    config: Config,
    route: Route,
    request: EditorRequest,
    to_editor: Sender<EditorResponse>,
) -> ControllerHandle {
    // NOTE 1024 is arbitrary
    let channel_capacity = 1024;

    let worker = Worker::spawn("Controller", channel_capacity, move |receiver, _| {
        controller::start(to_editor, receiver, &route, request, config);
    });

    ControllerHandle { worker }
}
