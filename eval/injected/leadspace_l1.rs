// Derived from ripgrep (crates/printer/src/util.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Planted-clone eval asset for semdup; not production code.

// Advance a range past leading blank bytes, stopping at content or at a
// line-terminator byte.
pub(crate) fn shave_leading_blanks(
    eol: LineTerminator,
    chunk: &[u8],
    span: Match,
) -> Match {
    fn is_blank(x: u8) -> bool {
        match x {
            b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r' | b' ' => true,
            _ => false,
        }
    }

    let n = chunk[span]
        .iter()
        .take_while(|&&x| -> bool {
            is_blank(x) && !eol.as_bytes().contains(&x)
        })
        .count();
    span.with_start(span.start() + n)
}
