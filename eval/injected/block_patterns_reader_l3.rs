// Derived from ripgrep (crates/cli/src/pattern.rs) at 4649aa9700619f94cf9c66876e9549d83420e16c,
// dual-licensed MIT OR Unlicense. Block-granularity planted-clone eval asset for semdup; not production code.

fn load_pattern_file<R: std::io::Read>(input: R) -> std::io::Result<Vec<String>> {
    let mut output = Vec::new();
    let mut current_line = 0usize;
    std::io::BufReader::new(input).for_byte_line(|line_bytes| {
        current_line += 1;
        let compiled = match pattern_from_bytes(line_bytes) {
            Ok(pattern) => pattern.to_string(),
            Err(problem) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("{}: {}", current_line, problem),
                ));
            }
        };
        output.push(compiled);
        Ok(true)
    })?;
    Ok(output)
}
