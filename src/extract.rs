use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tree_sitter::{Node, Parser};

pub struct Unit {
    pub path: String,
    pub name: String,
    pub lang: String,
    pub start_line: usize,
    pub end_line: usize,
    pub hash: String,
    pub text: String,
    pub ignored: bool,
    pub is_test: bool,
}

impl Unit {
    pub fn lines(&self) -> usize {
        self.end_line - self.start_line + 1
    }
}

const DEFAULT_EXCLUDES: &[&str] = &[
    "/.git/",
    "/target/",
    "/node_modules/",
    "/dist/",
    "/vendor/",
    "/build/",
    // semdup's own eval assets must never join a real corpus: `eval/injected`
    // holds planted clones, `eval/corpus` holds third-party checkouts.
    "/eval/injected/",
    "/eval/corpus/",
];

const IGNORE_DIRECTIVE: &str = "semdup:ignore";

pub fn extract_roots(roots: &[PathBuf], extra_excludes: &[String]) -> Result<Vec<Unit>> {
    let mut files = Vec::new();
    for root in roots {
        collect_files(root, extra_excludes, &mut files)?;
    }
    files.sort();
    let mut units = Vec::new();
    for file in &files {
        let src = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue, // non-utf8 or unreadable: skip
        };
        units.extend(
            extract_file(file, &src).with_context(|| format!("extracting {}", file.display()))?,
        );
    }
    Ok(units)
}

fn collect_files(dir: &Path, extra_excludes: &[String], out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let path_str = format!("{}/", path.display());
        if DEFAULT_EXCLUDES.iter().any(|e| path_str.contains(e))
            || extra_excludes.iter().any(|e| path_str.contains(e.as_str()))
        {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, extra_excludes, out)?;
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "ts" | "tsx")
        ) {
            out.push(path);
        }
    }
    Ok(())
}

pub fn extract_file(path: &Path, src: &str) -> Result<Vec<Unit>> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let (language, lang_name) = match ext {
        "rs" => (tree_sitter_rust::LANGUAGE.into(), "rust"),
        "ts" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "typescript",
        ),
        "tsx" => (tree_sitter_typescript::LANGUAGE_TSX.into(), "typescript"),
        _ => return Ok(Vec::new()),
    };
    let mut parser = Parser::new();
    parser.set_language(&language)?;
    let Some(tree) = parser.parse(src, None) else {
        return Ok(Vec::new());
    };
    let lines: Vec<&str> = src.lines().collect();
    let path_str = path.display().to_string();
    let path_is_test = is_test_path(&path_str);
    let mut units = Vec::new();
    walk(
        tree.root_node(),
        src,
        &lines,
        &path_str,
        lang_name,
        path_is_test,
        &mut units,
    );
    Ok(units)
}

fn walk(
    node: Node,
    src: &str,
    lines: &[&str],
    path: &str,
    lang: &str,
    path_is_test: bool,
    out: &mut Vec<Unit>,
) {
    if let Some((name, span_node)) = unit_of(node, src, lang) {
        let start_line = span_node.start_position().row + 1;
        let end_line = span_node.end_position().row + 1;
        let text = &src[span_node.byte_range()];
        out.push(Unit {
            path: path.to_string(),
            name,
            lang: lang.to_string(),
            start_line,
            end_line,
            hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
            text: text.to_string(),
            ignored: has_ignore_directive(lines, start_line),
            is_test: path_is_test || is_test_node(node, src, lang),
        });
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, lines, path, lang, path_is_test, out);
    }
}

/// Returns (name, node spanning the unit text) if `node` starts a function-like unit.
fn unit_of<'a>(node: Node<'a>, src: &str, lang: &str) -> Option<(String, Node<'a>)> {
    let kind = node.kind();
    let name_text = |n: Node| src[n.byte_range()].to_string();
    match lang {
        "rust" => {
            if kind == "function_item" {
                let name = node.child_by_field_name("name")?;
                Some((name_text(name), node))
            } else {
                None
            }
        }
        "typescript" => match kind {
            "function_declaration" | "generator_function_declaration" | "method_definition" => {
                let name = node.child_by_field_name("name")?;
                Some((name_text(name), node))
            }
            "variable_declarator" => {
                let value = node.child_by_field_name("value")?;
                if matches!(value.kind(), "arrow_function" | "function_expression") {
                    let name = node.child_by_field_name("name")?;
                    Some((name_text(name), node))
                } else {
                    None
                }
            }
            _ => None,
        },
        _ => None,
    }
}

fn has_ignore_directive(lines: &[&str], start_line: usize) -> bool {
    let lo = start_line.saturating_sub(4); // three lines above + the signature line
    lines[lo..start_line.min(lines.len())]
        .iter()
        .any(|l| l.contains(IGNORE_DIRECTIVE))
}

fn is_test_path(path: &str) -> bool {
    path.contains("/tests/")
        || path.ends_with("_test.rs")
        || path.ends_with("tests.rs")
        || path.contains(".test.")
        || path.contains(".spec.")
        || path.contains("__tests__")
}

/// Rust: `#[test]`-style attribute directly above, or an enclosing `mod tests`.
fn is_test_node(node: Node, src: &str, lang: &str) -> bool {
    if lang != "rust" {
        return false;
    }
    let mut sib = node.prev_named_sibling();
    while let Some(s) = sib {
        if s.kind() != "attribute_item" {
            break;
        }
        if src[s.byte_range()].contains("test") {
            return true;
        }
        sib = s.prev_named_sibling();
    }
    let mut anc = node.parent();
    while let Some(a) = anc {
        if a.kind() == "mod_item"
            && let Some(name) = a.child_by_field_name("name")
            && &src[name.byte_range()] == "tests"
        {
            return true;
        }
        anc = a.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_extraction_names_spans_and_directives() {
        let src = r#"
// semdup:ignore — mirror of write_thing by design
fn read_thing(x: u32) -> u32 {
    x + 1
}

fn write_thing(x: u32) -> u32 {
    x + 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn check() {
        assert_eq!(super::read_thing(1), 2);
    }
}
"#;
        let units = extract_file(Path::new("lib.rs"), src).unwrap();
        let by_name = |n: &str| units.iter().find(|u| u.name == n).unwrap();
        assert_eq!(units.len(), 3);
        assert!(by_name("read_thing").ignored);
        assert!(!by_name("write_thing").ignored);
        assert!(!by_name("write_thing").is_test);
        assert!(by_name("check").is_test);
        assert_eq!(by_name("read_thing").start_line, 3);
        assert_eq!(by_name("read_thing").end_line, 5);
    }

    #[test]
    fn typescript_extraction_functions_and_arrows() {
        let src = r#"
export function plain(a: number): number {
    return a * 2;
}

const arrow = (a: number) => {
    return a * 3;
};

class C {
    method(a: number) {
        return a * 4;
    }
}

const notAFunction = 42;
"#;
        let units = extract_file(Path::new("mod.ts"), src).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["plain", "arrow", "method"]);
    }

    #[test]
    fn test_paths_are_flagged() {
        assert!(is_test_path("src/foo.test.ts"));
        assert!(is_test_path("crate/tests/it.rs"));
        assert!(!is_test_path("src/attest.rs"));
    }
}
