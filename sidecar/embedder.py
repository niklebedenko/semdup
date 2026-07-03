#!/usr/bin/env python3
"""semdup sidecar embedding backend.

Protocol: JSON lines on stdin ({"id": n, "text": "..."}), JSON lines on
stdout ({"id": n, "vec": [...]}). One process per embed request; the parent
passes all pending texts at once so the model loads once.

Use this backend to trial models that have no ONNX export (trust_remote_code
architectures, brand-new releases). Requires: torch, sentence-transformers.
"""

import argparse
import json
import os
import sys

os.environ.setdefault("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--batch-size", type=int, default=16)
    ap.add_argument("--max-seq", type=int, default=2048)
    args = ap.parse_args()

    rows = []
    for line in sys.stdin:
        line = line.strip()
        if line:
            req = json.loads(line)
            rows.append((req["id"], req["text"]))
    if not rows:
        return
    print(f"embedding {len(rows)} texts with {args.model}", file=sys.stderr)

    import torch
    from sentence_transformers import SentenceTransformer

    device = "cuda" if torch.cuda.is_available() else "cpu"
    model = SentenceTransformer(
        args.model,
        trust_remote_code=True,
        model_kwargs={"torch_dtype": torch.float16 if device == "cuda" else torch.float32},
        device=device,
    )
    model.max_seq_length = min(args.max_seq, getattr(model, "max_seq_length", args.max_seq) or args.max_seq)

    # Length-sorted batches with a character budget: keeps padding waste and
    # peak memory bounded regardless of function-length distribution.
    rows.sort(key=lambda r: len(r[1]))
    batches, cur, cur_chars = [], [], 0
    for row in rows:
        cur.append(row)
        cur_chars += max(len(row[1]), 256)
        if len(cur) >= args.batch_size or cur_chars >= 24_000:
            batches.append(cur)
            cur, cur_chars = [], 0
    if cur:
        batches.append(cur)

    done = 0
    for batch in batches:
        vecs = model.encode(
            [t for _, t in batch], normalize_embeddings=True, show_progress_bar=False
        ).tolist()
        for (i, _), v in zip(batch, vecs):
            print(json.dumps({"id": i, "vec": v}))
        done += len(batch)
        if done % 320 == 0 or done == len(rows):
            print(f"  {done}/{len(rows)}", file=sys.stderr)
    sys.stdout.flush()


if __name__ == "__main__":
    main()
