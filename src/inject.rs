//! Planted-clone benchmark used to evaluate embedding models.
//!
//! Each injected unit is a semantics-preserving rewrite of one known source
//! unit. The benchmark ranks the real corpus by cosine similarity to the plant
//! and reports whether the known original appears at rank 1 or within the top 5.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use serde::Deserialize;

use crate::db::{self, UnitRow};
use crate::extract::UnitKind;
use crate::scan::{dot, drop_larger_overlapping_blocks};

#[derive(Deserialize)]
struct PlantEntry {
    /// Path suffix of the injected file (one planted unit per file).
    file: String,
    /// "path-suffix::name" or "path-suffix:start-end::name" of the original.
    original: String,
    /// Mutation level: 1 rename-only, 2 restructure, 3 re-derive.
    level: u8,
}

struct OriginalSelector<'a> {
    path_suffix: &'a str,
    name: &'a str,
    span: Option<(usize, usize)>,
}

impl<'a> OriginalSelector<'a> {
    fn parse(selector: &'a str) -> Result<Self> {
        let (path_and_span, name) = selector
            .split_once("::")
            .with_context(|| format!("bad selector {selector}"))?;
        let (path_suffix, span) = match path_and_span.rsplit_once(':') {
            Some((path, span)) => match span.split_once('-') {
                Some((start, end)) => match (start.parse::<usize>(), end.parse::<usize>()) {
                    (Ok(start), Ok(end)) => (path, Some((start, end))),
                    _ => (path_and_span, None),
                },
                None => (path_and_span, None),
            },
            None => (path_and_span, None),
        };
        Ok(OriginalSelector {
            path_suffix,
            name,
            span,
        })
    }

    fn matches(&self, unit: &UnitRow) -> bool {
        unit.name == self.name
            && unit.path.ends_with(self.path_suffix)
            && self
                .span
                .is_none_or(|(start, end)| unit.start_line == start && unit.end_line == end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(path: &str, name: &str, start_line: usize, end_line: usize) -> UnitRow {
        UnitRow {
            path: path.to_string(),
            name: name.to_string(),
            lang: "rust".to_string(),
            kind: UnitKind::Block,
            start_line,
            end_line,
            hash: "hash".to_string(),
            ignored: false,
            is_test: false,
        }
    }

    #[test]
    fn selector_matches_line_spanned_block_names_with_scope_separators() {
        let selector = OriginalSelector::parse(
            "crates/cli/src/pattern.rs:146-155::patterns_from_reader::match",
        )
        .unwrap();
        assert!(selector.matches(&unit(
            "eval/corpus/ripgrep/crates/cli/src/pattern.rs",
            "patterns_from_reader::match",
            146,
            155,
        )));
        assert!(!selector.matches(&unit(
            "eval/corpus/ripgrep/crates/cli/src/pattern.rs",
            "match",
            146,
            155,
        )));
    }
}

#[derive(Clone, Copy, Default)]
struct LevelAgg {
    n: usize,
    r1: usize,
    r5: usize,
    cos_sum: f32,
    margin_sum: f32,
}

impl LevelAgg {
    fn recall1(self) -> f32 {
        self.r1 as f32 / self.n as f32
    }

    fn recall5(self) -> f32 {
        self.r5 as f32 / self.n as f32
    }

    fn mean_cos(self) -> f32 {
        self.cos_sum / self.n as f32
    }

    fn mean_margin(self) -> f32 {
        self.margin_sum / self.n as f32
    }
}

pub struct InjectEvalOpts<'a> {
    pub manifest_path: &'a Path,
    pub min_lines: usize,
    pub unit_kind: UnitKind,
    pub min_recall5: Option<f32>,
    pub summary_row: bool,
    pub label: Option<&'a str>,
}

pub fn run(conn: &Connection, model: &str, opts: &InjectEvalOpts<'_>) -> Result<()> {
    let manifest: Vec<PlantEntry> =
        serde_json::from_str(&std::fs::read_to_string(opts.manifest_path)?)?;
    if manifest.is_empty() {
        bail!("manifest {} has no entries", opts.manifest_path.display());
    }
    let main: Vec<(UnitRow, Vec<f32>)> = db::load_units(conn, "main", model)?
        .into_iter()
        .filter(|(u, _)| u.kind == opts.unit_kind && u.lines() >= opts.min_lines)
        .collect();
    let injected: Vec<_> = db::load_units(conn, "injected", model)?
        .into_iter()
        .filter(|(u, _)| u.kind == opts.unit_kind)
        .collect();
    let (main, main_overlaps) = drop_larger_overlapping_blocks(main);
    let (injected, injected_overlaps) = drop_larger_overlapping_blocks(injected);
    if injected.is_empty() {
        bail!("no injected units with embeddings; run extract --corpus injected + embed first");
    }

    if !opts.summary_row {
        println!(
            "# inject-eval — model {model}, unit kind {}",
            opts.unit_kind.as_str()
        );
        println!(
            "\ncorpus {} units; {} plants; {} larger overlapping main block(s) and {} injected block(s) skipped\n",
            main.len(),
            manifest.len(),
            main_overlaps,
            injected_overlaps
        );
        println!("| level | plant | cos(orig) | rank | margin | top hit |");
        println!("|---|---|---|---|---|---|");
    }

    let mut agg: BTreeMap<u8, LevelAgg> = BTreeMap::new();

    for entry in &manifest {
        // A rewrite may carry helper fns; the plant is the largest unit in its file.
        let plant = injected
            .iter()
            .filter(|(u, _)| u.path.ends_with(&entry.file))
            .max_by_key(|(u, _)| u.lines())
            .with_context(|| format!("no injected unit from file {}", entry.file))?;
        let original = OriginalSelector::parse(&entry.original)?;

        // Rank all main units by cosine to the plant. The original may span
        // several rows (same fn extracted at several sites); best rank wins.
        let mut scored: Vec<(usize, f32)> = main
            .iter()
            .enumerate()
            .map(|(i, (_, v))| (i, dot(&plant.1, v)))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));

