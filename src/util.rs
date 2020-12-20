use crate::context::*;
use crate::position::*;
use crate::text_edit::*;
use crate::types::*;
use itertools::Itertools;
use libc;
use lsp_types::*;
use ropey::Rope;
use std::collections::HashMap;
use std::fs::File;
use std::io::{stderr, stdout, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::time::Duration;
use std::{env, fs, path, process, thread};
use whoami;

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
            let filename = filename
                .strip_prefix(&ctx.root_path)
                .ok()
                .and_then(|p| p.to_str())
                .or_else(|| filename.to_str())
                .unwrap();

            let position = get_kakoune_position(filename, &location.range.start, ctx)
                .unwrap_or_else(|| KakounePosition {
                    line: location.range.start.line + 1,
                    column: location.range.start.character + 1,
                });
            let description = format!("{:?} {}", kind, name);
            format!(
                "{}:{}:{}:{}",
                filename, position.line, position.column, description
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
            let DocumentSymbol {
                range, name, kind, ..
            } = symbol;
            let filename = path::PathBuf::from(&meta.buffile);
            let filename = filename
                .strip_prefix(&ctx.root_path)
                .ok()
                .and_then(|p| p.to_str())
                .unwrap_or(&meta.buffile);

            let position = get_kakoune_position(filename, &range.start, ctx).unwrap_or_else(|| {
                KakounePosition {
                    line: range.start.line + 1,
                    column: range.start.character + 1,
                }
            });
            let description = format!("{:?} {}", kind, name);
            format!(
                "{}:{}:{}:{}",
                filename, position.line, position.column, description
            )
        })
        .join("\n")
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

/// Wrapper for kakoune_position_to_lsp which uses context to get buffer content and offset encoding.
pub fn get_lsp_position(
    filename: &str,
    position: &KakounePosition,
    ctx: &Context,
) -> Option<Position> {
    ctx.documents.get(filename).and_then(|document| {
        Some(kakoune_position_to_lsp(
            position,
            &document.text,
            ctx.offset_encoding,
        ))
    })
}

/// Wrapper for lsp_position_to_kakoune which uses context to get buffer content and offset encoding.
/// Reads the file directly if it is not present in context (is not open in editor).
pub fn get_kakoune_position(
    filename: &str,
    position: &Position,
    ctx: &Context,
) -> Option<KakounePosition> {
    get_file_contents(filename, ctx).and_then(|text| {
        Some(lsp_position_to_kakoune(
            &position,
            &text,
            ctx.offset_encoding,
        ))
    })
}

/// Apply text edits to the file pointed by uri either by asking Kakoune to modify corresponding
/// buffer or by editing file directly when it's not open in editor.
pub fn apply_text_edits(meta: &EditorMeta, uri: &Url, edits: &[TextEdit], ctx: &Context) {
    if let Some(document) = ctx
        .documents
        .get(uri.to_file_path().unwrap().to_str().unwrap())
    {
        ctx.exec(
            meta.clone(),
            apply_text_edits_to_buffer(Some(uri), edits, &document.text, ctx.offset_encoding),
        );
    } else {
        if let Err(e) = apply_text_edits_to_file(uri, edits, ctx.offset_encoding) {
            error!("Failed to apply edits to file {} ({})", uri, e);
        };
    }
}

/// Get the contents of a file.
/// Searches ctx.documents first and falls back to reading the file directly.
pub fn get_file_contents(filename: &str, ctx: &Context) -> Option<Rope> {
    ctx.documents
        .get(filename)
        .and_then(|doc| Some(doc.text.clone()))
        .or_else(|| {
            File::open(filename)
                .ok()
                .and_then(|f| Rope::from_reader(BufReader::new(f)).ok())
        })
}
