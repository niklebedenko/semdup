//! Per-MR mode: rank functions touched by a diff against the indexed corpus.
//!
//! Absolute cosine thresholds are fragile (identifier vocabulary dominates
//! the embedding), but *relative* similarity is robust: in our planted-clone
//! benchmark the true original was the touched function's nearest neighbor in
//! 90% of cases, with positive margin. So this mode reports, per touched
//! function, its nearest corpus neighbors and flags:
//!   DUP    — cosine above the calibrated threshold, or
//!   LIKELY — the pair is a mutual nearest neighbor with a clear margin.
//!
//! Run from the repo root the corpus was extracted from, after `extract` +
//! `embed` on the base state (stale corpora degrade gracefully: results are
//! still ranked, just against slightly old code).

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use serde::Serialize;

use crate::db::{self, UnitRow};
use crate::embed::{Backend, strip_doc_comments};
use crate::extract::{self, Unit};
use crate::scan::dot;

const MUTUAL_NN_MARGIN: f32 = 0.03;

pub struct DiffOpts<'a> {
    pub base: String,
    pub min_lines: usize,
    /// Calibrated threshold for a hard DUP verdict; None disables it.
    pub threshold: Option<f32>,
    pub json: Option<&'a Path>,
    pub skip_tests: bool,
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

/// Returns the number of DUP/LIKELY findings (for CI exit codes).
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

    let touched = touched_units(&opts.base, opts.min_lines, opts.skip_tests)?;
    if touched.is_empty() {
        eprintln!("no touched functions (>= {} lines) in diff vs {}", opts.min_lines, opts.base);
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
                if s.trim().is_empty() { touched[i].text.clone() } else { s }
            })
            .collect();
        let embedded = backend.embed(&texts)?;
        for (&i, v) in missing.iter().zip(embedded) {
            vecs[i] = Some(v);
        }
    }

    let mut findings = 0usize;
    let mut reports = Vec::new();
    for (unit, vec) in touched.iter().zip(&vecs) {
        let vec = vec.as_ref().expect("all touched units embedded");
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
        let (nn_idx, cos1) = scored[0];
        let cos2 = scored.get(1).map_or(0.0, |&(_, s)| s);
        let margin = cos1 - cos2;

        let above = opts.threshold.is_some_and(|t| cos1 >= t);
        let verdict = if above {
            "DUP"
        } else if margin >= MUTUAL_NN_MARGIN && is_mutual_nn(&corpus, nn_idx, cos1) {
            "LIKELY"
        } else {
            "ok"
        };
        if verdict != "ok" {
            findings += 1;
        }
        reports.push(Report {
            unit: format!("{}:{}-{} {}", unit.path, unit.start_line, unit.end_line, unit.name),
            verdict,
            margin,
            neighbors: scored
                .iter()
                .take(3)
                .map(|&(i, s)| Neighbor { unit: corpus[i].0.label(), cosine: s })
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

/// True if `cos1` beats the neighbor's best similarity to anyone else in the
/// corpus — i.e. the touched unit and the neighbor point at each other.
fn is_mutual_nn(corpus: &[(UnitRow, Vec<f32>)], nn_idx: usize, cos1: f32) -> bool {
    let (nn_unit, nn_vec) = &corpus[nn_idx];
    let best_other = corpus
        .iter()
        .enumerate()
        .filter(|&(i, (c, _))| i != nn_idx && !(c.path == nn_unit.path && c.name == nn_unit.name))
        .map(|(_, (_, v))| dot(nn_vec, v))
        .fold(f32::NEG_INFINITY, f32::max);
    cos1 > best_other
}

/// Extract functions in the working tree that overlap changed lines vs `base`.
fn touched_units(base: &str, min_lines: usize, skip_tests: bool) -> Result<Vec<Unit>> {
    let out = Command::new("git")
        .args(["diff", "-U0", base, "--", "*.rs", "*.ts", "*.tsx"])
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
        let path = Path::new(&file);
        let Ok(src) = std::fs::read_to_string(path) else {
            continue; // deleted or unreadable
        };
        for u in extract::extract_file(path, &src)? {
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
        assert_eq!(parse_hunk_new_range("@@ -10,2 +12,3 @@ fn x()"), Some((12, 14)));
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
    fn path_suffix_equality() {
        assert!(paths_equal("repo/src/a.rs", "src/a.rs"));
        assert!(paths_equal("src/a.rs", "./src/a.rs"));
        assert!(!paths_equal("xsrc/a.rs", "src/a.rs"));
    }
}
