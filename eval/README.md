# Evaluation harness

Two complementary measurements, both runnable against your own repo and
model. All numbers semdup reports (thresholds, recall) come from here —
nothing is hand-tuned.

## 1. Planted-clone benchmark (`semdup inject-eval`)

Measures whether the embedding model can *find* a rewrite of a function it
has seen. We take real functions from public corpora, rewrite each at three
mutation levels, and check where the original ranks among the plant's
nearest neighbors:

- **L1 rename-only** — identical structure, every identifier and comment
  reworded. Tests robustness to vocabulary (the known weak spot of code
  embeddings).
- **L2 restructured** — same algorithm, different control flow (loop ↔
  iterator chain, extracted helpers, reshaped error handling).
- **L3 re-derived** — written fresh from a one-paragraph spec of the
  original's behavior; only the *semantics* are shared.

Run it:

```bash
eval/fetch-corpus.sh                  # pinned public-corpus checkouts (see table below)
semdup extract --root eval/corpus --corpus main
semdup extract --root eval/injected --corpus injected
semdup embed --model <MODEL> --model-dir <DIR>
semdup inject-eval --model <MODEL> --manifest eval/manifest.json
```

Report: per-plant cosine/rank/margin, then recall@1 and recall@5 per level.
A usable model should hold recall@1 ≈ 1.0 at L1 and keep the original in the
top 5 for most L3 plants. **Margin** (cosine to original minus cosine to
best non-original) is the number to compare across models: it measures class
separation independent of any threshold.

The plants live in `eval/injected/` with provenance headers;
`eval/manifest.json` maps each plant file to its original
(`path-suffix::name`) and level. To extend: add a rewrite file + manifest
entry. Keep one primary function per file (the largest unit in the file is
taken as the plant), don't reuse the original's name, and pick originals
≥ 12 lines with real logic, not boilerplate.

## 2. Picking a threshold

Thresholds are per-repo *and* per-model: different codebases have different
vocabulary overlap, and different models place the same pairs differently.
There is no command for this because the honest procedure is judgment, not
optimization — sweep a few values on your own code and keep the one whose
report you would actually act on:

```bash
for t in 0.55 0.60 0.65 0.70 0.75; do
  semdup scan --model <MODEL> --threshold $t --skip-tests | tail -1
done
```

Then read the report at your candidate value: the boundary you're placing is
between real duplication and *intentional* parallelism (read/write mirrors,
per-variant implementations), and only you know which side a pair belongs
on. Re-sweep after switching models. If no value gives a report you trust,
don't gate CI on a threshold at all — use `semdup diff` without `--check`
and read its neighbor evidence by hand.

## Prose vs code: `--strip-comments`

Doc comments are always stripped; `extract --strip-comments` additionally
removes inline/block comments and Python docstrings, so similarity is
computed over code alone. On this benchmark the effect is mildly positive —
recall@1 rises at L1 (0.89 → 0.93) and L3 (0.75 → 0.82) and mean margins
improve a few points, while one L2 plant slips out of the top 5
(recall@5 0.96 → 0.93). Expect it to matter most on codebases with heavy
shared comment boilerplate (license headers inside functions, templated
TODO/summary blocks); on prose-light code it's close to a wash. Rerun the
comparison on your own corpus before deciding — it's two extract runs into
two `--db` paths.

## Corpus provenance

| corpus | pin | license |
|---|---|---|
| BurntSushi/ripgrep | 4649aa97 (14.1.1) | MIT OR Unlicense |
| vuejs/core | 6eb29d34 (v3.5.13) | MIT |
| pallets/flask | ab814966 (3.1.0) | BSD-3-Clause |
| junegunn/fzf | 3347d615 (v0.60.0) | MIT |
| google/gson | 29e3d1d2 (2.12.1) | Apache-2.0 |
| JamesNK/Newtonsoft.Json | 0a2e291c (13.0.3) | MIT |
| guzzle/guzzle | d281ed31 (7.9.2) | MIT |
| sinatra/sinatra | 7b50a1bb (v4.1.1) | MIT |
| jqlang/jq | 71c2ab50 (jq-1.7.1) | MIT |
| fmtlib/fmt | 12391371 (11.1.4) | MIT |

Checkouts are gitignored; only the derived plants and manifest are committed.
