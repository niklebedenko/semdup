//! Full-corpus duplicate scan.
//!
//! Loads cached embeddings, filters units according to scan settings, finds
//! threshold-passing pairs, then groups them into display clusters.

use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use bat::PrettyPrinter;
use rayon::prelude::*;
use rusqlite::Connection;
use serde::Serialize;

use crate::db::{self, UnitRow};
use crate::extract::UnitKind;

#[derive(Serialize)]
struct PairOut {
    a: String,
    b: String,
    cosine: f32,
}

const SPARSE_AUTO_MIN_UNITS: usize = 20_000;
const LSH_PLANES: usize = 128;
const LSH_BANDS: usize = 16;
const LSH_ROWS_PER_BAND: usize = LSH_PLANES / LSH_BANDS;
const LSH_HYPERPLANE_NNZ: usize = 32;
const LSH_CANDIDATE_CAP: usize = 1024;
const EXACT_GEMM_MIN_UNITS: usize = 128;
const EXACT_GEMM_BLOCK: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateIndex {
    /// Exhaustive all-pairs cosine. Exact and deterministic.
    Exact,
    /// Sparse-random-projection LSH candidate generation, exact dense rerank.
    Sparse,
    /// Exact for ordinary repos; sparse index above `SPARSE_AUTO_MIN_UNITS`.
    Auto,
}

impl CandidateIndex {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "exact" => Ok(CandidateIndex::Exact),
            "sparse" => Ok(CandidateIndex::Sparse),
            "auto" => Ok(CandidateIndex::Auto),
            other => bail!("unknown scan index '{other}' (expected exact, sparse, or auto)"),
        }
    }

    fn resolve(self, n: usize) -> CandidateIndex {
        match self {
            CandidateIndex::Auto if n >= SPARSE_AUTO_MIN_UNITS => CandidateIndex::Sparse,
            CandidateIndex::Auto => CandidateIndex::Exact,
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
pub enum ColorChoice {
    /// Highlight when stdout is a terminal.
    Auto,
    /// Always emit ANSI color escape sequences.
    Always,
    /// Never emit ANSI color escape sequences.
    Never,
}

impl std::fmt::Display for ColorChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorChoice::Auto => f.write_str("auto"),
            ColorChoice::Always => f.write_str("always"),
            ColorChoice::Never => f.write_str("never"),
        }
    }
}

impl ColorChoice {
    fn enabled(self) -> bool {
        match self {
            ColorChoice::Auto => std::io::stdout().is_terminal(),
            ColorChoice::Always => true,
            ColorChoice::Never => false,
        }
    }
}

pub struct ScanOpts<'a> {
    pub threshold: f32,
    pub index: CandidateIndex,
    pub min_lines: usize,
    pub skip_tests: bool,
    pub unit_kind: Option<UnitKind>,
    pub json: Option<&'a Path>,
    pub show_bodies: bool,
    pub color: ColorChoice,
    pub top: usize,
    /// Only report clusters with at least this many members ("rule of three"
    /// gating: a helper is only worth it once the logic exists N times).
    pub min_cluster: usize,
}

