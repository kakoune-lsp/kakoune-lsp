use std::sync::atomic::Ordering::Relaxed;
use std::{borrow::Cow, sync::atomic::AtomicBool};

use crate::SessionId;
use crate::{
    context::meta_for_session, editor_quote, editor_transport::send_command_to_editor,
    EditorResponse,
};

macro_rules! error {
    ($session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($session, "ERRO", $fmt $(, $arg ) *)
    };
}
macro_rules! warn {
    ($session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($session, "WARN", $fmt $(, $arg ) *)
    };
}
macro_rules! info {
    ($session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($session, "INFO", $fmt $(, $arg ) *)
    };
}
macro_rules! debug {
    ($session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($session, "DEBG", $fmt $(, $arg ) *)
    };
}

macro_rules! log_impl {
    ($session:expr, $level:literal, $fmt:literal $(, $arg:expr )* $(,)?) => {
        {
            let message = format!($fmt $(, $arg ) *);
            crate::log::do_log(&$session, $level, message)
        }
    };
}

pub static DEBUG: AtomicBool = AtomicBool::new(false);

pub(crate) fn do_log(session: &SessionId, level: &'static str, message: String) {
    match level {
        "ERRO" => slog_scope::error!("{}", message),
        "WARN" => slog_scope::warn!("{}", message),
        "INFO" => slog_scope::info!("{}", message),
        "DEBG" => slog_scope::debug!("{}", message),
        _ => panic!(),
    }
    if level == "DEBG" && !DEBUG.load(Relaxed) {
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
