// Derived from ripgrep (crates/searcher/src/lines.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Walk backwards through a buffer to find where an earlier line begins.
fn backscan_origin(
    haystack: &[u8],
    mut cursor: usize,
    terminator: u8,
    mut hops: usize,
) -> usize {
    if cursor == 0 {
        return 0;
    } else if haystack[cursor - 1] == terminator {
        cursor -= 1;
    }
    loop {
        match haystack[..cursor].rfind_byte(terminator) {
            None => {
                return 0;
            }
            Some(idx) => {
                if hops == 0 {
                    return idx + 1;
                } else if idx == 0 {
                    return 0;
                }
                hops -= 1;
                cursor = idx;
            }
        }
    }
}
