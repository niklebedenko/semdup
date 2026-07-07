// Derived from ripgrep (crates/cli/src/pattern.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Block-granularity planted-clone eval asset for semdup; not production code.

fn collect_patterns_from_stream<R: std::io::Read>(source: R) -> std::io::Result<Vec<String>> {
    let mut found = vec![];
    let mut row = 0;
    std::io::BufReader::new(source).for_byte_line(|bytes| {
        row += 1;
        match pattern_from_bytes(bytes) {
            Ok(pattern) => {
                found.push(pattern.to_string());
                Ok(true)
            }
            Err(err) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("{}: {}", row, err),
            )),
        }
    })?;
    Ok(found)
}
