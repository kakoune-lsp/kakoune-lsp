#![allow(clippy::unused_unit)]

#[macro_use]
extern crate enum_primitive;
#[macro_use]
extern crate serde_derive;

#[macro_use]
pub mod log;

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
use itertools::Itertools;
use libc::O_NONBLOCK;
use libc::O_RDONLY;
use libc::SA_RESTART;
use libc::SIGCHLD;
use libc::SIGHUP;
use libc::SIGINT;
use libc::SIGQUIT;
use libc::SIGTERM;
use libc::STDOUT_FILENO;
use log::DEBUG;
use sloggers::file::FileLoggerBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;
use std::borrow::Cow;
use std::cell::OnceCell;
use std::env;
use std::ffi::CString;
use std::fs;
use std::io::ErrorKind;
use std::mem;
use std::panic;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::str::FromStr;
use std::sync::atomic::{
    AtomicBool,
    Ordering::{AcqRel, Acquire, Relaxed},
};
use std::sync::Mutex;

static RECEIVED_SIGCHLD: AtomicBool = AtomicBool::new(false);
static CLEANUP: Mutex<OnceCell<Box<dyn FnOnce() + Send>>> = Mutex::new(OnceCell::new());
static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

static SESSION: Mutex<SessionId> = Mutex::new(SessionId(String::new()));
static LAST_CLIENT: Mutex<Option<String>> = Mutex::new(None);