pub fn run(conn: &Connection, model: &str, opts: &ScanOpts) -> Result<()> {
    let units = load_scannable(conn, model, opts.min_lines, opts.skip_tests, opts.unit_kind)?;
    let pairs = similar_pairs_with_index(&units, opts.threshold, opts.index);

    // Union-find clustering for display.
    let mut parent: Vec<usize> = (0..units.len()).collect();
    fn find(parent: &mut Vec<usize>, i: usize) -> usize {
        if parent[i] != i {
            let r = find(parent, parent[i]);
            parent[i] = r;
        }
        parent[i]
    }
    for &(i, j, _) in &pairs {
        let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
        if ri != rj {
            parent[ri] = rj;
        }
    }
    let mut clusters: HashMap<usize, Vec<(usize, usize, f32)>> = HashMap::new();
    for &(i, j, s) in &pairs {
        clusters
            .entry(find(&mut parent, i))
            .or_default()
            .push((i, j, s));
    }
    let mut ordered: Vec<Cluster> = clusters
        .into_values()
        .flat_map(|mut c| {
            c.sort_by(|a, b| b.2.total_cmp(&a.2));
            let mut members: Vec<usize> = c.iter().flat_map(|&(i, j, _)| [i, j]).collect();
            members.sort_unstable();
            members.dedup();
            prune_larger_overlapping_members(
                Cluster {
                    members,
                    pairs: c,
                    hidden_overlaps: 0,
                },
                &units,
            )
        })
        .collect();
    ordered.sort_by(|a, b| b.pairs[0].2.total_cmp(&a.pairs[0].2));
    let total_clusters = ordered.len();
    ordered.retain(|c| c.members.len() >= opts.min_cluster);
    let kept_pairs: usize = ordered.iter().map(|c| c.pairs.len()).sum();

    println!(
        "# semdup scan — model {model}, threshold {}\n",
        opts.threshold
    );
    println!(
        "{} pairs above threshold in {} clusters",
        kept_pairs,
        ordered.len()
    );
    if ordered.len() < total_clusters {
        println!(
            "({} clusters below --min-cluster {} hidden)",
            total_clusters - ordered.len(),
            opts.min_cluster
        );
    }
    println!();
    for (k, cluster) in ordered.iter().take(opts.top).enumerate() {
        println!("## cluster {} ({} members)", k + 1, cluster.members.len());
        if cluster.hidden_overlaps > 0 {
            println!(
                "({} larger overlapping member{} hidden)",
                cluster.hidden_overlaps,
                if cluster.hidden_overlaps == 1 {
                    ""
                } else {
                    "s"
                }
            );
        }
        for &m in &cluster.members {
            println!("- {}", units[m].0.label());
        }
        for &(i, j, s) in cluster.pairs.iter().take(10) {
            println!(
                "  {s:.4}  {}  <->  {}",
                units[i].0.label(),
                units[j].0.label()
            );
        }
        if opts.show_bodies {
            print_cluster_bodies(conn, &units, cluster, opts.color)?;
        }
        println!();
    }
    if ordered.len() > opts.top {
        println!(
            "... {} more clusters (raise --top)",
            ordered.len() - opts.top
        );
    }

    if let Some(path) = opts.json {
        let out: Vec<PairOut> = ordered
            .iter()
            .flat_map(|c| &c.pairs)
            .map(|&(i, j, s)| PairOut {
                a: units[i].0.label(),
                b: units[j].0.label(),
                cosine: s,
            })
            .collect();
        std::fs::write(path, serde_json::to_string_pretty(&out)?)?;
        eprintln!("wrote {} pairs to {}", out.len(), path.display());
    }
    Ok(())
}

fn print_cluster_bodies(
    conn: &Connection,
    units: &[(UnitRow, Vec<f32>)],
    cluster: &Cluster,
    color: ColorChoice,
) -> Result<()> {
    println!("\n### bodies");
    let color = color.enabled();
    let mut bodies: HashMap<String, String> = HashMap::new();
    for &member in &cluster.members {
        let unit = &units[member].0;
        let body = match bodies.get(&unit.hash) {
            Some(body) => body,
            None => {
                let body = db::text_for_hash(conn, &unit.hash)?
                    .with_context(|| format!("missing source text for {}", unit.label()))?;
                bodies.insert(unit.hash.clone(), body);
                bodies.get(&unit.hash).expect("body was just cached")
            }
        };
        println!("\n#### {}", unit.label());
        if color && print_highlighted_body(body, &unit.lang).is_ok() {
            continue;
        }
        print_plain_body(body, &unit.lang);
    }
    Ok(())
}

