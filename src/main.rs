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
use controller::Tokenizer;
use controller::TokenizerState;
use daemonize::Daemonize;
use editor_transport::exec_fifo;
use editor_transport::show_error;
use indoc::formatdoc;
use itertools::Itertools;
use libc::SIGHUP;
use libc::SIGINT;
use libc::SIGPIPE;
use libc::SIGQUIT;
use libc::SIGTERM;
use libc::STDOUT_FILENO;
use log::DEBUG;
use sentry::integrations::panic::PanicIntegration;
use sloggers::file::FileLoggerBuilder;
use sloggers::null::NullLoggerBuilder;
use sloggers::terminal::{Destination, TerminalLoggerBuilder};
use sloggers::types::Severity;
use sloggers::Build;
use std::backtrace::Backtrace;
use std::cell::OnceCell;
use std::env;
use std::ffi::CString;
use std::fs;
use std::io;
use std::io::stdout;
use std::io::Read;
use std::io::Write;
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::panic;
use std::panic::PanicInfo;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::str::FromStr;
use std::sync::atomic::{
    AtomicBool,
    Ordering::{AcqRel, Acquire, Relaxed},
};
use std::sync::Mutex;

static CLEANUP: Mutex<OnceCell<Box<dyn FnOnce() + Send>>> = Mutex::new(OnceCell::new());
static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static LAST_CLIENT: Mutex<Option<ClientId>> = Mutex::new(None);

fn main() {
    process::exit(i32::from(run_main().is_err()))
}

