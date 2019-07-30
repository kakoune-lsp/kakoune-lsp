use crate::thread_worker::Worker;
use crate::types::*;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use jsonrpc_core::{self, Call, Output, Params, Version};
use lsp_types::notification::Notification;
use lsp_types::*;
use serde_json;
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, BufWriter, Error, ErrorKind, Read, Write};
use std::process::{Command, Stdio};

pub struct LanguageServerTransport {
    pub from_lang_server: Worker<Void, ServerMessage>,
    pub to_lang_server: Worker<ServerMessage, Void>,
    pub errors: Worker<Void, Void>,
}

pub fn start(cmd: &str, args: &[String]) -> LanguageServerTransport {
    info!("Starting Language server `{} {}`", cmd, args.join(" "));
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start language server");

    let writer = BufWriter::new(child.stdin.take().expect("Failed to open stdin"));
    let reader = BufReader::new(child.stdout.take().expect("Failed to open stdout"));

    // NOTE 1024 is arbitrary
    let channel_capacity = 1024;

    // XXX temporary way of tracing language server errors
    let mut stderr = BufReader::new(child.stderr.take().expect("Failed to open stderr"));
    let errors = Worker::spawn(
        "Language server errors",
        channel_capacity,
        move |receiver, _| loop {
            match receiver.try_recv() {
                Err(TryRecvError::Disconnected) => return,
                _ => {}
            };
            let mut buf = String::new();
            match stderr.read_to_string(&mut buf) {
                Ok(_) => {
                    if buf.is_empty() {
                        return;
                    }
                    error!("Language server error: {}", buf);
                }
                Err(e) => {
                    error!("Failed to read from language server stderr: {}", e);
                    return;
                }
            }
        },
    );
    // XXX

    let from_lang_server = Worker::spawn(
        "Messages from language server",
        channel_capacity,
        move |receiver, sender| {
            if let Err(msg) = reader_loop(reader, receiver, &sender) {
                error!("{}", msg);
            }
            // NOTE prevent zombie
            debug!("Waiting for language server process end");
            drop(child.stdin.take().unwrap());
            drop(child.stdout.take().unwrap());
            drop(child.stderr.take().unwrap());
            std::thread::sleep(std::time::Duration::from_secs(1));
            match child.try_wait() {
                Ok(None) => {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    match child.try_wait() {
                        Ok(None) => {
                            // Okay, we asked politely enough and waited long enough.
                            child.kill().unwrap();
                        }
                        _ => {}
                    }
                }
                Err(_) => {
                    error!("Language server wasn't running was it?!");
                }
                _ => {}
            }

            let notification = jsonrpc_core::Notification {
                jsonrpc: Some(Version::V2),
                method: notification::Exit::METHOD.to_string(),
                params: Params::None,
            };
            debug!("Sending exit notification back to controller");
            if sender
                .send(ServerMessage::Request(Call::Notification(notification)))
                .is_err()
            {
                return;
            };
        },
    );

    let to_lang_server = Worker::spawn(
        "Messages to language server",
        channel_capacity,
        move |receiver, _| {
            if writer_loop(writer, &receiver).is_err() {
                error!("Failed to write message to language server");
            }
            // NOTE we rely on assumption that if write failed then read is failed as well
            // or will fail shortly and do all exiting stuff
        },
    );

    LanguageServerTransport {
        from_lang_server,
        to_lang_server,
        errors,
    }
}

fn reader_loop(
    mut reader: impl BufRead,
    receiver: Receiver<Void>,
    sender: &Sender<ServerMessage>,
) -> io::Result<()> {
    let mut headers: HashMap<String, String> = HashMap::default();
    loop {
        match receiver.try_recv() {
            Err(TryRecvError::Disconnected) => return Ok(()),
            _ => {}
        };
        headers.clear();
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header)? == 0 {
                debug!("Language server closed pipe, stopping reading");
                return Ok(());
            }
            let header = header.trim();
            if header.is_empty() {
                break;
            }
            let parts: Vec<&str> = header.split(": ").collect();
            if parts.len() != 2 {
                return Err(Error::new(ErrorKind::Other, "Failed to parse header"));
            }
            headers.insert(parts[0].to_string(), parts[1].to_string());
        }
        let content_len = headers
            .get("Content-Length")
            .ok_or_else(|| Error::new(ErrorKind::Other, "Failed to get Content-Length header"))?
            .parse()
            .map_err(|_| Error::new(ErrorKind::Other, "Failed to parse Content-Length header"))?;
        let mut content = vec![0; content_len];
        reader.read_exact(&mut content)?;
        let msg = String::from_utf8(content)
            .map_err(|_| Error::new(ErrorKind::Other, "Failed to read content as UTF-8 string"))?;
        debug!("From server: {}", msg);
        let output: serde_json::Result<Output> = serde_json::from_str(&msg);
        match output {
            Ok(output) => {
                if sender.send(ServerMessage::Response(output)).is_err() {
                    return Err(Error::new(ErrorKind::Other, "Failed to send response"));
                }
            }
            Err(_) => {
                let msg: Call = serde_json::from_str(&msg).map_err(|_| {
                    Error::new(ErrorKind::Other, "Failed to parse language server message")
                })?;
                if sender.send(ServerMessage::Request(msg)).is_err() {
                    return Err(Error::new(ErrorKind::Other, "Failed to send response"));
                }
            }
        }
    }
}

fn writer_loop(mut writer: impl Write, receiver: &Receiver<ServerMessage>) -> io::Result<()> {
    for request in receiver {
        let request = match request {
            ServerMessage::Request(request) => serde_json::to_string(&request),
            ServerMessage::Response(response) => serde_json::to_string(&response),
        }?;
        debug!("To server: {}", request);
        write!(
            writer,
            "Content-Length: {}\r\n\r\n{}",
            request.len(),
            request
        )?;
        writer.flush()?;
    }
    // NOTE we rely on the assumption that language server will exit when its stdin is closed
    // without need to kill child process
    debug!("Received signal to stop language server, closing pipe");
    Ok(())
}
