//! semdup: embedding-based near-duplicate function detector.
//!
//! Pipeline: tree-sitter extraction -> SQLite unit+embedding cache
//! -> embedding (built-in ONNX Runtime backend, or a python sidecar for
//! arbitrary models) -> candidate search with exact scoring -> clustered report.
//! Suppression: put `semdup:ignore` in a comment on, or up to three lines
//! above, the function signature. Thresholds are per repo and per model:
//! dial one in by running `scan` at a few values against your own code.

mod config;
mod db;
mod diff;
mod embed;
mod extract;
// Only the onnx backend downloads models (and needs ureq); slim builds
// resolve the default model name from config::DEFAULT_MODEL.
#[cfg(feature = "onnx")]
mod fetch;
mod init;
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
    /// ONNX execution provider: auto, cpu, or cuda.
    #[arg(long)]
    provider: Option<String>,
    /// ONNX backend: directory with model.onnx + tokenizer.json + semdup-model.json.
    #[arg(long)]
    model_dir: Option<PathBuf>,
    /// Sidecar backend: python script implementing the JSONL protocol.
    #[arg(long)]
    script: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Set up this repo: detect roots, write semdup.toml, fetch the model,
    /// build the first index.
    Init {
        /// Accept all defaults without prompting.
        #[arg(long)]
        yes: bool,
    },
    /// Re-extract the configured roots and embed anything new or changed.
    Refresh {
        #[command(flatten)]
        embed: EmbedArgs,
    },
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
        /// Respect .gitignore and related git exclude files while walking roots.
        #[arg(long, conflicts_with = "no_respect_gitignore")]
        respect_gitignore: bool,
        /// Include files even when git ignore rules would skip them.
        #[arg(long)]
        no_respect_gitignore: bool,
        /// Strip comments and Python docstrings from unit text before
        /// hashing/embedding: compares meaning of code alone, and shows how
        /// much of the similarity signal is prose.
        #[arg(long)]
        strip_comments: bool,
        /// Unit granularity to extract. Repeat for multiple values; defaults to
        /// functions plus executable blocks.
        #[arg(long, value_enum)]
        granularity: Vec<extract::UnitKind>,
        /// Do not extract executable block units shorter than this many lines.
        #[arg(long)]
        min_block_lines: Option<usize>,
    },
    /// Embed all units that lack a vector for the configured model.
    Embed {
        #[command(flatten)]
        embed: EmbedArgs,
    },
    /// Report near-duplicate clusters above a cosine threshold.
    Scan {
        /// Skip the automatic re-index of configured roots before scanning.
        #[arg(long)]
        no_refresh: bool,
        #[command(flatten)]
        embed: EmbedArgs,
        /// Cosine threshold; dial in by trying a few values on your repo.
        #[arg(short = 't', long)]
        threshold: Option<f32>,
        /// Candidate search index: exact, sparse, or auto.
        #[arg(long)]
        index: Option<String>,
        /// Ignore units shorter than this many lines.
        #[arg(long)]
        min_lines: Option<usize>,
        /// Exclude test code from the scan.
        #[arg(long)]
        skip_tests: bool,
        /// Limit the scan to one extracted unit kind.
        #[arg(long, value_enum)]
        unit_kind: Option<extract::UnitKind>,
        /// Also write the full pair list as JSON here.
        #[arg(long)]
        json: Option<PathBuf>,
        /// Print the source body for each displayed duplicate candidate.
        #[arg(long)]
        show_bodies: bool,
        /// When to syntax-highlight snippet bodies: auto, always, or never.
        #[arg(long, value_enum, default_value_t = scan::ColorChoice::Auto)]
        color: scan::ColorChoice,
        /// Cap the number of clusters printed.
        #[arg(long, default_value_t = 50)]
        top: usize,
        /// Only report clusters with at least this many members (rule of
        /// three: pass 3 to see only logic that already exists 3+ times).
        #[arg(short = 'm', long)]
        min_cluster: Option<usize>,
    },
    /// Show nearest corpus neighbors for functions touched by a git diff (per-MR mode).
    Diff {
        /// Base ref to diff the working tree against.
        #[arg(long, default_value = "HEAD")]
        base: String,
        #[arg(long)]
        min_lines: Option<usize>,
        /// Threshold for DUP/REVIEW verdicts; omit for evidence-only output.
        #[arg(short = 't', long)]
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
        /// Label to use in --summary-row output.
        #[arg(long)]
        label: Option<String>,
        /// JSON: [{"file": "...", "original": "path::name", "level": 1}, ..]
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        min_lines: Option<usize>,
        /// Evaluate this extracted unit kind.
        #[arg(long, value_enum, default_value_t = extract::UnitKind::Function)]
        unit_kind: extract::UnitKind,
        /// Exit nonzero if any level's recall@5 falls below this (for CI).
        #[arg(long)]
        min_recall5: Option<f32>,
        /// Print one Markdown table row for the cross-model eval matrix.
        #[arg(long)]
        summary_row: bool,
    },
    /// Print unit/embedding counts.
    Status,
}

