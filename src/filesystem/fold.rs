/// Folds a path the way the default case-insensitive APFS volume compares
/// names: by full Unicode case folding, so `.ßh` opens `.ssh` and
/// `Containerſ` opens `Containers`. Protected names are ASCII, and these are
/// the only characters whose folding is ASCII but whose lowercase is not.
pub(super) fn fold(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            'ß' | 'ẞ' => folded.push_str("ss"),
            'ſ' => folded.push('s'),
            'ﬀ' => folded.push_str("ff"),
            'ﬁ' => folded.push_str("fi"),
            'ﬂ' => folded.push_str("fl"),
            'ﬃ' => folded.push_str("ffi"),
            'ﬄ' => folded.push_str("ffl"),
            'ﬅ' | 'ﬆ' => folded.push_str("st"),
            _ => folded.extend(ch.to_lowercase()),
        }
    }
    folded
}