fn run_main() -> Result<(), ()> {
    {
        let locale = CString::new("").unwrap();
        unsafe { libc::setlocale(libc::LC_ALL, locale.as_ptr()) };
    }

    let mut command = clap::Command::new("kak-lsp")
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
                .help("generate commands for Kakoune to plug in kak-lsp"),
        )
        .arg(
            Arg::new("config")
                .hide(true)
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("read config from FILE"),
        )
        .arg(
            Arg::new("daemonize")
                .hide(true)
                .short('d')
                .long("daemonize")
                .action(ArgAction::SetTrue)
                .help("daemonize kak-lsp process (server only)"),
        )
        .arg(
            Arg::new("session")
                .short('s')
                .long("session")
                .value_name("SESSION")
                .help("name of the Kakoune session to talk to (defaults to $kak_session)"),
        )
        .arg(
            Arg::new("timeout")
                .hide(true)
                .short('t')
                .long("timeout")
                .value_name("TIMEOUT")
                .help("session timeout in seconds (default is 18000 seconds)"),
        )
        .arg(
            Arg::new("v")
                .hide(true)
                .short('v')
                .action(ArgAction::Count)
                .help("set the level of verbosity (use up to 4 times)"),
        )
        .arg(
            Arg::new("debug")
                .long("debug")
                .action(ArgAction::SetTrue)
                .help("enable debug logging (see the 'lsp_debug' option)"),
        )
        .arg(
            Arg::new("log")
                .hide(true)
                .long("log")
                .value_name("PATH")
                .help("file to write the log into, in addition to the *debug* buffer"),
        )
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::SetTrue)
                .help("print help"),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::SetTrue)
                .help("print version"),
        );
    let matches = command.clone().get_matches();

    if matches.get_flag("help") {
        let _ = command.print_help();
        return Ok(());
    }

    if matches.get_flag("version") {
        return match handle_epipe(writeln!(stdout(), "{}", crate_version!())) {
            Ok(()) => Ok(()),
            Err(err) => {
                eprintln!("Error writing version: {}", err);
                Err(())
            }
        };
    }

    let kak_session = environment_variable(None, "kak_session")?;
    let externally_started = kak_session.is_none();

    let session = kak_session.or_else(|| matches.get_one::<String>("session").cloned());
    if matches.get_flag("kakoune") || session.is_none() {
        return kakoune();
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
        report_fatal_error(
            None,
            "Error: no session name given, please export '$kak_session'",
        );
        return Err(());
    };
    let session = SessionId(session);
    let env_var = |name| environment_variable(Some(&session), name);
    let fatal_error = |message: String| report_fatal_error(Some(&session), &message);

    let plugin_path = env_var("XDG_RUNTIME_DIR")?
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

    let mut session_symlink_path = plugin_path;
    session_symlink_path.push(format!("{}.ref", session));

    let existing_path = [&session_path, &session_symlink_path]
        .iter()
        .cloned()
        .find(|p| fs::symlink_metadata(p).is_ok());
    let externally_started_file = |session_path: &PathBuf| {
        let mut tmp = session_path.to_owned();
        tmp.push("externally-started");
        tmp
    };

    if let Some(existing_path) = existing_path {
        let was_externally_started = externally_started_file(&session_path).exists();
        if !was_externally_started {
            return Err(fatal_error(format!(
                "kak-lsp session file already exists at '{}'",
                existing_path.display()
            )));
        }
        if !externally_started {
            eprintln!("Attaching to externally-started kak-lsp server");
        }
    }
    println!("{}", session_symlink_path.display());
    unsafe {
        libc::close(STDOUT_FILENO);
    }
    if existing_path.is_some() {
        return Ok(());
    }

    let mut session_directory = SessionDirectory {
        symlink: None,
        fifos: [None, None],
        pid_files: None,
        externally_started_file: None,
        session_directory: TemporaryDirectory::new(session_path.clone()),
    };
    if let Err(err) = fs::create_dir_all(session_path.clone()) {
        return Err(fatal_error(format!(
            "failed to create session directory '{}': {}",
            session_path.display(),
            err
        )));
    }

    session_directory.symlink = Some(TemporaryFile::new(session_symlink_path.clone()));

    if let Err(err) = std::os::unix::fs::symlink(session.as_str(), session_symlink_path.clone()) {
        return Err(fatal_error(format!(
            "failed to create session directory symlink '{}': {}",
            session_symlink_path.display(),
            err,
        )));
    }
    if externally_started {
        let file = externally_started_file(&session_path);
        if let Err(err) = fs::File::create(file.clone()) {
            return Err(fatal_error(format!(
                "failed to create '{}': {}",
                file.display(),
                err
            )));
        }
        session_directory.externally_started_file = Some(TemporaryFile::new(file));
    }
    let mut create_fifo = |offset: usize, name: &str| {
        let mut fifo = session_path.clone();
        fifo.push(name);
        let tmp = TemporaryFile::new(fifo.clone());
        let fifo_cstr = tmp.0;
        session_directory.fifos[offset] = Some(tmp);
        if unsafe { libc::mkfifo(fifo_cstr.as_ptr(), 0o600) } != 0 {
            let err = std::io::Error::last_os_error();
            return Err(fatal_error(format!(
                "failed to create fifo '{}': {}",
                fifo.display(),
                err
            )));
        }
        Ok(fifo)
    };
    let fifo = create_fifo(0, "fifo")?;
    let alt_fifo = create_fifo(1, "alt-fifo")?;

    let mut pid_file = session_path.clone();
    pid_file.push("pid");
    let mut pid_file_tmp = session_path;
    pid_file_tmp.push("pid.tmp");
    session_directory.pid_files = Some([
        TemporaryFile::new(pid_file.clone()),
        TemporaryFile::new(pid_file_tmp.clone()),
    ]);

    let _cleanup = ScopeEnd::new(do_cleanup);
    CLEANUP.lock().unwrap().get_or_init(|| {
        Box::new(move || {
            drop(session_directory);
        })
    });

    for signal in [SIGHUP, SIGINT, SIGQUIT, SIGPIPE, SIGTERM] {
        unsafe {
            libc::signal(signal, handle_interrupt as libc::sighandler_t);
        }
    }

    let mut verbosity;
    #[allow(deprecated)]
    let mut config = if let Some(config_path) = config_path {
        let config = parse_legacy_config(&config_path, &session)?;
        verbosity = config.verbosity;
        config
    } else {
        let mut config = Config::default();
        verbosity = 2;
        config.server.timeout = 18000;
        if let Some(timeout) = env_var("kak_opt_lsp_timeout")? {
            config.server.timeout = timeout
                .parse()
                .map_err(|err| fatal_error(format!("failed to parse lsp_timeout: {err}")))?;
        }
        if let Some(snippet_support) = env_var("kak_opt_lsp_snippet_support")? {
            config.snippet_support = snippet_support != "false";
        }
        if let Some(file_watch_support) = env_var("kak_opt_lsp_file_watch_support")? {
            config.file_watch_support = file_watch_support != "false";
        }
        config
    };

    if let Some(debug) = env_var("kak_opt_lsp_debug")? {
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

    if let Some(timeout) = matches.get_one::<String>("timeout") {
        config.server.timeout = timeout
            .parse()
            .map_err(|err| fatal_error(format!("failed to parse --timeout parameter: {err}")))?;
    }

    if matches.get_flag("daemonize") {
        if let Err(e) = Daemonize::new()
            .working_directory(std::env::current_dir().unwrap())
            .start()
        {
            return Err(fatal_error(format!("Failed to daemonize process: {:?}", e)));
        }
    }

    if let Err(err) = fs::write(pid_file_tmp.clone(), process::id().to_string().as_bytes()) {
        return Err(fatal_error(format!(
            "failed to write pid file '{}': {}",
            pid_file_tmp.display(),
            err
        )));
    }
    if let Err(err) = fs::rename(pid_file_tmp.clone(), pid_file.clone()) {
        return Err(fatal_error(format!(
            "failed to rename pid file '{}': {}",
            pid_file.display(),
            err
        )));
    }

    let editor_transport = editor_transport::start(session.clone());
    let to_editor = editor_transport.sender();

    // Setting up the logger after potential daemonization,
    // otherwise it refuses to work properly.
    let _cleanup = ScopeEnd::new(destroy_logger);
    let log_path_parent = initialize_logger(&matches, verbosity);

    let old_hook = panic::take_hook();
    let _restore_hook = ScopeEnd::new(move || panic::set_hook(old_hook));
    {
        let session = session.clone();
        let sentry_guard = Mutex::new(Some(sentry::init(("https://4150385475481d83c026ddff07957dcf@o4508427288313856.ingest.de.sentry.io/4508427290607696", sentry::ClientOptions {
                  release: sentry::release_name!(),
                  ..Default::default()
                }))));
        panic::set_hook(Box::new(move |panic_info| {
            static PANICKING: AtomicBool = AtomicBool::new(false);
            if PANICKING
                .compare_exchange(false, true, AcqRel, Acquire)
                .is_err()
            {
                process::abort();
            }
            let backtrace = Backtrace::capture();
            let message = formatdoc!(
                "kak-lsp crashed, please report an issue or send this crash report.
                 See the *debug* buffer for more info.

                 {}
                 {}",
                panic_info,
                backtrace
            );
            let meta = LAST_CLIENT
                .lock()
                .unwrap()
                .take()
                .map(EditorMeta::for_client)
                .unwrap_or_default();
            show_error(&session, meta.clone(), None, message);
            do_cleanup();
            report_crash(
                &session,
                sentry_guard.lock().unwrap().take().unwrap(),
                meta,
                panic_info,
                backtrace,
            );

            destroy_logger();
            process::abort();
        }));
    }

    controller::start(
        session.clone(),
        config,
        to_editor,
        log_path_parent,
        fifo,
        alt_fifo,
    );
    info!(to_editor, "kak-lsp server exiting");
    Ok(())
}

