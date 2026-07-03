// Derived from ripgrep (crates/cli/src/human.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Turn a quota string like "64M" into a raw byte count.
fn read_quota(input: &str) -> Result<u64, ParseSizeError> {
    let split = input
        .as_bytes()
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(input.len());
    let (num_part, unit_part) = input.split_at(split);
    if num_part.is_empty() {
        return Err(ParseSizeError::format(input));
    }
    let base: u64 =
        num_part.parse().map_err(|e| ParseSizeError::int(input, e))?;
    let shift: u32 = match unit_part {
        "" => return Ok(base),
        "K" => 10,
        "M" => 20,
        "G" => 30,
        _ => return Err(ParseSizeError::format(input)),
    };
    base.checked_mul(1u64 << shift)
        .ok_or_else(|| ParseSizeError::overflow(input))
}
