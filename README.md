# semdup

Embedding-based near-duplicate function detection for source code. semdup
finds the clones token tools can't: renamed, restructured, and re-derived
implementations of the same logic — across files, modules, and languages.

- **Local-first.** Embeddings run on your machine (built-in ONNX Runtime
  backend, CPU or CUDA). Your code never leaves the box.
- **Thresholds are yours, per repo and per model.** Dial one in by running
  `scan` at a few values against your own code and seeing where the reports
  stop being useful — never copy one from a README (including this one).
- **Evidence over verdicts.** For merge requests, `semdup diff` shows each
  touched function's nearest corpus neighbors and applies only your chosen
  threshold (a hard `DUP` fails `--check`; a `REVIEW` band just below it is
  advisory). No hidden cleverness — see caveat 1.
- Rust, TypeScript, Python, Go, Java, C#, PHP, Ruby, and C/C++ today; one
  tree-sitter grammar + ~30 lines per additional language. (C/C++ caveat:
  sources are parsed unpreprocessed, so function-like macros aren't
  extracted as units and code hidden behind `#if` branches is seen as
  written — heavy macro metaprogramming will slip through.)

## Why embeddings?

Copy-paste detectors (jscpd, PMD CPD, Simian) match token sequences: rename
one identifier and half the match dissolves; restructure a loop and it's
gone. semdup embeds each function with a code-retrieval model and compares
meanings. In our planted-clone benchmark, fully *re-derived* implementations
(same spec, written fresh) still surface in the top-5 neighbors 92% of the
time.

Two honest caveats, both baked into the design:

1. **Identifier vocabulary dominates embeddings.** A rename-only clone can
   score *below* two unrelated functions that share jargon, and in a real
   repo the median function's nearest neighbor looks as "anomalously close"
   as a planted rewrite's original — we measured this, and no
   neighbor-closeness statistic we tried (top-1 margin, mutual nearest
   neighbor, z-score against the similarity background) separates them.
   That's why thresholds are a per-repo preference you sweep for yourself,
   and why `semdup diff` presents ranked evidence instead of pretending to
   a judgment it can't make.
2. **Detection ≠ judgment.** In real codebases the mid-similarity band is
   full of *intentional* parallelism (read/write mirrors, per-variant
   implementations, cpu/gpu backends). semdup gives you the evidence;
   `semdup:ignore` comments and baselines record your verdicts.

## Install

```bash
cargo install semdup                 # CPU inference
cargo install semdup --features cuda # CUDA execution provider (falls back to CPU)
```

The CUDA path needs cuDNN 9 on the library path at runtime (semdup prints a
loud warning and falls back to CPU otherwise). If you have a CUDA build of
torch installed, its bundled copy works:

```bash
export LD_LIBRARY_PATH=$(python3 -c 'import nvidia.cudnn,os;print(os.path.dirname(nvidia.cudnn.__file__)+"/lib")')
```

For GPU use, export the model with `--fp16` (about 2× embed throughput; the
verification gate still applies). A ~6k-function corpus embeds cold in ~30 s
on a midrange GPU; incremental runs only touch changed functions.

Get an embedding model directory (one-time; needs python with torch +
sentence-transformers + onnx + onnxruntime):

```bash
python3 scripts/export_onnx.py --model nomic-ai/CodeRankEmbed --out models/coderankembed
```

The export self-verifies against the reference implementation at several
sequence lengths and refuses to produce a directory that disagrees with it.

## Quickstart

```bash
# 1. Index your repo (fast; incremental by content hash)
semdup extract --root src --root lib
semdup embed --model-dir models/coderankembed --model nomic-ai/CodeRankEmbed

# 2. Look at the obvious stuff first: near-exact clones
semdup scan --model nomic-ai/CodeRankEmbed --threshold 0.95 --skip-tests

# Rule-of-three mode: only clusters where the logic already exists 3+ times
# (the strongest candidates for extracting a shared helper)
semdup scan --model nomic-ai/CodeRankEmbed --threshold 0.85 --skip-tests --min-cluster 3

# 3. Dial in a threshold: sweep a few values, keep the one whose report
#    you'd act on (expect somewhere in 0.55-0.75 for code-retrieval models)
for t in 0.55 0.60 0.65 0.70 0.75; do
  semdup scan --model nomic-ai/CodeRankEmbed --threshold $t --skip-tests | tail -1
done

# 4. Adopt without triaging history: snapshot today's pairs, report only new ones
semdup scan --model nomic-ai/CodeRankEmbed --threshold 0.625 --write-baseline semdup-baseline.json
semdup scan --model nomic-ai/CodeRankEmbed --threshold 0.625 --baseline semdup-baseline.json

# 5. Review merge requests with neighbor evidence + your chosen threshold
semdup diff --base origin/main --model nomic-ai/CodeRankEmbed --model-dir models/coderankembed --threshold 0.625 --check
```

Persistent settings live in `semdup.toml` at the repo root (discovered by
walking up from the working directory); CLI flags override it:

```toml
db = "semdup.sqlite"

[extract]
roots = ["src", "lib"]

[embed]
model = "nomic-ai/CodeRankEmbed"
backend = "onnx"
model_dir = "models/coderankembed"

[scan]
threshold = 0.625   # yours will differ: sweep a few values on your own repo
min_lines = 8
skip_tests = true
# min_cluster = 3   # rule of three: only report 3+-member clusters
baseline = "semdup-baseline.json"
```

## Suppressing a finding

Similarity is evidence, not a verdict. When two functions are similar *by
design* (a read/write mirror, per-variant implementations), say so in the
code — within three lines above the signature (or on it):

```rust
// semdup:ignore — mirror of cache_write; symmetric by design
fn cache_read(...) { ... }
```

Baseline entries expire when either function's body changes; ignore comments
don't. Use ignores for permanent design decisions, baselines for "not today."

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

- **onnx** (default): loads a model directory produced by
  `scripts/export_onnx.py`. Build with `--features cuda` for GPU inference.
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
semdup embed --model-dir models/coderankembed --model nomic-ai/CodeRankEmbed
semdup inject-eval --model nomic-ai/CodeRankEmbed --manifest eval/manifest.json --min-recall5 0.9
```

This measures recall@1/@5 of planted rewrites at three mutation levels
(rename-only / restructured / re-derived) — the numbers that tell you whether
a model actually works before you trust its scan output. See `eval/README.md`
for the methodology and for extending the benchmark. CI runs this end-to-end
on every PR (CPU, cached model) and gates on recall@5 ≥ 0.9.

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