fn main() -> Result<(), ()> {
    {
        let locale = CString::new("").unwrap();
        unsafe { libc::setlocale(libc::LC_ALL, locale.as_ptr()) };
    }
    let matches = clap::Command::new("kak-lsp")
        .version(crate_version!())
        .author("Ruslan Prokopchuk <fer.obbee@gmail.com>")
        .about("Kakoune Language Server Protocol Client")
        .after_help(concat!(
            "Unless --session is given, print commands to plug into a Kakoune session",
        ))
        .arg(
            Arg::new("kakoune")
                .hide(true)
                .long("kakoune")
                .action(ArgAction::SetTrue)
                .help("Generate commands for Kakoune to plug in kak-lsp"),
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
                .hide(true)
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
                .help("Name of the Kakoune session to talk to (defaults to $kak_session)"),
        )
        .arg(
            Arg::new("timeout")
                .hide(true)
                .short('t')
                .long("timeout")
                .value_name("TIMEOUT")
                .help("Session timeout in seconds (default is 18000 seconds)"),
        )
        .arg(
            Arg::new("v")
                .hide(true)
                .short('v')
                .action(ArgAction::Count)
                .help("Sets the level of verbosity (use up to 4 times)"),
        )
        .arg(
            Arg::new("debug")
                .long("debug")
                .action(ArgAction::SetTrue)
                .help("Enable debug logging (see the 'lsp_debug' option)"),
        )
        .arg(
            Arg::new("log")
                .hide(true)
                .long("log")
                .value_name("PATH")
                .help("File to write the log into, in addition to the *debug* buffer"),
        )
        .get_matches();

    let session = env_var("kak_session").or_else(|| matches.get_one::<String>("session").cloned());
    if matches.get_flag("kakoune") || session.is_none() {
        kakoune();
        process::exit(0);
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

    let Some(session) = session else {
        eprintln!("Error: no session name given, please export '$kak_session'");
        goodbye(1);
    };
    let session = SessionId(session);
    *SESSION.lock().unwrap() = session.clone();

    let plugin_path = env_var("XDG_RUNTIME_DIR")
        .filter(|dir| unsafe {
            let mut stat = mem::zeroed();
            let dir = CString::new(dir.clone()).unwrap();
            libc::stat(dir.as_ptr(), &mut stat) == 0 && stat.st_uid == libc::geteuid()
        })
        .map(|dir| PathBuf::from(format!("{}/kakoune-lsp", dir)))
        .unwrap_or_else(|| {
            let mut path = env::temp_dir();
            path.push(format!("kakoune-lsp-{}", whoami::username()));
            path
        });
    let mut session_path = plugin_path.clone();
    session_path.push(session.as_str());
    let mut session_directory = SessionDirectory {
        symlink: None,
        fifos: [None, None],
        pid_files: None,
        session_directory: TemporaryDirectory::new(session_path.clone()),
        plugin_directory: TemporaryDirectory::new(plugin_path.clone()),
    };
    if fs::create_dir_all(session_path.clone()).is_err() {
        report_error(
            &session,
            format!(
                "failed to create session directory '{}': {}",
                session_path.display(),
                std::io::Error::last_os_error()
            ),
        );
        return Err(());
    }

    let mut exists = false;

    let mut session_symlink_path = plugin_path;
    session_symlink_path.push(format!("{}.ref", session));
    session_directory.symlink = Some(TemporaryFile::new(session_symlink_path.clone()));
    if let Err(err) = std::os::unix::fs::symlink(session.as_str(), session_symlink_path.clone()) {
        if err.kind() == ErrorKind::AlreadyExists {
            exists = true;
        } else {
            report_error(
                &session,
                format!(
                    "failed to create session directory symlink '{}': {}",
                    session_symlink_path.display(),
                    err,
                ),
            );
            return Err(());
        }
    }

    let mut create_fifo = |offset: usize, name: &str| {
        let mut fifo = session_path.clone();
        fifo.push(name);
        let tmp = TemporaryInputFifo::new(fifo.clone());
        let fifo_cstr = tmp.0 .0;
        session_directory.fifos[offset] = Some(tmp);
        if unsafe { libc::mkfifo(fifo_cstr.as_ptr(), 0o600) } != 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == ErrorKind::AlreadyExists {
                exists = true;
            } else {
                report_error(
                    &session,
                    format!(
                        "failed to create fifo '{}': {}",
                        fifo.display(),
                        std::io::Error::last_os_error()
                    ),
                );
                return Err(());
            }
        }
        Ok(fifo)
    };
    let fifos = [create_fifo(0, "fifo1")?, create_fifo(1, "fifo2")?];

    if exists {
        eprintln!("Server seems to be already running at:");
    }
    println!("{}", session_symlink_path.display());
    unsafe {
        libc::close(STDOUT_FILENO);
    }
    if exists {
        process::exit(0);
    }

    let mut pid_file = session_path.clone();
    pid_file.push("pid");
    let mut pid_file_tmp = session_path;
    pid_file_tmp.push("pid.tmp");
    session_directory.pid_files = Some([
        TemporaryFile::new(pid_file.clone()),
        TemporaryFile::new(pid_file_tmp.clone()),
    ]);

    CLEANUP.lock().unwrap().get_or_init(|| {
        Box::new(move || {
            drop(session_directory);
        })
    });

    for signal in [SIGTERM, SIGHUP, SIGINT, SIGQUIT] {
        unsafe {
            libc::signal(signal, handle_interrupt as libc::sighandler_t);
        }
    }

    let mut verbosity;
    #[allow(deprecated)]
    let mut config = if let Some(config_path) = config_path {
        let config = parse_legacy_config(&config_path, &session);
        verbosity = config.verbosity;
        config
    } else {
        let mut config = Config::default();
        verbosity = 2;
        config.server.timeout = 18000;
        if let Some(timeout) = env_var("kak_opt_lsp_timeout") {
            config.server.timeout = timeout.parse().unwrap_or_else(|err| {
                report_config_error_and_exit(
                    &session,
                    format!("failed to parse lsp_timeout: {err}"),
                )
            });
        }
        if let Some(snippet_support) = env_var("kak_opt_lsp_snippet_support") {
            config.snippet_support = snippet_support != "false";
        }
        if let Some(file_watch_support) = env_var("kak_opt_lsp_file_watch_support") {
            config.file_watch_support = file_watch_support != "false";
        }
        config
    };

    if let Some(debug) = env_var("kak_opt_lsp_debug") {
        if debug != "false" {
            verbosity = 4;
        }
    } else if matches.get_flag("debug") {
        verbosity = 4;
    } else {
        let vs = matches.get_count("v");
        if vs != 0 {
            verbosity = vs;
        }
    }

    if let Some(timeout) = matches.get_one::<String>("timeout").map(|s| {
        s.parse().unwrap_or_else(|err| {
            report_config_error_and_exit(
                &session,
                format!("failed to parse --timeout parameter: {err}"),
            )
        })
    }) {
        config.server.timeout = timeout;
    }

    if matches.get_flag("daemonize") {
        if let Err(e) = Daemonize::new()
            .working_directory(std::env::current_dir().unwrap())
            .start()
        {
            eprintln!("Failed to daemonize process: {:?}", e);
            goodbye(1);
        }
    }

    if let Err(err) = fs::write(pid_file_tmp.clone(), process::id().to_string().as_bytes()) {
        report_config_error_and_exit(
            &session,
            format!(
                "failed to write pid file '{}': {}",
                pid_file_tmp.display(),
                err
            ),
        )
    }
    if let Err(err) = fs::rename(pid_file_tmp.clone(), pid_file.clone()) {
        report_config_error_and_exit(
            &session,
            format!(
                "failed to rename pid file '{}': {}",
                pid_file.display(),
                err
            ),
        )
    }

    unsafe {
        let mut act: libc::sigaction = std::mem::zeroed();
        act.sa_sigaction = handle_sigchld as usize;
        libc::sigemptyset(&mut act.sa_mask);
        act.sa_flags = SA_RESTART;
        libc::sigaction(SIGCHLD, &act, std::ptr::null_mut());
    }

    // Setting up the logger after potential daemonization,
    // otherwise it refuses to work properly.
    let log_path_parent = initialize_logger(&session, &matches, verbosity);
    let code = controller::start(session.clone(), config, log_path_parent, fifos);
    info!(session, "kak-lsp server exiting");
    goodbye(code);
}

