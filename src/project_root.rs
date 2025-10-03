use glob::glob;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;

use crate::editor_transport::ToEditorSender;
use crate::types::LanguageId;

pub fn find_project_root(
    to_editor: &ToEditorSender,
    language_id: &LanguageId,
    markers: &[String],
    path: &str,
) -> String {
    if let Ok(force_root) = env::var("KAK_LSP_FORCE_PROJECT_ROOT") {
        debug!(
            to_editor,
            "Using $KAK_LSP_FORCE_PROJECT_ROOT as project root: \"{}\"", force_root
        );
        return force_root;
    }
    let vars = gather_env_roots(to_editor, language_id);
    if vars.is_empty() {
        roots_by_marker(to_editor, markers, path)
    } else {
        roots_by_env(&vars, path).unwrap_or_else(|| roots_by_marker(to_editor, markers, path))
    }
}

fn roots_by_marker(to_editor: &ToEditorSender, roots: &[String], path: &str) -> String {
    let mut src = PathBuf::from(path);
    // For scratch buffers we get a bare filename.
    if !src.is_absolute() {
        src = env::current_dir().expect("cannot access current directory");
    }
    while !src.is_dir() {
        src.pop();
    }

    let mut pwd = src.clone();
    loop {
        for root in roots {
            // unwrap should be safe here because we walk up path previously converted from str
            let matches = glob(pwd.join(root).to_str().unwrap());
            if let Ok(mut m) = matches {
                if m.next().is_some() {
                    // ditto unwrap
                    let root_dir = pwd.to_str().unwrap().to_string();
                    debug!(
                        to_editor,
                        "Found project root \"{}\" because it contains \"{}\"", root_dir, root
                    );
                    return root_dir;
                }
            }
        }
        if !pwd.pop() {
            break;
        }
    }
    src.to_str().unwrap().to_string()
}

fn gather_env_roots(to_editor: &ToEditorSender, language_id: &LanguageId) -> HashSet<PathBuf> {
    let prefix = format!("KAK_LSP_PROJECT_ROOT_{}", language_id.to_uppercase());
    debug!(to_editor, "Searching for vars starting with {}", prefix);
    env::vars()
        .filter(|(k, _v)| k.starts_with(&prefix))
        .map(|(_k, v)| PathBuf::from(v))
        .collect()
}

fn roots_by_env(roots: &HashSet<PathBuf>, path: &str) -> Option<String> {
    let p = PathBuf::from(path);
    let pwd = if p.is_file() {
        p.parent().unwrap().to_path_buf()
    } else {
        p
    };
    roots
        .iter()
        .find(|x| pwd.starts_with(x))
        .map(|x| x.to_str().unwrap().to_string())
}
