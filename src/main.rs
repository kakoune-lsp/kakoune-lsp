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

mod types;
mod project_root;
mod language_server_transport;
mod editor_transport;
mod controller;

use clap::App;
use fnv::FnvHashMap;
use std::io::{BufReader, Read};
use std::fs::File;
use std::path::Path;
use types::*;

fn main() {
    let matches = App::new("kak-lsp")
        .version("1.0")
        .author("Ruslan Prokopchuk <fer.obbee@gmail.com>")
        .about("Kakoune Language Server Protocol client")
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
        .expect("Config path is not present in options and home directory os not available");

    let mut config_file =
        BufReader::new(File::open(config_path).expect("Failed to open config file"));

    let mut config = String::new();

    config_file
        .read_to_string(&mut config)
        .expect("Failed to read config");

    let config: FileConfig = toml::from_str(&config).expect("Failed to parse config file");

    let mut language = FnvHashMap::default();

    for (language_id, lang) in config.language {
        language.insert(
            language_id,
            LanguageConfig {
                extensions: lang.extensions,
                command: lang.command,
                args: lang.args.unwrap_or_else(Vec::new),
            },
        );
    }

    let config = Config {
        server: ServerConfig {
            port: matches
                .value_of("port")
                .and_then(|x| x.parse().ok())
                .or(config.server.as_ref().and_then(|x| x.port))
                .unwrap_or(31_337),
            ip: matches
                .value_of("ip")
                .and_then(|x| Some(x.to_string()))
                .or(config.server.and_then(|x| x.ip))
                .unwrap_or_else(|| "127.0.0.1".to_string()),
        },
        language,
    };

    controller::start(&config);
}
