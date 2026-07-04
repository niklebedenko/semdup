# semdup

Embedding-based near-duplicate function detection for source code. semdup
finds the clones token tools can't: renamed, restructured, and re-derived
implementations of the same logic, across files, modules, and languages.
Perfect for catching AI slop and preventing agents from constantly reimplementing
stuff.

- **Local-first.** Embeddings run on your machine (CPU or CUDA). Free and private.
- **Configurable sensitivity.** Adjust the sensitivity of matching (1.0 = only
  near-byte-identical matches; typically 0.5-0.95 is the useful range). You can have separate thresholds
  for emitting warnings vs errors.
- **Configurable arity.** Often, a shared helper only makes sense when a piece of code is
  duplicated 3+ times. `semdup` allows you to only search for duplicates that happen N times.
- **Language-agnostic.** Currently supports Rust, TypeScript, Python, Go, Java, C#, PHP, Ruby, 
  and C/C++; further extension is easy (contributions welcome!) *N.B. Heavily-macro'd C/C++
  code might not perform as well as the other languages.*

## Why semdup? Why embeddings?

Copy-paste detectors (jscpd, PMD CPD, Simian) match token sequences, so they're brittle to slight refactorings that still preserve the functions' semantic purpose.
semdup embeds each function with a code-retrieval model and compares
meanings. In our planted-clone benchmark, fully *re-derived* implementations
(same spec, written fresh) still surface in the top-5 neighbors 92% of the
time.

Similar tools like `slopo` require an external API. `semdup` keeps everything local.

The limitation of this approach is that it is approximate, by design. Sometimes,
things will land in the grey area, so that requires some judgement. Use the configurations
to help reduce false-positives.

## Install

```bash
cargo install semdup                 # CPU inference
cargo install semdup --features cuda # CUDA execution provider (falls back to CPU)
```

That's it. The embedding model (an MIT-licensed ONNX export of
nomic-ai/CodeRankEmbed) is downloaded automatically on first run —
checksum-verified and cached in `~/.cache/semdup`. With CUDA, a smaller fp16
variant is picked automatically. To bring your own model, see
[docs/models.md](docs/models.md).

The CUDA path needs cuDNN 9 on the library path at runtime (semdup prints a
loud warning and falls back to CPU otherwise). If you have a CUDA build of
torch installed, its bundled copy works:

```bash
export LD_LIBRARY_PATH=$(python3 -c 'import nvidia.cudnn,os;print(os.path.dirname(nvidia.cudnn.__file__)+"/lib")')
```

A ~6k-function corpus embeds cold in ~30 s on a midrange GPU (minutes on
CPU); after that only changed functions re-embed.

`cargo install` ships only the executable, so on first GPU use semdup links
onnxruntime's CUDA provider libraries in from ort's build-time download cache
(`~/.cache/ort.pyke.io`). That's automatic on the machine that ran the
install; only a binary copied to a different machine needs the
`libonnxruntime_providers_*.so` files placed next to it by hand.

## Quickstart

```bash
cd your-repo
semdup init   # detects your source roots, writes semdup.toml, builds the index
semdup scan   # report near-duplicate clusters
```

`scan` re-indexes changed functions automatically (content-hashed, so it's
incremental; `--no-refresh` to skip). Everything else is tuning:

```bash
# Look at the obvious stuff first: near-exact clones
semdup scan --threshold 0.95

# Rule-of-three mode: only clusters where the logic already exists 3+ times
# (the strongest candidates for extracting a shared helper)
semdup scan --threshold 0.85 --min-cluster 3

# Dial in your repo's threshold: sweep a few values, keep the one whose
# report you'd act on (expect somewhere in 0.55-0.75 for code-retrieval models)
for t in 0.55 0.60 0.65 0.70 0.75; do
  semdup scan --threshold $t | tail -1
done

# Review merge requests with neighbor evidence + your chosen threshold
semdup diff --base origin/main --check
```

Persistent settings live in `semdup.toml` at the repo root (discovered by
walking up from the working directory); CLI flags override it. `init` writes
the essentials; everything it accepts, annotated:

```toml
db = "semdup.sqlite"

[extract]
roots = ["src", "lib"]

# [embed] is only needed to swap models or backends (see docs/models.md);
# the default model is fetched automatically.

[scan]
threshold = 0.625   # yours will differ: sweep a few values on your own repo
min_lines = 8
skip_tests = true
# min_cluster = 3   # rule of three: only report 3+-member clusters
```

## Suppressing a finding

Similarity is evidence, not a verdict. When two functions are similar *by
design* (a read/write mirror, per-variant implementations), say so in the
code — within three lines above the signature (or on it):

```rust
// semdup:ignore — mirror of cache_write; symmetric by design
fn cache_read(...) { ... }
```

Adopting on a codebase with existing duplication? Start with a high
threshold, fix what it reports, and lower it in steps — each notch surfaces
the next tier of candidates without burying you in day-one findings.

## How it works

```
extract   tree-sitter → function units → SQLite (content-hash keyed)
embed     ONNX Runtime (CUDA→CPU) or python sidecar → cached vectors
scan      exact pairwise cosine (rayon) → union-find clusters → report
diff      git diff → touched units → top-3 neighbors + threshold verdicts
```

There is no approximate index: on an 8k-function corpus a full scan is one
parallel matmul, ~0.3 s. Embedding the whole corpus cold is minutes on CPU
and tens of seconds on a GPU; after that only changed functions re-embed.

Doc comments are stripped before embedding (shared doc boilerplate inflates
similarity). Test functions are tagged at extraction and excluded with
`--skip-tests`.

To go further, `semdup extract --strip-comments` (or `strip_comments = true`
under `[extract]`) removes *all* comments and Python docstrings from unit
text before hashing and embedding, so similarity reflects code alone rather
than shared prose. Stripped and unstripped text hash differently, so the two
variants coexist in the cache without cross-contamination; `semdup diff`
follows the config setting so MR units are compared in the same form as the
corpus.

### Embedding backends

- **onnx** (default): uses the auto-downloaded default model, or any
  directory produced by `scripts/export_onnx.py` via `model_dir`. Build with
  `--features cuda` for GPU inference.
- **sidecar**: spawns a python process (`sidecar/embedder.py`) speaking a
  JSONL protocol, for models without an ONNX export (`trust_remote_code`
  architectures, brand-new releases). Trial candidates with the sidecar;
  promote the winner to ONNX.

Vectors are cached by `(model id, content hash)`; swapping models means a
cold cache and a fresh threshold sweep — by design, since every threshold is
model-specific.

### Choosing a model

Run the bake-off yourself; it's one `inject-eval` per candidate. On our
benchmarks, **nomic-ai/CodeRankEmbed** (137M params)
beat embedding models 4× its size on class separation — code-contrastive
training matters more than scale. Expect newer models to win eventually;
that's what the harness is for.

## Evaluating on your own repo

`eval/` contains the methodology and a ready-made benchmark against public
corpora (one pinned repo per language — see the provenance table in
`eval/README.md`):

```bash
eval/fetch-corpus.sh
semdup extract --root eval/corpus --corpus main
semdup extract --root eval/injected --corpus injected
semdup embed
semdup inject-eval --manifest eval/manifest.json --min-recall5 0.9
```

This measures recall@1/@5 of planted rewrites at three mutation levels
(rename-only / restructured / re-derived) — the numbers that tell you whether
a model actually works before you trust its scan output. See `eval/README.md`
for the methodology and for extending the benchmark. CI runs this end-to-end
weekly and on any PR that touches the pipeline itself, seeded with
pre-computed embeddings so runners only embed what changed, gating on
recall@5 ≥ 0.9. The PR path proper just dogfoods semdup on its own source.

## Status

Early. The pipeline and evaluation harness are real and tested on a ~500k-line
proprietary Rust/TS codebase (where the first scan found, among other things,
a 41-line exact clone and a 20-member boilerplate cluster). API and report
formats may still move. Solo-maintained; issues triaged weekly, PRs welcome,
no SLA.

## License

MIT OR Apache-2.0. Eval assets under `eval/injected/` are derived from
ripgrep (MIT OR Unlicense), vuejs/core (MIT), pallets/flask (BSD-3-Clause),
junegunn/fzf (MIT), google/gson (Apache-2.0), Newtonsoft.Json (MIT),
guzzle (MIT), sinatra (MIT), jq (MIT), and fmt (MIT); each file carries its
attribution.
