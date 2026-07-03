// Derived from ripgrep (crates/globset/src/lib.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

/// Neutralize glob meta-characters in the given pattern by bracketing them.
pub fn quote_pattern(input: &str) -> String {
    let mut quoted = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            // '!' stays as-is; it only matters inside bracket classes.
            '?' | '*' | '[' | ']' => {
                quoted.push('[');
                quoted.push(ch);
                quoted.push(']');
            }
            ch => {
                quoted.push(ch);
            }
        }
    }
    quoted
}
