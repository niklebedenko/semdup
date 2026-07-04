//! semdup: embedding-based near-duplicate function detector.
//!
//! Pipeline: tree-sitter extraction -> SQLite unit+embedding cache
//! -> embedding (built-in ONNX Runtime backend, or a python sidecar for
//! arbitrary models) -> exact pairwise cosine -> clustered report.
//! Suppression: put `semdup:ignore` in a comment on, or up to three lines
//! above, the function signature. Thresholds are per repo and per model:
//! dial one in by running `scan` at a few values against your own code.

mod baseline;
mod config;
mod db;
mod diff;
mod embed;
mod extract;
mod inject;
mod scan;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use config::Config;

#[derive(Parser)]
#[command(
    name = "semdup",
    version,
    about = "embedding-based near-duplicate detector"
)]
struct Cli {
    /// SQLite cache path (default: from semdup.toml, else semdup.sqlite).
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args)]
struct EmbedArgs {
    /// Embedding model id (cache key for vectors).
    #[arg(long)]
    model: Option<String>,
    /// "onnx" (built-in) or "sidecar" (external script).
    #[arg(long)]
    backend: Option<String>,
    /// ONNX backend: directory with model.onnx + tokenizer.json + semdup-model.json.
    #[arg(long)]
    model_dir: Option<PathBuf>,
    /// Sidecar backend: python script implementing the JSONL protocol.
    #[arg(long)]
    script: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse source trees and (re)build the unit table for a corpus.
    Extract {
        /// Root directories to walk for supported source files.
        #[arg(long)]
        root: Vec<PathBuf>,
        /// Corpus name ("main" for the real tree, "injected" for eval plants).
        #[arg(long, default_value = "main")]
        corpus: String,
        /// Path substrings to skip in addition to the built-in excludes.
        #[arg(long)]
        exclude: Vec<String>,
        /// Strip comments and Python docstrings from unit text before
        /// hashing/embedding: compares meaning of code alone, and shows how
        /// much of the similarity signal is prose.
        #[arg(long)]
        strip_comments: bool,
    },
    /// Embed all units that lack a vector for the configured model.
    Embed {
        #[command(flatten)]
        embed: EmbedArgs,
    },
    /// Report near-duplicate clusters above a cosine threshold.
    Scan {
        #[arg(long)]
        model: Option<String>,
        /// Cosine threshold; dial in by trying a few values on your repo.
        #[arg(long)]
        threshold: Option<f32>,
        /// Ignore units shorter than this many lines.
        #[arg(long)]
        min_lines: Option<usize>,
        /// Exclude test code from the scan.
        #[arg(long)]
        skip_tests: bool,
        /// Also write the full pair list as JSON here.
        #[arg(long)]
        json: Option<PathBuf>,
        /// Cap the number of clusters printed.
        #[arg(long, default_value_t = 50)]
        top: usize,
        /// Only report clusters with at least this many members (rule of
        /// three: pass 3 to see only logic that already exists 3+ times).
        #[arg(long)]
        min_cluster: Option<usize>,
        /// Suppress pairs recorded in this baseline file.
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Write current pairs to this baseline file instead of reporting.
        #[arg(long)]
        write_baseline: Option<PathBuf>,
    },
    /// Show nearest corpus neighbors for functions touched by a git diff (per-MR mode).
    Diff {
        /// Base ref to diff the working tree against.
        #[arg(long, default_value = "HEAD")]
        base: String,
        #[arg(long)]
        min_lines: Option<usize>,
        /// Threshold for DUP/REVIEW verdicts; omit for evidence-only output.
        #[arg(long)]
        threshold: Option<f32>,
        #[arg(long)]
        skip_tests: bool,
        #[arg(long)]
        json: Option<PathBuf>,
        /// Exit nonzero if anything is flagged (for CI).
        #[arg(long)]
        check: bool,
        #[command(flatten)]
        embed: EmbedArgs,
    },
    /// Measure recall of planted semantics-preserving rewrites.
    InjectEval {
        #[arg(long)]
        model: Option<String>,
        /// JSON: [{"file": "...", "original": "path::name", "level": 1}, ..]
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        min_lines: Option<usize>,
        /// Exit nonzero if any level's recall@5 falls below this (for CI).
        #[arg(long)]
        min_recall5: Option<f32>,
    },
    /// Print unit/embedding counts.
    Status,
}

fn resolve_model(cli: Option<String>, cfg: &Config) -> Result<String> {
    cli.or_else(|| cfg.embed.model.clone())
        .context("no model given (--model or [embed].model in semdup.toml)")
}

