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
extern crate url;
extern crate url_serde;

mod types;
mod project_root;
mod language_server_transport;
mod editor_transport;
mod controller;

use clap::App;

fn main() {
    // TODO addr, port, language server commands...
    let _matches = App::new("kak-lsp")
        .version("1.0")
        .author("Ruslan Prokopchuk <fer.obbee@gmail.com>")
        .about("Kakoune Language Server Protocol client")
        .get_matches();

    controller::start();
}
