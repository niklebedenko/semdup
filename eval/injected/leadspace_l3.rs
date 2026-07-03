// Derived from ripgrep (crates/printer/src/util.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Spec: Given a line terminator, a byte slice, and a sub-range selecting
// part of it, return the same range with its start advanced past any
// leading ASCII whitespace bytes. Bytes belonging to the line terminator
// never count as skippable whitespace and halt the advance. The advance
// also halts at the first non-whitespace byte and never moves the start
// past the end of the range.
fn dedent_span(term: LineTerminator, text: &[u8], sel: Match) -> Match {
    const BLANKS: &[u8] = b" \t\n\x0B\x0C\r";
    let stoppers = term.as_bytes();
    let body = &text[sel];
    let keep_moving = |b: &u8| BLANKS.contains(b) && !stoppers.contains(b);
    let offset = match body.iter().position(|b| !keep_moving(b)) {
        Some(first_solid) => first_solid,
        // The whole selection was skippable indentation.
        None => body.len(),
    };
    sel.with_start(sel.start() + offset)
}
