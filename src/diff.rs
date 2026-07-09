//! Per-MR mode: rank functions touched by a diff against the indexed corpus.
//!
//! Design note, from planted-clone measurements (see eval/): rewrites rank
//! their original at top-1..5 reliably, but no *neighbor-closeness* statistic
//! we tested (top-1 margin, mutual nearest neighbor, z-score of the top hit
//! against the unit's similarity background) separates rewrites from ordinary
//! code — in a real repo the median function's nearest neighbor is as
//! "anomalously close" as a planted rewrite's original, because vocabulary
//! and house style dominate the embedding. So this mode makes no cleverness
//! claims: it prints each touched function's nearest neighbors as evidence
//! for the reviewer, and the only automated verdicts come from the
//! user's per-repo threshold:
//!   DUP    — top cosine at or above the threshold
//!   REVIEW — within REVIEW_BAND below it (worth a look, doesn't fail --check)
//!
//! Run from the repo root the corpus was extracted from, after `extract` +
//! `embed` on the base state (stale corpora degrade gracefully: results are
//! still ranked, just against slightly old code).

use std::fmt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use rusqlite::Connection;
use serde::Serialize;

use crate::db::{self, UnitRow};
use crate::embed::{Backend, strip_doc_comments};
use crate::extract::{self, Unit, UnitKind};
use crate::scan::dot;

/// How far below the threshold still earns a REVIEW tag.
const REVIEW_BAND: f32 = 0.05;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum DiffPolicy {
    /// Any touched function whose nearest neighbor crosses the threshold fails.
    Touched,
    /// Only duplicate relationships that were not already present on base fail.
    NewPairs,
    /// Only newly added functions can fail.
    Added,
}

impl fmt::Display for DiffPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiffPolicy::Touched => f.write_str("touched"),
            DiffPolicy::NewPairs => f.write_str("new-pairs"),
            DiffPolicy::Added => f.write_str("added"),
        }
    }
}

pub struct DiffOpts<'a> {
    pub base: String,
    pub min_lines: usize,
    /// Threshold for a hard DUP verdict; None disables it.
    pub threshold: Option<f32>,
    pub policy: DiffPolicy,
    pub json: Option<&'a Path>,
    pub skip_tests: bool,
    /// Must match how the corpus was extracted, or cosines are not
    /// comparable ([extract].strip_comments).
    pub strip_comments: bool,
    /// [extract].exclude — a file the corpus would never contain shouldn't
    /// be gated when a diff touches it (CI fixtures, vendored code).
    pub exclude: Vec<String>,
}

#[derive(Serialize)]
struct Neighbor {
    unit: String,
    cosine: f32,
}

#[derive(Serialize)]
struct Report {
    unit: String,
    verdict: &'static str,
    margin: f32,
    neighbors: Vec<Neighbor>,
}

/// Returns the number of DUP findings (for CI exit codes).
pub fn run(
    conn: &Connection,
    model: &str,
    opts: &DiffOpts,
    mk_backend: &mut dyn FnMut() -> Result<Box<dyn Backend>>,
) -> Result<usize> {
    let corpus = db::load_units(conn, "main", model)?;
    if corpus.is_empty() {
        bail!("corpus is empty for model {model}; run `semdup extract` and `semdup embed` first");
    }
    // Rank only against units scan would report on. touched_units applies
    // these filters to the touched side already; without the corpus side a
    // new function's own unit test (same vocabulary, is_test) or a
    // semdup:ignore'd by-design mirror surfaces as its nearest neighbor and
    // fails --check on duplication scan itself would never report.
    let corpus: Vec<_> = corpus
        .into_iter()
        .filter(|(c, _)| c.kind == UnitKind::Function && rankable(c, opts.skip_tests))
        .collect();
    if corpus.is_empty() {
        bail!(
            "no rankable function units in corpus for model {model}; \
             re-run `semdup extract` with function granularity and `semdup embed`"
        );
    }

    let touched = touched_units(opts)?;
    if touched.is_empty() {
        eprintln!(
            "no touched functions (>= {} effective lines) in diff vs {}",
            opts.min_lines, opts.base
        );
        return Ok(0);
    }
    eprintln!("{} touched function(s) vs {}", touched.len(), opts.base);

    // Embed touched units: cache by content hash first, backend for the rest.
    let mut vecs: Vec<Option<Vec<f32>>> = Vec::with_capacity(touched.len());
    for u in &touched {
        vecs.push(db::embedding_for(conn, model, &u.hash)?);
    }
    let missing: Vec<usize> = (0..touched.len()).filter(|&i| vecs[i].is_none()).collect();
    if !missing.is_empty() {
        let mut backend = mk_backend()?;
        let texts: Vec<String> = missing
            .iter()
            .map(|&i| {
                let s = strip_doc_comments(&touched[i].text);
                if s.trim().is_empty() {
                    touched[i].text.clone()
                } else {
                    s
                }
            })
            .collect();
        let embedded = backend.embed(&texts)?;
        for (&i, mut v) in missing.iter().zip(embedded) {
            db::normalize_vec(&mut v);
            vecs[i] = Some(v);
        }
    }

    let mut findings = 0usize;
    let mut reports = Vec::new();
    for (unit, vec) in touched.iter().zip(&vecs) {
        let vec = vec.as_ref().expect("all touched units embedded");
        let stale_self = corpus
            .iter()
            .find(|(c, _)| is_stale_self(unit, c))
            .map(|(_, v)| v);
        // Rank corpus, excluding the unit's own (possibly stale) entry.
        let mut scored: Vec<(usize, f32)> = corpus
            .iter()
            .enumerate()
            .filter(|(_, (c, _))| !is_stale_self(unit, c))
            .map(|(i, (_, cv))| (i, dot(vec, cv)))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        if scored.is_empty() {
            continue;
        }
        let (_, cos1) = scored[0];
        let cos2 = scored.get(1).map_or(0.0, |&(_, s)| s);
        let margin = cos1 - cos2;

        let verdict = match opts.threshold {
            Some(t) if policy_has_new_dup(opts.policy, &scored, &corpus, stale_self, t) => "DUP",
            Some(t) if cos1 >= t - REVIEW_BAND => "REVIEW",
            _ => "ok",
        };
        // Only hard DUPs fail --check; REVIEW is advisory.
        if verdict == "DUP" {
            findings += 1;
        }
        reports.push(Report {
            unit: format!(
                "{}:{}-{} {}",
                unit.path, unit.start_line, unit.end_line, unit.name
            ),
            verdict,
            margin,
            neighbors: scored
                .iter()
                .take(3)
                .map(|&(i, s)| Neighbor {
                    unit: corpus[i].0.label(),
                    cosine: s,
                })
                .collect(),
        });
    }

    for r in &reports {
        println!("[{:>6}] {}  (margin {:+.3})", r.verdict, r.unit, r.margin);
        for n in &r.neighbors {
            println!("          {:.4}  {}", n.cosine, n.unit);
        }
    }
    println!(
        "\n{} of {} touched function(s) flagged",
        findings,
        reports.len()
    );
    if let Some(path) = opts.json {
        std::fs::write(path, serde_json::to_string_pretty(&reports)?)?;
    }
    Ok(findings)
}