fn environment_variable(session: Option<&SessionId>, name: &str) -> Result<Option<String>, ()> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(err) => match err {
            env::VarError::NotPresent => Ok(None),
            env::VarError::NotUnicode(bytes) => Err(bytes),
        },
    }
    .map_err(|bytes| {
        report_fatal_error(
            session,
            &format!(
                "environment variable '{name}' value is not valid UTF-8: {}",
                String::from_utf8_lossy(bytes.as_bytes())
            ),
        )
    })
}

fn handle_epipe(r: io::Result<()>) -> io::Result<()> {
    match r {
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        _ => r,
    }
}

fn kakoune() -> Result<(), ()> {
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
    match handle_epipe(writeln!(stdout(), "{}{}", script, lsp_cmd)) {
        Ok(()) => Ok(()),
        Err(err) => {
            eprintln!("Error writing Kakoune script: {}", err);
            Err(())
        }
    }
}

fn report_fatal_error(session: Option<&SessionId>, message: &str) -> () {
    let Some(session) = session else {
        eprintln!("{}", message);
        return ();
    };
    let Ok(maybe_client) = environment_variable(Some(session), "kak_client") else {
        return ();
    };
    let meta = maybe_client
        .map(ClientId)
        .map(EditorMeta::for_client)
        .unwrap_or_default();
    show_error(session, meta, None, message);
    ()
}

