use crate::thread_worker::Worker;
use crate::types::*;
use crate::util::*;
use crossbeam_channel::{bounded, Receiver, Sender};
use std::borrow::Cow;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path;
use std::process::{Command, Stdio};

pub struct EditorTransport {
    // Not using Worker here as listener blocks forever and joining its thread
    // would block kak-lsp from exiting.
    pub from_editor: Receiver<String>,
    pub to_editor: Worker<EditorResponse, Void>,
}

pub fn start(
    session: &SessionId,
    lsp_session: &LspSessionId,
    initial_request: Option<String>,
) -> Result<EditorTransport, i32> {
    // NOTE 1024 is arbitrary
    let channel_capacity = 1024;

    let (sender, receiver) = bounded(channel_capacity);
    let mut path = temp_dir();
    path.push(lsp_session);
    if path.exists() {
        if UnixStream::connect(&path).is_err() {
            if fs::remove_file(&path).is_err() {
                error!(
                    session,
                    "Failed to clean up dead LSP session at {}",
                    path.to_str().unwrap()
                );
                return Err(1);
            };
        } else {
            error!(
                session,
                "Server is already running for LSP session {}", lsp_session
            );
            return Err(1);
        }
    }
    std::thread::spawn({
        let session = session.clone();
        move || {
            if let Some(initial_request) = initial_request {
                if sender.send(initial_request).is_err() {
                    return;
                };
            }
            start_unix(session, &path, sender);
        }
    });
    let from_editor = receiver;

    let to_editor = Worker::spawn(
        session.clone(),
        "Messages to editor",
        channel_capacity,
        move |receiver: Receiver<EditorResponse>, _| {
            for response in receiver {
                send_command_to_editor(response, true);
            }
        },
    );

    Ok(EditorTransport {
        from_editor,
        to_editor,
    })
}

pub fn send_command_to_editor(response: EditorResponse, log: bool) {
    match Command::new("kak")
        .args(["-p", &response.meta.session])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            let stdin = match child.stdin.as_mut() {
                Some(stdin) => stdin,
                None => {
                    if log {
                        error!(response.meta.session, "failed to get editor stdin");
                    }
                    return;
                }
            };

            let client = response.meta.client.as_ref();
            let command = match client.filter(|&s| !s.is_empty()) {
                Some(client) => {
                    let command = format!(
                        "evaluate-commands -client {} -verbatim -- {}",
                        client, response.command
                    );
                    if log {
                        debug!(
                            response.meta.session,
                            "To editor `{}`: {}", response.meta.session, command
                        );
                    }
                    Cow::from(command)
                }
                None => {
                    if log {
                        debug!(
                            response.meta.session,
                            "To editor `{}`: {}", response.meta.session, response.command
                        );
                    }
                    response.command
                }
            };

            if stdin.write_all(command.as_bytes()).is_err() {
                if log {
                    error!(response.meta.session, "Failed to write to editor stdin");
                }
                return;
            }
            // code should fail earlier if Kakoune was not spawned
            // otherwise something went completely wrong, better to panic
            let exit_code = child.wait().unwrap();
            if !exit_code.success() && log {
                error!(response.meta.session, "kak -p exited with non-zero status");
            }
        }
        Err(e) => {
            if log {
                error!(response.meta.session, "Failed to run Kakoune: {}", e);
            }
        }
    }
}

pub fn start_unix(session: SessionId, path: &path::Path, sender: Sender<String>) {
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(e) => {
            error!(session, "Failed to bind: {}", e);
            return;
        }
    };

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let mut request = String::new();
                match stream.read_to_string(&mut request) {
                    Ok(_) => {
                        if request.is_empty() {
                            continue;
                        }
                        debug!(session, "From editor: {}", request);
                        if sender.send(request).is_err() {
                            return;
                        };
                    }
                    Err(e) => {
                        error!(session, "Failed to read from stream: {}", e);
                    }
                }
            }
            Err(e) => {
                error!(session, "Failed to accept connection: {}", e);
            }
        }
    }
}
