use crossbeam_channel::{after, bounded, Receiver, Sender};
use std::fs;
use std::io::{stderr, stdout, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{exit, Command, Stdio};
use std::thread;
use std::time::Duration;
use toml;
use types::*;
use util;

pub fn start(config: &Config) -> (Sender<EditorResponse>, Receiver<EditorRequest>) {
    // NOTE 1024 is arbitrary
    let (reader_tx, reader_rx) = bounded(1024);

    if let Some(ref session) = config.server.session {
        let session = session.to_string();
        let timeout = config.server.timeout;

        if timeout > 0 {
            let (timeout_tx, timeout_rx) = bounded(1);
            thread::spawn(move || {
                let timeout = Duration::from_secs(timeout);
                loop {
                    select! {
                        recv(timeout_rx) => {}
                        recv(after(timeout)) => {
                            info!("Exiting by timeout");
                            stderr().flush().unwrap();
                            stdout().flush().unwrap();
                            thread::sleep(Duration::from_secs(1));
                            // TODO clean exit
                            exit(0);
                        }
                    }
                }
            });
            thread::spawn(move || start_unix(session, reader_tx, Some(timeout_tx)));
        } else {
            thread::spawn(move || start_unix(session, reader_tx, None));
        }
    } else {
        let port = config.server.port;
        let ip = config.server.ip.parse().expect("Failed to parse IP");
        thread::spawn(move || start_tcp(ip, port, reader_tx));
    }

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

pub fn start_tcp(ip: IpAddr, port: u16, reader_tx: Sender<EditorRequest>) {
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
                        reader_tx.send(request);
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
}

pub fn start_unix(
    session: String,
    reader_tx: Sender<EditorRequest>,
    timeout_tx: Option<Sender<()>>,
) {
    let mut path = util::sock_dir();
    path.push(&session);

    if path.exists() {
        if UnixStream::connect(&path).is_err() {
            if fs::remove_file(&path).is_err() {
                error!(
                    "Failed to clean up dead session at {}",
                    path.to_str().unwrap()
                );
                stderr().flush().unwrap();
                stdout().flush().unwrap();
                thread::sleep(Duration::from_secs(1));
                exit(1);
            };
        } else {
            error!("Server is already running for session {}", session);
            stderr().flush().unwrap();
            stdout().flush().unwrap();
            thread::sleep(Duration::from_secs(1));
            exit(1);
        }
    }

    let listener = UnixListener::bind(&path);

    if listener.is_err() {
        error!("Failed to bind {}", path.to_str().unwrap());
        stderr().flush().unwrap();
        stdout().flush().unwrap();
        thread::sleep(Duration::from_secs(1));
        exit(1);
    }

    let listener = listener.unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let mut request = String::new();
                match stream.read_to_string(&mut request) {
                    Ok(_) => {
                        if request.is_empty() {
                            continue;
                        }
                        debug!("From editor: {}", request);
                        let request: EditorRequest =
                            toml::from_str(&request).expect("Failed to parse editor request");
                        reader_tx.send(request);
                        if let Some(ref timeout_tx) = timeout_tx {
                            timeout_tx.send(());
                        }
                    }
                    Err(e) => {
                        error!("Failed to read from stream: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Failed to accept connection: {}", e);
            }
        }
    }
}
