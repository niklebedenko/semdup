//! Source-code extraction built on tree-sitter grammars.
//!
//! The extractor walks supported files, turns function-like syntax nodes into
//! hash-addressed units, and tags suppression directives and tests so later
//! scan modes can filter them consistently.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::ValueEnum;
use ignore::WalkBuilder;
use serde::Deserialize;
use tree_sitter::{Node, Parser};

const DEFAULT_MIN_BLOCK_LINES: usize = 8;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum UnitKind {
    Function,
    Block,
}

impl UnitKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitKind::Function => "function",
            UnitKind::Block => "block",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "function" => Some(UnitKind::Function),
            "block" => Some(UnitKind::Block),
            _ => None,
        }
    }
}

pub struct Unit {
    pub path: String,
    pub name: String,
    pub lang: String,
    pub kind: UnitKind,
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
    // semdup's own eval assets must never join a real corpus: `eval/injected`
    // holds planted clones, `eval/corpus` holds third-party checkouts.
    "/eval/injected/",
    "/eval/corpus/",
];

const IGNORE_DIRECTIVE: &str = "semdup:ignore";

#[derive(Clone)]
pub struct ExtractOpts {
    pub strip_comments: bool,
    pub respect_gitignore: bool,
    pub granularity: Vec<UnitKind>,
    pub min_block_lines: usize,
}

impl Default for ExtractOpts {
    fn default() -> Self {
        ExtractOpts {
            strip_comments: false,
            respect_gitignore: true,
            granularity: vec![UnitKind::Function, UnitKind::Block],
            min_block_lines: DEFAULT_MIN_BLOCK_LINES,
        }
    }
}

impl ExtractOpts {
    fn includes(&self, kind: UnitKind) -> bool {
        self.granularity.contains(&kind)
    }
}

/// File extensions `extract_file` knows how to parse (see the language table
/// there for how each maps to a grammar).
pub const SUPPORTED_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "py", "go", "java", "cs", "php", "rb", "c", "h", "cpp", "cc", "cxx", "hpp",
    "hh", "hxx",
];

pub fn extract_roots(
    roots: &[PathBuf],
    extra_excludes: &[String],
    opts: ExtractOpts,
) -> Result<Vec<Unit>> {
    let mut files = Vec::new();
    for root in roots {
        collect_files(root, extra_excludes, opts.respect_gitignore, &mut files)?;
    }
    files.sort();
    let mut units = Vec::new();
    for file in &files {
        let src = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(_) => continue, // non-utf8 or unreadable: skip
        };
        units.extend(
            extract_file_with_opts(file, &src, &opts)
                .with_context(|| format!("extracting {}", file.display()))?,
        );
    }
    Ok(units)
}

/// Substring match against the default and configured exclude patterns.
/// Callers control anchoring via the string they pass: extraction walkers use
/// display paths with a trailing `/`, while diff passes repo-relative file
/// paths prefixed with `/` so patterns anchor at the repo root.
pub fn is_path_excluded(path_str: &str, extra_excludes: &[String]) -> bool {
    DEFAULT_EXCLUDES.iter().any(|e| path_str.contains(e))
        || extra_excludes.iter().any(|e| path_str.contains(e.as_str()))
}

fn collect_files(
    dir: &Path,
    extra_excludes: &[String],
    respect_gitignore: bool,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    if respect_gitignore {
        collect_files_gitignore(dir, extra_excludes, out)
    } else {
        collect_files_plain(dir, extra_excludes, out)
    }
}

fn collect_files_gitignore(
    dir: &Path,
    extra_excludes: &[String],
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut builder = WalkBuilder::new(dir);
    let extra_excludes = extra_excludes.to_vec();
    builder
        .hidden(false)
        .parents(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false)
        .ignore(false)
        .filter_entry(move |entry| {
            !is_path_excluded(&format!("{}/", entry.path().display()), &extra_excludes)
        });
    for entry in builder.build() {
        let entry = entry.with_context(|| format!("walking {}", dir.display()))?;
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| SUPPORTED_EXTS.contains(&e))
        {
            out.push(path.to_path_buf());
        }
    }
    Ok(())
}

