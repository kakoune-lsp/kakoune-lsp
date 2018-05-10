#![feature(getpid)]

extern crate clap;
#[macro_use]
extern crate crossbeam_channel;
extern crate fnv;
extern crate jsonrpc_core;
extern crate languageserver_types;
extern crate regex;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate serde;
#[macro_use]
extern crate slog;
extern crate sloggers;
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
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use types::*;

fn main() {
    let matches = App::new("kak-lsp")
        .version("1.0")
        .author("Ruslan Prokopchuk <fer.obbee@gmail.com>")
        .about("Kakoune Language Server Protocol Client")
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

    controller::start(&config, logger);
}
