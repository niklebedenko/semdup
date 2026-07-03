// Derived from ripgrep (crates/searcher/src/lines.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Spec: Given a byte buffer, a position within it, a line-terminator byte,
// and a count N, return the smallest offset of the line that begins N lines
// before the line containing the position; with N = 0 this is the start of
// the containing line itself. A position sitting immediately after a
// terminator is treated as part of the line that terminator ends. If fewer
// than N earlier lines exist, the result saturates to offset 0.
fn nth_prior_line_start(buf: &[u8], pos: usize, eol: u8, n: usize) -> usize {
    // Normalize a just-past-the-terminator position back onto its line.
    let mut effective = pos;
    if effective > 0 && buf[effective - 1] == eol {
        effective -= 1;
    }
    // Gather the start offset of every line beginning at or before the
    // normalized position. Offset 0 always starts the first line.
    let mut starts = vec![0];
    for (i, &b) in buf.iter().enumerate().take(effective) {
        if b == eol {
            starts.push(i + 1);
        }
    }
    // The last collected start belongs to the current line; step back `n`
    // entries from it, clamping at the very beginning of the buffer.
    let idx = starts.len().saturating_sub(1).saturating_sub(n);
    starts[idx]
}
