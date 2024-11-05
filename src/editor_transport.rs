use crate::thread_worker::Worker;
use crate::types::*;
use crossbeam_channel::Receiver;
use std::borrow::Cow;
use std::io::Write;
use std::process::{Command, Stdio};

pub struct EditorTransport {
    pub to_editor: Worker<EditorResponse, Void>,
}

pub fn start(session: &SessionId) -> Result<EditorTransport, i32> {
    // NOTE 1024 is arbitrary
    let channel_capacity = 1024;

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

    Ok(EditorTransport { to_editor })
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

            if stdin.write_all(command.as_bytes()).is_err() && log {
                error!(response.meta.session, "Failed to write to editor stdin");
            }
        }
        Err(e) => {
            if log {
                error!(response.meta.session, "Failed to run Kakoune: {}", e);
            }
        }
    }
}