/// Build the embedding backend from CLI + config. Sidecar needs a script;
/// onnx needs a model dir; the default backend is onnx when a model_dir is
/// known, else sidecar.
fn make_backend(args: &EmbedArgs, cfg: &Config, model: &str) -> Result<Box<dyn embed::Backend>> {
    let backend = args
        .backend
        .clone()
        .or_else(|| cfg.embed.backend.clone())
        .unwrap_or_else(|| {
            if args.model_dir.is_some() || cfg.embed.model_dir.is_some() {
                "onnx".into()
            } else {
                "sidecar".into()
            }
        });
    match backend.as_str() {
        #[cfg(feature = "onnx")]
        "onnx" => {
            let dir = args
                .model_dir
                .clone()
                .or_else(|| cfg.embed.model_dir.clone())
                .context("onnx backend needs --model-dir (see scripts/export_onnx.py)")?;
            Ok(Box::new(embed::onnx::Onnx::load(&dir)?))
        }
        #[cfg(not(feature = "onnx"))]
        "onnx" => bail!("this build has no onnx backend (rebuild with --features onnx)"),
        "sidecar" => {
            let script = args
                .script
                .clone()
                .or_else(|| cfg.embed.script.clone())
                .context("sidecar backend needs --script")?;
            Ok(Box::new(embed::sidecar::Sidecar {
                script,
                model: model.to_string(),
                python: std::env::var("SEMDUP_PYTHON").unwrap_or_else(|_| "python3".into()),
            }))
        }
        other => bail!("unknown backend '{other}' (expected onnx or sidecar)"),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::discover(&std::env::current_dir()?)?;
    let db_path = cli
        .db
        .clone()
        .or_else(|| cfg.db.clone())
        .unwrap_or_else(|| PathBuf::from("semdup.sqlite"));
    let conn = db::open(&db_path)?;

    match cli.cmd {
        Cmd::Extract {
            root,
            corpus,
            exclude,
            strip_comments,
        } => {
            let roots = if root.is_empty() {
                cfg.extract
                    .roots
                    .clone()
                    .context("no roots given (--root or [extract].roots in semdup.toml)")?
            } else {
                root
            };
            let mut excludes = cfg.extract.exclude.clone().unwrap_or_default();
            excludes.extend(exclude);
            let strip = strip_comments || cfg.extract.strip_comments.unwrap_or(false);
            let units = extract::extract_roots(&roots, &excludes, strip)?;
            let n = units.len();
            db::replace_corpus(&conn, &corpus, &units)?;
            eprintln!("extracted {n} units into corpus '{corpus}'");
        }
        Cmd::Embed { embed: args } => {
            let model = resolve_model(args.model.clone(), &cfg)?;
            let mut backend = make_backend(&args, &cfg, &model)?;
            embed::run(&conn, &model, backend.as_mut())?;
        }
        Cmd::Scan {
            model,
            threshold,
            min_lines,
            skip_tests,
            json,
            top,
            min_cluster,
            baseline,
            write_baseline,
        } => {
            let model = resolve_model(model, &cfg)?;
            let threshold = threshold
                .or(cfg.scan.threshold)
                .context("no threshold given (--threshold or [scan].threshold in semdup.toml)")?;
            let opts = scan::ScanOpts {
                threshold,
                min_lines: min_lines.or(cfg.scan.min_lines).unwrap_or(5),
                skip_tests: skip_tests || cfg.scan.skip_tests.unwrap_or(false),
                json: json.as_deref(),
                top,
                min_cluster: min_cluster.or(cfg.scan.min_cluster).unwrap_or(2),
                baseline: baseline.as_deref().or(cfg.scan.baseline.as_deref()),
                write_baseline: write_baseline.as_deref(),
            };
            scan::run(&conn, &model, &opts)?;
        }
        Cmd::Diff {
            base,
            min_lines,
            threshold,
            skip_tests,
            json,
            check,
            embed: args,
        } => {
            let model = resolve_model(args.model.clone(), &cfg)?;
            let opts = diff::DiffOpts {
                base,
                min_lines: min_lines.or(cfg.scan.min_lines).unwrap_or(5),
                threshold: threshold.or(cfg.scan.threshold),
                json: json.as_deref(),
                skip_tests: skip_tests || cfg.scan.skip_tests.unwrap_or(false),
                strip_comments: cfg.extract.strip_comments.unwrap_or(false),
            };
            let mut mk = || make_backend(&args, &cfg, &model);
            let findings = diff::run(&conn, &model, &opts, &mut mk)?;
            if check && findings > 0 {
                std::process::exit(1);
            }
        }
        Cmd::InjectEval {
            model,
            manifest,
            min_lines,
            min_recall5,
        } => {
            let model = resolve_model(model, &cfg)?;
            inject::run(
                &conn,
                &model,
                &manifest,
                min_lines.or(cfg.scan.min_lines).unwrap_or(5),
                min_recall5,
            )?;
        }
        Cmd::Status => db::print_status(&conn)?,
    }
    Ok(())
}