fn print_highlighted_body(body: &str, lang: &str) -> Result<()> {
    let body = body.trim_end();
    let mut highlighted = String::new();
    PrettyPrinter::new()
        .input_from_bytes(body.as_bytes())
        .language(bat_lang(lang))
        .colored_output(true)
        .true_color(true)
        .header(false)
        .line_numbers(false)
        .grid(false)
        .print_with_writer(Some(&mut highlighted))?;
    print!("{highlighted}");
    if !highlighted.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn print_plain_body(body: &str, lang: &str) {
    let fence = code_fence(body);
    println!("{fence}{}", markdown_lang(lang));
    println!("{}", body.trim_end());
    println!("{fence}");
}

fn markdown_lang(lang: &str) -> &str {
    match lang {
        "csharp" => "csharp",
        "cpp" => "cpp",
        "c" => "c",
        "go" => "go",
        "java" => "java",
        "php" => "php",
        "python" => "python",
        "ruby" => "ruby",
        "rust" => "rust",
        "typescript" => "typescript",
        _ => "",
    }
}

fn bat_lang(lang: &str) -> &str {
    match lang {
        "csharp" => "cs",
        "cpp" => "cpp",
        "c" => "c",
        "go" => "go",
        "java" => "java",
        "php" => "php",
        "python" => "py",
        "ruby" => "rb",
        "rust" => "rs",
        "typescript" => "ts",
        _ => "txt",
    }
}

fn code_fence(text: &str) -> String {
    let mut longest = 0usize;
    let mut current = 0usize;
    for ch in text.chars() {
        if ch == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    "`".repeat(longest.max(2) + 1)
}

struct Cluster {
    members: Vec<usize>,
    pairs: Vec<(usize, usize, f32)>,
    hidden_overlaps: usize,
}

fn prune_larger_overlapping_members(
    cluster: Cluster,
    units: &[(UnitRow, Vec<f32>)],
) -> Vec<Cluster> {
    let removed = larger_overlapping_members(&cluster.members, units);
    if removed.is_empty() {
        return vec![cluster];
    }
    let pairs: Vec<(usize, usize, f32)> = cluster
        .pairs
        .into_iter()
        .filter(|(i, j, _)| !removed.contains(i) && !removed.contains(j))
        .collect();
    split_cluster_pairs(pairs, &removed, units)
}

fn larger_overlapping_members(members: &[usize], units: &[(UnitRow, Vec<f32>)]) -> HashSet<usize> {
    let mut removed = HashSet::new();
    for &candidate in members {
        let unit = &units[candidate].0;
        if members
            .iter()
            .any(|&other| candidate != other && is_larger_overlapping_unit(unit, &units[other].0))
        {
            removed.insert(candidate);
        }
    }
    removed
}

fn is_larger_overlapping_unit(a: &UnitRow, b: &UnitRow) -> bool {
    a.path == b.path && a.lines() > b.lines() && ranges_overlap(a, b)
}

fn split_cluster_pairs(
    pairs: Vec<(usize, usize, f32)>,
    removed: &HashSet<usize>,
    units: &[(UnitRow, Vec<f32>)],
) -> Vec<Cluster> {
    if pairs.is_empty() {
        return Vec::new();
    }

    let mut all_members: Vec<usize> = pairs.iter().flat_map(|&(i, j, _)| [i, j]).collect();
    all_members.sort_unstable();
    all_members.dedup();
    let member_pos: HashMap<usize, usize> = all_members
        .iter()
        .enumerate()
        .map(|(pos, &member)| (member, pos))
        .collect();

    let mut parent: Vec<usize> = (0..all_members.len()).collect();
    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            let root = find(parent, parent[i]);
            parent[i] = root;
        }
        parent[i]
    }
    for &(i, j, _) in &pairs {
        let pi = member_pos[&i];
        let pj = member_pos[&j];
        let ri = find(&mut parent, pi);
        let rj = find(&mut parent, pj);
        if ri != rj {
            parent[ri] = rj;
        }
    }

    let mut groups: HashMap<usize, Vec<(usize, usize, f32)>> = HashMap::new();
    for pair @ (i, _, _) in pairs {
        let pi = member_pos[&i];
        let root = find(&mut parent, pi);
        groups.entry(root).or_default().push(pair);
    }

    groups
        .into_values()
        .map(|mut pairs| {
            pairs.sort_by(|a, b| b.2.total_cmp(&a.2));
            let mut members: Vec<usize> = pairs.iter().flat_map(|&(i, j, _)| [i, j]).collect();
            members.sort_unstable();
            members.dedup();
            let hidden_overlaps = removed
                .iter()
                .filter(|&&removed_member| {
                    members.iter().any(|&member| {
                        is_larger_overlapping_unit(&units[removed_member].0, &units[member].0)
                    })
                })
                .count();
            Cluster {
                members,
                pairs,
                hidden_overlaps,
            }
        })
        .collect()
}

