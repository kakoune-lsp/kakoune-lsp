use crossbeam_channel::{bounded, Receiver, Sender};
use fnv::FnvHashMap;
use project_root::find_project_root;
use serde_json;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use types::*;

fn get_language_id(extensions: &FnvHashMap<String, String>, path: &str) -> Option<String> {
    extensions
        .get(Path::new(path).extension()?.to_str()?)
        .cloned()
}

pub fn start(config: &Config) -> (Sender<EditorResponse>, Receiver<RoutedEditorRequest>) {
    let mut extensions = FnvHashMap::default();
    for (language_id, language) in &config.language {
        for extension in &language.extensions {
            extensions.insert(extension.clone(), language_id.clone());
        }
    }
    let port = config.server.port;
    let ip = config.server.ip.parse().expect("Failed to parse IP");
    // NOTE 1024 is arbitrary
    let (reader_tx, reader_rx) = bounded(1024);
    let languages = config.language.clone();
    thread::spawn(move || {
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
            let language_id =
                get_language_id(&extensions, &buffile).expect("Failed to recognize language");
            let root_path =
                find_project_root(&languages.get(&language_id).unwrap().roots, &buffile)
                    .expect("File must reside in project");
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
