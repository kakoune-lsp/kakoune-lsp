use crate::types::*;
use std::os::unix::fs::DirBuilderExt;
use std::{collections::HashMap, path::Path};
use std::{env, fs, io, path};

pub fn temp_dir() -> path::PathBuf {
    let mut path = env::temp_dir();
    path.push("kakoune-lsp");
    let old_mask = unsafe { libc::umask(0) };
    // Ignoring possible error during $TMPDIR/kakoune-lsp creation to have a chance to restore umask.
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

pub fn mkfifo() -> String {
    let mut path = temp_dir();
    for attempt in 0..10 {
        path.push(format!("{:x}", rand::random::<u64>()));
        let path = path.to_str().unwrap().to_string();
        let mkfifo_result = unsafe {
            let path = std::ffi::CString::new(path.clone()).unwrap();
            libc::mkfifo(path.as_ptr(), 0o600)
        };
        if mkfifo_result == 0 {
            return path;
        }
        error!("mkfifo attempt {attempt} failed, retrying");
    }
    panic!("failed to create fifo");
}

pub fn temp_fifo() -> TempFifo {
    TempFifo { path: mkfifo() }
}

impl Drop for TempFifo {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Escape Kakoune string wrapped into single quote
pub fn editor_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Escape Kakoune string wrapped into double quote
pub fn editor_escape_double_quotes(s: &str) -> String {
    s.replace('"', "\"\"").replace('%', "%%")
}

#[allow(dead_code)]
pub fn editor_escape_keys(s: &str) -> String {
    s.replace('<', "<lt>")
}

/// Convert to Kakoune string by wrapping into quotes and escaping
pub fn editor_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || "-_".contains(c)) {
        return s.into();
    }
    format!("'{}'", editor_escape(s))
}

#[allow(dead_code)]
/// Convert to Kakoune string by wrapping into double quotes and escaping
pub fn editor_quote_double_quotes(s: &str) -> String {
    format!("\"{}\"", editor_escape_double_quotes(s))
}

// Escape a sequence of printable keys so they can safely be passed to "execute-keys".
pub fn escape_keys(s: &str) -> String {
    s.replace('<', "<lt>")
}

/// Escape Kakoune tuple element, as used in option types "completions", "line-specs" and
/// "range-specs".
pub fn escape_tuple_element(s: &str) -> String {
    s.replace('\\', "\\\\").replace('|', "\\|")
}

/// Convert language filetypes configuration into a more lookup-friendly form.
pub fn filetype_to_language_id_map(
    config: &Config,
) -> HashMap<String, (LanguageId, Vec<ServerName>)> {
    let mut filetypes: HashMap<String, (LanguageId, Vec<ServerName>)> = HashMap::default();

    for (server_name, lang_config) in &config.language_server {
        for filetype in &lang_config.filetypes {
            let entry = filetypes.entry(filetype.clone()).or_insert((
                config
                    .language_ids
                    .get(filetype)
                    .cloned()
                    .unwrap_or_else(|| filetype.clone()),
                Vec::new(),
            ));
            let (_, servers) = entry;
            servers.push(server_name.clone());
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
