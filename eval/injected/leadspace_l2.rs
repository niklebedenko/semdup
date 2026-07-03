// Derived from ripgrep (crates/printer/src/util.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Same behavior, restructured: the take_while/count iterator chain becomes
// an explicit cursor walk, and the whitespace helper is inlined as a
// matches! test.
fn skip_indent(terminator: LineTerminator, buf: &[u8], window: Match) -> Match {
    let term_bytes = terminator.as_bytes();
    let mut cursor = window.start();
    while cursor < window.end() {
        let b = buf[cursor];
        let blank = matches!(b, b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r' | b' ');
        if !blank || term_bytes.contains(&b) {
            break;
        }
        cursor += 1;
    }
    window.with_start(cursor)
}
