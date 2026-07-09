//! Built-in ONNX Runtime backend.
//!
//! Loads a model directory produced by `scripts/export_onnx.py`:
//!   model.onnx         — (input_ids, attention_mask) -> embedding [batch, dim];
//!                        pooling + L2 normalization are baked into the graph
//!   tokenizer.json     — HF fast tokenizer
//!   semdup-model.json  — { "max_seq": .., "dim": .. }
//!
//! Uses the requested ONNX Runtime execution provider (`auto`, `cpu`, or
//! `cuda`). `auto` picks CUDA when the crate is built with `--features cuda`
//! and the CUDA EP is usable; otherwise it falls back to CPU.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use ort::session::Session;
use ort::value::Tensor;
use serde::Deserialize;
use tokenizers::Tokenizer;

use super::Backend;

static WARNED_VISIBLE_CUDA_GPU_CPU_PATH: AtomicBool = AtomicBool::new(false);
const NVIDIA_SMI_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Deserialize)]
struct ModelMeta {
    max_seq: usize,
    dim: usize,
}

pub struct Onnx {
    session: Session,
    tokenizer: Tokenizer,
    meta: ModelMeta,
    /// Padded-token budget per batch (batch_len * max_len_in_batch).
    max_batch_tokens: usize,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Provider {
    Auto,
    Cpu,
    Cuda,
}

impl Provider {
    fn parse(s: &str) -> Result<Self> {
        match s {
            "auto" => Ok(Provider::Auto),
            "cpu" => Ok(Provider::Cpu),
            "cuda" => Ok(Provider::Cuda),
            other => bail!("unknown ONNX provider '{other}' (expected auto, cpu, or cuda)"),
        }
    }
}

impl Onnx {
    pub fn load(model_dir: &Path, provider: &str) -> Result<Onnx> {
        let provider = Provider::parse(provider)?;
        let meta: ModelMeta = serde_json::from_str(
            &std::fs::read_to_string(model_dir.join("semdup-model.json"))
                .with_context(|| format!("reading {}/semdup-model.json", model_dir.display()))?,
        )?;
        let mut builder = Session::builder()?;
        #[cfg(feature = "cuda")]
        {
            use ort::execution_providers::{CUDAExecutionProvider, ExecutionProvider};
            match provider {
                Provider::Cpu => {
                    eprintln!("onnx backend: CPU");
                    warn_if_visible_cuda_gpu_on_cpu_path(
                        CpuEmbeddingWarning::ExplicitCpuWithCudaBuild,
                    );
                }
                Provider::Auto | Provider::Cuda => {
                    let cuda = CUDAExecutionProvider::default();
                    if cuda.is_available()? {
                        // A `cargo install`ed binary lacks the provider libraries
                        // onnxruntime dlopens from the executable's directory; link
                        // them in from ort's download cache before they're needed.
                        match super::provider_libs::ensure_next_to_exe() {
                            Ok(Some(note)) => eprintln!("onnx backend: {note}"),
                            Ok(None) => {}
                            Err(e) => eprintln!("onnx backend: {e:#}"),
                        }
                        // error_on_failure: without it ort quietly falls back to CPU
                        // when the EP dylib can't load (e.g. cuDNN 9 missing), which
                        // looks identical to GPU mode but runs ~15x slower.
                        match builder.with_execution_providers([cuda.build().error_on_failure()]) {
                            Ok(b) => {
                                eprintln!("onnx backend: CUDA execution provider");
                                builder = b;
                            }
                            Err(e) if provider == Provider::Cuda => {
                                bail!("CUDA EP failed to load: {e}");
                            }
                            Err(e) => {
                                eprintln!(
                                    "onnx backend: CUDA EP failed to load, using CPU.\n  {e}\n  \
                                     hint: the CUDA EP needs cuDNN 9 on the library path; if you \
                                     have torch installed, try\n  export LD_LIBRARY_PATH=$(python3 \
                                     -c 'import nvidia.cudnn,os;print(os.path.dirname(\
                                     nvidia.cudnn.__file__)+\"/lib\")')"
                                );
                                warn_if_visible_cuda_gpu_on_cpu_path(
                                    CpuEmbeddingWarning::AutoFallbackCudaEpFailed,
                                );
                                builder = Session::builder()?;
                            }
                        }
                    } else if provider == Provider::Cuda {
                        bail!("CUDA execution provider is not available");
                    } else {
                        eprintln!("onnx backend: CUDA not available, using CPU");
                        warn_if_visible_cuda_gpu_on_cpu_path(
                            CpuEmbeddingWarning::AutoFallbackCudaUnavailable,
                        );
                    }
                }
            }
        }
        #[cfg(not(feature = "cuda"))]
        match provider {
            Provider::Cuda => {
                bail!("this build has no CUDA provider (rebuild with --features cuda)")
            }
            Provider::Auto => {
                eprintln!("onnx backend: CPU (build with --features cuda for GPU)");
                warn_if_visible_cuda_gpu_on_cpu_path(CpuEmbeddingWarning::AutoWithoutCudaBuild);
            }
            Provider::Cpu => {
                eprintln!("onnx backend: CPU (build with --features cuda for GPU)");
                warn_if_visible_cuda_gpu_on_cpu_path(
                    CpuEmbeddingWarning::ExplicitCpuWithoutCudaBuild,
                );
            }
        }
        let session = builder
            .commit_from_file(model_dir.join("model.onnx"))
            .with_context(|| format!("loading {}/model.onnx", model_dir.display()))?;
        let mut tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("loading tokenizer.json: {e}"))?;
        // Truncate in the tokenizer, not by slicing ids afterwards: the
        // tokenizer truncates before appending the trailing special token
        // ([SEP]), matching the reference implementation on long inputs.
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: meta.max_seq,
                ..Default::default()
            }))
            .map_err(|e| anyhow::anyhow!("setting truncation: {e}"))?;
        Ok(Onnx {
            session,
            tokenizer,
            meta,
            max_batch_tokens: 16384,
        })
    }

    fn run_batch(
        &mut self,
        ids: Vec<i64>,
        mask: Vec<i64>,
        batch: usize,
        len: usize,
    ) -> Result<Vec<Vec<f32>>> {
        // Input names vary by exporter (the dynamo exporter mangles them), so
        // bind positionally: (input_ids, attention_mask) is the export order.
        ensure!(
            self.session.inputs().len() == 2,
            "expected 2 model inputs, got {}",
            self.session.inputs().len()
        );
        let names: Vec<String> = self
            .session
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();
        let ids = Tensor::from_array(([batch, len], ids))?;
        let mask_t = Tensor::from_array(([batch, len], mask))?;
        let outputs = self
            .session
            .run(ort::inputs![names[0].as_str() => ids, names[1].as_str() => mask_t])?;
        // The graph pools and normalizes on-device; output is [batch, dim].
        let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
        ensure!(
            shape.len() == 2 && shape[1] as usize == self.meta.dim,
            "unexpected output shape {shape:?} (re-export the model: this \
             semdup expects pooled [batch, dim] graphs)"
        );
        Ok(data
            .chunks_exact(self.meta.dim)
            .map(<[f32]>::to_vec)
            .collect())
    }
}

