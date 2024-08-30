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
use clap::{self, crate_version, Arg, ArgAction};
use context::meta_for_session;
use daemonize::Daemonize;
use editor_transport::send_command_to_editor;
use fs4::FileExt;
use itertools::Itertools;
use libc::STDOUT_FILENO;
use sloggers::file::FileLoggerBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;
use std::env;
use std::ffi::CString;
use std::fs;
use std::io;
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
    let matches = clap::Command::new("kak-lsp")
        .version(crate_version!())
        .author("Ruslan Prokopchuk <fer.obbee@gmail.com>")
        .about("Kakoune Language Server Protocol Client")
        .arg(
            Arg::new("kakoune")
                .long("kakoune")
                .action(ArgAction::SetTrue)
                .help("Generate commands for Kakoune to plug in kak-lsp"),
        )
        .arg(
            Arg::new("request")
                .long("request")
                .action(ArgAction::SetTrue)
                .help("Forward stdin to kak-lsp server"),
        )
        .arg(
            Arg::new("config")
                .hide(true)
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Read config from FILE"),
        )
        .arg(
            Arg::new("daemonize")
                .short('d')
                .long("daemonize")
                .action(ArgAction::SetTrue)
                .help("Daemonize kak-lsp process (server only)"),
        )
        .arg(
            Arg::new("session")
                .short('s')
                .long("session")
                .value_name("SESSION")
                .required_unless_present("kakoune")
                .help("Session id to communicate via unix socket"),
        )
        .arg(
            Arg::new("initial-request")
                .hide(true)
                .long("initial-request")
                .action(ArgAction::SetTrue)
                .help("Read initial request from stdin"),
        )
        .arg(
            Arg::new("log")
                .long("log")
                .value_name("PATH")
                .help("File to write the log into instead of stderr"),
        )
        .arg(
            clap::Arg::new("snippet-support")
                .long("snippet-support")
                .value_parser(clap::value_parser!(bool))
                .default_value("true")
                .help("Snippet support (completions with placeholders)")
                .hide(true),
        )
        .arg(
            Arg::new("file-watch-support")
                .long("file-watch-support")
                .value_parser(clap::value_parser!(bool))
                .default_value("false")
                .help("File watcher support")
                .hide(true),
        )
        .arg(
            Arg::new("v")
                .short('v')
                .action(ArgAction::Count)
                .help("Sets the level of verbosity (use up to 4 times)"),
        )
        .arg(
            Arg::new("timeout")
                .short('t')
                .long("timeout")
                .value_name("TIMEOUT")
                .help("Session timeout in seconds (default is 1800 seconds)"),
        )
        .get_matches();

    if matches.get_flag("kakoune") {
        process::exit(kakoune());
    }

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
        .get_one::<String>("config")
        .map(|config| Path::new(&config).to_owned())
        .or_else(|| {
            try_config_dir(
                env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .or_else(|| dirs::home_dir().map(|h| h.join(".config"))),
            )
        })
        .or_else(|| try_config_dir(dirs::config_dir())) // Historical value on macOS.
        .or_else(|| try_config_dir(dirs::preference_dir())) // Historical config dir on macOS.
        ;

    let session = String::from(matches.get_one::<String>("session").unwrap());

    let mut raw_request = Vec::new();
    if matches.get_flag("request") || matches.get_flag("initial-request") {
        stdin()
            .read_to_end(&mut raw_request)
            .expect("Failed to read stdin");
    }

    let mut verbosity;
    #[allow(deprecated)]
    let mut config = if let Some(config_path) = config_path {
        let mut config = parse_legacy_config(&config_path, &raw_request, &session);
        config.server.session = session;
        verbosity = config.verbosity;
        config
    } else {
        let mut config = Config::default();
        config.server.session = session;
        config.server.timeout = 1800;
        config.snippet_support = *matches.get_one::<bool>("snippet-support").unwrap();
        config.file_watch_support = *matches.get_one::<bool>("file-watch-support").unwrap();
        verbosity = 2;
        config
    };

    let vs = matches.get_count("v");
    if vs != 0 {
        verbosity = vs;
    }

    if let Some(timeout) = matches.get_one::<String>("timeout").map(|s| {
        s.parse().unwrap_or_else(|err| {
            report_config_error(
                &config.server.session,
                &raw_request,
                format!("failed to parse --timeout parameter: {err}"),
            )
        })
    }) {
        config.server.timeout = timeout;
    }

    if matches.get_flag("request") {
        let mut path = util::temp_dir();
        path.push(&config.server.session);
        let connect = || match UnixStream::connect(&path) {
            Ok(mut stream) => {
                stream
                    .write_all(&raw_request)
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
            spin_up_server(&raw_request);
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
        let initial_request = if matches.get_flag("initial-request") {
            Some(String::from_utf8_lossy(&raw_request).to_string())
        } else {
            None
        };
        if matches.get_flag("daemonize") {
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
        let (_guard, log_path) = setup_logger(&matches, verbosity);
        let log_path = Box::leak(log_path);
        let code = session::start(&config, log_path, initial_request);
        goodbye(&config.server.session, code);
    }
}

fn kakoune() -> i32 {
    let script = include_str!("../rc/lsp.kak");
    let args = env::args()
        .skip(1)
        .filter(|arg| arg != "--kakoune")
        .join(" ");
    let lsp_cmd = if args.is_empty() {
        "".to_string()
    } else {
        let cmd = env::current_exe().unwrap();
        let cmd = cmd.to_str().unwrap();
        format!(
            "set-option global lsp_cmd '{} {}'\n",
            editor_escape(cmd),
            editor_escape(&args)
        )
    };
    if unsafe { libc::isatty(STDOUT_FILENO) } == 0 {
        println!("{}{}", script, lsp_cmd);
        return 0;
    }
    let pager = env::var_os("PAGER").unwrap_or("less".into());
    let mut child = match process::Command::new(&pager).stdin(Stdio::piped()).spawn() {
        Ok(child) => child,
        Err(err) => {
            println!("failed to run pager {}: {}", pager.to_string_lossy(), err);
            return 1;
        }
    };
    match write!(child.stdin.as_mut().unwrap(), "{}{}", script, lsp_cmd) {
        Ok(()) => (),
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => (),
        Err(err) => {
            println!(
                "failed to run write to pager {}: {}",
                pager.to_string_lossy(),
                err
            );
            panic!();
        }
    };
    if let Err(err) = child.wait() {
        println!(
            "failed to wait for pager {}: {}",
            pager.to_string_lossy(),
            err
        );
        return 1;
    }
    0
}

fn report_config_error(session: &str, raw_request: &[u8], error_message: String) -> ! {
    let editor_request: Option<EditorRequest> =
        toml::from_str(&String::from_utf8_lossy(raw_request)).ok();
    let command = format!("lsp-show-error {}", &editor_quote(&error_message));
    if let Err(err) = send_command_to_editor(EditorResponse {
        meta: meta_for_session(
            session.to_string(),
            editor_request.and_then(|req| req.meta.client),
        ),
        command: command.into(),
    }) {
        println!("Failed to send lsp-show-error command to editor: {}", err);
    }
    println!("{}", error_message);
    process::exit(1);
}

fn parse_legacy_config(config_path: &PathBuf, raw_request: &[u8], session: &str) -> Config {
    let raw_config = fs::read_to_string(config_path).expect("Failed to read config");
    #[allow(deprecated)]
    #[allow(clippy::blocks_in_conditions)]
    match toml::from_str(&raw_config)
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
                    cfg.language_server.insert(
                        format!(
                            "{}:{}",
                            language_id,
                            language.command.as_ref().unwrap_or(&"".to_string())
                        ),
                        language,
                    );
                }
            }
            Ok(cfg)
        }) {
        Ok(cfg) => cfg,
        Err(err) => report_config_error(
            session,
            raw_request,
            format!(
                "failed to parse config file {}: {}",
                config_path.display(),
                err
            ),
        ),
    }
}

fn spin_up_server(raw_request: &[u8]) {
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
        .write_all(raw_request)
        .expect("Failed to write initial request");
    child.wait().expect("Failed to daemonize server");
}

fn setup_logger(
    matches: &ArgMatches,
    verbosity: u8,
) -> (slog_scope::GlobalLoggerGuard, Box<Option<PathBuf>>) {
    let level = match verbosity {
        0 => Severity::Error,
        1 => Severity::Warning,
        2 => Severity::Info,
        3 => Severity::Debug,
        _ => Severity::Trace,
    };

    let mut log_path = Box::default();
    let logger = if let Some(path) = matches.get_one::<String>("log") {
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
