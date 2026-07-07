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
- Two hosted variants, both from release tag `model-coderankembed-1`:

| variant | size | picked when |
|---|---|---|
| fp32 | ~548 MB | CPU (default) |
| fp16 | ~274 MB | built with `--features cuda` and the CUDA EP is usable |

fp16 halves the download and is faster on GPU, but is *slower* than fp32 on
the CPU execution provider, so CPU always gets fp32.

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
| CPU | `fast-cpu` | `cpu` | dynamic-int8 ONNX |
| NVIDIA GPU | `fast-gpu` | `cuda` | fp16 ONNX |

CPU quantization experiments use explicit model keys so cached embeddings do
not collide with the default fp32/fp16 keys:

```sh
python3 scripts/quantize_onnx.py \
  --mode int8-dynamic \
  --input models/coderankembed-fp32 \
  --out models/coderankembed-int8-dynamic

semdup embed \
  --model nomic-ai/CodeRankEmbed@int8-dynamic \
  --provider cpu \
  --model-dir models/coderankembed-int8-dynamic
```

Use `eval/model-row.sh --model fast-cpu` or
`eval/model-row.sh --model fast-gpu` to publish eval rows for the fast-path
variants. FP8 is not a default CPU path here; ONNX Runtime's CPU quantization
tooling is int8-oriented.

## Updating the hosted export

1. Re-export both variants with `scripts/export_onnx.py` (`--fp16` for the
   second) and compute blake3 hashes of `model.onnx` / `tokenizer.json`.
2. Create a new release tag (`model-coderankembed-2`, ...) with the three
   assets: `coderankembed-fp32.onnx`, `coderankembed-fp16.onnx`,
   `coderankembed-tokenizer.json`.
3. Update `RELEASE_BASE`, the pins, and the sizes in `src/fetch.rs`; bump the
   `hosted-model-*` cache keys in `.github/workflows/*.yml`.

Old tags stay up so released semdup binaries keep working: a binary's pins
always reference the tag it was built against.