fn collect_files_plain(
    dir: &Path,
    extra_excludes: &[String],
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let path_str = format!("{}/", path.display());
        if is_path_excluded(&path_str, extra_excludes) {
            continue;
        }
        if path.is_dir() {
            collect_files_plain(&path, extra_excludes, out)?;
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| SUPPORTED_EXTS.contains(&e))
        {
            out.push(path);
        }
    }
    Ok(())
}

pub fn extract_file(path: &Path, src: &str, strip_comments: bool) -> Result<Vec<Unit>> {
    extract_file_with_opts(
        path,
        src,
        &ExtractOpts {
            strip_comments,
            granularity: vec![UnitKind::Function],
            ..ExtractOpts::default()
        },
    )
}

pub fn extract_file_with_opts(path: &Path, src: &str, opts: &ExtractOpts) -> Result<Vec<Unit>> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let (language, lang_name) = match ext {
        "rs" => (tree_sitter_rust::LANGUAGE.into(), "rust"),
        "ts" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "typescript",
        ),
        "tsx" => (tree_sitter_typescript::LANGUAGE_TSX.into(), "typescript"),
        "py" => (tree_sitter_python::LANGUAGE.into(), "python"),
        "go" => (tree_sitter_go::LANGUAGE.into(), "go"),
        "java" => (tree_sitter_java::LANGUAGE.into(), "java"),
        "cs" => (tree_sitter_c_sharp::LANGUAGE.into(), "csharp"),
        "php" => (tree_sitter_php::LANGUAGE_PHP.into(), "php"),
        "rb" => (tree_sitter_ruby::LANGUAGE.into(), "ruby"),
        // .h goes through the C++ grammar, which is a superset of the C one,
        // so plain-C headers extract fine and C++ headers get full template
        // support. Both C grammars parse the raw, unpreprocessed source:
        // function-like macros are not extracted, and heavy #if/#else use can
        // hide the branch not taken from the parser.
        "c" => (tree_sitter_c::LANGUAGE.into(), "c"),
        "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => {
            (tree_sitter_cpp::LANGUAGE.into(), "cpp")
        }
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
        opts,
        None,
        &mut units,
    );
    Ok(units)
}

#[derive(Clone)]
struct FunctionContext {
    name: String,
    ignored: bool,
    is_test: bool,
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    src: &str,
    lines: &[&str],
    path: &str,
    lang: &str,
    path_is_test: bool,
    opts: &ExtractOpts,
    function_context: Option<&FunctionContext>,
    out: &mut Vec<Unit>,
) {
    let mut child_context = function_context.cloned();
    if let Some((name, span_node)) = unit_of(node, src, lang) {
        let ignored = has_ignore_directive(lines, span_node.start_position().row + 1);
        let is_test = path_is_test || is_test_node(node, src, lang);
        if opts.includes(UnitKind::Function) {
            push_unit(
                out,
                UnitSpec {
                    path,
                    name: name.clone(),
                    lang,
                    kind: UnitKind::Function,
                    span_node,
                    src,
                    lines,
                    strip_comments: opts.strip_comments,
                    ignored,
                    is_test,
                    min_lines: 1,
                },
            );
        }
        child_context = Some(FunctionContext {
            name,
            ignored,
            is_test,
        });
    }

    if opts.includes(UnitKind::Block)
        && let Some(ctx) = function_context
        && is_executable_block(node, lang)
    {
        let start_line = node.start_position().row + 1;
        push_unit(
            out,
            UnitSpec {
                path,
                name: format!("{}::{}", ctx.name, block_name(node)),
                lang,
                kind: UnitKind::Block,
                span_node: node,
                src,
                lines,
                strip_comments: opts.strip_comments,
                ignored: ctx.ignored || has_ignore_directive(lines, start_line),
                is_test: path_is_test || ctx.is_test,
                min_lines: opts.min_block_lines,
            },
        );
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            src,
            lines,
            path,
            lang,
            path_is_test,
            opts,
            child_context.as_ref(),
            out,
        );
    }
}