#[derive(Clone, Copy)]
enum CpuEmbeddingWarning {
    #[cfg(feature = "cuda")]
    ExplicitCpuWithCudaBuild,
    #[cfg(feature = "cuda")]
    AutoFallbackCudaEpFailed,
    #[cfg(feature = "cuda")]
    AutoFallbackCudaUnavailable,
    #[cfg(not(feature = "cuda"))]
    AutoWithoutCudaBuild,
    #[cfg(not(feature = "cuda"))]
    ExplicitCpuWithoutCudaBuild,
}

impl CpuEmbeddingWarning {
    fn message(self) -> &'static str {
        match self {
            #[cfg(feature = "cuda")]
            CpuEmbeddingWarning::ExplicitCpuWithCudaBuild => {
                "warning: CUDA GPU appears to be visible, but `--provider cpu` selected CPU \
                 embeddings. Use `--provider auto` or `--provider cuda` to run embeddings on \
                 the GPU."
            }
            #[cfg(feature = "cuda")]
            CpuEmbeddingWarning::AutoFallbackCudaEpFailed => {
                "warning: CUDA GPU appears to be visible, but the CUDA execution provider failed \
                 to load, so semdup is using CPU embeddings. Fix the CUDA/cuDNN library path or \
                 pass `--provider cuda` to make this a hard error."
            }
            #[cfg(feature = "cuda")]
            CpuEmbeddingWarning::AutoFallbackCudaUnavailable => {
                "warning: CUDA GPU appears to be visible, but ONNX Runtime reports the CUDA \
                 execution provider is unavailable, so semdup is using CPU embeddings. Check \
                 the NVIDIA driver and CUDA provider setup."
            }
            #[cfg(not(feature = "cuda"))]
            CpuEmbeddingWarning::AutoWithoutCudaBuild => {
                "warning: CUDA GPU appears to be visible, but this semdup binary was built \
                 without CUDA support and is using CPU embeddings. Install or build semdup \
                 with `--features cuda` to run embeddings on the GPU."
            }
            #[cfg(not(feature = "cuda"))]
            CpuEmbeddingWarning::ExplicitCpuWithoutCudaBuild => {
                "warning: CUDA GPU appears to be visible, but this semdup binary was built \
                 without CUDA support and `--provider cpu` selected CPU embeddings. Install or \
                 build semdup with `--features cuda`, then use `--provider auto` or \
                 `--provider cuda` to run embeddings on the GPU."
            }
        }
    }
}

fn warn_if_visible_cuda_gpu_on_cpu_path(reason: CpuEmbeddingWarning) {
    if WARNED_VISIBLE_CUDA_GPU_CPU_PATH.load(Ordering::Relaxed) {
        return;
    }
    if visible_cuda_gpu() && !WARNED_VISIBLE_CUDA_GPU_CPU_PATH.swap(true, Ordering::Relaxed) {
        eprintln!("{}", reason.message());
    }
}

