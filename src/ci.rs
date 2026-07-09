//! CI wrapper for semdup's ordinary refresh/scan/diff commands.
//!
//! GitHub Actions owns checkout and cache restore/save, but the behavioral
//! policy lives here so CI runs stay reproducible from a local shell.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use rusqlite::Connection;

use crate::config::Config;
use crate::diff::{self, DiffPolicy};
use crate::extract::UnitKind;
use crate::init;
use crate::scan;
use crate::{EmbedArgs, refresh, resolve_model};

const DEFAULT_CI_THRESHOLD: f32 = 0.85;
const DEFAULT_CI_MIN_LINES: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CiMode {
    /// Diff in PR/base contexts, scan otherwise.
    Auto,
    /// Run only the PR-style diff gate.
    Diff,
    /// Refresh and run a full-corpus advisory scan.
    Scan,
    /// Run scan first, then the diff gate.
    Both,
}

impl fmt::Display for CiMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CiMode::Auto => f.write_str("auto"),
            CiMode::Diff => f.write_str("diff"),
            CiMode::Scan => f.write_str("scan"),
            CiMode::Both => f.write_str("both"),
        }
    }
}

pub struct CiOpts<'a> {
    pub mode: CiMode,
    pub base: Option<String>,
    pub refresh_base: bool,
    pub threshold: Option<f32>,
    pub policy: DiffPolicy,
    pub min_lines: Option<usize>,
    pub include_tests: bool,
    pub roots: Vec<PathBuf>,
    pub exclude: Vec<String>,
    pub json: Option<&'a Path>,
    pub embed: EmbedArgs,
}

/// Run the CI wrapper. The returned value is the number of hard duplicate
/// findings; callers should exit nonzero when it is greater than zero.
pub fn run(conn: &Connection, cfg: &Config, opts: &CiOpts) -> Result<usize> {
    let repo = std::env::current_dir()?;
    let cfg = ci_config(cfg, &repo, opts)?;
    let base = opts.base.clone().or_else(github_base_ref);
    let mode = resolve_mode(opts.mode, base.as_deref());

    match mode {
        CiMode::Scan => {
            run_scan(conn, &cfg, opts)?;
            Ok(0)
        }
        CiMode::Diff => run_diff(conn, &cfg, opts, base),
        CiMode::Both => {
            run_scan(conn, &cfg, opts)?;
            run_diff(conn, &cfg, opts, base)
        }
        CiMode::Auto => unreachable!("auto resolves before execution"),
    }
}

fn resolve_mode(mode: CiMode, base: Option<&str>) -> CiMode {
    match mode {
        CiMode::Auto if is_github_pull_request() || base.is_some() => CiMode::Diff,
        CiMode::Auto => CiMode::Scan,
        other => other,
    }
}

fn is_github_pull_request() -> bool {
    matches!(
        std::env::var("GITHUB_EVENT_NAME").ok().as_deref(),
        Some("pull_request" | "pull_request_target")
    )
}

fn github_base_ref() -> Option<String> {
    let base = std::env::var("GITHUB_BASE_REF").ok()?;
    if base.is_empty() {
        None
    } else {
        Some(format!("origin/{base}"))
    }
}

fn run_scan(conn: &Connection, cfg: &Config, opts: &CiOpts) -> Result<()> {
    eprintln!("semdup ci: refresh + advisory scan");
    refresh(conn, cfg, &opts.embed)?;
    let model = resolve_model(opts.embed.model.clone(), cfg, Some(&opts.embed));
    let scan_opts = scan::ScanOpts {
        threshold: threshold(cfg, opts),
        index: scan::CandidateIndex::parse(
            cfg.scan
                .index
                .clone()
                .unwrap_or_else(|| "exact".into())
                .as_str(),
        )?,
        min_lines: min_lines(cfg, opts),
        skip_tests: skip_tests(cfg, opts),
        unit_kind: cfg.scan.unit_kind,
        json: None,
        show_bodies: false,
        color: scan::ColorChoice::Never,
        top: 25,
        min_cluster: cfg.scan.min_cluster.unwrap_or(2),
    };
    scan::run(conn, &model.key, &scan_opts)
}