fn env_var(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) => Some(value),
        Err(err) => match err {
            env::VarError::NotPresent => None,
            env::VarError::NotUnicode(_bytes) => {
                eprintln!("environment variable '{name}' is not valid UTF-8");
                goodbye(1);
            }
        },
    }
}

fn kakoune() {
    let script = concat!(
        include_str!("../rc/lsp.kak"),
        include_str!("../rc/servers.kak")
    );
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
    println!("{}{}", script, lsp_cmd);
}

fn report_error(session: &SessionId, error_message: String) {
    let command = format!("lsp-show-error {}", &editor_quote(&error_message));
    send_command_to_editor(
        EditorResponse {
            meta: meta_for_session(session.clone(), env_var("kak_client")),
            command: command.into(),
        },
        false,
    );
    eprintln!("{}", error_message);
}

fn report_config_error_and_exit(session: &SessionId, error_message: String) -> ! {
    report_error(session, error_message);
    goodbye(1);
}

fn parse_legacy_config(config_path: &PathBuf, session: &SessionId) -> Config {
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
                for (language_id, language) in &cfg.language {
                    cfg.language_server.insert(
                        format!(
                            "{}:{}",
                            language_id,
                            language.command.as_ref().unwrap_or(&"".to_string())
                        ),
                        language.clone(),
                    );
                }
            }
            Ok(cfg)
        }) {
        Ok(cfg) => cfg,
        Err(err) => report_config_error_and_exit(
            session,
            format!(
                "failed to parse config file {}: {}",
                config_path.display(),
                err
            ),
        ),
    }
}

