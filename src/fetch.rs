//! Auto-download of the default embedding model.
//!
//! `cargo install semdup && semdup init` must work without python, so the
//! ONNX export of the default model (produced by `scripts/export_onnx.py`,
//! pooling + L2-norm baked in) is hosted as GitHub release assets and
//! downloaded on first use into the user cache directory. Every file is
//! blake3-verified against the pins below before it is moved into place, so
//! a corrupted or tampered download can never be loaded.
//!
//! Two variants exist because fp16 halves the download and is faster on GPU,
//! but is slower than fp32 on the CPU execution provider (~1.3x measured):
//! `fp32` (CPU default) and `fp16` (picked when the CUDA EP is usable).
//! `scripts/export_onnx.py` remains the bring-your-own-model path; anything
//! with an explicit `model_dir` never touches the network.

use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail, ensure};

use crate::config::DEFAULT_MODEL;

const RELEASE_BASE: &str =
    "https://github.com/niklebedenko/semdup/releases/download/model-coderankembed-1";

struct Asset {
    file: &'static str,
    url_name: &'static str,
    blake3: &'static str,
    bytes: u64,
}

struct Variant {
    name: &'static str,
    assets: [Asset; 2],
    /// Written locally; 108 bytes is not worth a download.
    meta_json: &'static str,
}

const FP32: Variant = Variant {
    name: "fp32",
    assets: [
        Asset {
            file: "model.onnx",
            url_name: "coderankembed-fp32.onnx",
            blake3: "9ee4be33ea135098d40072446f0fc5a1a935e68173676c8e7f13582c136037b9",
            bytes: 548_069_789,
        },
        Asset {
            file: "tokenizer.json",
            url_name: "coderankembed-tokenizer.json",
            blake3: "9323872b7f8abfe19f9bf09eb789d33378be39c951627bdde0aad0e9baeb839d",
            bytes: 711_649,
        },
    ],
    meta_json: r#"{"model":"nomic-ai/CodeRankEmbed","max_seq":2048,"dim":768,"pooling":"cls","fp16":false}"#,
};

// Only reachable when the cuda feature is compiled in (see default_variant).
#[cfg_attr(not(feature = "cuda"), allow(dead_code))]
const FP16: Variant = Variant {
    name: "fp16",
    assets: [
        Asset {
            file: "model.onnx",
            url_name: "coderankembed-fp16.onnx",
            blake3: "3de1fbc8534a7e059c1a22e62ba222a1b10c2624db1390123bec397b4816db11",
            bytes: 274_344_297,
        },
        Asset {
            file: "tokenizer.json",
            url_name: "coderankembed-tokenizer.json",
            blake3: "9323872b7f8abfe19f9bf09eb789d33378be39c951627bdde0aad0e9baeb839d",
            bytes: 711_649,
        },
    ],
    meta_json: r#"{"model":"nomic-ai/CodeRankEmbed","max_seq":2048,"dim":768,"pooling":"cls","fp16":true}"#,
};

/// Pick fp16 only when the CUDA EP reports usable; on the CPU EP fp16 is
/// slower than fp32, so CPU always gets fp32.
fn default_variant() -> &'static Variant {
    #[cfg(feature = "cuda")]
    {
        use ort::execution_providers::{CUDAExecutionProvider, ExecutionProvider};
        if CUDAExecutionProvider::default()
            .is_available()
            .unwrap_or(false)
        {
            return &FP16;
        }
    }
    &FP32
}

fn cache_dir() -> Result<PathBuf> {
    if let Ok(d) = std::env::var("SEMDUP_CACHE") {
        return Ok(PathBuf::from(d));
    }
    if let Ok(d) = std::env::var("XDG_CACHE_HOME")
        && !d.is_empty()
    {
        return Ok(PathBuf::from(d).join("semdup"));
    }
    if let Ok(h) = std::env::var("HOME")
        && !h.is_empty()
    {
        return Ok(PathBuf::from(h).join(".cache").join("semdup"));
    }
    if let Ok(d) = std::env::var("LOCALAPPDATA")
        && !d.is_empty()
    {
        return Ok(PathBuf::from(d).join("semdup"));
    }
    bail!("cannot locate a cache directory (set SEMDUP_CACHE)")
}

