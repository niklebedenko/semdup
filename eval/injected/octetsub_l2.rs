// Derived from ripgrep (crates/searcher/src/line_buffer.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Same behavior, restructured: the shrinking-slice search loops collapse
// into a single indexed pass that records the earliest substitution site.
fn substitute_all(data: &mut [u8], from: u8, to: u8) -> Option<usize> {
    if from == to {
        return None;
    }
    let mut first: Option<usize> = None;
    for i in 0..data.len() {
        if data[i] != from {
            continue;
        }
        data[i] = to;
        if first.is_none() {
            first = Some(i);
        }
    }
    first
}