struct UnitSpec<'a> {
    path: &'a str,
    name: String,
    lang: &'a str,
    kind: UnitKind,
    span_node: Node<'a>,
    src: &'a str,
    lines: &'a [&'a str],
    strip_comments: bool,
    ignored: bool,
    is_test: bool,
    min_lines: usize,
}

fn push_unit(out: &mut Vec<Unit>, spec: UnitSpec<'_>) {
    let start_line = spec.span_node.start_position().row + 1;
    let end_line = spec.span_node.end_position().row + 1;
    if end_line.saturating_sub(start_line) + 1 < spec.min_lines {
        return;
    }
    let text = if spec.strip_comments {
        stripped_text(spec.span_node, spec.src, spec.lang)
    } else {
        spec.src[spec.span_node.byte_range()].to_string()
    };
    if text.trim().is_empty() {
        return;
    }
    out.push(Unit {
        path: spec.path.to_string(),
        name: spec.name,
        lang: spec.lang.to_string(),
        kind: spec.kind,
        start_line,
        end_line,
        hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
        text,
        ignored: spec.ignored || has_ignore_directive(spec.lines, start_line),
        is_test: spec.is_test,
    });
}

fn is_executable_block(node: Node, lang: &str) -> bool {
    let kind = node.kind();
    match lang {
        "rust" => matches!(kind, "block" | "match_block"),
        "typescript" => kind == "statement_block",
        "python" => kind == "block",
        "go" | "java" | "csharp" => kind == "block",
        "php" | "c" | "cpp" => kind == "compound_statement",
        // Ruby has several body-like nodes, but they do not map as cleanly to
        // a single executable block; leave it function-only for now.
        _ => false,
    }
}

fn block_name(node: Node) -> &'static str {
    let Some(parent) = node.parent() else {
        return "block";
    };
    match parent.kind() {
        "if_expression" | "if_statement" => "if",
        "else_clause" => "else",
        "for_expression" | "for_statement" | "for_in_clause" => "for",
        "while_expression" | "while_statement" => "while",
        "loop_expression" => "loop",
        "match_expression" | "switch_expression" | "switch_statement" => "match",
        "try_statement" | "try_expression" => "try",
        "catch_clause" | "except_clause" => "catch",
        "function_item"
        | "function_definition"
        | "function_declaration"
        | "generator_function_declaration"
        | "method_definition"
        | "method_declaration"
        | "constructor_declaration"
        | "local_function_statement" => "body",
        "arrow_function" | "function_expression" | "closure_expression" => "closure",
        _ => "block",
    }
}