/// Directory for the default model, downloading it on first use. Returns an
/// existing, verified-at-download-time directory unchanged.
pub fn ensure_default_model(model: &str) -> Result<PathBuf> {
    ensure!(
        model == DEFAULT_MODEL,
        "no model_dir configured and {model} is not the default model ({DEFAULT_MODEL}); \
         export it with scripts/export_onnx.py and set [embed].model_dir"
    );
    let variant = default_variant();
    let dir = cache_dir()?
        .join("models")
        .join(format!("coderankembed-{}", variant.name));
    let ready = dir.join(".complete");
    if ready.exists() {
        return Ok(dir);
    }
    std::fs::create_dir_all(&dir)?;
    eprintln!(
        "downloading {DEFAULT_MODEL} ({}, {} MB) to {} — one-time setup",
        variant.name,
        variant.assets.iter().map(|a| a.bytes).sum::<u64>() / (1024 * 1024),
        dir.display()
    );
    for asset in &variant.assets {
        download_verified(asset, &dir)
            .with_context(|| format!("downloading {}", asset.url_name))?;
    }
    std::fs::write(dir.join("semdup-model.json"), variant.meta_json)?;
    std::fs::write(&ready, "")?;
    Ok(dir)
}

fn download_verified(asset: &Asset, dir: &std::path::Path) -> Result<()> {
    let url = format!("{RELEASE_BASE}/{}", asset.url_name);
    // Pid-unique temp path: concurrent cold-cache invocations (parallel CI
    // jobs, two terminals) must not truncate each other's in-flight download.
    let tmp = dir.join(format!("{}.{}.part", asset.file, std::process::id()));
    // No overall deadline (the fp32 asset is ~548 MB on arbitrarily slow
    // links), but a stalled connect or read must fail instead of hanging.
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(30))
        .timeout_read(std::time::Duration::from_secs(60))
        .build();
    let resp = agent.get(&url).call().with_context(|| {
        format!(
            "GET {url}\n  (release asset missing or offline? see docs/models.md \
             for exporting the model yourself)"
        )
    })?;
    // The tmp name is pid-unique, so nothing ever reclaims a leftover: on
    // any failure past this point (stall, disk full, bad checksum) the file
    // must be removed here or it accumulates in the cache forever.
    let streamed = (|| -> Result<()> {
        let mut reader = resp.into_reader();
        let mut out = std::fs::File::create(&tmp)?;
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut done: u64 = 0;
        let mut last_report: u64 = 0;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            done += n as u64;
            if done - last_report >= 64 * 1024 * 1024 {
                eprintln!(
                    "  {} — {} / {} MB",
                    asset.file,
                    done / (1024 * 1024),
                    asset.bytes / (1024 * 1024)
                );
                last_report = done;
            }
        }
        out.flush()?;
        drop(out);
        let got = hasher.finalize().to_hex().to_string();
        ensure!(
            got == asset.blake3,
            "checksum mismatch for {} (got {got}, want {}) — truncated or tampered download",
            asset.url_name,
            asset.blake3
        );
        Ok(())
    })();
    if streamed.is_err() {
        std::fs::remove_file(&tmp).ok();
    }
    streamed?;
    std::fs::rename(&tmp, dir.join(asset.file))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_honors_override() {
        // Not a full env-isolation test; just the precedence of the override.
        unsafe { std::env::set_var("SEMDUP_CACHE", "/tmp/semdup-cache-test") };
        assert_eq!(
            cache_dir().unwrap(),
            PathBuf::from("/tmp/semdup-cache-test")
        );
        unsafe { std::env::remove_var("SEMDUP_CACHE") };
    }

    #[test]
    fn non_default_model_is_rejected() {
        let err = ensure_default_model("someone/other-model").unwrap_err();
        assert!(err.to_string().contains("model_dir"));
    }
}
