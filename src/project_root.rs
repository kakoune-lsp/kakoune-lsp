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
            match matches {
                Ok(mut m) => if m.next().is_some() {
                    // ditto unwrap
                    return pwd.to_str().unwrap().to_string();
                },
                _ => (),
            }
        }
        if !pwd.pop() {
            return src;
        }
    }
}
