use crate::thread_worker::{ToEditorDispatcher, Worker};
use crate::{editor_quote, types::*};
use crossbeam_channel::{Receiver, Sender};
use std::borrow::Cow;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

pub type ToEditor = Sender<EditorResponse>;

pub fn send_command_to_editor(to_editor: &ToEditor, response: EditorResponse) {
    let log = !response.suppress_logging;
    let result = to_editor.send(response);
    if log {
        if let Err(err) = result {
            error!(to_editor, "Failed to send error message to editor: {}", err);
        }
    }
}

pub fn start(session: SessionId) -> Worker<EditorResponse, Void> {
    // NOTE 1024 is arbitrary
    let channel_capacity = 1024;

    Worker::spawn_to_editor_dispatcher(
        session.clone(),
        "Messages to editor",
        channel_capacity,
        move |receiver: Receiver<EditorResponse>, _| {
            for response in receiver {
                send_command_to_editor_here(&session, response);
            }
        },
    )
}

#[cfg(test)]
pub fn mock_to_editor() -> ToEditor {
    let (to_editor, _) = crossbeam_channel::unbounded::<EditorResponse>();
    to_editor
}

pub fn send_command_to_editor_here(session: &SessionId, response: EditorResponse) {
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
        debug!(session:session, "To editor `{}`: {}", session, command);
    }

    match Command::new("kak")
        .args(["-p", session])
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
                        error!(session:session, "failed to get editor stdin");
                    }
                    return;
                }
            };
            if let Err(err) = stdin.write_all(command.as_bytes()) {
                if log {
                    error!(session:session, "Failed to write to editor stdin: {}", err);
                }
            };
            let exit_code = child.wait().unwrap();
            if !exit_code.success() && log {
                error!(session:session, "kak -p exited with non-zero status");
            }
        }
        Err(err) => {
            if log {
                error!(session:session, "Failed to run Kakoune: {}", err);
            }
        }
    }
}

pub fn exec_fifo<S>(
    dispatcher: &ToEditorDispatcher,
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    command: S,
) where
    S: Into<Cow<'static, str>>,
{
    let command = command.into();
    if let Some(mut response_fifo) = response_fifo {
        let fifo = response_fifo.take().unwrap();
        debug!(
            dispatcher:dispatcher,
            "To editor via fifo '{}': {}",
            &fifo,
            command
        );
        fs::write(fifo, command.as_bytes()).expect("Failed to write command to fifo");
        return;
    }
    dispatcher.send(EditorResponse::new(meta, command));
}

pub fn show_error(
    dispatcher: &ToEditorDispatcher,
    meta: EditorMeta,
    response_fifo: Option<ResponseFifo>,
    message: impl AsRef<str>,
) {
    let message = message.as_ref();
    let sync = response_fifo.is_some();
    if meta.hook && !sync {
        // Historically, we have not shown errors in hook contexts.
        debug!(dispatcher:dispatcher, "{}", message);
        return;
    }
    if !sync {
        error!(dispatcher:dispatcher, "{}", message);
    }
    exec_fifo(
        dispatcher,
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