/// Unit text with comments (and Python docstrings) removed, for measuring
/// how much of the embedding signal is prose vs. code. Lines left empty by
/// the removal are dropped.
fn stripped_text(span: Node, src: &str, lang: &str) -> String {
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    collect_strip_ranges(span, lang, &mut cuts);
    let text = &src[span.byte_range()];
    if cuts.is_empty() {
        return text.to_string();
    }
    let base = span.start_byte();
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for (s, e) in cuts {
        let (s, e) = (s - base, e - base);
        if s > pos {
            out.push_str(&text[pos..s]);
        }
        pos = pos.max(e);
    }
    out.push_str(&text[pos..]);
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_strip_ranges(node: Node, lang: &str, out: &mut Vec<(usize, usize)>) {
    // Kind names across our grammars: rust/java use line_comment/block_comment,
    // the rest use comment.
    if matches!(node.kind(), "comment" | "line_comment" | "block_comment") {
        out.push((node.start_byte(), node.end_byte()));
        return;
    }
    if lang == "python" && is_python_docstring(node) {
        out.push((node.start_byte(), node.end_byte()));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_strip_ranges(child, lang, out);
    }
}

/// A bare string as the first statement of a function body.
fn is_python_docstring(node: Node) -> bool {
    node.kind() == "expression_statement"
        && node.named_child_count() == 1
        && node.named_child(0).is_some_and(|c| c.kind() == "string")
        && node.parent().is_some_and(|block| {
            block.kind() == "block"
                && block.named_child(0).is_some_and(|first| first == node)
                && block
                    .parent()
                    .is_some_and(|f| f.kind() == "function_definition")
        })
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
        "python" => {
            if kind == "function_definition" {
                let name = node.child_by_field_name("name")?;
                // Span the decorators too: they are part of what the function means.
                let span = match node.parent() {
                    Some(p) if p.kind() == "decorated_definition" => p,
                    _ => node,
                };
                Some((name_text(name), span))
            } else {
                None
            }
        }
        "go" => match kind {
            "function_declaration" | "method_declaration" => {
                let name = node.child_by_field_name("name")?;
                Some((name_text(name), node))
            }
            _ => None,
        },
        "java" => match kind {
            "method_declaration" | "constructor_declaration" => {
                let name = node.child_by_field_name("name")?;
                Some((name_text(name), node))
            }
            _ => None,
        },
        "csharp" => match kind {
            "method_declaration" | "constructor_declaration" | "local_function_statement" => {
                let name = node.child_by_field_name("name")?;
                Some((name_text(name), node))
            }
            _ => None,
        },
        "php" => match kind {
            "function_definition" | "method_declaration" => {
                let name = node.child_by_field_name("name")?;
                Some((name_text(name), node))
            }
            _ => None,
        },
        "ruby" => match kind {
            // `def foo` and `def self.foo`.
            "method" | "singleton_method" => {
                let name = node.child_by_field_name("name")?;
                Some((name_text(name), node))
            }
            _ => None,
        },
        "c" | "cpp" => {
            if kind == "function_definition" {
                let name = c_function_name(node)?;
                // Span the template header too, like Python decorators.
                let span = match node.parent() {
                    Some(p) if p.kind() == "template_declaration" => p,
                    _ => node,
                };
                Some((name_text(name), span))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// C/C++: the function name hides at the bottom of a declarator chain
/// (`*(*name(args))`, `Class::name`, `operator==`, ...). Descend to it.
fn c_function_name(node: Node<'_>) -> Option<Node<'_>> {
    let mut decl = node.child_by_field_name("declarator")?;
    // Bounded: real declarator chains are shallow; bail on pathological input.
    for _ in 0..8 {
        match decl.kind() {
            "identifier"
            | "field_identifier"
            | "qualified_identifier"
            | "destructor_name"
            | "operator_name" => return Some(decl),
            "function_declarator" | "pointer_declarator" | "reference_declarator" => {
                decl = decl.child_by_field_name("declarator")?;
            }
            "parenthesized_declarator" => decl = decl.named_child(0)?,
            _ => return None,
        }
    }
    None
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
        || path.ends_with("_test.go")
        || path.ends_with("_test.py")
        || path.contains("/test_")
        || path.contains("/src/test/")
        || path.contains("/conftest")
        || path.ends_with("_spec.rb")
        || path.ends_with("_test.rb")
        || path.contains("/spec/")
        || path.ends_with("Test.php")
        || path.ends_with("Tests.cs")
        || path.contains(".Tests/")
}

fn is_test_node(node: Node, src: &str, lang: &str) -> bool {
    match lang {
        "rust" => is_rust_test_node(node, src),
        // pytest collects test_* functions; unittest methods also start with test.
        "python" => node
            .child_by_field_name("name")
            .is_some_and(|n| src[n.byte_range()].starts_with("test")),
        // go test only runs Test*/Benchmark*/Fuzz*/Example* in _test.go files,
        // which is_test_path already catches; name alone is not a test marker.
        "go" => false,
        // JUnit-style annotation in the method's modifiers (@Test, @ParameterizedTest, ...).
        "java" => node.children(&mut node.walk()).any(|c| {
            c.kind() == "modifiers"
                && src[c.byte_range()]
                    .lines()
                    .any(|l| l.trim_start().starts_with("@") && l.contains("Test"))
        }),
        // NUnit/xUnit/MSTest mark tests with attributes on the method.
        "csharp" => node.children(&mut node.walk()).any(|c| {
            c.kind() == "attribute_list"
                && ["Test", "Fact", "Theory"]
                    .iter()
                    .any(|m| src[c.byte_range()].contains(m))
        }),
        // PHPUnit runs public methods whose name starts with "test".
        "php" => node
            .child_by_field_name("name")
            .is_some_and(|n| src[n.byte_range()].starts_with("test")),
        // minitest runs test_* methods; RSpec is block-based and lives under
        // /spec/, which is_test_path already catches.
        "ruby" => node
            .child_by_field_name("name")
            .is_some_and(|n| src[n.byte_range()].starts_with("test_")),
        // C/C++ have no universal in-source test marker; path rules only.
        _ => false,
    }
}

/// Rust: `#[test]`-style attribute directly above, or an enclosing `mod tests`.
// semdup:ignore — per-language variant of is_test_node's dispatch; parallel by design
fn is_rust_test_node(node: Node, src: &str) -> bool {
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
    fn exclude_matching_anchors_by_caller_convention() {
        let extra = vec!["/.github/".to_string()];
        // diff convention: repo-relative file path with a leading `/`.
        assert!(is_path_excluded("/.github/smoke/plant.rs", &extra));
        assert!(is_path_excluded("/.git/hooks/post-commit", &[]));
        assert!(!is_path_excluded("/src/vendor/lib.rs", &[]));
        assert!(!is_path_excluded("/src/main.rs", &extra));
        // collect_files convention: no leading slash, so a default exclude
        // does not fire when it is itself the explicit root.
        assert!(!is_path_excluded("eval/corpus/flask/app.py/", &[]));
        assert!(is_path_excluded("repo/eval/corpus/flask/app.py/", &[]));
    }

    #[test]
    fn extract_roots_respects_gitignore_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "generated/\n").unwrap();
        std::fs::create_dir(tmp.path().join("src")).unwrap();
        std::fs::create_dir(tmp.path().join("generated")).unwrap();
        std::fs::write(tmp.path().join("src/lib.rs"), "fn kept() {}\n").unwrap();
        std::fs::write(tmp.path().join("generated/out.rs"), "fn ignored() {}\n").unwrap();
        let roots = vec![tmp.path().to_path_buf()];

        let respected = extract_roots(
            &roots,
            &[],
            ExtractOpts {
                strip_comments: false,
                respect_gitignore: true,
                ..ExtractOpts::default()
            },
        )
        .unwrap();
        assert_eq!(respected.len(), 1);
        assert_eq!(respected[0].name, "kept");

        let ignored_disabled = extract_roots(
            &roots,
            &[],
            ExtractOpts {
                strip_comments: false,
                respect_gitignore: false,
                ..ExtractOpts::default()
            },
        )
        .unwrap();
        let mut names: Vec<_> = ignored_disabled.iter().map(|u| u.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["ignored", "kept"]);
    }

    #[test]
    fn extract_roots_forwards_block_granularity() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("lib.rs"),
            "fn kept(xs: &[u32]) -> u32 {\n    for x in xs {\n        if *x > 1 {\n            return *x;\n        }\n    }\n    0\n}\n",
        )
        .unwrap();
        let roots = vec![tmp.path().to_path_buf()];
        let units = extract_roots(
            &roots,
            &[],
            ExtractOpts {
                granularity: vec![UnitKind::Function, UnitKind::Block],
                min_block_lines: 1,
                ..ExtractOpts::default()
            },
        )
        .unwrap();
        assert!(units.iter().any(|u| u.kind == UnitKind::Function));
        assert!(units.iter().any(|u| u.kind == UnitKind::Block));
    }

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
        let units = extract_file(Path::new("lib.rs"), src, false).unwrap();
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
    fn rust_block_granularity_extracts_nested_executable_blocks() {
        let src = r#"
// semdup:ignore - duplicated state machine by design
fn outer(xs: &[u32]) -> u32 {
    let mut sum = 0;
    for x in xs {
        if *x > 10 {
            sum += x;
        }
    }
    sum
}
"#;
        let units = extract_file_with_opts(
            Path::new("lib.rs"),
            src,
            &ExtractOpts {
                strip_comments: false,
                granularity: vec![UnitKind::Function, UnitKind::Block],
                min_block_lines: 1,
                ..ExtractOpts::default()
            },
        )
        .unwrap();
        let function = units
            .iter()
            .find(|u| u.kind == UnitKind::Function && u.name == "outer")
            .unwrap();
        assert!(function.ignored);
        let blocks: Vec<_> = units.iter().filter(|u| u.kind == UnitKind::Block).collect();
        assert!(blocks.len() >= 3);
        assert!(blocks.iter().all(|u| u.ignored));
        assert!(blocks.iter().any(|u| u.name == "outer::body"));
        assert!(blocks.iter().any(|u| u.name == "outer::for"));
        assert!(blocks.iter().any(|u| u.name == "outer::if"));
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
        let units = extract_file(Path::new("mod.ts"), src, false).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["plain", "arrow", "method"]);
    }

    #[test]
    fn python_extraction_functions_methods_decorators() {
        let src = r#"
def plain(a):
    return a * 2

@lru_cache
def decorated(a):
    return a * 3

class C:
    def method(self, a):
        return a * 4

def test_plain():
    assert plain(1) == 2
"#;
        let units = extract_file(Path::new("mod.py"), src, false).unwrap();
        let by_name = |n: &str| units.iter().find(|u| u.name == n).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["plain", "decorated", "method", "test_plain"]);
        // Decorated span starts at the decorator line.
        assert_eq!(by_name("decorated").start_line, 5);
        assert!(by_name("decorated").text.starts_with("@lru_cache"));
        assert!(by_name("test_plain").is_test);
        assert!(!by_name("method").is_test);
    }

    #[test]
    fn go_extraction_functions_and_methods() {
        let src = r#"
package main

func plain(a int) int {
	return a * 2
}

func (r *Recv) method(a int) int {
	return a * 3
}
"#;
        let units = extract_file(Path::new("main.go"), src, false).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["plain", "method"]);
        assert!(units.iter().all(|u| !u.is_test));
    }

    #[test]
    fn java_extraction_methods_constructors_junit() {
        let src = r#"
class Widget {
    Widget(int size) {
        this.size = size;
    }

    int grow(int by) {
        return size + by;
    }

    @Test
    void growsByAmount() {
        assert grow(1) == 2;
    }
}
"#;
        let units = extract_file(Path::new("Widget.java"), src, false).unwrap();
        let by_name = |n: &str| units.iter().find(|u| u.name == n).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["Widget", "grow", "growsByAmount"]);
        assert!(by_name("growsByAmount").is_test);
        assert!(!by_name("grow").is_test);
    }

    #[test]
    fn csharp_extraction_methods_constructors_locals() {
        let src = r#"
class Widget {
    Widget(int size) {
        _size = size;
    }

    int Grow(int by) {
        int Doubled(int x) => x * 2;
        return _size + Doubled(by);
    }

    [Fact]
    public void GrowsByAmount() {
        Assert.Equal(3, new Widget(1).Grow(1));
    }
}
"#;
        let units = extract_file(Path::new("Widget.cs"), src, false).unwrap();
        let by_name = |n: &str| units.iter().find(|u| u.name == n).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["Widget", "Grow", "Doubled", "GrowsByAmount"]);
        assert!(by_name("GrowsByAmount").is_test);
        assert!(!by_name("Grow").is_test);
    }

    #[test]
    fn php_extraction_functions_and_methods() {
        let src = r#"<?php
function plain($a) {
    return $a * 2;
}

class C {
    public function method($a) {
        return $a * 4;
    }

    public function testMethod() {
        assert($this->method(1) === 4);
    }
}
"#;
        let units = extract_file(Path::new("mod.php"), src, false).unwrap();
        let by_name = |n: &str| units.iter().find(|u| u.name == n).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["plain", "method", "testMethod"]);
        assert!(by_name("testMethod").is_test);
        assert!(!by_name("method").is_test);
    }

    #[test]
    fn ruby_extraction_methods_and_singletons() {
        let src = r#"
def plain(a)
  a * 2
end

class C
  def method_a(a)
    a * 4
  end

  def self.builder(a)
    new(a)
  end

  def test_method_a
    raise unless method_a(1) == 4
  end
end
"#;
        let units = extract_file(Path::new("mod.rb"), src, false).unwrap();
        let by_name = |n: &str| units.iter().find(|u| u.name == n).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["plain", "method_a", "builder", "test_method_a"]);
        assert!(by_name("test_method_a").is_test);
        assert!(!by_name("builder").is_test);
    }

    #[test]
    fn c_extraction_declarator_chains() {
        let src = r#"
static int plain(int a) {
    return a * 2;
}

char *make_name(const char *base, int n) {
    char *out = malloc(n);
    return out;
}

int (*pick_handler(int kind))(int) {
    return 0;
}
"#;
        let units = extract_file(Path::new("util.c"), src, false).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["plain", "make_name", "pick_handler"]);
    }

    #[test]
    fn cpp_extraction_methods_templates_operators() {
        let src = r#"
int plain(int a) {
    return a * 2;
}

template <typename T>
T doubled(T a) {
    return a * 2;
}

struct P {
    bool operator==(const P &o) const {
        return x == o.x;
    }
    int x;
};

int P::times(int a) {
    return x * a;
}
"#;
        let units = extract_file(Path::new("util.cpp"), src, false).unwrap();
        let by_name = |n: &str| units.iter().find(|u| u.name == n).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["plain", "doubled", "operator==", "P::times"]);
        // Template span starts at the template header.
        assert!(by_name("doubled").text.starts_with("template"));
    }

    #[test]
    fn strip_comments_removes_prose_keeps_code() {
        let rust_src = r#"
/// Doc comment about doubling.
fn doubled(a: u32) -> u32 {
    // inline note
    a * 2 /* trailing */
}
"#;
        let units = extract_file(Path::new("lib.rs"), rust_src, true).unwrap();
        assert_eq!(units[0].text, "fn doubled(a: u32) -> u32 {\n    a * 2\n}");

        let py_src = r#"
def doubled(a):
    """Docstring prose."""
    # note
    return a * 2
"#;
        let units = extract_file(Path::new("mod.py"), py_src, true).unwrap();
        assert_eq!(units[0].text, "def doubled(a):\n    return a * 2");
        // The hash follows the stripped text, so cached embeddings distinguish
        // stripped and unstripped variants of the same function.
        let unstripped = extract_file(Path::new("mod.py"), py_src, false).unwrap();
        assert_ne!(units[0].hash, unstripped[0].hash);
    }

    #[test]
    fn test_paths_are_flagged() {
        assert!(is_test_path("src/foo.test.ts"));
        assert!(is_test_path("crate/tests/it.rs"));
        assert!(!is_test_path("src/attest.rs"));
        assert!(is_test_path("pkg/walk_test.go"));
        assert!(is_test_path("pkg/test_walk.py"));
        assert!(is_test_path("app/src/test/java/FooTest.java"));
        assert!(!is_test_path("pkg/protest.go"));
        assert!(is_test_path("spec/app_spec.rb"));
        assert!(is_test_path("tests/GuzzleTest.php"));
        assert!(is_test_path("Foo.Tests/WidgetTests.cs"));
        assert!(!is_test_path("lib/inspect.rb"));
    }
}
