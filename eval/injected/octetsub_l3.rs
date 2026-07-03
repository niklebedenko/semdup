// Derived from ripgrep (crates/searcher/src/line_buffer.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Spec: Overwrite every byte in a mutable buffer that equals a given target
// value with a substitute value, and return the offset of the first byte
// that was overwritten. If the target and substitute values are the same
// byte, or the target never occurs in the buffer, return None and leave the
// buffer untouched.
fn blot_out(region: &mut [u8], victim: u8, stamp: u8) -> Option<usize> {
    if victim == stamp {
        // Nothing would change; the caller treats this as "no hit".
        return None;
    }
    let lead = match region.iter().position(|&b| b == victim) {
        Some(offset) => offset,
        None => return None,
    };
    // Everything before `lead` is already known clean, so only the tail
    // needs rewriting.
    for cell in region[lead..].iter_mut() {
        if *cell == victim {
            *cell = stamp;
        }
    }
    Some(lead)
}
