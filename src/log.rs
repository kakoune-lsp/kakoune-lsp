use std::borrow::Cow;
use std::sync::atomic::Ordering::Relaxed;

use crate::{
    context::meta_for_session, editor_quote, editor_transport::send_command_to_editor,
    EditorResponse,
};
use crate::{SessionId, LOG_LEVEL, LOG_PATH, LOG_PATH_DYNAMICALLY_SET};

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
            crate::log::log_to_debug_buffer(&$session, $level, message)
        }
    };
}

pub(crate) fn log_to_debug_buffer(session: &SessionId, level: slog::Level, message: String) {
    if LOG_PATH.lock().unwrap().is_some() && LOG_PATH_DYNAMICALLY_SET.load(Relaxed) {
        return;
    }
    if !level.is_at_least(LOG_LEVEL.lock().unwrap().unwrap().as_level()) {
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
