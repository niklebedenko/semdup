// Derived from ripgrep (crates/globset/src/lib.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Spec: Given a glob pattern string, return a new string in which every glob
// meta-character ('?', '*', '[', ']') has been made literal by enclosing it
// in a single-character bracket class, e.g. '*' becomes "[*]". Every other
// character is emitted unchanged, in the original order.
pub fn defang_glob(raw: &str) -> String {
    const SPECIALS: [char; 4] = ['?', '*', '[', ']'];
    let mut rendered = String::new();
    let mut rest = raw;
    while let Some(pos) = rest.find(|c: char| SPECIALS.contains(&c)) {
        rendered.push_str(&rest[..pos]);
        let meta = rest[pos..].chars().next().unwrap();
        rendered.push('[');
        rendered.push(meta);
        rendered.push(']');
        rest = &rest[pos + meta.len_utf8()..];
    }
    rendered.push_str(rest);
    rendered
}
