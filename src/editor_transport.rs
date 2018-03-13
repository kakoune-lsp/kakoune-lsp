use crossbeam_channel::{bounded, Receiver, Sender};
use project_root::find_project_root;
use serde_json;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use types::*;

fn get_language_id(path: &str) -> Option<String> {
    match Path::new(path).extension()?.to_str()? {
        "rs" => Some("rust".to_string()),
        "js" => Some("javascript".to_string()),
        _ => None,
    }
}

pub fn start() -> (Sender<EditorResponse>, Receiver<RoutedEditorRequest>) {
    // NOTE 1024 is arbitrary
    let (reader_tx, reader_rx) = bounded(1024);
    thread::spawn(move || {
        // TODO configurable
        let ip = "127.0.0.1".parse().unwrap();
        // TODO configurable
        let port = 31_337;
        let addr = SocketAddr::new(ip, port);

        let listener = TcpListener::bind(&addr).expect("Failed to start TCP server");

        for stream in listener.incoming() {
            let mut stream = stream.expect("Failed to connect to TCP stream");
            let mut request = String::new();
            stream
                .read_to_string(&mut request)
                .expect("Failed to read from TCP stream");
            let request: EditorRequest =
                serde_json::from_str(&request).expect("Failed to parse editor request");
            let session = request.meta.session.clone();
            let buffile = request.meta.buffile.clone();
            let language_id = get_language_id(&buffile).expect("Failed to recognize language");
            let root_path = find_project_root(&buffile).expect("File must reside in project");
            let route = (session, language_id, root_path);
            let routed_request = RoutedEditorRequest { request, route };
            reader_tx
                .send(routed_request)
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
            let stdin = child.stdin.as_mut().expect("Failed to get editor stdin");
            // NOTE fingers crossed no 🦀 will appear in response.command
            write!(
                stdin,
                "eval -client {} %🦀{}🦀",
                response.meta.client, response.command
            ).expect("Failed to write to editor stdin");
        }
    });

    (writer_tx, reader_rx)
}
