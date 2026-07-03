// Derived from ripgrep (crates/ignore/src/gitignore.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

use std::path::PathBuf;

/// Locate the `core.excludesfile` assignment inside git config bytes and
/// return the referenced path, tilde-expanded.
fn core_ignorefile_setting(contents: &[u8]) -> Option<PathBuf> {
    use regex_automata::{meta::Regex, util::syntax};

    let rx = Regex::builder()
        .configure(Regex::config().utf8_empty(false))
        .syntax(syntax::Config::new().utf8(false))
        .build(r#"(?im-u)^\s*excludesfile\s*=\s*"?\s*(\S+?)\s*"?\s*$"#)
        .ok()?;
    let mut cap = rx.create_captures();
    rx.captures(contents, &mut cap);
    let bytes = cap.get_group(1).map(|span| &contents[span])?;
    let text = std::str::from_utf8(bytes).ok()?;
    Some(PathBuf::from(home_expanded(text)))
}
