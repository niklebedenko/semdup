use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use rayon::prelude::*;
use rusqlite::Connection;
use serde::Serialize;

use crate::db::{self, UnitRow};

#[derive(Serialize)]
struct PairOut {
    a: String,
    b: String,
    cosine: f32,
}

pub struct ScanOpts<'a> {
    pub threshold: f32,
    pub min_lines: usize,
    pub skip_tests: bool,
    pub json: Option<&'a Path>,
    pub top: usize,
    /// Only report clusters with at least this many members ("rule of three"
    /// gating: a helper is only worth it once the logic exists N times).
    pub min_cluster: usize,
}

pub fn run(conn: &Connection, model: &str, opts: &ScanOpts) -> Result<()> {
    let units = load_scannable(conn, model, opts.min_lines, opts.skip_tests)?;
    let pairs = similar_pairs(&units, opts.threshold);

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
        .map(|mut c| {
            c.sort_by(|a, b| b.2.total_cmp(&a.2));
            let mut members: Vec<usize> = c.iter().flat_map(|&(i, j, _)| [i, j]).collect();
            members.sort_unstable();
            members.dedup();
            Cluster { members, pairs: c }
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
        for &m in &cluster.members {
            println!("- {}", units[m].0.label());
        }
        for &(i, j, s) in cluster.pairs.iter().take(10) {
            println!("  {s:.4}  {}  <->  {}", units[i].0.name, units[j].0.name);
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

struct Cluster {
    members: Vec<usize>,
    pairs: Vec<(usize, usize, f32)>,
}

pub fn load_scannable(
    conn: &Connection,
    model: &str,
    min_lines: usize,
    skip_tests: bool,
) -> Result<Vec<(UnitRow, Vec<f32>)>> {
    let all = db::load_units(conn, "main", model)?;
    let n_ignored = all.iter().filter(|(u, _)| u.ignored).count();
    let units: Vec<(UnitRow, Vec<f32>)> = all
        .into_iter()
        .filter(|(u, _)| !u.ignored && u.lines() >= min_lines && !(skip_tests && u.is_test))
        .collect();
    eprintln!(
        "scanning {} units (>= {min_lines} lines, {n_ignored} suppressed by semdup:ignore)",
        units.len()
    );
    Ok(units)
}

/// All pairs (i, j, cosine) with cosine >= threshold, excluding pairs where one
/// unit lexically contains the other (nested functions).
pub fn similar_pairs(units: &[(UnitRow, Vec<f32>)], threshold: f32) -> Vec<(usize, usize, f32)> {
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

fn contains(a: &UnitRow, b: &UnitRow) -> bool {
    (a.start_line <= b.start_line && b.end_line <= a.end_line)
        || (b.start_line <= a.start_line && a.end_line <= b.end_line)
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
