use std::sync::atomic::Ordering::Relaxed;
use std::{borrow::Cow, sync::atomic::AtomicBool};

use crate::{editor_quote, EditorResponse};
use crate::{EditorMeta, ToEditor};

macro_rules! error {
    ($to_editor:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($to_editor, slog::Level::Error, $fmt $(, $arg ) *)
    };
}
macro_rules! warn {
    ($to_editor:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($to_editor, slog::Level::Warning, $fmt $(, $arg ) *)
    };
}
macro_rules! info {
    ($to_editor:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($to_editor, slog::Level::Info, $fmt $(, $arg ) *)
    };
}
macro_rules! debug {
    ($to_editor:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($to_editor, slog::Level::Debug, $fmt $(, $arg ) *)
    };
}

macro_rules! log_impl {
    ($to_editor:expr, $level:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
        {
            let message = format!($fmt $(, $arg ) *);
            slog_scope::with_logger(|logger| slog::slog_log!(logger, $level, "", "{}", message));
            crate::log::do_log($to_editor, $level, message);
        }
    };
}

pub static DEBUG: AtomicBool = AtomicBool::new(false);

pub(crate) fn do_log(to_editor: &impl ToEditor, level: slog::Level, message: String) {
    if level == slog::Level::Debug && !DEBUG.load(Relaxed) {
        return;
    }
    let command = format!("echo -debug -- LSP: {} {}", level, editor_quote(&message));
    let mut command = EditorResponse::new(EditorMeta::default(), Cow::Owned(command));
    command.suppress_logging = true;
    to_editor.dispatch(command);
}