fn initialize_logger(
    session: &SessionId,
    matches: &ArgMatches,
    verbosity: u8,
) -> &'static Option<PathBuf> {
    let level = match verbosity {
        0 => Severity::Error,
        1 => Severity::Warning,
        2 => Severity::Info,
        3 => Severity::Debug,
        _ => Severity::Trace,
    };
    if verbosity >= 3 {
        DEBUG.store(true, Relaxed);
    }

    let path = matches
        .get_one::<String>("log")
        .map(|path| PathBuf::from_str(path).unwrap());
    let log_path_parent =
        Box::leak(Box::new(path.as_ref().and_then(|path| {
            path.parent().and_then(|parent| parent.canonicalize().ok())
        })));
    *LOG_PATH.lock().unwrap() = path;

    set_logger(level);

    let session = session.clone();
    panic::set_hook(Box::new(move |panic_info| {
        let message = format!(
            "kak-lsp crashed, please report a bug. Find more details in the *debug* buffer.\n{}\n{}",
            panic_info,
            std::backtrace::Backtrace::capture()
        );
        error!(session, "{message}");
        static PANICKING: AtomicBool = AtomicBool::new(false);
        if PANICKING
            .compare_exchange(false, true, AcqRel, Acquire)
            .is_err()
        {
            process::abort();
        }
        let command = format!("lsp-show-error {}", editor_quote(&message));
        let meta = meta_for_session(
            std::mem::take(&mut SESSION.lock().unwrap()),
            std::mem::take(&mut LAST_CLIENT.lock().unwrap()),
        );
        let command = EditorResponse {
            meta,
            command: Cow::Owned(command),
        };
        send_command_to_editor(command, false);
        if let Some(cleanup) = CLEANUP.lock().unwrap().take() {
            (cleanup)();
        }
        process::abort();
    }));

    log_path_parent
}

fn set_logger(level: Severity) {
    let logger = if let Some(path) = LOG_PATH.lock().unwrap().as_ref() {
        let mut builder = FileLoggerBuilder::new(path.clone());
        builder.level(level);
        builder.build().unwrap()
    } else {
        let mut builder = TerminalLoggerBuilder::new();
        builder.level(level);
        builder.destination(Destination::Stderr);
        builder.build().unwrap()
    };
    slog_scope::set_global_logger(logger).cancel_reset();
}

// Cleanup and gracefully exit
fn goodbye(code: i32) -> ! {
    if let Some(cleanup) = CLEANUP.lock().unwrap().take() {
        (cleanup)();
    }
    process::exit(code);
}

extern "C" fn handle_interrupt(_sig: libc::c_int) -> ! {
    goodbye(1)
}

extern "C" fn handle_sigchld(_sig: libc::c_int) {
    RECEIVED_SIGCHLD.store(true, Relaxed);
}

// for async-signal-safe cleanup
fn immortalize_path(path: PathBuf) -> &'static CString {
    Box::leak(Box::new(
        CString::new(path.into_os_string().into_encoded_bytes()).unwrap(),
    ))
}

struct TemporaryFile(&'static CString);
impl TemporaryFile {
    fn new(path: PathBuf) -> Self {
        Self(immortalize_path(path))
    }
}
impl Drop for TemporaryFile {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::unlink(self.0.as_ptr());
        }
    }
}

struct TemporaryInputFifo(TemporaryFile);
impl TemporaryInputFifo {
    fn new(path: PathBuf) -> Self {
        Self(TemporaryFile::new(path))
    }
}
impl Drop for TemporaryInputFifo {
    fn drop(&mut self) {
        unsafe {
            let fd = libc::open(self.0 .0.as_ptr(), O_RDONLY | O_NONBLOCK);
            if fd != -1 {
                let _ = libc::close(fd);
            }
        }
    }
}

struct TemporaryDirectory(&'static CString);
impl TemporaryDirectory {
    fn new(path: PathBuf) -> Self {
        Self(immortalize_path(path))
    }
}
impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::rmdir(self.0.as_ptr());
        }
    }
}

struct SessionDirectory {
    symlink: Option<TemporaryFile>,
    fifos: [Option<TemporaryInputFifo>; 2],
    pid_files: Option<[TemporaryFile; 2]>,
    #[allow(dead_code)]
    session_directory: TemporaryDirectory,
    #[allow(dead_code)]
    plugin_directory: TemporaryDirectory,
}