pub fn load_scannable(
    conn: &Connection,
    model: &str,
    min_lines: usize,
    skip_tests: bool,
    unit_kind: Option<UnitKind>,
) -> Result<Vec<(UnitRow, Vec<f32>)>> {
    let (units, n_ignored) =
        db::load_scannable_units(conn, "main", model, min_lines, skip_tests, unit_kind)?;
    let (units, n_overlap) = drop_larger_overlapping_blocks(units);
    let kind = unit_kind.map_or("all", UnitKind::as_str);
    eprintln!(
        "scanning {} {kind} units (>= {min_lines} lines, {n_ignored} suppressed by semdup:ignore, {n_overlap} larger overlapping blocks skipped)",
        units.len(),
    );
    Ok(units)
}

pub(crate) fn drop_larger_overlapping_blocks(
    units: Vec<(UnitRow, Vec<f32>)>,
) -> (Vec<(UnitRow, Vec<f32>)>, usize) {
    let mut by_path: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, (unit, _)) in units.iter().enumerate() {
        if unit.kind == UnitKind::Block {
            by_path.entry(unit.path.clone()).or_default().push(i);
        }
    }

    let mut remove = vec![false; units.len()];
    for indices in by_path.values() {
        for &candidate in indices {
            let unit = &units[candidate].0;
            if indices.iter().any(|&other| {
                candidate != other && is_larger_overlapping_unit(unit, &units[other].0)
            }) {
                remove[candidate] = true;
            }
        }
    }

    let n_removed = remove.iter().filter(|&&r| r).count();
    let units = units
        .into_iter()
        .enumerate()
        .filter_map(|(i, unit)| (!remove[i]).then_some(unit))
        .collect();
    (units, n_removed)
}

pub fn similar_pairs_with_index(
    units: &[(UnitRow, Vec<f32>)],
    threshold: f32,
    index: CandidateIndex,
) -> Vec<(usize, usize, f32)> {
    match index.resolve(units.len()) {
        CandidateIndex::Exact => exact_similar_pairs(units, threshold),
        CandidateIndex::Sparse => sparse_lsh_pairs(units, threshold),
        CandidateIndex::Auto => unreachable!("auto resolves to exact or sparse"),
    }
}

fn exact_similar_pairs(units: &[(UnitRow, Vec<f32>)], threshold: f32) -> Vec<(usize, usize, f32)> {
    if let Some((matrix, dim)) = pack_embeddings(units)
        && units.len() >= EXACT_GEMM_MIN_UNITS
    {
        return exact_gemm_pairs(units, &matrix, dim, threshold);
    }
    eprintln!("candidate index: exact all-pairs");
    exact_dot_pairs(units, threshold)
}

fn exact_dot_pairs(units: &[(UnitRow, Vec<f32>)], threshold: f32) -> Vec<(usize, usize, f32)> {
    let mut pairs: Vec<(usize, usize, f32)> = (0..units.len())
        .into_par_iter()
        .flat_map_iter(|i| {
            let (ui, vi) = &units[i];
            units[i + 1..]
                .iter()
                .enumerate()
                .filter_map(move |(off, (uj, vj))| {
                    let j = i + 1 + off;
                    if ui.path == uj.path && contains(ui, uj) {
                        return None;
                    }
                    let s = dot(vi, vj);
                    (s >= threshold).then_some((i, j, s))
                })
        })
        .collect();
    pairs.sort_by(|a, b| b.2.total_cmp(&a.2));
    pairs
}