/// Whether a corpus unit participates in neighbor ranking; mirrors the
/// touched-side filters in `touched_units` (minus min_lines: a short helper
/// is still a legitimate duplication source as a neighbor).
fn rankable(c: &UnitRow, skip_tests: bool) -> bool {
    !(c.ignored || skip_tests && c.is_test)
}

fn policy_has_new_dup(
    policy: DiffPolicy,
    scored: &[(usize, f32)],
    corpus: &[(UnitRow, Vec<f32>)],
    stale_self: Option<&Vec<f32>>,
    threshold: f32,
) -> bool {
    match policy {
        DiffPolicy::Touched => scored.first().is_some_and(|&(_, s)| s >= threshold),
        DiffPolicy::Added => {
            stale_self.is_none() && scored.first().is_some_and(|&(_, s)| s >= threshold)
        }
        DiffPolicy::NewPairs => scored
            .iter()
            .take_while(|&&(_, s)| s >= threshold)
            .any(|&(i, _)| !pair_existed_on_base(stale_self, &corpus[i].1, threshold)),
    }
}

fn pair_existed_on_base(
    stale_self: Option<&Vec<f32>>,
    neighbor_vec: &[f32],
    threshold: f32,
) -> bool {
    stale_self.is_some_and(|v| dot(v, neighbor_vec) >= threshold)
}

/// The corpus row for the touched function itself, from before the edit:
/// same file (modulo path prefix differences) and same name.
fn is_stale_self(unit: &Unit, c: &UnitRow) -> bool {
    c.name == unit.name && paths_equal(&c.path, &unit.path)
}

fn paths_equal(a: &str, b: &str) -> bool {
    let a = a.trim_start_matches("./");
    let b = b.trim_start_matches("./");
    a == b
        || (a.len() > b.len() && a.ends_with(b) && a.as_bytes()[a.len() - b.len() - 1] == b'/')
        || (b.len() > a.len() && b.ends_with(a) && b.as_bytes()[b.len() - a.len() - 1] == b'/')
}

/// Extract functions in the working tree that overlap changed lines vs `base`.
fn touched_units(opts: &DiffOpts) -> Result<Vec<Unit>> {
    let (base, min_lines, skip_tests) = (&opts.base, opts.min_lines, opts.skip_tests);
    let out = Command::new("git")
        .args([
            "diff", "-U0", base, "--", "*.rs", "*.ts", "*.tsx", "*.py", "*.go", "*.java", "*.cs",
            "*.php", "*.rb", "*.c", "*.h", "*.cpp", "*.cc", "*.cxx", "*.hpp", "*.hh", "*.hxx",
        ])
        .output()
        .context("running git diff")?;
    if !out.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let ranges = parse_diff(&String::from_utf8_lossy(&out.stdout));

    let mut units = Vec::new();
    for (file, changed) in ranges {
        // Anchor at the repo root (git paths are repo-relative) so configured
        // exclude patterns match the same way they do in extract.
        if extract::is_path_excluded(&format!("/{file}"), &opts.exclude) {
            continue;
        }
        let path = Path::new(&file);
        let Ok(src) = std::fs::read_to_string(path) else {
            continue; // deleted or unreadable
        };
        for u in extract::extract_file(path, &src, opts.strip_comments)? {
            let overlaps = changed
                .iter()
                .any(|&(lo, hi)| u.start_line <= hi && lo <= u.end_line);
            if overlaps && !u.ignored && u.lines() >= min_lines && !(skip_tests && u.is_test) {
                units.push(u);
            }
        }
    }
    Ok(units)
}

