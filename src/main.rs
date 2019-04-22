#[macro_use]
extern crate enum_primitive;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate slog_scope;

mod context;
mod controller;
mod diagnostics;
mod editor_transport;
mod general;
mod language_features;
mod language_server_transport;
mod position;
mod project_root;
mod session;
mod text_edit;
mod text_sync;
mod types;
mod util;
mod workspace;

use crate::types::*;
use crate::util::*;
use clap::{crate_version, App, Arg};
use daemonize::Daemonize;
use itertools::Itertools;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;
use std::env;
use std::fs;
use std::io::{stdin, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};

fn main() {
    let matches = App::new("kak-lsp")
        .version(crate_version!())
        .author("Ruslan Prokopchuk <fer.obbee@gmail.com>")
        .about("Kakoune Language Server Protocol Client")
        .arg(
            Arg::with_name("kakoune")
                .long("kakoune")
                .help("Generate commands for Kakoune to plug in kak-lsp"),
        )
        .arg(
            Arg::with_name("request")
                .long("request")
                .help("Forward stdin to kak-lsp server"),
        )
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("Read config from FILE (default $HOME/.config/kak-lsp/kak-lsp.toml)")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("daemonize")
                .short("d")
                .long("daemonize")
                .help("Daemonize kak-lsp process (server only)"),
        )
        .arg(
            Arg::with_name("session")
                .short("s")
                .long("session")
                .value_name("SESSION")
                .help("Session id to communicate via unix socket instead of tcp")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("timeout")
                .short("t")
                .long("timeout")
                .value_name("TIMEOUT")
                .help("Session timeout in seconds (default 1800)")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .value_name("PORT")
                .help("Port to listen for commands from Kakoune (default 31337)")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("ip")
                .long("ip")
                .value_name("ADDR")
                .help("Address to listen for commands from Kakoune (default 127.0.0.1)")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("initial-request")
                .long("initial-request")
                .help("Read initial request from stdin"),
        )
        .arg(
            Arg::with_name("v")
                .short("v")
                .multiple(true)
                .help("Sets the level of verbosity"),
        )
        .get_matches();

    let mut config = include_str!("../kak-lsp.toml").to_string();

    let config_path = matches
        .value_of("config")
        .and_then(|config| Some(Path::new(&config).to_owned()))
        .or_else(|| {
            dirs::home_dir().and_then(|home| {
                let path = Path::new(&home.join(".config/kak-lsp/kak-lsp.toml")).to_owned();
                if path.exists() {
                    Some(path)
                } else {
                    None
                }
            })
        });

    if let Some(config_path) = config_path {
        config = fs::read_to_string(config_path).expect("Failed to read config");
    }

    let mut config: Config = toml::from_str(&config).expect("Failed to parse config file");

    if let Some(port) = matches.value_of("port") {
        config.server.port = port.parse().unwrap();
    }

    if let Some(ip) = matches.value_of("ip") {
        config.server.ip = ip.to_string();
    }

    if let Some(session) = matches.value_of("session") {
        config.server.session = Some(session.to_string());
    }

    if let Some(timeout) = matches.value_of("timeout") {
        config.server.timeout = timeout.parse().unwrap();
    }

    let mut verbosity = matches.occurrences_of("v") as u8;

    if verbosity == 0 {
        verbosity = config.verbosity
    }

    let level = match verbosity {
        0 => Severity::Error,
        1 => Severity::Warning,
        2 => Severity::Info,
        3 => Severity::Debug,
        _ => Severity::Trace,
    };

    let mut builder = TerminalLoggerBuilder::new();
    builder.level(level);
    builder.destination(Destination::Stderr);
    let logger = builder.build().unwrap();
    let _guard = slog_scope::set_global_logger(logger);

    if matches.is_present("request") {
        request(&config);
    } else if matches.is_present("kakoune") {
        kakoune(&config);
    } else {
        // It's important to read input before daemonizing even if we don't use it.
        // Otherwise it will be empty.
        let initial_request = if matches.is_present("initial-request") {
            let mut input = Vec::new();
            stdin()
                .read_to_end(&mut input)
                .expect("Failed to read stdin");
            Some(String::from_utf8_lossy(&input).to_string())
        } else {
            None
        };
        let session = config.server.session.as_ref().unwrap_or(&config.server.ip);
        let mut pid_path = util::temp_dir();
        pid_path.push(format!("{}.pid", session));
        if matches.is_present("daemonize")
            && Daemonize::new()
                .pid_file(&pid_path)
                .working_directory(std::env::current_dir().unwrap())
                .start()
                .is_err()
        {
            error!("Failed to daemonize process");
            goodbye(&config, 1);
        } else {
            let code = session::start(&config, initial_request);
            goodbye(&config, code);
        }
    }
}

fn kakoune(_config: &Config) {
    let script: &str = include_str!("../rc/lsp.kak");
    let args = env::args()
        .skip(1)
        .filter(|arg| arg != "--kakoune")
        .join(" ");
    let cmd = env::current_exe().unwrap();
    let cmd = cmd.to_str().unwrap();
    let lsp_cmd = format!(
        "set global lsp_cmd '{} {}'",
        editor_escape(cmd),
        editor_escape(&args)
    );
    println!("{}\n{}", script, lsp_cmd);
}

fn request(config: &Config) {
    let mut input = Vec::new();
    stdin()
        .read_to_end(&mut input)
        .expect("Failed to read stdin");
    if let Some(ref session) = config.server.session {
        let mut path = util::temp_dir();
        path.push(session);
        if let Ok(mut stream) = UnixStream::connect(&path) {
            stream
                .write_all(&input)
                .expect("Failed to send stdin to server");
        } else {
            spin_up_server(&input);
        }
    } else {
        let port = config.server.port;
        let ip = config.server.ip.parse().expect("Failed to parse IP");
        let addr = SocketAddr::new(ip, port);
        if let Ok(mut stream) = TcpStream::connect(addr) {
            stream
                .write_all(&input)
                .expect("Failed to send stdin to server");
        } else {
            spin_up_server(&input);
        }
    }
}

fn spin_up_server(input: &[u8]) {
    let args = env::args()
        .filter(|arg| arg != "--request")
        .collect::<Vec<_>>();
    let mut cmd = Command::new(&args[0]);
    let mut child = cmd
        .args(&args[1..])
        .args(&["--daemonize", "--initial-request"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("Failed to run server");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input)
        .expect("Failed to write initial request");
    child.wait().expect("Failed to daemonize server");
}
