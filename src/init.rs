//! `semdup init`: one-command setup for a repo.
//!
//! Detects source roots (via `git ls-files` when available, so .gitignore is
//! respected), walks the user through the handful of choices that matter
//! (roots, threshold, min lines, test handling), writes `semdup.toml`, and
//! leaves model download + first index to the caller. `--yes` accepts every
//! default for scripted setup.

use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::extract::SUPPORTED_EXTS;

/// Wizard defaults; each is a starting point, not a recommendation — the
/// threshold in particular is per repo and per model.
const DEFAULT_THRESHOLD: f32 = 0.72;
const DEFAULT_MIN_LINES: usize = 8;

/// Run the wizard and write `semdup.toml` into `dir`. Returns the config path.
pub fn run(dir: &Path, yes: bool) -> Result<PathBuf> {
    // Before any prompt, including the overwrite one below: a piped stdin
    // must get the clear error, not an invisible read.
    if !yes && !std::io::stdin().is_terminal() {
        bail!("stdin is not a terminal; use `semdup init --yes` for non-interactive setup");
    }
    let config_path = dir.join("semdup.toml");
    if config_path.exists() {
        if yes {
            bail!(
                "{} already exists; edit it directly or delete it to re-init",
                config_path.display()
            );
        }
        if !confirm(&format!(
            "{} already exists. Overwrite?",
            config_path.display()
        ))? {
            bail!("keeping the existing config");
        }
    }

    let (roots, by_lang) = detect_roots(dir)?;
    if by_lang.is_empty() {
        bail!(
            "no supported source files found under {} (supported extensions: {})",
            dir.display(),
            SUPPORTED_EXTS.join(", ")
        );
    }
    let langs: Vec<String> = by_lang
        .iter()
        .map(|(lang, n)| format!("{lang} ({n} files)"))
        .collect();
    eprintln!("detected: {}", langs.join(", "));

    let roots = if yes { roots } else { prompt_roots(&roots)? };
    let threshold = if yes {
        DEFAULT_THRESHOLD
    } else {
        prompt_parse(
            "similarity threshold (higher = fewer, closer matches)",
            DEFAULT_THRESHOLD,
        )?
    };
    let min_lines = if yes {
        DEFAULT_MIN_LINES
    } else {
        prompt_parse("ignore functions shorter than (lines)", DEFAULT_MIN_LINES)?
    };
    let skip_tests = if yes {
        true
    } else {
        confirm_default("exclude test code from scans?", true)?
    };

    let toml = render_config(&roots, threshold, min_lines, skip_tests);
    std::fs::write(&config_path, &toml)
        .with_context(|| format!("writing {}", config_path.display()))?;
    eprintln!("wrote {}", config_path.display());
    if !gitignore_covers_db(dir) {
        eprintln!("hint: add `semdup.sqlite` to .gitignore (it is a local cache)");
    }
    Ok(config_path)
}

fn render_config(roots: &[String], threshold: f32, min_lines: usize, skip_tests: bool) -> String {
    // Hand-rendered rather than serialized: the file is user-facing and the
    // comments are part of the product.
    let roots_toml = roots
        .iter()
        .map(|r| format!("{:?}", r))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "# semdup configuration — https://github.com/niklebedenko/semdup\n\
         # CLI flags override anything here; `semdup <cmd> --help` lists them.\n\
         \n\
         [extract]\n\
         roots = [{roots_toml}]\n\
         respect_gitignore = true\n\
         # granularity = [\"function\"] # uncomment for function-only indexing\n\
         # min_block_lines = 8\n\
         \n\
         [scan]\n\
         # Cosine similarity cutoff. Per repo and per model: dial it in by\n\
         # running `semdup scan --threshold <t>` at a few values.\n\
         threshold = {threshold}\n\
         min_lines = {min_lines}\n\
         skip_tests = {skip_tests}\n\
         # index = \"exact\" # exact | sparse | auto; sparse is approximate\n"
    )
}

/// Top-level directories (relative to `dir`) that contain supported source
/// files, plus a per-language file count. Files at the repo root put "." in
/// the list.
fn detect_roots(dir: &Path) -> Result<(Vec<String>, BTreeMap<&'static str, usize>)> {
    let files = list_files(dir)?;
    let mut roots: Vec<String> = Vec::new();
    let mut by_lang: BTreeMap<&'static str, usize> = BTreeMap::new();
    for rel in &files {
        let Some(ext) = rel.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !SUPPORTED_EXTS.contains(&ext) {
            continue;
        }
        *by_lang.entry(lang_label(ext)).or_default() += 1;
        let top = match rel.components().next() {
            Some(std::path::Component::Normal(c)) if rel.components().count() > 1 => {
                c.to_string_lossy().into_owned()
            }
            _ => ".".to_string(),
        };
        if !roots.contains(&top) {
            roots.push(top);
        }
    }
    roots.sort();
    // "." subsumes everything else.
    if roots.iter().any(|r| r == ".") {
        roots.retain(|r| r == ".");
    }
    Ok((roots, by_lang))
}

