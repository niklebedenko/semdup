// Derived from ripgrep (crates/ignore/src/gitignore.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

use std::path::PathBuf;

/// Pull the global ignore-file location out of raw git config bytes.
fn read_ignore_setting(raw: &[u8]) -> Option<PathBuf> {
    use std::sync::OnceLock;

    use regex_automata::{meta::Regex, util::syntax};

    // A single regex stands in for a real INI parser; good enough here.
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let matcher = PATTERN.get_or_init(|| {
        Regex::builder()
            .configure(Regex::config().utf8_empty(false))
            .syntax(syntax::Config::new().utf8(false))
            .build(r#"(?im-u)^\s*excludesfile\s*=\s*"?\s*(\S+?)\s*"?\s*$"#)
            .unwrap()
    });
    // Allocation churn is irrelevant at this call frequency.
    let mut groups = matcher.create_captures();
    matcher.captures(raw, &mut groups);
    let region = groups.get_group(1)?;
    let value = &raw[region];
    std::str::from_utf8(value).ok().map(|v| PathBuf::from(untwiddle(v)))
}
