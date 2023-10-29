#![allow(clippy::unused_unit)]

#[macro_use]
extern crate enum_primitive;
#[macro_use]
extern crate serde_derive;
extern crate slog;
#[macro_use]
extern crate slog_scope;

mod capabilities;
mod context;
mod controller;
mod diagnostics;
mod editor_transport;
mod language_features;
mod language_server_transport;
mod markup;
mod position;
mod progress;
mod project_root;
mod session;
mod settings;
mod show_message;
mod text_edit;
mod text_sync;
mod thread_worker;
mod types;
mod util;
mod wcwidth;
mod workspace;

use crate::types::*;
use crate::util::*;
use clap::ArgMatches;
use clap::{crate_version, App, Arg, ArgAction};
use context::meta_for_session;
use daemonize::Daemonize;
use editor_transport::send_command_to_editor;
use fs4::FileExt;
use itertools::Itertools;
use sloggers::file::FileLoggerBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;
use std::env;
use std::ffi::CString;
use std::fs;
use std::io::stderr;
use std::io::stdout;
use std::io::{stdin, Read, Write};
use std::os::unix::net::UnixStream;
use std::panic;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::thread;
use std::time::Duration;

fn main() {
    {
        let locale = CString::new("").unwrap();
        unsafe { libc::setlocale(libc::LC_ALL, locale.as_ptr()) };
    }
    let matches = App::new("kak-lsp")
        .version(crate_version!())
        .author("Ruslan Prokopchuk <fer.obbee@gmail.com>")
        .about("Kakoune Language Server Protocol Client")
        .arg(
            Arg::new("kakoune")
                .long("kakoune")
                .help("Generate commands for Kakoune to plug in kak-lsp"),
        )
        .arg(
            Arg::new("request")
                .long("request")
                .help("Forward stdin to kak-lsp server"),
        )
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Read config from FILE")
                .takes_value(true),
        )
        .arg(
            Arg::new("daemonize")
                .short('d')
                .long("daemonize")
                .help("Daemonize kak-lsp process (server only)"),
        )
        .arg(
            Arg::new("session")
                .short('s')
                .long("session")
                .value_name("SESSION")
                .help("Session id to communicate via unix socket")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::new("timeout")
                .short('t')
                .long("timeout")
                .value_name("TIMEOUT")
                .help("Session timeout in seconds (default 1800)")
                .takes_value(true),
        )
        .arg(
            Arg::new("initial-request")
                .long("initial-request")
                .help("Read initial request from stdin"),
        )
        .arg(
            Arg::new("v")
                .short('v')
                .action(ArgAction::Count)
                .help("Sets the level of verbosity (use up to 4 times)"),
        )
        .arg(
            Arg::new("log")
                .long("log")
                .value_name("PATH")
                .help("File to write the log into instead of stderr")
                .takes_value(true),
        )
        .get_matches();

    if matches.is_present("kakoune") {
        return kakoune();
    }

    let mut config = include_str!("../kak-lsp.toml").to_string();

    let try_config_dir = |config_dir: Option<PathBuf>| {
        let config_dir = match config_dir {
            Some(c) => c,
            None => return None,
        };
        let path = config_dir.join("kak-lsp/kak-lsp.toml");
        if path.exists() {
            Some(path)
        } else {
            None
        }
    };

    let config_path = matches
        .value_of("config")
        .map(|config| Path::new(&config).to_owned())
        .or_else(|| {
            try_config_dir(
                env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .or_else(|| dirs::home_dir().map(|h| h.join(".config"))),
            )
        })
        .or_else(|| try_config_dir(dirs::config_dir()));

    if let Some(config_path) = config_path {
        config = fs::read_to_string(config_path).expect("Failed to read config");
    }

    let session = String::from(matches.value_of("session").unwrap());

    let mut config: Config = match toml::from_str(&config)
        .map_err(|err| err.to_string())
        .and_then(|mut cfg: Config| {
            // Translate legacy config.
            if !cfg.language.is_empty()
                && (!cfg.language_server.is_empty() || !cfg.language_ids.is_empty())
            {
                return Err(
                    "incompatible options: language_server/language_id and legacy language"
                        .to_string(),
                );
            }
            if cfg.language_server.is_empty() {
                for (language_id, language) in cfg.language.drain() {
                    for filetype in &language.filetypes {
                        if filetype != &language_id {
                            cfg.language_ids
                                .insert(filetype.clone(), language_id.clone());
                        }
                    }
                    cfg.language_server
                        .insert(language.command.clone(), language);
                }
            }
            Ok(cfg)
        }) {
        Ok(cfg) => cfg,
        Err(err) => {
            let command = format!(
                "lsp-show-error {}",
                editor_quote(&format!("failed to parse config file: {}", err)),
            );
            if let Err(err) = send_command_to_editor(EditorResponse {
                meta: meta_for_session(session, None),
                command: command.into(),
            }) {
                println!("Failed to send lsp-show-error command to editor: {}", err);
            }
            panic!("invalid configuration: {}", err)
        }
    };

    config.server.session = session;

    if let Some(timeout) = matches.value_of("timeout") {
        config.server.timeout = timeout.parse().unwrap();
    }

    let mut input = Vec::new();
    if matches.is_present("request") || matches.is_present("initial-request") {
        stdin()
            .read_to_end(&mut input)
            .expect("Failed to read stdin");
    }
    if matches.is_present("request") {
        let mut path = util::temp_dir();
        path.push(&config.server.session);
        let connect = || match UnixStream::connect(&path) {
            Ok(mut stream) => {
                stream
                    .write_all(&input)
                    .expect("Failed to send stdin to server");
                true
            }
            _ => false,
        };
        if connect() {
            return;
        }
        let mut lockfile_path = util::temp_dir();
        lockfile_path.push(format!("{}.lock", config.server.session));
        let lockfile = match fs::File::create(&lockfile_path) {
            Ok(lockfile) => lockfile,
            Err(err) => {
                println!("Failed to create lock file: {:?}", err);
                goodbye(&config.server.session, 1)
            }
        };
        if lockfile.try_lock_exclusive().is_ok() {
            spin_up_server(&input);
            if let Err(err) = lockfile.unlock() {
                println!("Failed to unlock lock file: {:?}", err);
                goodbye(&config.server.session, 1);
            }
            fs::remove_file(&lockfile_path).expect("Failed to remove lock file");
            return;
        }
        for _attempt in 0..10 {
            if connect() {
                return;
            }
            thread::sleep(Duration::from_millis(30));
        }
        println!("Could not launch server or connect to it, giving up after 10 attempts");
        goodbye(&config.server.session, 1);
    } else {
        // It's important to read input before daemonizing even if we don't use it.
        // Otherwise it will be empty.
        let initial_request = if matches.is_present("initial-request") {
            Some(String::from_utf8_lossy(&input).to_string())
        } else {
            None
        };
        if matches.is_present("daemonize") {
            let mut pid_path = util::temp_dir();
            pid_path.push(format!("{}.pid", config.server.session));
            if let Err(e) = Daemonize::new()
                .pid_file(&pid_path)
                .working_directory(std::env::current_dir().unwrap())
                .start()
            {
                println!("Failed to daemonize process: {:?}", e);
                goodbye(&config.server.session, 1);
            }
        }
        // Setting up the logger after potential daemonization,
        // otherwise it refuses to work properly.
        let (_guard, log_path) = setup_logger(&config, &matches);
        let log_path = Box::leak(log_path);
        let code = session::start(&config, log_path, initial_request);
        goodbye(&config.server.session, code);
    }
}