struct ModelSelection {
    /// Key used for embedding cache rows and scan/eval lookups.
    key: String,
    /// Model id passed to backends that load by name instead of model_dir.
    backend_model: String,
}

fn default_model_key(args: Option<&EmbedArgs>, cfg: &Config) -> &'static str {
    match args
        .and_then(|args| args.backend.as_deref())
        .or(cfg.embed.backend.as_deref())
    {
        Some("sidecar") => config::DEFAULT_MODEL,
        Some("onnx") | None if cfg!(feature = "onnx") => config::DEFAULT_HOSTED_MODEL,
        Some("onnx") | None => config::DEFAULT_MODEL,
        Some(_) => config::DEFAULT_HOSTED_MODEL,
    }
}

fn resolve_model(cli: Option<String>, cfg: &Config, args: Option<&EmbedArgs>) -> ModelSelection {
    if let Some(model) = cli.or_else(|| cfg.embed.model.clone()) {
        return ModelSelection {
            key: model.clone(),
            backend_model: model,
        };
    }
    ModelSelection {
        key: default_model_key(args, cfg).to_string(),
        backend_model: config::DEFAULT_MODEL.to_string(),
    }
}

/// Build the embedding backend from CLI + config. Sidecar needs a script;
/// onnx needs a model dir or the default model (auto-downloaded); the
/// default backend is onnx when the feature is compiled in, else sidecar.
fn make_backend(
    args: &EmbedArgs,
    cfg: &Config,
    model: &ModelSelection,
) -> Result<Box<dyn embed::Backend>> {
    let backend = args
        .backend
        .clone()
        .or_else(|| cfg.embed.backend.clone())
        .unwrap_or_else(|| {
            if cfg!(feature = "onnx") {
                "onnx".into()
            } else {
                "sidecar".into()
            }
        });
    match backend.as_str() {
        #[cfg(feature = "onnx")]
        "onnx" => {
            let provider = args
                .provider
                .clone()
                .or_else(|| cfg.embed.provider.clone())
                .unwrap_or_else(|| "auto".into());
            // Explicit model_dir wins; otherwise the default model is
            // downloaded into the user cache on first use (fetch.rs).
            let dir = match args
                .model_dir
                .clone()
                .or_else(|| cfg.embed.model_dir.clone())
            {
                Some(d) => d,
                None => fetch::ensure_default_model(&model.key, &provider)?,
            };
            Ok(Box::new(embed::onnx::Onnx::load(&dir, &provider)?))
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
                model: model.backend_model.clone(),
                python: std::env::var("SEMDUP_PYTHON").unwrap_or_else(|_| "python3".into()),
            }))
        }
        other => bail!("unknown backend '{other}' (expected onnx or sidecar)"),
    }
}

/// Re-extract the configured roots into the "main" corpus and embed anything
/// the cache is missing. Shared by `init`, `refresh`, and `scan`'s
/// auto-refresh.
fn refresh(conn: &rusqlite::Connection, cfg: &Config, args: &EmbedArgs) -> Result<()> {
    let roots = cfg
        .extract
        .roots
        .clone()
        .context("no roots configured (run `semdup init` or set [extract].roots)")?;
    let excludes = cfg.extract.exclude.clone().unwrap_or_default();
    let strip = cfg.extract.strip_comments.unwrap_or(false);
    let units = extract::extract_roots(
        &roots,
        &excludes,
        extract::ExtractOpts {
            strip_comments: strip,
            respect_gitignore: cfg.extract.respect_gitignore.unwrap_or(true),
            granularity: cfg
                .extract
                .granularity
                .clone()
                .unwrap_or_else(|| extract::ExtractOpts::default().granularity),
            min_block_lines: cfg
                .extract
                .min_block_lines
                .unwrap_or_else(|| extract::ExtractOpts::default().min_block_lines),
        },
    )?;
    eprintln!("indexed {} units", units.len());
    db::replace_corpus(conn, "main", &units)?;
    let model = resolve_model(args.model.clone(), cfg, Some(args));
    embed_pending(conn, cfg, args, &model)?;
    Ok(())
}

fn embed_pending(
    conn: &rusqlite::Connection,
    cfg: &Config,
    args: &EmbedArgs,
    model: &ModelSelection,
) -> Result<()> {
    if db::pending_count(conn, &model.key)? == 0 {
        eprintln!("nothing to embed for {}", model.key);
        return Ok(());
    }
    let mut backend = make_backend(args, cfg, model)?;
    embed::run(conn, &model.key, backend.as_mut())?;
    Ok(())
}