fn pack_embeddings(units: &[(UnitRow, Vec<f32>)]) -> Option<(Vec<f32>, usize)> {
    let dim = units.first()?.1.len();
    if dim == 0 || units.iter().any(|(_, v)| v.len() != dim) {
        return None;
    }
    let mut matrix = Vec::with_capacity(units.len() * dim);
    for (_, vec) in units {
        matrix.extend_from_slice(vec);
    }
    Some((matrix, dim))
}

fn exact_gemm_pairs(
    units: &[(UnitRow, Vec<f32>)],
    matrix: &[f32],
    dim: usize,
    threshold: f32,
) -> Vec<(usize, usize, f32)> {
    eprintln!("candidate index: exact blocked GEMM (block {EXACT_GEMM_BLOCK})");
    let n = units.len();
    let starts: Vec<usize> = (0..n).step_by(EXACT_GEMM_BLOCK).collect();
    let block_pairs: Vec<(usize, usize)> = starts
        .iter()
        .enumerate()
        .flat_map(|(i, &i0)| starts[i..].iter().map(move |&j0| (i0, j0)))
        .collect();
    let mut pairs: Vec<(usize, usize, f32)> = block_pairs
        .into_par_iter()
        .flat_map_iter(|(i0, j0)| {
            let rows = (n - i0).min(EXACT_GEMM_BLOCK);
            let cols = (n - j0).min(EXACT_GEMM_BLOCK);
            let mut scores = vec![0.0f32; rows * cols];
            // Compute scores = matrix[i0..i0+rows] * matrix[j0..j0+cols]^T.
            // Embeddings are row-major, so B is addressed as a transposed view.
            unsafe {
                matrixmultiply::sgemm(
                    rows,
                    dim,
                    cols,
                    1.0,
                    matrix.as_ptr().add(i0 * dim),
                    dim as isize,
                    1,
                    matrix.as_ptr().add(j0 * dim),
                    1,
                    dim as isize,
                    0.0,
                    scores.as_mut_ptr(),
                    cols as isize,
                    1,
                );
            }
            let mut out = Vec::new();
            for local_i in 0..rows {
                let i = i0 + local_i;
                let ui = &units[i].0;
                for local_j in 0..cols {
                    let j = j0 + local_j;
                    if j <= i {
                        continue;
                    }
                    let uj = &units[j].0;
                    if ui.path == uj.path && contains(ui, uj) {
                        continue;
                    }
                    let s = scores[local_i * cols + local_j];
                    if s >= threshold {
                        out.push((i, j, s));
                    }
                }
            }
            out
        })
        .collect();
    pairs.sort_by(|a, b| b.2.total_cmp(&a.2));
    pairs
}

fn sparse_lsh_pairs(units: &[(UnitRow, Vec<f32>)], threshold: f32) -> Vec<(usize, usize, f32)> {
    eprintln!(
        "candidate index: sparse LSH ({LSH_PLANES} planes, {LSH_BANDS} bands, cap {LSH_CANDIDATE_CAP}/unit)"
    );
    let signatures: Vec<Signature> = units
        .par_iter()
        .map(|(_, vec)| Signature::from_vec(vec))
        .collect();
    let buckets = lsh_buckets(&signatures);
    let reranked = AtomicUsize::new(0);
    let mut pairs: Vec<(usize, usize, f32)> = (0..units.len())
        .into_par_iter()
        .flat_map_iter(|i| {
            let (ui, vi) = &units[i];
            let mut counts: HashMap<usize, u8> = HashMap::new();
            for band in 0..LSH_BANDS {
                if let Some(bucket) = buckets.get(&signatures[i].band_key(band)) {
                    for &j in bucket {
                        if j <= i {
                            continue;
                        }
                        let (uj, _) = &units[j];
                        if ui.path == uj.path && contains(ui, uj) {
                            continue;
                        }
                        counts
                            .entry(j)
                            .and_modify(|n| *n = n.saturating_add(1))
                            .or_insert(1);
                    }
                }
            }
            let mut candidates: Vec<(usize, u8)> = counts.into_iter().collect();
            candidates.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            if candidates.len() > LSH_CANDIDATE_CAP {
                candidates.truncate(LSH_CANDIDATE_CAP);
            }
            reranked.fetch_add(candidates.len(), Ordering::Relaxed);
            candidates.into_iter().filter_map(move |(j, _)| {
                let s = dot(vi, &units[j].1);
                (s >= threshold).then_some((i, j, s))
            })
        })
        .collect();
    eprintln!(
        "sparse LSH reranked {} candidate pair(s) exactly",
        reranked.load(Ordering::Relaxed)
    );
    pairs.sort_by(|a, b| b.2.total_cmp(&a.2));
    pairs
}

