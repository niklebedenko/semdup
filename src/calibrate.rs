use std::path::Path;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use serde::Deserialize;

use crate::db::{self, UnitRow};
use crate::scan::dot;

#[derive(Deserialize)]
struct Labels {
    positives: Vec<(String, String)>,
    negatives: Vec<(String, String)>,
}

/// Selector: "path-suffix::name". The path part matches on suffix so labels
/// stay valid if the repo root moves.
fn resolve<'a>(units: &'a [(UnitRow, Vec<f32>)], sel: &str) -> Result<&'a (UnitRow, Vec<f32>)> {
    let (path_sfx, name) = sel
        .rsplit_once("::")
        .with_context(|| format!("selector '{sel}' missing '::'"))?;
    let matches: Vec<_> = units
        .iter()
        .filter(|(u, _)| u.name == name && u.path.ends_with(path_sfx))
        .collect();
    match matches.len() {
        0 => bail!("selector '{sel}' matched nothing"),
        1 => Ok(matches[0]),
        n => bail!("selector '{sel}' ambiguous ({n} matches)"),
    }
}

pub fn run(conn: &Connection, model: &str, labels_path: &Path) -> Result<()> {
    let labels: Labels = serde_json::from_str(&std::fs::read_to_string(labels_path)?)?;
    let units = db::load_units(conn, "main", model)?;

    let score = |pairs: &[(String, String)]| -> Result<Vec<f32>> {
        pairs
            .iter()
            .map(|(a, b)| {
                let ua = resolve(&units, a)?;
                let ub = resolve(&units, b)?;
                Ok(dot(&ua.1, &ub.1))
            })
            .collect()
    };
    let pos = score(&labels.positives)?;
    let neg = score(&labels.negatives)?;

    println!("# calibrate — model {model}");
    println!("\npositives ({}):", pos.len());
    for (s, (a, b)) in pos.iter().zip(&labels.positives) {
        println!("  {s:.4}  {a}  <->  {b}");
    }
    println!("\nnegatives ({}):", neg.len());
    for (s, (a, b)) in neg.iter().zip(&labels.negatives) {
        println!("  {s:.4}  {a}  <->  {b}");
    }

    let stats = |v: &[f32]| {
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        let min = v.iter().copied().fold(f32::INFINITY, f32::min);
        let max = v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        (mean, min, max)
    };
    let (pm, pmin, pmax) = stats(&pos);
    let (nm, nmin, nmax) = stats(&neg);
    println!("\npos: mean {pm:.4} min {pmin:.4} max {pmax:.4}");
    println!("neg: mean {nm:.4} min {nmin:.4} max {nmax:.4}");

    // Threshold sweep maximizing balanced accuracy.
    let mut best = (0.0f32, 0.0f32);
    let mut t = 0.5f32;
    while t < 0.995 {
        let tpr = pos.iter().filter(|&&s| s >= t).count() as f32 / pos.len() as f32;
        let tnr = neg.iter().filter(|&&s| s < t).count() as f32 / neg.len() as f32;
        let bal = (tpr + tnr) / 2.0;
        if bal > best.1 {
            best = (t, bal);
        }
        t += 0.005;
    }
    println!(
        "\nsuggested threshold: {:.3} (balanced accuracy {:.3}, separation margin {:.4})",
        best.0,
        best.1,
        pmin - nmax
    );
    Ok(())
}