pub fn report_crash(
    session: &SessionId,
    _sentry_guard: sentry::ClientInitGuard,
    meta: EditorMeta,
    panic_info: &PanicInfo,
    backtrace: Backtrace,
) {
    let fifo = mkfifo(session);
    let command = formatdoc!(
        r#"evaluate-commands %[
               define-command -override lsp-dont-report %[
                   echo -markup '{{Information}}Did not send crash report'
                   echo -to-file {fifo}
               ]
               prompt \
                   'Send crash report? [yes/no]: ' \
                   -on-abort lsp-dont-report \
               %[
                   try %[
                       evaluate-commands %sh[ [ "$kak_text" = yes ] && echo fail ]
                       lsp-dont-report
                   ] catch %[
                       prompt 'optional email address, name or username: ' \
                           -on-abort lsp-dont-report \
                       %[
                           set-option buffer lsp_crash_report_email %val[text]
                           prompt 'optional other info such as steps to reproduce: ' \
                               -on-abort lsp-dont-report %[
                               echo -to-file {fifo} -quoting shell \
                                   %opt[lsp_crash_report_email] %val[text]
                           ]
                       ]
                   ]
               ]
           ]"#,
    );
    exec_fifo(session, meta.clone(), None, command, false);
    let mut details = vec![];
    fs::File::open(fifo)
        .unwrap()
        .read_to_end(&mut details)
        .unwrap();
    if details.is_empty() {
        return;
    }
    let mut tokenizer = TokenizerState {
        input: details,
        ..Default::default()
    };
    tokenizer.input.push(b' ');
    let email = tokenizer.read_token();
    let message = tokenizer.read_token();
    let event_id = sentry::with_integration(|integration: &PanicIntegration, hub| {
        let mut event = integration.event_from_panic_info(panic_info);
        let event_id = event.event_id;
        exec_fifo(
            session,
            meta.clone(),
            None,
            format!(
                "echo -markup '{{Information}}Sending crash report with ID {}'",
                event_id
            ),
            false,
        );
        event.message = Some(format!("{}\n\n{}\n{}", message, panic_info, backtrace));
        event.user = Some(sentry::User {
            email: Some(email),
            ..Default::default()
        });
        hub.capture_event(event);
        if let Some(client) = hub.client() {
            client.flush(None);
        }
        event_id
    });
    exec_fifo(
        session,
        meta,
        None,
        format!(
            "echo -markup '{{Information}}Sent crash report with ID {}'",
            event_id
        ),
        false,
    );
}

fn parse_legacy_config(config_path: &PathBuf, session: &SessionId) -> Result<Config, ()> {
    let raw_config = fs::read_to_string(config_path).expect("Failed to read config");
    #[allow(deprecated)]
    toml::from_str(&raw_config)
        .map_err(|err| err.to_string())
        .and_then(|mut cfg: Config| {
            // Translate legacy config.
            if !cfg.language.is_empty()
                && (!cfg.language_server.is_empty() || !cfg.language_ids.is_empty())
            {
                return Err(
                    "incompatible options: language_server/language_id and legacy language table"
                        .to_string(),
                );
            }
            if cfg.language_server.is_empty() {
                if cfg.language.values().any(|l| l.command.is_none()) {
                    return Err("missing 'command' field in legacy language table".to_string());
                }
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
        })
        .map_err(|err| {
            report_fatal_error(
                Some(session),
                &format!(
                    "failed to parse config file {}: {}",
                    config_path.display(),
                    err
                ),
            )
        })
}

fn initialize_logger(matches: &ArgMatches, verbosity: u8) -> &'static Option<PathBuf> {
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

fn destroy_logger() {
    slog_scope::set_global_logger(NullLoggerBuilder {}.build().unwrap()).cancel_reset();
}

fn do_cleanup() {
    if let Some(cleanup) = CLEANUP.lock().unwrap().take() {
        (cleanup)();
    }
}

extern "C" fn handle_interrupt(sig: libc::c_int) -> ! {
    destroy_logger();
    do_cleanup();
    process::exit(if sig == SIGPIPE { 0 } else { 1 });
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
    fifos: [Option<TemporaryFile>; 2],
    pid_files: Option<[TemporaryFile; 2]>,
    externally_started_file: Option<TemporaryFile>,
    #[allow(dead_code)]
    session_directory: TemporaryDirectory,
}

struct ScopeEnd<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> ScopeEnd<F> {
    pub fn new(f: F) -> Self {
        Self(Some(f))
    }
}

impl<F: FnOnce()> Drop for ScopeEnd<F> {
    fn drop(&mut self) {
        (self.0.take().unwrap())()
    }
}
