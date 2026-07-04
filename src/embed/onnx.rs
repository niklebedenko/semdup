//! Built-in ONNX Runtime backend.
//!
//! Loads a model directory produced by `scripts/export_onnx.py`:
//!   model.onnx         — (input_ids, attention_mask) -> embedding [batch, dim];
//!                        pooling + L2 normalization are baked into the graph
//!   tokenizer.json     — HF fast tokenizer
//!   semdup-model.json  — { "max_seq": .., "dim": .. }
//!
//! Uses the CUDA execution provider when the crate is built with `--features
//! cuda` and a GPU is present; otherwise falls back to CPU.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use ort::session::Session;
use ort::value::Tensor;
use serde::Deserialize;
use tokenizers::Tokenizer;

use super::Backend;

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

impl Onnx {
    pub fn load(model_dir: &Path) -> Result<Onnx> {
        let meta: ModelMeta = serde_json::from_str(
            &std::fs::read_to_string(model_dir.join("semdup-model.json"))
                .with_context(|| format!("reading {}/semdup-model.json", model_dir.display()))?,
        )?;
        let mut builder = Session::builder()?;
        #[cfg(feature = "cuda")]
        {
            use ort::execution_providers::{CUDAExecutionProvider, ExecutionProvider};
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
                    Err(e) => {
                        eprintln!(
                            "onnx backend: CUDA EP failed to load, using CPU.\n  {e}\n  \
                             hint: the CUDA EP needs cuDNN 9 on the library path; if you \
                             have torch installed, try\n  export LD_LIBRARY_PATH=$(python3 \
                             -c 'import nvidia.cudnn,os;print(os.path.dirname(\
                             nvidia.cudnn.__file__)+\"/lib\")')"
                        );
                        builder = Session::builder()?;
                    }
                }
            } else {
                eprintln!("onnx backend: CUDA not available, using CPU");
            }
        }
        #[cfg(not(feature = "cuda"))]
        eprintln!("onnx backend: CPU (build with --features cuda for GPU)");
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
