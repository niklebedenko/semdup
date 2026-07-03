// Derived from ripgrep (crates/globset/src/lib.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

/// Render glob syntax inert by wrapping each meta-character in a
/// one-character bracket class.
pub fn neutralize_specials(pattern: &str) -> String {
    let is_meta = |c: char| matches!(c, '?' | '*' | '[' | ']');
    pattern.chars().fold(
        String::with_capacity(pattern.len()),
        |mut acc, c| {
            if is_meta(c) {
                acc.push('[');
                acc.push(c);
                acc.push(']');
            } else {
                acc.push(c);
            }
            acc
        },
    )
}
