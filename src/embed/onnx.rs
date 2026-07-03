//! Built-in ONNX Runtime backend.
//!
//! Loads a model directory produced by `scripts/export_onnx.py`:
//!   model.onnx         — encoder taking (input_ids, attention_mask) -> last_hidden_state
//!   tokenizer.json     — HF fast tokenizer
//!   semdup-model.json  — { "max_seq": .., "dim": .. }
//!
//! Uses the CUDA execution provider when the crate is built with `--features
//! cuda` and a GPU is present; otherwise falls back to CPU. Pooling is masked
//! mean + L2 normalize, matching the sentence-transformers reference.

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
    /// "cls" or "mean", from the model's sentence-transformers pooling config.
    #[serde(default = "default_pooling")]
    pooling: String,
}

fn default_pooling() -> String {
    "mean".into()
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
                eprintln!("onnx backend: CUDA execution provider");
                // The error variant carries the (non-Send) builder; format it
                // instead of `?`-converting to anyhow.
                builder = builder
                    .with_execution_providers([cuda.build()])
                    .map_err(|e| anyhow::anyhow!("registering CUDA EP: {e}"))?;
            } else {
                eprintln!("onnx backend: CUDA not available, using CPU");
            }
        }
        #[cfg(not(feature = "cuda"))]
        eprintln!("onnx backend: CPU (build with --features cuda for GPU)");
        let session = builder
            .commit_from_file(model_dir.join("model.onnx"))
            .with_context(|| format!("loading {}/model.onnx", model_dir.display()))?;
        let tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("loading tokenizer.json: {e}"))?;
        Ok(Onnx {
            session,
            tokenizer,
            meta,
            max_batch_tokens: 8192,
        })
    }

    fn run_batch(&mut self, ids: Vec<i64>, mask: Vec<i64>, batch: usize, len: usize) -> Result<Vec<Vec<f32>>> {
        // Input names vary by exporter (the dynamo exporter mangles them), so
        // bind positionally: (input_ids, attention_mask) is the export order.
        ensure!(
            self.session.inputs().len() == 2,
            "expected 2 model inputs, got {}",
            self.session.inputs().len()
        );
        let names: Vec<String> = self.session.inputs().iter().map(|i| i.name().to_string()).collect();
        let ids = Tensor::from_array(([batch, len], ids))?;
        let mask_t = Tensor::from_array(([batch, len], mask.clone()))?;
        let outputs = self
            .session
            .run(ort::inputs![names[0].as_str() => ids, names[1].as_str() => mask_t])?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
        ensure!(
            shape.len() == 3 && shape[2] as usize == self.meta.dim,
            "unexpected output shape {shape:?}"
        );
        let (seq, dim) = (shape[1] as usize, self.meta.dim);
        let mut out = Vec::with_capacity(batch);
        for b in 0..batch {
            let mut pooled;
            if self.meta.pooling == "cls" {
                pooled = data[b * seq * dim..b * seq * dim + dim].to_vec();
            } else {
                pooled = vec![0f32; dim];
                let mut n = 0f32;
                for t in 0..seq {
                    if mask[b * len + t] == 0 {
                        continue;
                    }
                    n += 1.0;
                    let row = &data[(b * seq + t) * dim..(b * seq + t + 1) * dim];
                    for (p, x) in pooled.iter_mut().zip(row) {
                        *p += x;
                    }
                }
                let norm_n = if n > 0.0 { n } else { 1.0 };
                for p in &mut pooled {
                    *p /= norm_n;
                }
            }
            let norm = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for p in &mut pooled {
                    *p /= norm;
                }
            }
            out.push(pooled);
        }
        Ok(out)
    }
}

impl Backend for Onnx {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Tokenize (truncated, unpadded) to learn lengths.
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("tokenize: {e}"))?;
        let lens: Vec<usize> = encodings
            .iter()
            .map(|e| e.get_ids().len().min(self.meta.max_seq))
            .collect();

        // Length-sorted greedy batches under a padded-token budget: bounds
        // both padding waste and peak (attention) memory.
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&i| lens[i]);
        let mut out = vec![Vec::new(); texts.len()];
        let mut batch: Vec<usize> = Vec::new();
        let flush = |this: &mut Self, batch: &mut Vec<usize>, out: &mut Vec<Vec<f32>>| -> Result<()> {
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
                && ((batch.len() + 1) * candidate_max > self.max_batch_tokens
                    || batch.len() >= 32)
            {
                flush(self, &mut batch, &mut out)?;
            }
            batch.push(i);
        }
        flush(self, &mut batch, &mut out)?;
        Ok(out)
    }
}
