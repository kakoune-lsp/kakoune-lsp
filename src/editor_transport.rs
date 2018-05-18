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
            match stream {
                Ok(mut stream) => {
                    let mut request = String::new();
                    match stream.read_to_string(&mut request) {
                        Ok(_) => {
                            debug!("From editor: {}", request);
                            let request: EditorRequest =
                                toml::from_str(&request).expect("Failed to parse editor request");
                            reader_tx
                                .send(request)
                                .expect("Failed to send request from server");
                        }
                        Err(e) => {
                            error!("Failed to read from TCP stream: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    });

    // NOTE 1024 is arbitrary
    let (writer_tx, writer_rx): (Sender<EditorResponse>, Receiver<EditorResponse>) = bounded(1024);
    thread::spawn(move || {
        for response in writer_rx {
            match Command::new("kak")
                .args(&["-p", &response.meta.session])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    {
                        let stdin = child.stdin.as_mut();
                        if stdin.is_none() {
                            error!("Failed to get editor stdin");
                            return;
                        }
                        let stdin = stdin.unwrap();
                        let command = match response.meta.client.clone() {
                            Some(client) => {
                                // NOTE fingers crossed no 🦀 will appear in response.command
                                format!("eval -client {} %🦀{}🦀", client, response.command)
                            }
                            None => response.command.to_string(),
                        };
                        debug!("To editor `{}`: {}", response.meta.session, command);
                        if stdin.write_all(command.as_bytes()).is_err() {
                            error!("Failed to write to editor stdin");
                        }
                    }
                    // code should fail earlier if Kakoune was not spawned
                    // otherwise something went completely wrong, better to panic
                    child.wait().unwrap();
                }
                Err(e) => error!("Failed to run Kakoune: {}", e),
            }
        }
    });

    (writer_tx, reader_rx)
}
