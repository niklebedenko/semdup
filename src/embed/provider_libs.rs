//! Self-heal for `cargo install`ed CUDA builds.
//!
//! onnxruntime loads its CUDA execution provider by dlopening
//! `libonnxruntime_providers_shared.so` / `_cuda.so` from the directory
//! containing the executable. `cargo build` works because ort's build script
//! copies them next to the binary in `target/`, but `cargo install` ships the
//! executable alone, so a crates.io install with `--features cuda` loses the
//! GPU. The libraries do exist on the installing machine — in ort's download
//! cache (`~/.cache/ort.pyke.io/dfbin/<target>/<hash>/`) — so link them next
//! to the executable on first use.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

#[cfg(windows)]
const PROVIDER_LIBS: [&str; 2] = [
    "onnxruntime_providers_shared.dll",
    "onnxruntime_providers_cuda.dll",
];
#[cfg(not(windows))]
const PROVIDER_LIBS: [&str; 2] = [
    "libonnxruntime_providers_shared.so",
    "libonnxruntime_providers_cuda.so",
];

/// onnxruntime resolves the provider libraries relative to `dladdr`'s
/// `dli_fname` for the (statically linked) main module — which on glibc is
/// argv[0]. Invoked as a bare name via PATH, argv[0] has no directory and
/// the lookup falls back to the working directory, missing the libraries
/// next to the executable. Re-exec once with an absolute argv[0] so the
/// lookup lands where `ensure_next_to_exe` put them. No-op (and no loop
/// risk) when argv[0] already has a directory component.
#[cfg(unix)]
pub fn reexec_with_absolute_argv0() {
    let Some(argv0) = std::env::args_os().next() else {
        return;
    };
    if argv0.as_encoded_bytes().contains(&b'/') {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    use std::os::unix::process::CommandExt;
    // exec only returns on failure; degrade to the CWD-lookup status quo.
    let _ = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .exec();
}

/// Make the CUDA provider libraries loadable, linking them from ort's
/// download cache when the executable's directory lacks them. Returns a
/// note describing what was done, or `None` if nothing was needed.
pub fn ensure_next_to_exe() -> Result<Option<String>> {
    let exe = std::env::current_exe().context("locating current executable")?;
    let exe_dir = exe.parent().context("executable has no parent directory")?;
    let missing: Vec<&str> = PROVIDER_LIBS
        .iter()
        .copied()
        .filter(|lib| !exe_dir.join(lib).exists())
        .collect();
    if missing.is_empty() {
        return Ok(None);
    }
    let Some(src) = newest_provider_dir(&cache_roots()) else {
        bail!(
            "CUDA provider libraries ({}) are neither next to {} nor in ort's \
             download cache; copy them there from the target/ directory of a \
             `cargo build --features cuda`",
            missing.join(", "),
            exe.display()
        );
    };
    for lib in &missing {
        link_or_copy(&src.join(lib), &exe_dir.join(lib))
            .with_context(|| format!("linking {lib} into {}", exe_dir.display()))?;
    }
    Ok(Some(format!(
        "linked CUDA provider libraries from {} (cargo install ships only the binary)",
        src.display()
    )))
}

/// Places ort's build script may have put its `dfbin` download cache.
fn cache_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(d) = std::env::var("XDG_CACHE_HOME")
        && !d.is_empty()
    {
        roots.push(PathBuf::from(d).join("ort.pyke.io"));
    }
    if let Ok(h) = std::env::var("HOME")
        && !h.is_empty()
    {
        roots.push(PathBuf::from(&h).join(".cache").join("ort.pyke.io"));
        roots.push(PathBuf::from(&h).join("Library/Caches/ort.pyke.io"));
    }
    if let Ok(d) = std::env::var("LOCALAPPDATA")
        && !d.is_empty()
    {
        roots.push(PathBuf::from(d).join("ort.pyke.io"));
    }
    roots
}

/// Newest `<root>/dfbin/<target>/<hash>/` directory containing every provider
/// library. The cache can hold several onnxruntime versions (one per ort
/// upgrade) and the hash is opaque, so newest-mtime is a heuristic: a
/// mismatched pick fails EP registration and lands in the ordinary
/// CPU-fallback warning — no worse than the missing-library status quo.
fn newest_provider_dir(roots: &[PathBuf]) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for root in roots {
        let Ok(targets) = std::fs::read_dir(root.join("dfbin")) else {
            continue;
        };
        for target in targets.flatten() {
            let Ok(hashes) = std::fs::read_dir(target.path()) else {
                continue;
            };
            for hash in hashes.flatten() {
                let dir = hash.path();
                if !PROVIDER_LIBS.iter().all(|lib| dir.join(lib).is_file()) {
                    continue;
                }
                let Ok(mtime) = std::fs::metadata(&dir).and_then(|m| m.modified()) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(t, _)| mtime > *t) {
                    best = Some((mtime, dir));
                }
            }
        }
    }
    best.map(|(_, dir)| dir)
}

fn link_or_copy(src: &Path, dst: &Path) -> Result<()> {
    // A dangling symlink (cache purged since the last heal) reads as
    // "missing" in ensure_next_to_exe but still blocks creation here.
    if std::fs::symlink_metadata(dst).is_ok() {
        std::fs::remove_file(dst)?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dst)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::copy(src, dst)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plant(dir: &Path, libs: &[&str]) {
        std::fs::create_dir_all(dir).unwrap();
        for lib in libs {
            std::fs::write(dir.join(lib), "so").unwrap();
        }
    }

    #[test]
    fn picks_newest_complete_dir_across_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let old_root = tmp.path().join("old");
        let new_root = tmp.path().join("new");
        let older = old_root.join("dfbin/x86_64-unknown-linux-gnu/aaaa");
        let newer = new_root.join("dfbin/x86_64-unknown-linux-gnu/bbbb");
        let incomplete = new_root.join("dfbin/x86_64-unknown-linux-gnu/cccc");
        plant(&older, &PROVIDER_LIBS);
        // Directory mtimes must differ for "newest" to be well-defined.
        std::thread::sleep(std::time::Duration::from_millis(30));
        plant(&newer, &PROVIDER_LIBS);
        std::thread::sleep(std::time::Duration::from_millis(30));
        plant(&incomplete, &PROVIDER_LIBS[..1]);

        let got = newest_provider_dir(&[old_root, new_root]).unwrap();
        assert_eq!(got, newer);
        assert!(newest_provider_dir(&[tmp.path().join("absent")]).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn link_or_copy_replaces_dangling_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("lib.so");
        std::fs::write(&src, "so").unwrap();
        let dst = tmp.path().join("dst.so");
        std::os::unix::fs::symlink(tmp.path().join("purged.so"), &dst).unwrap();
        assert!(!dst.exists()); // dangling: the heal sees it as missing
        link_or_copy(&src, &dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"so");
    }
}
