# semdup

Fuzzy-search tool for detecting code duplication. Perfect for catching AI slop and preventing agents from constantly reimplementing
stuff.

- **Robust.** Detects similarity by semantics, instead of grepping for token
  sequences. See `eval/README.md` for benchmark methodology and results.
- **Local-first.** Embeddings run on your machine (CPU or CUDA). Free and private.
- **Configurable sensitivity.** Adjust the sensitivity of matching (1.0 = only
  near-byte-identical matches; typically 0.5-0.95 is the useful range). You can have separate thresholds
  for emitting warnings vs errors.
- **Configurable arity.** Often, a shared helper only makes sense when a piece of code is
  duplicated 3+ times. `semdup` allows you to only search for duplicates that happen N times.
- **Language-agnostic.** Currently supports Rust, TypeScript, Python, Go, Java, C#, PHP, Ruby, 
  and C/C++; further extension is easy (contributions welcome!) *N.B. Heavily-macro'd C/C++
  code might not perform as well as the other languages.*

I published this tool because I found it helpful for my own work. It's still young, so expect bugs. Contributions welcome!

## Getting started

Install (choose one option):

```bash
cargo install semdup                 # CPU inference
cargo install semdup --features cuda # CUDA execution provider (falls back to CPU)
```

Run:

```bash
cd your-repo
semdup init   # detects your source roots, writes semdup.toml, builds the index
semdup scan   # report near-duplicate clusters
semdup scan -t 0.95 -m 3   # threshold + 3-member minimum clusters
semdup scan --show-bodies --top 1   # include source snippets for displayed clusters
semdup diff --base origin/main --check # built-in PR review mode
```

`semdup scan --show-bodies` syntax-highlights snippets when stdout is a
terminal. Use `--color always` or `--color never` to override that.

## GitHub Actions

The lowest-friction setup is the reusable workflow:

```yaml
name: Semdup

on:
  pull_request:
  push:
    branches: [main]

jobs:
  semdup:
    uses: niklebedenko/semdup/.github/workflows/semdup.yml@v1
```

For custom workflows, use the action as a step:

```yaml
- uses: actions/checkout@v4
- uses: niklebedenko/semdup@v1
```

The action restores the model cache, restores the SQLite embedding DB
(`semdup.sqlite*`), fetches the PR base ref, refreshes the base corpus in a
temporary worktree, runs `semdup ci`, emits GitHub annotations, and saves
updated caches only from trusted default-branch pushes. With no `semdup.toml`,
CI auto-detects source roots and uses conservative defaults: function-only
indexing, `min_lines = 8`, `skip_tests = true`, and threshold `0.85`.

Tune only what you need:

```yaml
jobs:
  semdup:
    uses: niklebedenko/semdup/.github/workflows/semdup.yml@v1
    with:
      threshold: "0.88"
      roots: src,lib
```

The `init` step can take some time (about 30s on my midrange GPU on a 500k-line repo).
After the first initialisation, everything else is incremental and super fast.

This tool uses fuzzy-search, so expect some false positives. Play around with the settings in the
`semdup.toml` or use extra flags from `semdup scan --help` to fine-tune.

By default, embeddings include functions and executable blocks inside
functions. Use `granularity = ["function"]` for function-only indexing.

## Why `semdup`?

Copy-paste detectors (jscpd, PMD CPD, Simian, `dupehound`) match token sequences, so they're
brittle to slight refactorings that still preserve the functions' semantic purpose.
`semdup` embeds each function with a code-retrieval model and compares
meanings. The planted-clone benchmark checks this against rewrites in pinned
public corpora; see `eval/README.md` for the current numbers.

Similar tools like `slopo` require an external API. `semdup` keeps everything local.

## Settings

Persistent settings live in `semdup.toml` at the repo root (discovered by
walking up from the working directory); CLI flags override it. `init` writes
the essentials; everything it accepts, annotated:

```toml
db = "semdup.sqlite"

[extract]
roots = ["src", "lib"]
respect_gitignore = true # default: skip files ignored by git
# granularity = ["function"] # optional: function-only indexing
# min_block_lines = 8                 # extraction-time block noise/cost guard

# [embed] is only needed to swap models or backends (see docs/models.md);
# the default model is fetched automatically.
# provider = "auto"  # auto | cpu | cuda

[scan]
threshold = 0.625   # yours will differ: sweep a few values on your own repo
min_lines = 8       # effective body lines; signatures, blanks, and brace-only lines do not count
skip_tests = true
index = "exact"     # exact | sparse | auto
# unit_kind = "block" # function | block; omit to scan all extracted units
# min_cluster = 3   # rule of three: only report 3+-member clusters
```

## Suppressing a finding

```rust
// semdup:ignore — mirror of cache_write; symmetric by design
fn cache_read(...) { ... }
```

Adopting on a codebase with existing duplication? Start with a high
threshold, fix what it reports, and lower it in steps — each notch surfaces
the next tier of candidates without burying you in day-one findings.

## How it works

```
extract   tree-sitter → function/block units → SQLite (content-hash keyed)
embed     ONNX Runtime (CUDA→CPU) or python sidecar → cached vectors
scan      candidate search → exact cosine rerank → union-find clusters → report
diff      git diff → touched units → top-3 neighbors + threshold verdicts
```

The default scan index is `exact`: every pair is compared with dense cosine
using blocked CPU matrix multiplication, so it does not drop matches. For very
large repos, `semdup scan --index sparse` builds a sparse-random-projection LSH
candidate index and then reranks those candidates with the same dense cosine
score. That mode is approximate, so it can miss pairs that exact search would
find, but it avoids the quadratic all-pairs pass. `--index auto` keeps exact
search below 20k scannable units and switches to sparse search above that.

For development experiments, `scripts/bench_scan_gpu.py` benchmarks an exact
CUDA matrix-multiply scan against an existing `semdup.sqlite` cache. It is not
part of the runtime dependency set.

Embedding the whole corpus cold is minutes on CPU and tens of seconds on a GPU;
after that only changed units re-embed.

Doc comments are stripped before embedding (shared doc boilerplate inflates
similarity). Test functions are tagged at extraction and excluded with
`--skip-tests`.

When block granularity is enabled, scan ignores larger block units that overlap
smaller block units from the same file. This keeps nested blocks from counting
as extra duplicate members of themselves. Non-overlapping duplicates in the
same file are still reported.

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

The default embedding model is **nomic-ai/CodeRankEmbed** via the hosted
`nomic-ai/CodeRankEmbed@fast` ONNX export. See `docs/models.md` for cache
behavior and bring-your-own-model setup. Use `eval/README.md` to benchmark
candidate models before trusting their scan thresholds.

## Benchmarks

`eval/README.md` contains the planted-clone methodology, run commands, current
default-model results, F1 convention, corpus provenance, and extension notes.

## License

MIT or Apache-2.0, at your option.

Eval assets under `eval/injected/` are derived from third-party projects; see
`eval/README.md` for the full provenance and license table. Each file carries
its attribution.
