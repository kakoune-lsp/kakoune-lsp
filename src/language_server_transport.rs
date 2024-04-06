use crate::thread_worker::Worker;
use crate::types::*;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use jsonrpc_core::{self, Call, Output};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, BufWriter, Error, ErrorKind, Write};
use std::process::{Command, Stdio};

pub struct LanguageServerTransport {
    // The field order is important as it defines the order of drop.
    // We want to exit a writer loop first (after sending exit notification),
    // then close all pipes and wait until child process is finished.
    // That helps to ensure that reader loop is not stuck trying to read from the language server.
    pub to_lang_server: Worker<ServerMessage, Void>,
    pub from_lang_server: Worker<Void, ServerMessage>,
    pub errors: Worker<Void, Void>,
}

pub fn start(
    server_name: &str,
    cmd: &str,
    args: &[String],
    envs: &HashMap<String, String>,
) -> Result<LanguageServerTransport, String> {
    info!(
        "Starting Language server {server_name} as `{} {}`",
        cmd,
        args.join(" ")
    );
    let mut child = match Command::new(cmd)
        .args(args)
        .envs(envs)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(err) => {
            return Err(match err.kind() {
                ErrorKind::NotFound | ErrorKind::PermissionDenied => format!("{}: {}", err, cmd),
                _ => format!("{}", err),
            })
        }
    };

    let writer = BufWriter::new(child.stdin.take().expect("Failed to open stdin"));
    let reader = BufReader::new(child.stdout.take().expect("Failed to open stdout"));

    // NOTE 1024 is arbitrary
    let channel_capacity = 1024;

    // XXX temporary way of tracing language server errors
    let mut stderr = BufReader::new(child.stderr.take().expect("Failed to open stderr"));
    let errors = Worker::spawn(
        "Language server errors",
        channel_capacity,
        move |receiver, _| {
            if let Err(TryRecvError::Disconnected) = receiver.try_recv() {
                return;
            }
            let mut line = String::new();
            loop {
                line.clear();
                match stderr.read_line(&mut line) {
                    Ok(0) => return,
                    Ok(_n) => {
                        error!("Language server stderr: {}", line.trim_end_matches('\n'));
                    }
                    Err(e) => {
                        error!("Failed to read from language server stderr: {}", e);
                        return;
                    }
                }
            }
        },
    );
    // XXX

    let from_lang_server = {
        let server_name = server_name.to_string();
        Worker::spawn(
            "Messages from language server",
            channel_capacity,
            move |receiver, sender| {
                if let Err(msg) = reader_loop(&server_name, reader, receiver, &sender) {
                    error!("{}", msg);
                }
            },
        )
    };

    let to_lang_server = {
        let server_name = server_name.to_string();
        Worker::spawn(
            "Messages to language server",
            channel_capacity,
            move |receiver, _| {
                if writer_loop(&server_name, writer, &receiver).is_err() {
                    error!("Failed to write message to language server");
                }
                // NOTE prevent zombie
                debug!("Waiting for language server process end");
                drop(child.stdin.take());
                drop(child.stdout.take());
                drop(child.stderr.take());
                std::thread::sleep(std::time::Duration::from_secs(1));
                match child.try_wait() {
                    Ok(None) => {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        if let Ok(None) = child.try_wait() {
                            // Okay, we asked politely enough and waited long enough.
                            child.kill().unwrap();
                        }
                    }
                    Err(_) => {
                        error!("Language server wasn't running was it?!");
                    }
                    _ => {}
                }
            },
        )
    };

    Ok(LanguageServerTransport {
        to_lang_server,
        from_lang_server,
        errors,
    })
}

fn reader_loop(
    server_name: &str,
    mut reader: impl BufRead,
    receiver: Receiver<Void>,
    sender: &Sender<ServerMessage>,
) -> io::Result<()> {
    let mut headers: HashMap<String, String> = HashMap::default();
    loop {
        if let Err(TryRecvError::Disconnected) = receiver.try_recv() {
            return Ok(());
        }
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
        debug!("From server {server_name}: {msg}");
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

fn writer_loop(
    server_name: &str,
    mut writer: impl Write,
    receiver: &Receiver<ServerMessage>,
) -> io::Result<()> {
    for request in receiver {
        let request = match request {
            ServerMessage::Request(request) => serde_json::to_string(&request),
            ServerMessage::Response(response) => serde_json::to_string(&response),
        }?;
        debug!("To server {server_name}: {request}");
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
