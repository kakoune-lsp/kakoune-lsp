use glob::glob;
use std::collections::HashSet;
use std::env;
use std::path::PathBuf;

pub fn find_project_root(language: &str, markers: &[String], path: &str) -> String {
    let vars = gather_env_roots(language);
    if vars.is_empty() {
        roots_by_marker(markers, path)
    } else {
        roots_by_env(&vars, path).unwrap_or_else(|| roots_by_marker(markers, path))
    }
}

pub fn roots_by_marker(roots: &[String], path: &str) -> String {
    let mut pwd = PathBuf::from(path);
    if pwd.is_file() {
        pwd.pop();
    }
    let src = pwd.to_str().unwrap().to_string();

    loop {
        for root in roots {
            // unwrap should be safe here because we walk up path previously converted from str
            let matches = glob(pwd.join(root).to_str().unwrap());
            if let Ok(mut m) = matches {
                if m.next().is_some() {
                    // ditto unwrap
                    return pwd.to_str().unwrap().to_string();
                }
            }
        }
        if !pwd.pop() {
            return src;
        }
    }
}

pub fn gather_env_roots(language: &str) -> HashSet<PathBuf> {
    let prefix = format!("KAK_LSP_PROJECT_ROOT_{}", language.to_uppercase());
    debug!("Searching for vars starting with {}", prefix);
    env::vars()
        .filter(|(k, _v)| k.starts_with(&prefix))
        .map(|(_k, v)| PathBuf::from(v))
        .collect()
}

pub fn roots_by_env(roots: &HashSet<PathBuf>, path: &str) -> Option<String> {
    let p = PathBuf::from(path);
    let pwd = if p.is_file() {
        p.parent().unwrap().to_path_buf()
    } else {
        p
    };
    roots
        .iter()
        .find(|x| pwd.starts_with(&x))
        .map(|x| x.to_str().unwrap().to_string())
}
