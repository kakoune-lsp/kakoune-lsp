use glob::glob;
use std::path::PathBuf;

pub fn find_project_root(roots: &[String], path: &str) -> Option<String> {
    let mut pwd = PathBuf::from(path);
    if pwd.is_file() {
        pwd.pop();
    }

    loop {
        for root in roots {
            // unwrap should be safe here because we walk up path previously converted from str
            let matches = glob(pwd.join(root).to_str().unwrap());
            match matches {
                Ok(mut m) => if m.next().is_some() {
                    // ditto unwrap
                    return Some(pwd.to_str().unwrap().to_string());
                },
                _ => (),
            }
        }
        if !pwd.pop() {
            return None;
        }
    }
}
