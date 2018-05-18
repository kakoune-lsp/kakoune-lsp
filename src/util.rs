use url::Url;

pub fn path_to_uri(path: &str) -> Url {
    let mut url = Url::parse("file:///").unwrap();
    url.set_path(path);
    url
}
