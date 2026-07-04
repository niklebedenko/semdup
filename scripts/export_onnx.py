#!/usr/bin/env python3
"""Export a sentence-transformers encoder to a semdup ONNX model directory.

Produces <out>/model.onnx, <out>/tokenizer.json, <out>/semdup-model.json.
The exported graph takes (input_ids, attention_mask) int64 [batch, seq] and
returns the finished embedding float32 [batch, dim] — pooling (cls or masked
mean, per the model's sentence-transformers config) and L2 normalization are
baked into the graph so they run on the GPU and only dim floats per text
cross back to the host.

--fp16 exports half-precision weights (embeds ~2x faster on GPU; requires
CUDA at export time). The default fp32 export is the right choice for
CPU-only use.

After export this script verifies the ONNX model against the original at
several sequence lengths (rotary-embedding models can trace incorrectly for
lengths other than the export length; the check catches that).

Requires: torch, sentence-transformers, onnx, onnxruntime.
Example:
  python3 scripts/export_onnx.py --model nomic-ai/CodeRankEmbed --out models/coderankembed
"""

import argparse
import json
from pathlib import Path

import numpy as np
import torch


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument(
        "--max-seq", type=int, default=0,
        help="0 = min(model default, 2048); rotary models are only exported "
        "up to the length whose cache was warmed",
    )
    ap.add_argument("--opset", type=int, default=17)
    # trace + the cache pre-warm below verifies bit-exact for CodeRankEmbed;
    # the dynamo/torch.export path currently fails on it (batch-dim guard bug).
    ap.add_argument("--exporter", choices=["trace", "dynamo"], default="trace")
    ap.add_argument("--fp16", action="store_true", help="half-precision weights (GPU)")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    from sentence_transformers import SentenceTransformer

    st = SentenceTransformer(args.model, trust_remote_code=True, device="cpu")
    st.eval()
    max_seq = args.max_seq or min(st.max_seq_length, 2048)
    st.max_seq_length = max_seq
    transformer = st[0].auto_model.float().eval()
    tokenizer = st.tokenizer
    dim = st.get_sentence_embedding_dimension()

    # sentence-transformers pooling module config decides how semdup pools.
    pooling = "mean"
    for m in st:
        cfg = getattr(m, "get_config_dict", dict)()
        if "pooling_mode" in cfg:
            pooling = cfg["pooling_mode"]
        for key in ("pooling_mode_cls_token", "pooling_mode_mean_tokens"):
            if cfg.get(key):
                pooling = "cls" if "cls" in key else "mean"
    if pooling not in ("cls", "mean"):
        raise SystemExit(f"unsupported pooling mode: {pooling}")
    print(f"pooling: {pooling}")

    class Wrapper(torch.nn.Module):
        def __init__(self, m, pooling):
            super().__init__()
            self.m = m
            self.pooling = pooling

        def forward(self, input_ids, attention_mask):
            out = self.m(input_ids=input_ids, attention_mask=attention_mask)
            hidden = out.last_hidden_state if hasattr(out, "last_hidden_state") else out[0]
            if self.pooling == "cls":
                emb = hidden[:, 0]
            else:
                m = attention_mask.unsqueeze(-1).to(hidden.dtype)
                emb = (hidden * m).sum(1) / m.sum(1).clamp(min=1e-9)
            # Pool in model dtype, hand back fp32 unit vectors either way.
            return torch.nn.functional.normalize(emb.float(), dim=-1)

    device = "cpu"
    if args.fp16:
        if not torch.cuda.is_available():
            raise SystemExit("--fp16 requires CUDA at export time")
        device = "cuda"
        transformer = transformer.half()
    wrapper = Wrapper(transformer, pooling).to(device)

    # Pre-warm rotary/position caches to max_seq so no cache-growth branch is
    # taken while exporting (the branch is untraceable control flow).
    with torch.no_grad():
        warm = torch.randint(100, 1000, (1, max_seq), dtype=torch.int64, device=device)
        wrapper(warm, torch.ones_like(warm))

    ex_ids = torch.randint(100, 1000, (2, 37), dtype=torch.int64, device=device)
    ex_mask = torch.ones_like(ex_ids)
    ex_mask[1, 30:] = 0

    # dynamo (torch.export-based) tracks symbolic shapes, which legacy tracing
    # can get wrong for rotary-embedding models; trace is kept as a fallback
    # since the cache pre-warm above fixes the common rotary failure mode.
    torch.onnx.export(
        wrapper,
        (ex_ids, ex_mask),
        str(out / "model.onnx"),
        input_names=["input_ids", "attention_mask"],
        output_names=["embedding"],
        dynamic_axes={
            "input_ids": {0: "batch", 1: "seq"},
            "attention_mask": {0: "batch", 1: "seq"},
            "embedding": {0: "batch"},
        },
        opset_version=args.opset,
        dynamo=args.exporter == "dynamo",
    )
    tokenizer.save_pretrained(out / "_tok")
    (out / "tokenizer.json").write_bytes((out / "_tok" / "tokenizer.json").read_bytes())
    (out / "semdup-model.json").write_text(
        json.dumps(
            {
                "model": args.model,
                "max_seq": max_seq,
                "dim": dim,
                "pooling": pooling,
                "fp16": args.fp16,
            },
            indent=2,
        )
    )
    print(
        f"exported to {out} (max_seq {max_seq}, dim {dim}, pooling {pooling}, "
        f"{'fp16' if args.fp16 else 'fp32'})"
    )

    # --- verification at several lengths ---
    import onnxruntime as onnxrt

    sess = onnxrt.InferenceSession(str(out / "model.onnx"), providers=["CPUExecutionProvider"])
    # Exporters may mangle input names; bind by position (ids, mask).
    in_names = [i.name for i in sess.get_inputs()]
    samples = [
        "fn add(a: u32, b: u32) -> u32 { a + b }",
        "def hist(xs):\n" + "\n".join(f"    v{i} = xs[{i}] * {i}" for i in range(60)),
        "export const clamp = (x: number, lo: number, hi: number) => "
        + " + ".join(f"Math.min({i}, x)" for i in range(200)),
        "long_repeat " * 900,
    ]
    worst = 1.0
    for text in samples:
        ref = st.encode([text], normalize_embeddings=True)[0]
        enc = tokenizer([text], return_tensors="np", truncation=True, max_length=max_seq)
        ids = enc["input_ids"].astype(np.int64)
        mask = enc["attention_mask"].astype(np.int64)
        outputs = sess.run(None, {in_names[0]: ids, in_names[1]: mask})
        pooled = outputs[0][0]
        cos = float(np.dot(ref, pooled))
        worst = min(worst, cos)
        print(f"  len {ids.shape[1]:5d}  cos(onnx, reference) = {cos:.6f}")
    gate = 0.998 if args.fp16 else 0.999
    if worst < gate:
        raise SystemExit(f"VERIFICATION FAILED: worst cosine {worst:.6f} < {gate}")
    print(f"verification OK (worst cosine {worst:.6f})")


if __name__ == "__main__":
    main()
