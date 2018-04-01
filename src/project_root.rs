use std::path::{Path, PathBuf};

pub fn find_project_root(roots: &[String], path: &str) -> Option<String> {
    let mut pwd = PathBuf::from(path);
    if pwd.is_file() {
        pwd.pop();
    }

    let names: Vec<&Path> = roots.iter().map(|x| Path::new(x)).collect();

    loop {
        for name in &names {
            if pwd.join(name).exists() {
                return Some(pwd.to_str().unwrap().to_string());
            }
        }
        if !pwd.pop() {
            return None;
        }
    }
}