fn kakoune() {
    let script = include_str!("../rc/lsp.kak");
    let args = env::args()
        .skip(1)
        .filter(|arg| arg != "--kakoune")
        .join(" ");
    let cmd = env::current_exe().unwrap();
    let cmd = cmd.to_str().unwrap();
    let lsp_cmd = format!(
        "set-option global lsp_cmd '{} {}'",
        editor_escape(cmd),
        editor_escape(&args)
    );
    println!("{}\n{}", script, lsp_cmd);
}

fn spin_up_server(input: &[u8]) {
    let args = env::args()
        .filter(|arg| arg != "--request")
        .collect::<Vec<_>>();
    let mut cmd = Command::new(&args[0]);
    let mut child = cmd
        .args(&args[1..])
        .args(["--daemonize", "--initial-request"])
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

fn setup_logger(
    config: &Config,
    matches: &ArgMatches,
) -> (slog_scope::GlobalLoggerGuard, Box<Option<PathBuf>>) {
    let mut verbosity = matches.get_count("v");

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

    let mut log_path = Box::default();
    let logger = if let Some(path) = matches.value_of("log") {
        log_path = Box::new({
            let path = PathBuf::from_str(path).unwrap();
            path.parent().and_then(|path| path.canonicalize().ok())
        });
        let mut builder = FileLoggerBuilder::new(path);
        builder.level(level);
        builder.build().unwrap()
    } else {
        let mut builder = TerminalLoggerBuilder::new();
        builder.level(level);
        builder.destination(Destination::Stderr);
        builder.build().unwrap()
    };

    panic::set_hook(Box::new(|panic_info| {
        error!("panic: {}", panic_info);
    }));

    (slog_scope::set_global_logger(logger), log_path)
}

// Cleanup and gracefully exit
fn goodbye(session: &str, code: i32) -> ! {
    if code == 0 {
        let path = temp_dir();
        let sock_path = path.join(session);
        let pid_path = path.join(format!("{}.pid", session));
        if fs::remove_file(sock_path).is_err() {
            warn!("Failed to remove socket file");
        };
        if pid_path.exists() && fs::remove_file(pid_path).is_err() {
            warn!("Failed to remove pid file");
        };
    }
    stderr().flush().unwrap();
    stdout().flush().unwrap();
    // give stdio a chance to actually flush
    thread::sleep(Duration::from_secs(1));
    process::exit(code);
}