fn run_diff(conn: &Connection, cfg: &Config, opts: &CiOpts, base: Option<String>) -> Result<usize> {
    let base = base.unwrap_or_else(|| "HEAD".to_string());
    if opts.refresh_base {
        refresh_base_corpus(conn, cfg, opts, &base)?;
    }

    eprintln!("semdup ci: diff gate vs {base}");
    let model = resolve_model(opts.embed.model.clone(), cfg, Some(&opts.embed));
    let diff_opts = diff::DiffOpts {
        base,
        min_lines: min_lines(cfg, opts),
        threshold: Some(threshold(cfg, opts)),
        policy: opts.policy,
        json: opts.json,
        skip_tests: skip_tests(cfg, opts),
        strip_comments: cfg.extract.strip_comments.unwrap_or(false),
        exclude: cfg.extract.exclude.clone().unwrap_or_default(),
    };
    let mut mk = || crate::make_backend(&opts.embed, cfg, &model);
    diff::run(conn, &model.key, &diff_opts, &mut mk)
}

fn refresh_base_corpus(
    conn: &Connection,
    current_cfg: &Config,
    opts: &CiOpts,
    base: &str,
) -> Result<()> {
    let current_repo = std::env::current_dir()?;
    let worktree = BaseWorktree::add(base)?;
    let mut base_cfg = current_cfg.clone();
    if let Some(roots) = &current_cfg.extract.roots {
        base_cfg.extract.roots = Some(
            roots
                .iter()
                .map(|root| rebase_root(root, &current_repo, &worktree.path))
                .collect(),
        );
    }
    eprintln!("semdup ci: refreshing base corpus from {base}");
    refresh(conn, &base_cfg, &opts.embed)
}

fn ci_config(cfg: &Config, repo: &Path, opts: &CiOpts) -> Result<Config> {
    let mut cfg = cfg.clone();

    if !opts.roots.is_empty() {
        cfg.extract.roots = Some(
            opts.roots
                .iter()
                .map(|p| absolutize(repo, p))
                .collect::<Vec<_>>(),
        );
    } else if cfg.extract.roots.is_none() {
        let (roots, by_lang) = init::detect_roots(repo)?;
        if by_lang.is_empty() {
            bail!(
                "no semdup.toml and no supported source files found; pass --root or run `semdup init`"
            );
        }
        cfg.extract.roots = Some(
            roots
                .iter()
                .map(|r| absolutize(repo, Path::new(r)))
                .collect(),
        );
        eprintln!("semdup ci: detected roots {}", roots.join(", "));
    }

    if !opts.exclude.is_empty() {
        cfg.extract
            .exclude
            .get_or_insert_with(Vec::new)
            .extend(opts.exclude.clone());
    }
    cfg.extract.respect_gitignore.get_or_insert(true);
    cfg.extract
        .granularity
        .get_or_insert_with(|| vec![UnitKind::Function]);
    cfg.extract.min_block_lines.get_or_insert(8);
    cfg.scan.threshold.get_or_insert(DEFAULT_CI_THRESHOLD);
    cfg.scan.min_lines.get_or_insert(DEFAULT_CI_MIN_LINES);
    cfg.scan.skip_tests.get_or_insert(true);
    Ok(cfg)
}

fn absolutize(repo: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo.join(path)
    }
}

fn rebase_root(root: &Path, from_repo: &Path, to_repo: &Path) -> PathBuf {
    root.strip_prefix(from_repo)
        .map(|rel| to_repo.join(rel))
        .unwrap_or_else(|_| root.to_path_buf())
}

fn threshold(cfg: &Config, opts: &CiOpts) -> f32 {
    opts.threshold
        .or(cfg.scan.threshold)
        .unwrap_or(DEFAULT_CI_THRESHOLD)
}

fn min_lines(cfg: &Config, opts: &CiOpts) -> usize {
    opts.min_lines
        .or(cfg.scan.min_lines)
        .unwrap_or(DEFAULT_CI_MIN_LINES)
}

fn skip_tests(cfg: &Config, opts: &CiOpts) -> bool {
    if opts.include_tests {
        false
    } else {
        cfg.scan.skip_tests.unwrap_or(true)
    }
}

struct BaseWorktree {
    path: PathBuf,
}

impl BaseWorktree {
    fn add(base: &str) -> Result<Self> {
        let root = std::env::var_os("RUNNER_TEMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = root.join(format!("semdup-base-{}-{nonce}", std::process::id()));
        let status = Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&path)
            .arg(base)
            .status()
            .with_context(|| format!("creating temporary worktree for {base}"))?;
        if !status.success() {
            bail!("git worktree add failed for base ref {base}");
        }
        Ok(BaseWorktree { path })
    }
}

impl Drop for BaseWorktree {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .status();
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
