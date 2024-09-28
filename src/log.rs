use std::sync::atomic::Ordering::Relaxed;
use std::{borrow::Cow, sync::atomic::AtomicBool};

use crate::SessionId;
use crate::{
    context::meta_for_session, editor_quote, editor_transport::send_command_to_editor,
    EditorResponse,
};

macro_rules! error {
    ($session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($session, slog::Level::Error, $fmt $(, $arg ) *)
    };
}
macro_rules! warn {
    ($session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($session, slog::Level::Warning, $fmt $(, $arg ) *)
    };
}
macro_rules! info {
    ($session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($session, slog::Level::Info, $fmt $(, $arg ) *)
    };
}
macro_rules! debug {
    ($session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($session, slog::Level::Debug, $fmt $(, $arg ) *)
    };
}

macro_rules! log_impl {
    ($session:expr, $level:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
        {
            let message = format!($fmt $(, $arg ) *);
            slog_scope::with_logger(
                |logger|
                 slog::slog_log!(logger, $level, "", "{}", message)
            );
            crate::log::do_log(&$session, $level, message)
        }
    };
}

pub static DEBUG: AtomicBool = AtomicBool::new(false);

pub(crate) fn do_log(session: &SessionId, level: slog::Level, message: String) {
    if level == slog::Level::Debug && !DEBUG.load(Relaxed) {
        return;
    }
    let command = format!("echo -debug -- LSP: {} {}", level, editor_quote(&message));
    let meta = meta_for_session(session.clone(), None);
    let command = EditorResponse {
        meta,
        command: Cow::Owned(command),
    };
    send_command_to_editor(command, false);
}
