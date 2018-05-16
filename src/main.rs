#[macro_use]
extern crate clap;
#[macro_use]
extern crate crossbeam_channel;
extern crate fnv;
extern crate glob;
extern crate handlebars;
extern crate jsonrpc_core;
extern crate languageserver_types;
extern crate regex;
#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate serde;
#[macro_use]
extern crate slog;
extern crate sloggers;
#[macro_use]
extern crate slog_scope;
extern crate toml;
extern crate url;
extern crate url_serde;
#[macro_use]
extern crate enum_primitive;

mod context;
mod controller;
mod diagnostics;
mod editor_transport;
mod general;
mod language_features;
mod language_server_transport;
mod project_root;
mod text_sync;
mod types;

use clap::{App, Arg};
use handlebars::{no_escape, Handlebars};
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;
use std::fs::File;
use std::io::{stdin, stdout, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use types::*;

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
            Arg::with_name("v")
                .short("v")
                .multiple(true)
                .help("Sets the level of verbosity"),
        )
        .get_matches();

    let config_path = matches
        .value_of("config")
        .and_then(|config| Some(Path::new(&config).to_owned()))
        .or_else(|| {
            std::env::home_dir().and_then(|home| {
                let path = Path::new(&home.join(".config/kak-lsp/kak-lsp.toml")).to_owned();
                if path.exists() {
                    Some(path)
                } else {
                    None
                }
            })
        })
        .or_else(|| {
            std::env::current_exe()
                .and_then(|p| p.canonicalize())
                .ok()
                .and_then(|p| {
                    p.parent()
                        .and_then(|p| Some(p.join("kak-lsp.toml").to_owned()))
                })
        })
        .unwrap();

    let mut config_file =
        BufReader::new(File::open(config_path).expect("Failed to open config file"));

    let mut config = String::new();

    config_file
        .read_to_string(&mut config)
        .expect("Failed to read config");

    let mut config: Config = toml::from_str(&config).expect("Failed to parse config file");

    if let Some(port) = matches.value_of("port") {
        config.server.port = port.parse().unwrap();
    }

    if let Some(ip) = matches.value_of("ip") {
        config.server.ip = ip.to_string();
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
        controller::start(&config);
    }
}

fn kakoune(config: &Config) {
    let mut handlebars = Handlebars::new();
    handlebars.register_escape_fn(no_escape);
    let template: &str = include_str!("../rc/lsp.kak");
    let args = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--kakoune")
        .collect::<Vec<_>>()
        .join(" ");
    let cmd = std::env::current_exe().unwrap().to_owned();
    handlebars
        .render_template_to_write(
            template,
            &json!({
                "cmd": cmd,
                "args": args,
                "hover": config.editor.hover
            }),
            &mut stdout(),
        )
        .unwrap();
}

fn request(config: &Config) {
    let port = config.server.port;
    let ip = config.server.ip.parse().expect("Failed to parse IP");
    let addr = SocketAddr::new(ip, port);
    let mut stream = TcpStream::connect(addr).expect("Failed to connect to kak-lsp server");
    let mut input = Vec::new();
    stdin()
        .read_to_end(&mut input)
        .expect("Failed to read stdin");
    stream
        .write_all(&input)
        .expect("Failed to send stdin to server");
}