        let Some(rank) = scored
            .iter()
            .position(|&(i, _)| original.matches(&main[i].0))
        else {
            bail!("original {} not found in main corpus", entry.original);
        };
        let cos_orig = scored[rank].1;
        let best_other = scored
            .iter()
            .find(|&&(i, _)| !original.matches(&main[i].0))
            .map_or(0.0, |&(_, s)| s);
        let margin = cos_orig - best_other;
        let top_hit = &main[scored[0].0].0;

        if !opts.summary_row {
            println!(
                "| {} | {} | {:.4} | {} | {:+.4} | {} |",
                entry.level,
                plant.0.label(),
                cos_orig,
                rank + 1,
                margin,
                top_hit.label(),
            );
        }

        let e = agg.entry(entry.level).or_default();
        e.n += 1;
        e.r1 += usize::from(rank == 0);
        e.r5 += usize::from(rank < 5);
        e.cos_sum += cos_orig;
        e.margin_sum += margin;
    }

    if opts.summary_row {
        print_summary_row(model, opts.label, &agg)?;
    } else {
        print_level_summary(&agg);
    }

    let worst_recall5 = agg
        .values()
        .map(|a| a.recall5())
        .fold(f32::INFINITY, f32::min);
    if let Some(min) = opts.min_recall5
        && worst_recall5 < min
    {
        bail!("recall@5 {worst_recall5:.2} at the worst level is below --min-recall5 {min:.2}");
    }
    Ok(())
}

fn print_level_summary(agg: &BTreeMap<u8, LevelAgg>) {
    println!("\n| level | n | recall@1 | recall@5 | F1@1 | mean cos | mean margin |");
    println!("|---|---|---|---|---|---|---|");
    for (level, a) in agg {
        // There is exactly one top-1 prediction and one relevant original per
        // plant, so precision@1 == recall@1 and F1@1 has the same value.
        let f1_at_1 = a.recall1();
        println!(
            "| {} | {} | {:.2} | {:.2} | {:.2} | {:.4} | {:+.4} |",
            level,
            a.n,
            a.recall1(),
            a.recall5(),
            f1_at_1,
            a.mean_cos(),
            a.mean_margin(),
        );
    }
}

fn print_summary_row(model: &str, label: Option<&str>, agg: &BTreeMap<u8, LevelAgg>) -> Result<()> {
    let level = |n| {
        agg.get(&n)
            .copied()
            .with_context(|| format!("no level {n} results in manifest"))
    };
    let l1 = level(1)?;
    let l2 = level(2)?;
    let l3 = level(3)?;
    let total_n: usize = agg.values().map(|a| a.n).sum();
    let macro_f1_at_1 = agg.values().map(|a| a.recall1()).sum::<f32>() / agg.len() as f32;
    println!(
        "| {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:+.4} |",
        label.unwrap_or(model),
        total_n,
        l1.recall1(),
        l1.recall5(),
        l2.recall1(),
        l2.recall5(),
        l3.recall1(),
        l3.recall5(),
        macro_f1_at_1,
        l3.mean_margin(),
    );
    Ok(())
}
