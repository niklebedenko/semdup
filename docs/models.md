# Models

semdup embeds functions with [nomic-ai/CodeRankEmbed](https://huggingface.co/nomic-ai/CodeRankEmbed)
(137M params, MIT) by default. There is no official ONNX export, so this repo
hosts its own as GitHub release assets, produced by `scripts/export_onnx.py`
with CLS pooling and L2 normalization baked into the graph.

## Auto-download

The first `semdup init` / `refresh` / `scan` downloads the model once into
the user cache and verifies every file against blake3 pins compiled into the
binary (`src/fetch.rs`), so a corrupted or tampered download is rejected
before it is ever loaded.

- Cache location: `$SEMDUP_CACHE`, else `$XDG_CACHE_HOME/semdup`, else
  `~/.cache/semdup` (on Windows `%LOCALAPPDATA%\semdup`).
- Four hosted variants, all from release tag `model-coderankembed-1`:

| variant | size | picked when |
|---|---|---|
| nbits-int4-asym | ~149 MB | CPU default for omitted `--model` |
| fp16 | ~274 MB | built with `--features cuda` and the CUDA EP is usable for omitted `--model` |
| fp32 | ~548 MB | explicit legacy `--model nomic-ai/CodeRankEmbed` on CPU |
| int8-dynamic | ~200 MB | explicit `--model nomic-ai/CodeRankEmbed@cpu-int8-dynamic --provider cpu` |

The omitted-model hosted default uses cache key
`nomic-ai/CodeRankEmbed@fast`. That prevents the int4 CPU embeddings from
reusing older fp32 rows stored under the legacy `nomic-ai/CodeRankEmbed` key.
fp16 halves the download and is faster on GPU, but is *slower* than fp32 on
the CPU execution provider, so the CPU speed path uses ORT MatMulNBits int4
instead. The int8 artifact remains hosted for reproducible quantization
experiments; re-sweep quality and thresholds before using it as a gate.

Nothing is downloaded when an explicit `model_dir` is configured.

## Bring your own model

Any sentence-transformers-compatible embedding model works:

```sh
pip install torch sentence-transformers onnx onnxruntime einops
python3 scripts/export_onnx.py --model <hf-model-id> --out models/mymodel
```

Then point semdup at the export in `semdup.toml`:

```toml
[embed]
model = "<hf-model-id>"       # cache key for vectors
provider = "auto"             # auto | cpu | cuda
model_dir = "models/mymodel"  # model.onnx + tokenizer.json + semdup-model.json
```

Vectors are cached per `(model, function-text hash)`, so switching models
re-embeds; switching back reuses the old vectors. Thresholds are per model —
re-sweep after a change (`semdup scan --threshold <t>` at a few values).

For arbitrary backends (API-based embedders, models without an ONNX path),
use the python sidecar instead: `backend = "sidecar"` plus a script
implementing the JSONL protocol in `sidecar/embedder.py`.

## Quantized variants

The ONNX backend separates the execution provider from the artifact. Current
fast paths use different artifacts but the same backend:

| target | alias | provider | artifact |
| --- | --- | --- | --- |
| CPU | `fast-cpu` | `cpu` | MatMulNBits int4 asymmetric ONNX |
| NVIDIA GPU | `fast-gpu` | `cuda` | fp16 ONNX |

CPU quantization experiments use explicit model keys so cached embeddings do
not collide with other artifacts:

```sh
python3 scripts/quantize_onnx.py \
  --mode nbits-int4-asym \
  --input models/coderankembed-fp32 \
  --out models/coderankembed-nbits-int4-asym

semdup embed \
  --model nomic-ai/CodeRankEmbed@cpu-nbits-int4-asym \
  --provider cpu
```

`fast-cpu` downloads the hosted int4 artifact; the
`coderankembed-nbits-int4-asym-*` aliases in `eval/models.tsv` use the local
directory produced by the command above. Use `eval/model-row.sh --model
fast-cpu` or `eval/model-row.sh --model fast-gpu` to publish eval rows for
the fast-path variants. FP8 is not a default CPU path here; ONNX Runtime's CPU
quantization tooling for this model is int8 or weight-only int4 oriented.

## Updating the hosted export

1. Re-export both floating-point variants with `scripts/export_onnx.py`
   (`--fp16` for the second). Quantize the fp32 export with
   `scripts/quantize_onnx.py --mode nbits-int4-asym` and, if still needed for
   comparisons, `--mode int8-dynamic`.
   Compute blake3 hashes of every hosted file, including external data files.
2. Create a new release tag (`model-coderankembed-2`, ...) with the hosted
   assets: `coderankembed-fp32.onnx`, `coderankembed-fp16.onnx`,
   `coderankembed-nbits-int4-asym.onnx`,
   `coderankembed-nbits-int4-asym.onnx.data`,
   `coderankembed-int8-dynamic.onnx`, plus `coderankembed-tokenizer.json`.
3. Update `RELEASE_BASE`, the pins, and the sizes in `src/fetch.rs`; bump the
   `hosted-model-*` cache keys in `.github/workflows/*.yml`.

Old tags stay up so released semdup binaries keep working: a binary's pins
always reference the tag it was built against.