/// Parse `git diff -U0` output into new-side changed line ranges per file.
fn parse_diff(diff: &str) -> Vec<(String, Vec<(usize, usize)>)> {
    let mut out: Vec<(String, Vec<(usize, usize)>)> = Vec::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            out.push((path.to_string(), Vec::new()));
        } else if line.starts_with("@@")
            && let Some(cur) = out.last_mut()
            && let Some(range) = parse_hunk_new_range(line)
        {
            cur.1.push(range);
        }
    }
    out.retain(|(_, r)| !r.is_empty());
    out
}

/// `@@ -a,b +c,d @@` -> new-side range (c, c+d-1); `+c` alone means d=1;
/// d=0 (pure deletion) yields no range.
fn parse_hunk_new_range(line: &str) -> Option<(usize, usize)> {
    let plus = line.split_whitespace().find(|t| t.starts_with('+'))?;
    let body = &plus[1..];
    let (start, count) = match body.split_once(',') {
        Some((s, c)) => (s.parse().ok()?, c.parse().ok()?),
        None => (body.parse().ok()?, 1usize),
    };
    if count == 0 {
        return None;
    }
    Some((start, start + count - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunk_ranges() {
        assert_eq!(
            parse_hunk_new_range("@@ -10,2 +12,3 @@ fn x()"),
            Some((12, 14))
        );
        assert_eq!(parse_hunk_new_range("@@ -10 +12 @@"), Some((12, 12)));
        assert_eq!(parse_hunk_new_range("@@ -10,2 +12,0 @@"), None);
    }

    #[test]
    fn diff_parse_groups_by_file() {
        let diff = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,4 @@
+x
@@ -20 +22 @@
+y
diff --git a/src/gone.rs b/src/gone.rs
--- a/src/gone.rs
+++ /dev/null
@@ -1,5 +0,0 @@
";
        let got = parse_diff(diff);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "src/a.rs");
        assert_eq!(got[0].1, vec![(1, 4), (22, 22)]);
    }

    #[test]
    fn ranking_skips_ignored_and_optionally_tests() {
        let unit = |ignored, is_test| UnitRow {
            path: "src/a.rs".into(),
            name: "f".into(),
            lang: "rust".into(),
            kind: UnitKind::Function,
            start_line: 1,
            end_line: 10,
            effective_lines: 10,
            hash: "hash".into(),
            ignored,
            is_test,
        };
        assert!(rankable(&unit(false, false), true));
        assert!(!rankable(&unit(true, false), true));
        assert!(!rankable(&unit(false, true), true));
        assert!(rankable(&unit(false, true), false));
        assert!(!rankable(&unit(true, true), false));
    }

    #[test]
    fn path_suffix_equality() {
        assert!(paths_equal("repo/src/a.rs", "src/a.rs"));
        assert!(paths_equal("src/a.rs", "./src/a.rs"));
        assert!(!paths_equal("xsrc/a.rs", "src/a.rs"));
    }

    fn row(name: &str) -> UnitRow {
        UnitRow {
            path: format!("src/{name}.rs"),
            name: name.to_string(),
            lang: "rust".into(),
            kind: UnitKind::Function,
            start_line: 1,
            end_line: 10,
            effective_lines: 10,
            hash: name.into(),
            ignored: false,
            is_test: false,
        }
    }

    #[test]
    fn new_pairs_policy_ignores_base_existing_pair() {
        let corpus = vec![(row("neighbor"), vec![1.0, 0.0])];
        let scored = vec![(0usize, 0.95)];
        assert!(!policy_has_new_dup(
            DiffPolicy::NewPairs,
            &scored,
            &corpus,
            Some(&vec![1.0, 0.0]),
            0.9,
        ));
        assert!(policy_has_new_dup(
            DiffPolicy::NewPairs,
            &scored,
            &corpus,
            Some(&vec![0.0, 1.0]),
            0.9,
        ));
    }

    #[test]
    fn added_policy_only_fails_added_units() {
        let corpus = vec![(row("neighbor"), vec![1.0, 0.0])];
        let scored = vec![(0usize, 0.95)];
        assert!(policy_has_new_dup(
            DiffPolicy::Added,
            &scored,
            &corpus,
            None,
            0.9,
        ));
        assert!(!policy_has_new_dup(
            DiffPolicy::Added,
            &scored,
            &corpus,
            Some(&vec![0.0, 1.0]),
            0.9,
        ));
    }
}
