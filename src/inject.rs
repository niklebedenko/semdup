use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use serde::Deserialize;

use crate::db::{self, UnitRow};
use crate::scan::dot;

#[derive(Deserialize)]
struct PlantEntry {
    /// Path suffix of the injected file (one planted unit per file).
    file: String,
    /// "path-suffix::name" of the original the plant was derived from.
    original: String,
    /// Mutation level: 1 rename-only, 2 restructure, 3 re-derive.
    level: u8,
}

pub fn run(
    conn: &Connection,
    model: &str,
    manifest_path: &Path,
    min_lines: usize,
    min_recall5: Option<f32>,
) -> Result<()> {
    let manifest: Vec<PlantEntry> = serde_json::from_str(&std::fs::read_to_string(manifest_path)?)?;
    let main: Vec<(UnitRow, Vec<f32>)> = db::load_units(conn, "main", model)?
        .into_iter()
        .filter(|(u, _)| u.lines() >= min_lines)
        .collect();
    let injected = db::load_units(conn, "injected", model)?;
    if injected.is_empty() {
        bail!("no injected units with embeddings; run extract --corpus injected + embed first");
    }

    println!("# inject-eval — model {model}");
    println!("\ncorpus {} units; {} plants\n", main.len(), manifest.len());
    println!("| level | plant | cos(orig) | rank | margin | top hit |");
    println!("|---|---|---|---|---|---|");

    // level -> (count, recall@1 hits, recall@5 hits, sum cosine, sum margin)
    let mut agg: HashMap<u8, (usize, usize, usize, f32, f32)> = HashMap::new();

    for entry in &manifest {
        // A rewrite may carry helper fns; the plant is the largest unit in its file.
        let plant = injected
            .iter()
            .filter(|(u, _)| u.path.ends_with(&entry.file))
            .max_by_key(|(u, _)| u.lines())
            .with_context(|| format!("no injected unit from file {}", entry.file))?;
        let (opath, oname) = entry
            .original
            .rsplit_once("::")
            .with_context(|| format!("bad selector {}", entry.original))?;

        // Rank all main units by cosine to the plant. The original may span
        // several rows (same fn extracted at several sites); best rank wins.
        let mut scored: Vec<(usize, f32)> = main
            .iter()
            .enumerate()
            .map(|(i, (_, v))| (i, dot(&plant.1, v)))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));

        let is_orig = |u: &UnitRow| u.name == oname && u.path.ends_with(opath);
        let Some(rank) = scored.iter().position(|&(i, _)| is_orig(&main[i].0)) else {
            bail!("original {} not found in main corpus", entry.original);
        };
        let cos_orig = scored[rank].1;
        let best_other = scored
            .iter()
            .find(|&&(i, _)| !is_orig(&main[i].0))
            .map_or(0.0, |&(_, s)| s);
        let margin = cos_orig - best_other;
        let top_hit = &main[scored[0].0].0;

        println!(
            "| {} | {} | {:.4} | {} | {:+.4} | {} |",
            entry.level,
            entry.file,
            cos_orig,
            rank + 1,
            margin,
            top_hit.name,
        );

        let e = agg.entry(entry.level).or_default();
        e.0 += 1;
        e.1 += usize::from(rank == 0);
        e.2 += usize::from(rank < 5);
        e.3 += cos_orig;
        e.4 += margin;
    }

    println!("\n| level | n | recall@1 | recall@5 | mean cos | mean margin |");
    println!("|---|---|---|---|---|---|");
    let mut levels: Vec<_> = agg.into_iter().collect();
    levels.sort_by_key(|&(l, _)| l);
    let mut worst_recall5 = f32::INFINITY;
    for (level, (n, r1, r5, cos_sum, margin_sum)) in levels {
        let recall5 = r5 as f32 / n as f32;
        worst_recall5 = worst_recall5.min(recall5);
        println!(
            "| {} | {} | {:.2} | {:.2} | {:.4} | {:+.4} |",
            level,
            n,
            r1 as f32 / n as f32,
            recall5,
            cos_sum / n as f32,
            margin_sum / n as f32,
        );
    }
    if let Some(min) = min_recall5
        && worst_recall5 < min
    {
        bail!("recall@5 {worst_recall5:.2} at the worst level is below --min-recall5 {min:.2}");
    }
    Ok(())
}
