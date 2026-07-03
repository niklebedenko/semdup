// Derived from ripgrep (crates/cli/src/human.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Spec: Parse a string made of a decimal integer optionally followed by
// exactly one of the suffixes "K", "M", or "G", scaling the number by
// 2^10, 2^20, or 2^30 respectively, and return the result as a u64 byte
// count. It is an error if there are no leading digits, if anything other
// than one of those three suffixes trails the digits, if the digit run
// does not fit in a u64, or if applying the multiplier overflows.

fn decode_limit(spec: &str) -> Result<u64, ParseSizeError> {
    let mut digit_count = 0usize;
    for ch in spec.chars() {
        if !ch.is_ascii_digit() {
            break;
        }
        digit_count += 1;
    }
    if digit_count == 0 {
        return Err(ParseSizeError::format(spec));
    }
    let amount = spec[..digit_count]
        .parse::<u64>()
        .map_err(|e| ParseSizeError::int(spec, e))?;
    let factor: u64 = match &spec[digit_count..] {
        "" => 1,
        "K" => 1024,
        "M" => 1024 * 1024,
        "G" => 1024 * 1024 * 1024,
        _ => return Err(ParseSizeError::format(spec)),
    };
    amount
        .checked_mul(factor)
        .ok_or_else(|| ParseSizeError::overflow(spec))
}
