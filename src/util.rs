use crate::context::*;
use crate::position::*;
use crate::types::*;
use itertools::Itertools;
use lsp_types::*;
use std::convert::TryInto;
use std::io::{stderr, stdout, Write};
use std::os::unix::fs::DirBuilderExt;
use std::time::Duration;
use std::{collections::HashMap, path::Path};
use std::{env, fs, io, path, process, thread};

pub fn temp_dir() -> path::PathBuf {
    let mut path = env::temp_dir();
    path.push("kak-lsp");
    let old_mask = unsafe { libc::umask(0) };
    // Ignoring possible error during $TMPDIR/kak-lsp creation to have a chance to restore umask.
    let _ = fs::DirBuilder::new()
        .recursive(true)
        .mode(0o1777)
        .create(&path);
    unsafe {
        libc::umask(old_mask);
    }
    path.push(whoami::username());
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&path)
        .unwrap();
    path
}

pub struct TempFifo {
    pub path: String,
}

pub fn temp_fifo() -> Option<TempFifo> {
    let mut path = temp_dir();
    path.push(format!("{:x}", rand::random::<u64>()));
    let path = path.to_str().unwrap().to_string();
    let fifo_result = unsafe {
        let path = std::ffi::CString::new(path.clone()).unwrap();
        libc::mkfifo(path.as_ptr(), 0o600)
    };
    if fifo_result != 0 {
        return None;
    }
    Some(TempFifo { path })
}

impl Drop for TempFifo {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Represent list of symbol information as filetype=grep buffer content.
/// Paths are converted into relative to project root.
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
            let filename_str = filename.to_str().unwrap();
            let position =
                get_kakoune_position_with_fallback(filename_str, location.range.start, ctx);
            let description = format!("{:?} {}", kind, name);
            format!(
                "{}:{}:{}:{}",
                short_file_path(filename_str, &ctx.root_path),
                position.line,
                position.column,
                description
            )
        })
        .join("\n")
}

/// Represent list of document symbol as filetype=grep buffer content.
/// Paths are converted into relative to project root.
pub fn format_document_symbol(
    items: Vec<DocumentSymbol>,
    meta: &EditorMeta,
    ctx: &Context,
) -> String {
    items
        .into_iter()
        .map(|symbol| {
            let position = get_kakoune_position_with_fallback(
                &meta.buffile,
                symbol.selection_range.start,
                ctx,
            );
            let description = format!("{:?} {}", symbol.kind, symbol.name);
            format!(
                "{}:{}:{}:{}",
                short_file_path(&meta.buffile, &ctx.root_path),
                position.line,
                position.column,
                description
            )
        })
        .join("\n")
}

/// Escape Kakoune string wrapped into single quote
pub fn editor_escape(s: &str) -> String {
    s.replace("'", "''")
}

/// Escape Kakoune string wrapped into double quote
pub fn editor_escape_double_quotes(s: &str) -> String {
    s.replace("\"", "\"\"")
}

/// Convert to Kakoune string by wrapping into quotes and escaping
pub fn editor_quote(s: &str) -> String {
    format!("'{}'", editor_escape(s))
}

#[allow(dead_code)]
/// Convert to Kakoune string by wrapping into double quotes and escaping
pub fn editor_quote_double_quotes(s: &str) -> String {
    format!("\"{}\"", editor_escape_double_quotes(s))
}

/// Escape Kakoune tuple element, as used in option types "completions", "line-specs" and
/// "range-specs".
pub fn escape_tuple_element(s: &str) -> String {
    s.replace("\\", "\\\\").replace("|", "\\|")
}

// Cleanup and gracefully exit
pub fn goodbye(session: &str, code: i32) {
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

/// Convert language filetypes configuration into a more lookup-friendly form.
pub fn filetype_to_language_id_map(config: &Config) -> HashMap<String, String> {
    let mut filetypes = HashMap::default();
    for (language_id, language) in &config.language {
        for filetype in &language.filetypes {
            filetypes.insert(filetype.clone(), language_id.clone());
        }
    }
    filetypes
}

pub fn read_document(filename: &str) -> io::Result<String> {
    // We can ignore invalid UTF-8 since we only use this to compute positions.  The width of
    // the replacement character is 1, which should usually be correct.
    Ok(String::from_utf8_lossy(&fs::read(filename)?).to_string())
}

pub fn short_file_path<'a>(target: &'a str, current_dir: &str) -> &'a str {
    Path::new(target)
        .strip_prefix(current_dir)
        .ok()
        .and_then(|p| p.to_str())
        .unwrap_or(target)
}

/// Given a starting Location, try to find `name` in a file and report its Position
///
/// If `treat_name_as_symbol` is true, then we're not doing a pure text search but
/// treating name as a symbol. We apply a crude heuristic to ensure we have found
/// the `name` symbol instead of `name` as part of a larger string.
///
/// If `treat_name_as_symbol` is false then this a pure search for a string in filename.
pub fn find_name_in_file(
    filename: &str,
    ctx: &Context,
    start_from: Position,
    name: &str,
    treat_name_as_symbol: bool,
) -> Option<Position> {
    if name.is_empty() {
        return Some(start_from);
    }
    let contents = get_file_contents(filename, ctx).unwrap();
    let line_offset: usize = contents.line_to_char(start_from.line.try_into().unwrap());
    let within_line_offset: usize = start_from.character.try_into().unwrap();
    let char_offset = line_offset + within_line_offset;
    let mut it = name.chars();
    let mut maybe_found = None;
    for (num, c) in contents.chars_at(char_offset).enumerate() {
        match it.next() {
            Some(itc) => {
                if itc != c {
                    it = name.chars();
                    let itc2 = it.next().unwrap();
                    if itc2 != c {
                        it = name.chars();
                    }
                }
            }
            None => {
                // Example. Take the python expression `[f.name for f in fields]`
                // Let us say we get -------------------------------^
                // i.e. We get char 8 starting position when searching for variable `f`.
                // Now `for` starts with `f` also so we will wrongly return 8 when we
                // should return 12.
                //
                // A simple heuristic will be that if the next character is alphanumeric then
                // we have _not_ found our symbol and we have to continue our search.
                // This heuristic may not work in all cases but is "good enough".
                if treat_name_as_symbol && c.is_alphanumeric() {
                    it = name.chars();
                    let itc = it.next().unwrap();
                    if itc != c {
                        it = name.chars();
                    }
                } else {
                    maybe_found = Some(num - 1);
                    // We're done!
                    break;
                }
            }
        }
    }

    if let Some(found) = maybe_found {
        let line = contents.char_to_line(char_offset + found);
        let character = char_offset + found - contents.line_to_char(line);
        Some(Position {
            line: line.try_into().unwrap(),
            character: character.try_into().unwrap(),
        })
    } else {
        None
    }
}