fn main() -> Result<()> {
    #[cfg(all(unix, feature = "cuda"))]
    embed::provider_libs::reexec_with_absolute_argv0();
    let cli = Cli::parse();
    let cfg = Config::discover(&std::env::current_dir()?)?;
    let db_path = cli
        .db
        .clone()
        .or_else(|| cfg.db.clone())
        .unwrap_or_else(|| PathBuf::from("semdup.sqlite"));
    let conn = db::open(&db_path)?;

    match cli.cmd {
        Cmd::Init { yes } => {
            let dir = std::env::current_dir()?;
            init::run(&dir, yes)?;
            // Re-discover: the wizard just wrote the config this run indexes with.
            let cfg = Config::discover(&dir)?;
            let args = EmbedArgs {
                model: None,
                backend: None,
                provider: None,
                model_dir: None,
                script: None,
            };
            refresh(&conn, &cfg, &args)?;
            eprintln!("\nready — run `semdup scan` to see near-duplicate clusters");
        }
        Cmd::Refresh { embed: args } => refresh(&conn, &cfg, &args)?,
        Cmd::Extract {
            root,
            corpus,
            exclude,
            respect_gitignore,
            no_respect_gitignore,
            strip_comments,
            granularity,
            min_block_lines,
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
            let respect_gitignore = if respect_gitignore {
                true
            } else if no_respect_gitignore {
                false
            } else {
                cfg.extract.respect_gitignore.unwrap_or(true)
            };
            let units = extract::extract_roots(
                &roots,
                &excludes,
                extract::ExtractOpts {
                    strip_comments: strip,
                    respect_gitignore,
                    granularity: if granularity.is_empty() {
                        cfg.extract
                            .granularity
                            .clone()
                            .unwrap_or_else(|| extract::ExtractOpts::default().granularity)
                    } else {
                        granularity
                    },
                    min_block_lines: min_block_lines
                        .or(cfg.extract.min_block_lines)
                        .unwrap_or_else(|| extract::ExtractOpts::default().min_block_lines),
                },
            )?;
            let n = units.len();
            db::replace_corpus(&conn, &corpus, &units)?;
            eprintln!("extracted {n} units into corpus '{corpus}'");
        }
        Cmd::Embed { embed: args } => {
            let model = resolve_model(args.model.clone(), &cfg, Some(&args));
            embed_pending(&conn, &cfg, &args, &model)?;
        }
        Cmd::Scan {
            no_refresh,
            embed: args,
            threshold,
            index,
            min_lines,
            skip_tests,
            unit_kind,
            json,
            show_bodies,
            color,
            top,
            min_cluster,
        } => {
            // Keep the index current unless told not to; repos driving
            // extract/embed explicitly (no configured roots) scan as-is.
            if !no_refresh && cfg.extract.roots.is_some() {
                refresh(&conn, &cfg, &args)?;
            }
            let model = resolve_model(args.model.clone(), &cfg, Some(&args));
            let threshold = threshold
                .or(cfg.scan.threshold)
                .context("no threshold given (--threshold, or run `semdup init`)")?;
            let opts = scan::ScanOpts {
                threshold,
                index: scan::CandidateIndex::parse(
                    index
                        .or(cfg.scan.index)
                        .unwrap_or_else(|| "exact".into())
                        .as_str(),
                )?,
                min_lines: min_lines.or(cfg.scan.min_lines).unwrap_or(5),
                skip_tests: skip_tests || cfg.scan.skip_tests.unwrap_or(false),
                unit_kind: unit_kind.or(cfg.scan.unit_kind),
                json: json.as_deref(),
                show_bodies,
                color,
                top,
                min_cluster: min_cluster.or(cfg.scan.min_cluster).unwrap_or(2),
            };
            scan::run(&conn, &model.key, &opts)?;
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
            let model = resolve_model(args.model.clone(), &cfg, Some(&args));
            let opts = diff::DiffOpts {
                base,
                min_lines: min_lines.or(cfg.scan.min_lines).unwrap_or(5),
                threshold: threshold.or(cfg.scan.threshold),
                json: json.as_deref(),
                skip_tests: skip_tests || cfg.scan.skip_tests.unwrap_or(false),
                strip_comments: cfg.extract.strip_comments.unwrap_or(false),
                exclude: cfg.extract.exclude.clone().unwrap_or_default(),
            };
            let mut mk = || make_backend(&args, &cfg, &model);
            let findings = diff::run(&conn, &model.key, &opts, &mut mk)?;
            if check && findings > 0 {
                std::process::exit(1);
            }
        }
        Cmd::InjectEval {
            model,
            label,
            manifest,
            min_lines,
            unit_kind,
            min_recall5,
            summary_row,
        } => {
            let model = resolve_model(model, &cfg, None);
            let opts = inject::InjectEvalOpts {
                manifest_path: &manifest,
                min_lines: min_lines.or(cfg.scan.min_lines).unwrap_or(5),
                unit_kind,
                min_recall5,
                summary_row,
                label: label.as_deref(),
            };
            inject::run(&conn, &model.key, &opts)?;
        }
        Cmd::Status => db::print_status(&conn)?,
    }
    Ok(())
}
