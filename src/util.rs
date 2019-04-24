use crate::context::*;
use crate::position::*;
use crate::types::*;
use itertools::Itertools;
use libc;
use lsp_types::request::GotoDefinitionResponse;
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

/// Convert `GotoDefinitionResponse` into `Option<Location>`.
///
/// If multiple locations are present, returns the first one. Transforms LocationLink into Location
/// by dropping source information.
pub fn goto_definition_response_to_location(
    result: Option<GotoDefinitionResponse>,
) -> Option<Location> {
    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => Some(location),
        Some(GotoDefinitionResponse::Array(mut locations)) => {
            if locations.is_empty() {
                None
            } else {
                Some(locations.remove(0))
            }
        }
        Some(GotoDefinitionResponse::Link(mut locations)) => {
            if locations.is_empty() {
                None
            } else {
                let LocationLink {
                    target_uri,
                    target_range,
                    ..
                } = locations.remove(0);

                Some(Location {
                    uri: target_uri,
                    range: target_range,
                })
            }
        }
        None => None,
    }
}

pub fn get_lsp_position(
    filename: &str,
    position: &KakounePosition,
    ctx: &Context,
) -> Option<Position> {
    ctx.documents.get(filename).and_then(|document| {
        Some(kakoune_position_to_lsp(
            position,
            &document.text,
            &ctx.offset_encoding,
        ))
    })
}

pub fn get_kakoune_position(
    filename: &str,
    position: &Position,
    ctx: &Context,
) -> Option<KakounePosition> {
    ctx.documents
        .get(filename)
        .and_then(|doc| Some(doc.text.clone()))
        .or_else(|| {
            File::open(filename)
                .ok()
                .and_then(|f| Rope::from_reader(BufReader::new(f)).ok())
        })
        .and_then(|text| {
            Some(lsp_position_to_kakoune(
                &position,
                &text,
                &ctx.offset_encoding,
            ))
        })
}
