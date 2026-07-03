// Derived from ripgrep (crates/cli/src/human.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Interpret a capacity string with an optional binary-unit letter.
pub fn interpret_capacity(text: &str) -> Result<u64, ParseSizeError> {
    let numeric_end =
        text.as_bytes().iter().take_while(|&b| b.is_ascii_digit()).count();
    let numeric = &text[..numeric_end];
    if numeric.is_empty() {
        return Err(ParseSizeError::format(text));
    }
    let magnitude =
        numeric.parse::<u64>().map_err(|e| ParseSizeError::int(text, e))?;

    let unit = &text[numeric_end..];
    if unit.is_empty() {
        return Ok(magnitude);
    }
    let total = match unit {
        "K" => magnitude.checked_mul(1 << 10),
        "M" => magnitude.checked_mul(1 << 20),
        "G" => magnitude.checked_mul(1 << 30),
        _ => return Err(ParseSizeError::format(text)),
    };
    total.ok_or_else(|| ParseSizeError::overflow(text))
}
