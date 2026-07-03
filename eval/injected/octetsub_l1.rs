// Derived from ripgrep (crates/searcher/src/line_buffer.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Swap every occurrence of one octet for another, reporting where the
// first swap happened.
fn rewrite_octets(
    mut buf: &mut [u8],
    needle: u8,
    fill: u8,
) -> Option<usize> {
    if needle == fill {
        return None;
    }
    let head = buf.find_byte(needle)?;
    buf[head] = fill;
    buf = &mut buf[head + 1..];
    while let Some(k) = buf.find_byte(needle) {
        buf[k] = fill;
        buf = &mut buf[k + 1..];
        while buf.get(0) == Some(&needle) {
            buf[0] = fill;
            buf = &mut buf[1..];
        }
    }
    Some(head)
}
