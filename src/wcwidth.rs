extern "C" {
    pub fn wcwidth(c: libc::wchar_t) -> libc::c_int;
}

pub fn expected_width_or_fallback<'a>(
    emoji_str: &'a str,
    expected_width: usize,
    fallback: &'a str,
) -> &'a str {
    let mut chars = emoji_str.chars();
    let emoji = chars.next().unwrap();
    assert!(chars.next().is_none());
    let libc_width = unsafe { wcwidth(emoji as libc::wchar_t) };
    if libc_width as usize == expected_width {
        emoji_str
    } else {
        fallback
    }
}