#[derive(Clone, Copy)]
struct Signature {
    bits: [u64; 2],
}

impl Signature {
    fn from_vec(vec: &[f32]) -> Signature {
        let mut bits = [0u64; 2];
        if vec.is_empty() {
            return Signature { bits };
        }
        for plane in 0..LSH_PLANES {
            let mut acc = 0.0f32;
            for k in 0..LSH_HYPERPLANE_NNZ {
                let h = splitmix64(
                    0x9e37_79b9_7f4a_7c15
                        ^ (plane as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
                        ^ (k as u64).wrapping_mul(0x94d0_49bb_1331_11eb),
                );
                let dim = (h as usize) % vec.len();
                let sign = if h & (1 << 63) == 0 { 1.0 } else { -1.0 };
                acc += sign * vec[dim];
            }
            if acc >= 0.0 {
                bits[plane / 64] |= 1u64 << (plane % 64);
            }
        }
        Signature { bits }
    }

    fn band_key(self, band: usize) -> u64 {
        debug_assert!(band < LSH_BANDS);
        let start = band * LSH_ROWS_PER_BAND;
        let value = if start < 64 {
            (self.bits[0] >> start) & ((1u64 << LSH_ROWS_PER_BAND) - 1)
        } else {
            (self.bits[1] >> (start - 64)) & ((1u64 << LSH_ROWS_PER_BAND) - 1)
        };
        ((band as u64) << 56) | value
    }
}

fn lsh_buckets(signatures: &[Signature]) -> HashMap<u64, Vec<usize>> {
    let mut buckets: HashMap<u64, Vec<usize>> = HashMap::with_capacity(signatures.len() * 2);
    for (i, sig) in signatures.iter().copied().enumerate() {
        for band in 0..LSH_BANDS {
            buckets.entry(sig.band_key(band)).or_default().push(i);
        }
    }
    buckets
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn contains(a: &UnitRow, b: &UnitRow) -> bool {
    (a.start_line <= b.start_line && b.end_line <= a.end_line)
        || (b.start_line <= a.start_line && a.end_line <= b.end_line)
}

fn ranges_overlap(a: &UnitRow, b: &UnitRow) -> bool {
    a.start_line <= b.end_line && b.start_line <= a.end_line
}

/// Dot product for unit-normalized embeddings; equivalent to cosine.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(path: &str, start_line: usize, end_line: usize) -> UnitRow {
        UnitRow {
            path: path.to_string(),
            name: format!("f{start_line}"),
            lang: "rust".to_string(),
            kind: UnitKind::Function,
            start_line,
            end_line,
            hash: format!("{path}:{start_line}:{end_line}"),
            ignored: false,
            is_test: false,
        }
    }

    fn block(path: &str, start_line: usize, end_line: usize) -> UnitRow {
        UnitRow {
            kind: UnitKind::Block,
            ..unit(path, start_line, end_line)
        }
    }

    fn one_hot(dim: usize, idx: usize) -> Vec<f32> {
        let mut v = vec![0.0; dim];
        v[idx] = 1.0;
        v
    }

