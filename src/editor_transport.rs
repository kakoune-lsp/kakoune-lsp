use crossbeam_channel::{bounded, Receiver, Sender};
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path;
use std::process::{Command, Stdio};
use std::thread;
use toml;
use types::*;
use util::*;

pub struct EditorTransport {
    pub sender: Sender<EditorResponse>,
    pub receiver: Receiver<EditorRequest>,
    pub thread: thread::JoinHandle<()>,
}

pub fn start(config: &Config, initial_request: Option<String>) -> Result<EditorTransport, i32> {
    // NOTE 1024 is arbitrary
    let (reader_tx, reader_rx) = bounded(1024);

    if let Some(initial_request) = initial_request {
        let initial_request: EditorRequest =
            toml::from_str(&initial_request).expect("Failed to parse initial request");
        reader_tx.send(initial_request);
    }

    if let Some(ref session) = config.server.session {
        let session = session.to_string();
        let mut path = temp_dir();
        path.push(&session);
        if path.exists() {
            if UnixStream::connect(&path).is_err() {
                if fs::remove_file(&path).is_err() {
                    error!(
                        "Failed to clean up dead session at {}",
                        path.to_str().unwrap()
                    );
                    return Err(1);
                };
            } else {
                error!("Server is already running for session {}", session);
                return Err(1);
            }
        }
        thread::spawn(move || start_unix(&path, &reader_tx))
    } else {
        let port = config.server.port;
        let ip = config.server.ip.parse().expect("Failed to parse IP");
        thread::spawn(move || start_tcp(ip, port, &reader_tx))
    };

    // NOTE 1024 is arbitrary
    let (writer_tx, writer_rx): (Sender<EditorResponse>, Receiver<EditorResponse>) = bounded(1024);
    let writer_thread = thread::spawn(move || {
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
                        let client = response.meta.client.as_ref();
                        let command = if client.is_some() && !client.unwrap().is_empty() {
                            format!(
                                "eval -client {} {}",
                                client.unwrap(),
                                editor_quote(&response.command)
                            )
                        } else {
                            response.command.to_string()
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

    Ok(EditorTransport {
        sender: writer_tx,
        receiver: reader_rx,
        // not joining reader thread because it's unclear how to stop it
        thread: writer_thread,
    })
}

pub fn start_tcp(ip: IpAddr, port: u16, reader_tx: &Sender<EditorRequest>) {
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

pub fn start_unix(path: &path::PathBuf, reader_tx: &Sender<EditorRequest>) {
    let listener = UnixListener::bind(&path);

    if listener.is_err() {
        error!("Failed to bind {}", path.to_str().unwrap());
        return;
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
