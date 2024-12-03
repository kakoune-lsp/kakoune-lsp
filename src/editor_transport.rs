use crate::thread_worker::Worker;
use crate::{editor_quote, types::*};
use crossbeam_channel::{Receiver, Sender};
use std::borrow::Cow;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

pub type ToEditorSender = Sender<EditorResponse>;

pub fn start(session: SessionId) -> Worker<SessionId, EditorResponse, Void> {
    // NOTE 1024 is arbitrary
    let channel_capacity = 1024;

    Worker::spawn(
        session,
        "Messages to editor",
        channel_capacity,
        move |session: SessionId, receiver: Receiver<EditorResponse>, _| {
            for response in receiver {
                session.dispatch(response);
            }
        },
    )
}

#[cfg(test)]
pub fn mock_to_editor() -> ToEditorSender {
    let (to_editor, _) = crossbeam_channel::unbounded::<EditorResponse>();
    to_editor
}

impl ToEditor for ToEditorSender {
    fn dispatch(&self, response: EditorResponse) {
        let log = !response.suppress_logging;
        let result = self.send(response);
        if log {
            if let Err(err) = result {
                error!(self, "Failed to send error message to editor: {}", err);
            }
        }
    }
}

impl ToEditor for SessionId {
    fn dispatch(&self, response: EditorResponse) {
        let log = !response.suppress_logging;

        let client = response.meta.client.as_ref();
        let command = match client.filter(|&s| !s.is_empty()) {
            Some(client) => {
                let command = format!(
                    "evaluate-commands -client {} -verbatim -- {}",
                    client, response.command
                );
                Cow::from(command)
            }
            None => response.command,
        };
        if log {
            debug!(self, "To editor `{}`: {}", self, command);
        }

        match Command::new("kak")
            .args(["-p", self])
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
                            error!(self, "failed to get editor stdin");
                        }
                        return;
                    }
                };
                if let Err(err) = stdin.write_all(command.as_bytes()) {
                    if log {
                        error!(self, "Failed to write to editor stdin: {}", err);
                    }
                };
                let exit_code = child.wait().unwrap();
                if !exit_code.success() && log {
                    error!(self, "kak -p exited with non-zero status");
                }
            }
            Err(err) => {
                if log {
                    error!(self, "Failed to run Kakoune: {}", err);
                }
            }
        }
    }
}

pub fn exec_fifo<S>(
    to_editor: &impl ToEditor,
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    command: S,
) where
    S: Into<Cow<'static, str>>,
{
    let command = command.into();
    if let Some(mut response_fifo) = response_fifo {
        let fifo = response_fifo.take().unwrap();
        debug!(to_editor, "To editor via fifo '{}': {}", &fifo, command);
        fs::write(fifo, command.as_bytes()).expect("Failed to write command to fifo");
        return;
    }
    to_editor.dispatch(EditorResponse::new(meta, command));
}

pub fn show_error(
    to_editor: &impl ToEditor,
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    message: impl AsRef<str>,
) {
    let message = message.as_ref();
    let sync = response_fifo.is_some();
    if meta.hook && !sync {
        // Historically, we have not shown errors in hook contexts.
        debug!(to_editor, "{}", message);
        return;
    }
    if !sync {
        error!(to_editor, "{}", message);
    }
    exec_fifo(
        to_editor,
        meta,
        response_fifo,
        if sync {
            // Allow silencing the error with 'try'.
            format!("fail -- {}", editor_quote(message))
        } else {
            format!("lsp-show-error {}", editor_quote(message))
        },
    );
}
