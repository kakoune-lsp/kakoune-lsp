use context::*;
use itertools::Itertools;
use languageserver_types::*;
use std::io::{stderr, stdout, Write};
use std::os::unix::fs::DirBuilderExt;
use std::time::Duration;
use std::{env, fs, path, process, thread};
use types::*;

pub fn temp_dir() -> path::PathBuf {
    let mut path = env::temp_dir();
    path.push("kak-lsp");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&path)
        .unwrap();
    path
}

pub fn lsp_range_to_kakoune(range: Range) -> String {
    // LSP ranges are 0-based, but Kakoune's 1-based.
    // LSP ranges are exclusive, but Kakoune's are inclusive.
    // Also from LSP spec: If you want to specify a range that contains a line including
    // the line ending character(s) then use an end position denoting the start of the next
    // line.
    let mut end_line = range.end.line;
    let mut end_char = range.end.character;
    if end_char > 0 {
        end_line += 1;
    } else {
        end_char = 1_000_000;
    }
    format!(
        "{}.{},{}.{}",
        range.start.line + 1,
        range.start.character + 1,
        end_line,
        end_char,
    )
}

pub fn format_symbol_information(items: Vec<SymbolInformation>, ctx: &Context) -> String {
    items
        .into_iter()
        .map(|symbol| {
            let SymbolInformation {
                location,
                name,
                kind,
                ..
            } = symbol;
            let filename = location.uri.to_file_path().unwrap();
            let filename = filename
                .strip_prefix(&ctx.root_path)
                .ok()
                .and_then(|p| Some(p.to_str().unwrap()))
                .or_else(|| filename.to_str())
                .unwrap();

            let position = location.range.start;
            let description = format!("{:?} {}", kind, name);
            format!(
                "{}:{}:{}:{}",
                filename,
                position.line + 1,
                position.character + 1,
                description
            )
        }).join("\n")
}

/// Escape Kakoune string wrapped into single quote
pub fn editor_escape(s: &str) -> String {
    s.replace("'", "''")
}

/// Convert to Kakoune string by wrapping into quotes and escaping
pub fn editor_quote(s: &str) -> String {
    format!("'{}'", editor_escape(s))
}

// Cleanup and gracefully exit
pub fn goodbye(config: &Config, code: i32) {
    if code == 0 {
        if let Some(ref session) = config.server.session {
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
    }
    stderr().flush().unwrap();
    stdout().flush().unwrap();
    // give stdio a chance to actually flush
    thread::sleep(Duration::from_secs(1));
    process::exit(code);
}

pub fn apply_text_edits(
    uri: Option<&Url>,
    text_edits: &[TextEdit],
    meta: &EditorMeta,
    ctx: &mut Context,
) {
    // empty text edits processed as a special case because Kakoune's `select` command
    // doesn't support empty arguments list
    if text_edits.is_empty() {
        // nothing to do, but sending command back to the editor is required to handle case when
        // editor is blocked waiting for response via fifo
        ctx.exec(meta.clone(), "nop".to_string());
        return;
    }
    let edits = text_edits
        .iter()
        .map(|text_edit| {
            let TextEdit { range, new_text } = text_edit;
            // LSP ranges are 0-based, but Kakoune's 1-based.
            // LSP ranges are exclusive, but Kakoune's are inclusive.
            // Also from LSP spec: If you want to specify a range that contains a line including
            // the line ending character(s) then use an end position denoting the start of the next
            // line.
            let mut start_line = range.start.line;
            let mut start_char = range.start.character;
            let mut end_line = range.end.line;
            let mut end_char = range.end.character;

            if start_line == end_line && start_char == end_char && start_char == 0 {
                start_char = 1_000_000;
            } else {
                start_line += 1;
                start_char += 1;
            }

            if end_char > 0 {
                end_line += 1;
            } else {
                end_char = 1_000_000;
            }

            let insert = start_line == end_line && start_char - 1 == end_char;

            (
                format!(
                    "{}.{}",
                    start_line,
                    if !insert { start_char } else { end_char }
                ),
                format!("{}.{}", end_line, end_char),
                new_text,
                insert,
            )
        }).collect::<Vec<_>>();

    let select_edits = edits
        .iter()
        .map(|(start, end, _, _)| format!("{},{}", start, end))
        .join(" ");

    let apply_edits = edits
        .iter()
        .enumerate()
        .map(|(i, (_, _, content, insert))| {
            format!(
                "exec 'z{}<space>'
                    {} {}",
                if i > 0 {
                    format!("{})", i)
                } else {
                    "".to_string()
                },
                if *insert {
                    "lsp-insert-after-selection"
                } else {
                    "lsp-replace-selection"
                },
                editor_quote(&content)
            )
        }).join("\n");

    let command = format!(
        "select {}
            exec -save-regs '' Z
            {}",
        select_edits, apply_edits
    );
    let command = format!("eval -draft -save-regs '^' {}", editor_quote(&command));
    match uri {
        Some(uri) => {
            let buffile = uri.to_file_path().unwrap();
            let command = format!(
                "lsp-apply-edits-to-file {} {}",
                editor_quote(buffile.to_str().unwrap()),
                editor_quote(&command)
            );
            ctx.exec(meta.clone(), command);
        }
        None => ctx.exec(meta.clone(), command),
    }
}
