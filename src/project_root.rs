use glob::glob;
use std::path::PathBuf;

pub fn find_project_root(roots: &[String], path: &str) -> Option<String> {
    let mut pwd = PathBuf::from(path);
    if pwd.is_file() {
        pwd.pop();
    }

    loop {
        for root in roots {
            let matches = glob(pwd.join(root).to_str().unwrap());
            if matches.is_ok() {
                let mut m = matches.unwrap();
                if m.next().is_some() {
                    return Some(pwd.to_str().unwrap().to_string());
                }
            }
        }
        if !pwd.pop() {
            return None;
        }
    }
}
