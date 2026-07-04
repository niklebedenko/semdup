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
eval/fetch-corpus.sh                  # pinned ripgrep + vuejs/core checkouts
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

## 2. Threshold calibration (`semdup calibrate`)

Measures where *your* repo's duplicate/not-duplicate boundary sits for a
given model. You label real pairs from your own code, and calibrate sweeps
thresholds to maximize balanced accuracy.

```bash
cp eval/labels.example.json labels.json   # then edit: label your own pairs
semdup calibrate --model <MODEL> --labels labels.json
```

Labeling guidance:

- Pull candidates from a permissive `semdup scan --threshold 0.5` run, plus
  a few pairs you already know about.
- `"dup": true` — you would want a review comment on this pair (copy-paste,
  re-implementation, divergent twins).
- `"dup": false` — similar-looking but intentionally parallel (read/write
  mirrors, per-variant impls, API boilerplate). These hard negatives are the
  valuable ones; ~10 of each class is enough to locate the boundary.
- Selectors are `path-suffix::name`; the suffix only needs to be unambiguous.

Calibration is per-repo *and* per-model: different codebases have different
vocabulary overlap, and different models place the same pairs differently.
Re-run after switching models. If calibrate reports a mushy optimum (wide
plateau, poor balanced accuracy), don't gate CI on the threshold at all —
use `semdup diff` without `--check` and read its neighbor evidence by hand.

## Corpus provenance

| corpus | pin | license |
|---|---|---|
| BurntSushi/ripgrep | 4649aa97 (14.1.1) | MIT OR Unlicense |
| vuejs/core | 6eb29d34 (v3.5.13) | MIT |

Checkouts are gitignored; only the derived plants and manifest are committed.
