#![feature(getpid)]

extern crate clap;
extern crate crossbeam_channel;
extern crate fnv;
extern crate jsonrpc_core;
extern crate languageserver_types;
extern crate regex;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate serde;
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

use clap::App;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use types::*;

fn main() {
    let matches = App::new("kak-lsp")
        .version("1.0")
        .author("Ruslan Prokopchuk <fer.obbee@gmail.com>")
        .about("Kakoune Language Server Protocol Client")
        .arg_from_usage(
            "-c, --config=[FILE] 'Read config from FILE (default $HOME/.config/kak-lsp/kak-lsp.toml)'
             -p, --port=[PORT]   'Port to listen for commands from Kakoune (default 31337)'
             --ip=[ADDR]         'Address to listen for commands from Kakoune (default 127.0.0.1)'
             ",
        )
        .get_matches();

    let config_path = matches
        .value_of("config")
        .and_then(|config| Some(Path::new(&config).to_owned()))
        .or_else(|| {
            std::env::home_dir().and_then(|home| {
                Some(Path::new(&home.join(".config/kak-lsp/kak-lsp.toml")).to_owned())
            })
        })
        .expect("Config path is not present in options and home directory is not available");

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

    controller::start(&config);
}
