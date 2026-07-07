// Derived from ripgrep (crates/cli/src/pattern.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Block-granularity planted-clone eval asset for semdup; not production code.

fn read_pattern_lines<R: std::io::Read>(reader: R) -> std::io::Result<Vec<String>> {
    let mut collected = Vec::new();
    let mut line_no = 0usize;
    let mut buffered = std::io::BufReader::new(reader);
    buffered.for_byte_line(|raw| {
        line_no += 1;
        let parsed = pattern_from_bytes(raw).map(|pattern| pattern.to_string());
        match parsed {
            Ok(text) => {
                collected.push(text);
                Ok(true)
            }
            Err(error) => {
                let message = format!("{}: {}", line_no, error);
                Err(std::io::Error::new(std::io::ErrorKind::Other, message))
            }
        }
    })?;
    Ok(collected)
}
