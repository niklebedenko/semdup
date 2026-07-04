//! `semdup.toml` discovery and merging. Config supplies defaults; explicit
//! CLI flags always win. Every field is optional — a missing config file is
//! equivalent to an empty one.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// SQLite cache path (relative paths resolve against the config file).
    pub db: Option<PathBuf>,
    #[serde(default)]
    pub extract: Extract,
    #[serde(default)]
    pub embed: Embed,
    #[serde(default)]
    pub scan: Scan,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Extract {
    pub roots: Option<Vec<PathBuf>>,
    pub exclude: Option<Vec<String>>,
    /// Strip comments and Python docstrings from unit text before embedding.
    pub strip_comments: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Embed {
    /// Embedding model id; also the cache key for vectors.
    pub model: Option<String>,
    /// "onnx" (built-in) or "sidecar" (external python script).
    pub backend: Option<String>,
    /// ONNX backend: directory holding model.onnx + tokenizer.json + semdup-model.json.
    pub model_dir: Option<PathBuf>,
    /// Sidecar backend: script path.
    pub script: Option<PathBuf>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Scan {
    /// Cosine threshold; per repo and per model, so dial it in on your own code.
    pub threshold: Option<f32>,
    pub min_lines: Option<usize>,
    pub skip_tests: Option<bool>,
    /// Only report clusters with at least this many members.
    pub min_cluster: Option<usize>,
    /// Baseline file of known pairs to suppress.
    pub baseline: Option<PathBuf>,
}

impl Config {
    /// Search `start` and its ancestors for `semdup.toml`. Paths inside the
    /// config are rebased onto the directory the file was found in.
    pub fn discover(start: &Path) -> Result<Config> {
        for dir in start.ancestors() {
            let candidate = dir.join("semdup.toml");
            if candidate.is_file() {
                let text = std::fs::read_to_string(&candidate)?;
                let mut cfg: Config = toml::from_str(&text)
                    .with_context(|| format!("parsing {}", candidate.display()))?;
                cfg.rebase(dir);
                return Ok(cfg);
            }
        }
        Ok(Config::default())
    }

    fn rebase(&mut self, dir: &Path) {
        let rebase = |p: &mut PathBuf| {
            if p.is_relative() {
                *p = dir.join(&*p);
            }
        };
        if let Some(p) = &mut self.db {
            rebase(p);
        }
        if let Some(roots) = &mut self.extract.roots {
            roots.iter_mut().for_each(rebase);
        }
        if let Some(p) = &mut self.embed.model_dir {
            rebase(p);
        }
        if let Some(p) = &mut self.embed.script {
            rebase(p);
        }
        if let Some(p) = &mut self.scan.baseline {
            rebase(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_rebases() {
        let cfg: Config = toml::from_str(
            r#"
            db = "cache.sqlite"
            [extract]
            roots = ["src", "/abs/lib"]
            [embed]
            model = "nomic-ai/CodeRankEmbed"
            backend = "onnx"
            [scan]
            threshold = 0.625
            skip_tests = true
            "#,
        )
        .unwrap();
        let mut cfg = cfg;
        cfg.rebase(Path::new("/repo"));
        assert_eq!(cfg.db.as_deref(), Some(Path::new("/repo/cache.sqlite")));
        let roots = cfg.extract.roots.unwrap();
        assert_eq!(roots[0], Path::new("/repo/src"));
        assert_eq!(roots[1], Path::new("/abs/lib"));
        assert_eq!(cfg.scan.threshold, Some(0.625));
    }

    #[test]
    fn unknown_fields_rejected() {
        assert!(toml::from_str::<Config>("thresold = 0.5").is_err());
    }
}