/// Repo files relative to `dir`: `git ls-files` (tracked + untracked,
/// .gitignore respected) when inside a git checkout, else a filesystem walk
/// with the extractor's default excludes.
fn list_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let git = std::process::Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(dir)
        .output();
    if let Ok(out) = git
        && out.status.success()
    {
        return Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(PathBuf::from)
            .collect());
    }
    let mut abs = Vec::new();
    walk(dir, dir, &mut abs)?;
    Ok(abs
        .iter()
        .filter_map(|p| p.strip_prefix(dir).ok().map(PathBuf::from))
        .collect())
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Same excludes the extractor will apply, so detected roots never
        // point at directories extraction would skip anyway. Matched on the
        // root-relative path (`./`-anchored): the absolute path would make a
        // workspace whose parent happens to match an anchored exclude disappear.
        let rel = path.strip_prefix(root).unwrap_or(&path);
        if name.starts_with('.')
            || crate::extract::is_path_excluded(&format!("./{}/", rel.display()), &[])
        {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn lang_label(ext: &str) -> &'static str {
    match ext {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "cs" => "csharp",
        "php" => "php",
        "rb" => "ruby",
        "c" => "c",
        _ => "c++",
    }
}

fn prompt_roots(detected: &[String]) -> Result<Vec<String>> {
    let joined = detected.join(", ");
    let line = prompt(&format!("directories to scan [{joined}]"))?;
    if line.is_empty() {
        return Ok(detected.to_vec());
    }
    Ok(line
        .split(',')
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

fn prompt_parse<T: std::str::FromStr + std::fmt::Display + Copy>(
    label: &str,
    default: T,
) -> Result<T> {
    loop {
        let line = prompt(&format!("{label} [{default}]"))?;
        if line.is_empty() {
            return Ok(default);
        }
        match line.parse() {
            Ok(v) => return Ok(v),
            Err(_) => eprintln!("  could not parse {line:?}, try again"),
        }
    }
}

fn confirm(label: &str) -> Result<bool> {
    confirm_default(label, false)
}

// semdup:ignore — confirm() is a one-line delegate to this; the shared
// vocabulary reads as a near-duplicate to the embedding, but there is
// nothing to deduplicate.
fn confirm_default(label: &str, default: bool) -> Result<bool> {
    let hint = if default { "Y/n" } else { "y/N" };
    let line = prompt(&format!("{label} [{hint}]"))?;
    Ok(match line.to_lowercase().as_str() {
        "" => default,
        "y" | "yes" => true,
        _ => false,
    })
}

fn prompt(label: &str) -> Result<String> {
    eprint!("{label}: ");
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn gitignore_covers_db(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join(".gitignore")) else {
        return false;
    };
    text.lines()
        .map(str::trim)
        .any(|l| l.contains("semdup.sqlite") || l == "*.sqlite")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_top_level_roots_and_languages() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::create_dir_all(tmp.path().join("scripts")).unwrap();
        std::fs::create_dir_all(tmp.path().join("docs")).unwrap();
        std::fs::write(tmp.path().join("src/a.rs"), "").unwrap();
        std::fs::write(tmp.path().join("scripts/b.py"), "").unwrap();
        std::fs::write(tmp.path().join("docs/readme.md"), "").unwrap();
        let (roots, by_lang) = detect_roots(tmp.path()).unwrap();
        assert_eq!(roots, vec!["scripts".to_string(), "src".to_string()]);
        assert_eq!(by_lang.get("rust"), Some(&1));
        assert_eq!(by_lang.get("python"), Some(&1));
    }

    #[test]
    fn fallback_walk_excludes_are_root_relative() {
        // A workspace that itself lives under a directory named like a
        // previously common artifact directory must not have everything
        // excluded; semdup's own eval corpora inside it still must be.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("build").join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("eval/corpus")).unwrap();
        std::fs::write(root.join("src/a.rs"), "").unwrap();
        std::fs::write(root.join("eval/corpus/gen.rs"), "").unwrap();
        let mut out = Vec::new();
        walk(&root, &root, &mut out).unwrap();
        assert_eq!(out, vec![root.join("src/a.rs")]);
    }

    #[test]
    fn root_level_file_collapses_to_dot() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/a.rs"), "").unwrap();
        std::fs::write(tmp.path().join("main.go"), "").unwrap();
        let (roots, _) = detect_roots(tmp.path()).unwrap();
        assert_eq!(roots, vec![".".to_string()]);
    }

    #[test]
    fn rendered_config_parses_back() {
        let toml = render_config(&["src".into(), "lib".into()], 0.72, 8, true);
        let cfg: crate::config::Config = toml::from_str(&toml).unwrap();
        assert_eq!(
            cfg.extract.roots.unwrap(),
            vec![PathBuf::from("src"), PathBuf::from("lib")]
        );
        assert_eq!(cfg.scan.threshold, Some(0.72));
        assert_eq!(cfg.scan.skip_tests, Some(true));
    }
}
