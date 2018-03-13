use crossbeam_channel::{bounded, Receiver, Sender};
use fnv::FnvHashMap;
use jsonrpc_core::{Call, Output};
use serde_json;
use std::io::{self, BufRead, BufReader, BufWriter, Error, ErrorKind, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use types::*;

pub fn start(cmd: &str, args: &Vec<String>) -> (Sender<ServerMessage>, Receiver<ServerMessage>) {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start language server");

    let writer = BufWriter::new(child.stdin.take().expect("Failed to open stdin"));
    let reader = BufReader::new(child.stdout.take().expect("Failed to open stdout"));

    // XXX temporary way of tracing language server errors
    let mut stderr = BufReader::new(child.stderr.take().expect("Failed to open stderr"));
    thread::spawn(move || loop {
        let mut buf = String::new();
        stderr.read_to_string(&mut buf).unwrap();
        if buf.is_empty() {
            break;
        }
        println!("Language server error: {}", buf);
    });
    // XXX

    // NOTE 1024 is arbitrary
    let (reader_tx, reader_rx) = bounded(1024);
    thread::spawn(move || {
        reader_loop(reader, &reader_tx).expect("Failed to read message from language server");
    });

    // NOTE 1024 is arbitrary
    let (writer_tx, writer_rx): (Sender<ServerMessage>, Receiver<ServerMessage>) = bounded(1024);
    thread::spawn(move || {
        writer_loop(writer, &writer_rx).expect("Failed to write message to language server");
    });

    (writer_tx, reader_rx)
}

fn reader_loop(mut reader: impl BufRead, tx: &Sender<ServerMessage>) -> io::Result<()> {
    let mut headers = FnvHashMap::default();
    loop {
        headers.clear();
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header)? == 0 {
                return Err(Error::new(ErrorKind::Other, "Failed to read from server"));
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
            .expect("Failed to find Content-Length header")
            .parse()
            .expect("Failed to parse Content-Length header");
        let mut content = vec![0; content_len];
        reader.read_exact(&mut content)?;
        let msg = String::from_utf8(content).expect("Failed to read content as UTF-8 string");
        let output: serde_json::Result<Output> = serde_json::from_str(&msg);
        match output {
            Ok(output) => tx.send(ServerMessage::Response(output))
                .expect("Failed to send message from language server"),
            Err(_) => {
                let msg: Call =
                    serde_json::from_str(&msg).expect("Failed to parse language server message");
                tx.send(ServerMessage::Request(msg))
                    .expect("Failed to send message from language server");
            }
        }
    }
}

fn writer_loop(mut writer: impl Write, rx: &Receiver<ServerMessage>) -> io::Result<()> {
    for request in rx {
        let request = match request {
            ServerMessage::Request(request) => serde_json::to_string(&request),
            ServerMessage::Response(response) => serde_json::to_string(&response),
        }?;
        write!(
            writer,
            "Content-Length: {}\r\n\r\n{}",
            request.len(),
            request
        )?;
        writer.flush()?;
    }
    Ok(())
}