    #[test]
    fn sparse_lsh_reranks_identical_vectors() {
        let units = vec![
            (unit("a.rs", 1, 10), one_hot(768, 7)),
            (unit("b.rs", 1, 10), one_hot(768, 7)),
            (unit("c.rs", 1, 10), one_hot(768, 99)),
        ];
        let pairs = similar_pairs_with_index(&units, 0.99, CandidateIndex::Sparse);
        assert_eq!(pairs.len(), 1);
        assert_eq!((pairs[0].0, pairs[0].1), (0, 1));
    }

    #[test]
    fn exact_gemm_matches_dot_pairs() {
        let n = EXACT_GEMM_MIN_UNITS + 2;
        let mut units: Vec<(UnitRow, Vec<f32>)> = (0..n)
            .map(|i| (unit(&format!("{i}.rs"), 1, 10), one_hot(n, i)))
            .collect();
        units[n - 1].1 = units[0].1.clone();
        let (matrix, dim) = pack_embeddings(&units).unwrap();
        let expected = exact_dot_pairs(&units, 0.99);
        let actual = exact_gemm_pairs(&units, &matrix, dim, 0.99);
        assert_eq!(actual, expected);
    }

    #[test]
    fn auto_uses_exact_for_small_inputs() {
        assert_eq!(CandidateIndex::Auto.resolve(10), CandidateIndex::Exact);
        assert_eq!(
            CandidateIndex::Auto.resolve(SPARSE_AUTO_MIN_UNITS),
            CandidateIndex::Sparse
        );
    }

    #[test]
    fn rejects_unknown_index() {
        assert!(CandidateIndex::parse("annoy").is_err());
    }

    #[test]
    fn code_fence_outgrows_body_backticks() {
        assert_eq!(code_fence("no backticks"), "```");
        assert_eq!(code_fence("contains ``` fence"), "````");
    }

    #[test]
    fn bat_language_tokens_match_common_extensions() {
        assert_eq!(bat_lang("rust"), "rs");
        assert_eq!(bat_lang("typescript"), "ts");
        assert_eq!(bat_lang("python"), "py");
        assert_eq!(bat_lang("unknown"), "txt");
    }

    #[test]
    fn pruning_drops_larger_overlapping_members() {
        let units = vec![
            (block("a.rs", 1, 10), one_hot(4, 0)),
            (block("a.rs", 3, 5), one_hot(4, 1)),
            (block("b.rs", 1, 10), one_hot(4, 2)),
            (block("b.rs", 3, 5), one_hot(4, 3)),
        ];
        let clusters = prune_larger_overlapping_members(
            Cluster {
                members: vec![0, 1, 2, 3],
                pairs: vec![(0, 2, 1.0), (0, 3, 1.0), (1, 3, 1.0)],
                hidden_overlaps: 0,
            },
            &units,
        );
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].members, vec![1, 3]);
        assert_eq!(clusters[0].pairs, vec![(1, 3, 1.0)]);
        assert_eq!(clusters[0].hidden_overlaps, 2);
    }

    #[test]
    fn loading_prunes_larger_overlapping_blocks_globally() {
        let units = vec![
            (block("a.rs", 1, 10), one_hot(5, 0)),
            (block("a.rs", 3, 5), one_hot(5, 1)),
            (block("a.rs", 12, 20), one_hot(5, 2)),
            (unit("a.rs", 1, 30), one_hot(5, 3)),
            (block("b.rs", 1, 10), one_hot(5, 4)),
        ];
        let (units, n_removed) = drop_larger_overlapping_blocks(units);
        assert_eq!(n_removed, 1);
        let labels: Vec<_> = units.iter().map(|(u, _)| u.label()).collect();
        assert!(!labels.iter().any(|label| label.contains("a.rs:1-10")));
        assert!(labels.iter().any(|label| label.contains("a.rs:3-5")));
        assert!(labels.iter().any(|label| label.contains("a.rs:1-30")));
        assert!(labels.iter().any(|label| label.contains("b.rs:1-10")));
    }
}