fn visible_cuda_gpu() -> bool {
    if !cuda_visible_devices_allows_gpu(std::env::var("CUDA_VISIBLE_DEVICES").ok().as_deref())
        || !cuda_visible_devices_allows_gpu(std::env::var("NVIDIA_VISIBLE_DEVICES").ok().as_deref())
    {
        return false;
    }

    if let Some(stdout) = nvidia_smi_stdout_with_timeout(NVIDIA_SMI_TIMEOUT)
        && nvidia_smi_lists_gpu(&stdout)
    {
        return true;
    }

    linux_nvidia_device_visible()
}

fn cuda_visible_devices_allows_gpu(value: Option<&str>) -> bool {
    let Some(value) = value.map(str::trim) else {
        return true;
    };
    if value.is_empty() || value == "-1" {
        return false;
    }
    !matches!(
        value.to_ascii_lowercase().as_str(),
        "none" | "void" | "nodevfiles"
    )
}

fn nvidia_smi_lists_gpu(stdout: &[u8]) -> bool {
    String::from_utf8_lossy(stdout)
        .lines()
        .any(|line| line.trim_start().starts_with("GPU "))
}

fn nvidia_smi_stdout_with_timeout(timeout: Duration) -> Option<Vec<u8>> {
    let mut child = Command::new("nvidia-smi")
        .arg("-L")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().ok()? {
            if !status.success() {
                return None;
            }
            let mut stdout = Vec::new();
            child.stdout.take()?.read_to_end(&mut stdout).ok()?;
            return Some(stdout);
        }
        if started.elapsed() >= timeout {
            child.kill().ok();
            child.wait().ok();
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn linux_nvidia_device_visible() -> bool {
    std::fs::read_dir("/dev")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .any(|name| {
            name.strip_prefix("nvidia").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
            })
        })
        || std::fs::read_dir("/proc/driver/nvidia/gpus")
            .ok()
            .is_some_and(|mut entries| entries.next().is_some())
}

impl Backend for Onnx {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Tokenize (truncated, unpadded) to learn lengths.
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("tokenize: {e}"))?;
        // The tokenizer's truncation config caps these at max_seq.
        let lens: Vec<usize> = encodings.iter().map(|e| e.get_ids().len()).collect();

        // Length-sorted greedy batches under a padded-token budget: bounds
        // both padding waste and peak (attention) memory.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&i| lens[i]);
        let mut out = vec![Vec::new(); texts.len()];
        let mut batch: Vec<usize> = Vec::new();
        let flush =
            |this: &mut Self, batch: &mut Vec<usize>, out: &mut Vec<Vec<f32>>| -> Result<()> {
                if batch.is_empty() {
                    return Ok(());
                }
                let max_len = batch.iter().map(|&i| lens[i]).max().unwrap_or(1).max(1);
                let mut ids = Vec::with_capacity(batch.len() * max_len);
                let mut mask = Vec::with_capacity(batch.len() * max_len);
                for &i in batch.iter() {
                    let e = &encodings[i];
                    let n = lens[i];
                    ids.extend(e.get_ids()[..n].iter().map(|&x| x as i64));
                    mask.extend(e.get_attention_mask()[..n].iter().map(|&x| x as i64));
                    ids.extend(std::iter::repeat_n(0i64, max_len - n));
                    mask.extend(std::iter::repeat_n(0i64, max_len - n));
                }
                let vecs = this.run_batch(ids, mask, batch.len(), max_len)?;
                for (&i, v) in batch.iter().zip(vecs) {
                    out[i] = v;
                }
                batch.clear();
                Ok(())
            };
        for &i in &order {
            let candidate_max = lens[i].max(batch.iter().map(|&j| lens[j]).max().unwrap_or(0));
            if !batch.is_empty()
                && ((batch.len() + 1) * candidate_max > self.max_batch_tokens || batch.len() >= 64)
            {
                flush(self, &mut batch, &mut out)?;
            }
            batch.push(i);
        }
        flush(self, &mut batch, &mut out)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuda_visible_devices_hiding_values_disable_gpu_warning() {
        for value in [
            Some(""),
            Some("-1"),
            Some("none"),
            Some("void"),
            Some("NoDevFiles"),
        ] {
            assert!(!cuda_visible_devices_allows_gpu(value));
        }
    }

    #[test]
    fn cuda_visible_devices_visible_values_allow_gpu_warning() {
        for value in [
            None,
            Some("0"),
            Some("0,1"),
            Some("GPU-deadbeef"),
            Some("all"),
        ] {
            assert!(cuda_visible_devices_allows_gpu(value));
        }
    }

    #[test]
    fn nvidia_smi_output_detection_requires_listed_gpu() {
        assert!(nvidia_smi_lists_gpu(
            b"GPU 0: NVIDIA RTX 4090 (UUID: GPU-deadbeef)\n"
        ));
        assert!(!nvidia_smi_lists_gpu(b"No devices were found\n"));
        assert!(!nvidia_smi_lists_gpu(b""));
    }
}
