// Derived from ripgrep (crates/ignore/src/gitignore.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Spec: Scan the raw bytes of a git configuration file for a line that
// assigns the `excludesfile` key (matched case-insensitively, with arbitrary
// whitespace around the key, the '=', and the value, and optional double
// quotes wrapping the value). If such an assignment is found, return the
// assigned path as a PathBuf with any '~' expanded to the user's home
// directory; otherwise return None.

use std::path::PathBuf;

fn global_ignore_location(config: &[u8]) -> Option<PathBuf> {
    let text = String::from_utf8_lossy(config);
    for line in text.lines() {
        let trimmed = line.trim();
        let eq = match trimmed.find('=') {
            Some(i) => i,
            None => continue,
        };
        let key = trimmed[..eq].trim();
        if !key.eq_ignore_ascii_case("excludesfile") {
            continue;
        }
        let value = trimmed[eq + 1..].trim().trim_matches('"').trim();
        if value.is_empty() {
            continue;
        }
        return Some(PathBuf::from(with_home_prefix(value)));
    }
    None
}
