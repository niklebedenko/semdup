// Derived from ripgrep (crates/searcher/src/lines.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Find the start offset of the line lying `back` lines before the line
// holding `at`. Restructured: match-in-loop folded into while-let, and the
// reverse byte search expressed through a reverse iterator position.
fn seek_line_head(data: &[u8], mut at: usize, eol: u8, mut back: usize) -> usize {
    if at == 0 {
        return 0;
    }
    if data[at - 1] == eol {
        // A position just past a terminator belongs to the terminated line.
        at -= 1;
    }
    while let Some(hit) = data[..at].iter().rposition(|&b| b == eol) {
        if back == 0 {
            return hit + 1;
        }
        if hit == 0 {
            return 0;
        }
        back -= 1;
        at = hit;
    }
    0
}
