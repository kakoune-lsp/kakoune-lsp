use glob::glob;
use std::path::PathBuf;

pub fn find_project_root(roots: &[String], path: &str) -> String {
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
