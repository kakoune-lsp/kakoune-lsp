use std::sync::atomic::Ordering::Relaxed;
use std::{borrow::Cow, sync::atomic::AtomicBool};

use crate::EditorMeta;
use crate::{editor_quote, EditorResponse};

macro_rules! error {
    ($to_editor:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($to_editor, slog::Level::Error, $fmt $(, $arg ) *)
    };
    (session:$session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!(here, $session, slog::Level::Error, $fmt $(, $arg ) *)
    };
    (dispatcher:$dispatcher:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!(dispatcher, $dispatcher, slog::Level::Error, $fmt $(, $arg ) *)
    };
}
macro_rules! warn {
    ($to_editor:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($to_editor, slog::Level::Warning, $fmt $(, $arg ) *)
    };
    (session:$session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!(here, $session, slog::Level::Warning, $fmt $(, $arg ) *)
    };
    (dispatcher:$dispatcher:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!(dispatcher, $dispatcher, slog::Level::Warning, $fmt $(, $arg ) *)
    };
}
macro_rules! info {
    ($to_editor:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($to_editor, slog::Level::Info, $fmt $(, $arg ) *)
    };
    (session:$session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!(here, $session, slog::Level::Info, $fmt $(, $arg ) *)
    };
    (dispatcher:$dispatcher:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!(dispatcher, $dispatcher, slog::Level::Info, $fmt $(, $arg ) *)
    };
}
macro_rules! debug {
    ($to_editor:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!($to_editor, slog::Level::Debug, $fmt $(, $arg ) *)
    };
    (session:$session:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!(here, $session, slog::Level::Debug, $fmt $(, $arg ) *)
    };
    (dispatcher:$dispatcher:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
            log_impl!(dispatcher, $dispatcher, slog::Level::Debug, $fmt $(, $arg ) *)
    };
}

macro_rules! do_slog {
    ($level:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
        {
            let message = format!($fmt $(, $arg ) *);
            slog_scope::with_logger(|logger| slog::slog_log!(logger, $level, "", "{}", message));
            message
        }
    }
}

macro_rules! log_impl {
    ($to_editor:expr, $level:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
        crate::log::do_log(
            $level, do_slog!($level, $fmt $(, $arg ) *),
            |resp| crate::editor_transport::send_command_to_editor(&$to_editor, resp),
        )
    };
    (here, $session:expr, $level:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
        crate::log::do_log(
            $level, do_slog!($level, $fmt $(, $arg ) *),
            |resp| crate::editor_transport::send_command_to_editor_here(&$session, resp),
        )
    };
    (dispatcher, $dispatcher:expr, $level:expr, $fmt:literal $(, $arg:expr )* $(,)?) => {
        match &$dispatcher {
            crate::thread_worker::ToEditorDispatcher::ThisThread(session) => {
                log_impl!(here, session, $level, $fmt $(, $arg ) *);
            }
            crate::thread_worker::ToEditorDispatcher::OtherThread(to_editor) => {
                log_impl!(to_editor, $level, $fmt $(, $arg ) *);
            }
        }
    };
}

pub static DEBUG: AtomicBool = AtomicBool::new(false);

pub(crate) fn do_log(level: slog::Level, message: String, dispatcher: impl FnOnce(EditorResponse)) {
    if level == slog::Level::Debug && !DEBUG.load(Relaxed) {
        return;
    }
    let command = format!("echo -debug -- LSP: {} {}", level, editor_quote(&message));
    let command = EditorResponse::new_without_logging(EditorMeta::default(), Cow::Owned(command));
    (dispatcher)(command);
}
