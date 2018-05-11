use crossbeam_channel::{bounded, Receiver, Sender};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::process::{Command, Stdio};
use std::thread;
use toml;
use types::*;

pub fn start(config: &Config) -> (Sender<EditorResponse>, Receiver<EditorRequest>) {
    let port = config.server.port;
    let ip = config.server.ip.parse().expect("Failed to parse IP");
    // NOTE 1024 is arbitrary
    let (reader_tx, reader_rx) = bounded(1024);
    thread::spawn(move || {
        info!("Starting editor transport on {}:{}", ip, port);
        let addr = SocketAddr::new(ip, port);

        let listener = TcpListener::bind(&addr).expect("Failed to start TCP server");

        for stream in listener.incoming() {
            let mut stream = stream.expect("Failed to connect to TCP stream");
            let mut request = String::new();
            stream
                .read_to_string(&mut request)
                .expect("Failed to read from TCP stream");
            debug!("From editor: {}", request);
            let request: EditorRequest =
                toml::from_str(&request).expect("Failed to parse editor request");
            reader_tx
                .send(request)
                .expect("Failed to send request from server");
        }
    });

    // NOTE 1024 is arbitrary
    let (writer_tx, writer_rx): (Sender<EditorResponse>, Receiver<EditorResponse>) = bounded(1024);
    thread::spawn(move || {
        for response in writer_rx {
            let mut child = Command::new("kak")
                .args(&["-p", &response.meta.session])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("Failed to run Kakoune");
            {
                let stdin = child.stdin.as_mut().expect("Failed to get editor stdin");
                let command = match response.meta.client.clone() {
                    Some(client) => {
                        // NOTE fingers crossed no 🦀 will appear in response.command
                        format!("eval -client {} %🦀{}🦀", client, response.command)
                    }
                    None => response.command.to_string(),
                };
                debug!("To editor `{}`: {}", response.meta.session, command);
                stdin
                    .write_all(command.as_bytes())
                    .expect("Failed to write to editor stdin");
            }
            child.wait().unwrap();
        }
    });

    (writer_tx, reader_rx)
}
