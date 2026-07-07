#!/usr/bin/env python3
"""Create smaller ONNX variants for semdup model experiments.

Currently this wraps ONNX Runtime's dynamic int8 quantizer, which is the
practical CPU quantization path for transformer-style ONNX graphs. The output
directory keeps semdup's tokenizer.json and semdup-model.json next to the
quantized model.onnx so it can be passed directly to `semdup embed`.
"""

import argparse
import json
from pathlib import Path
from shutil import copy2


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--input", required=True, help="source semdup ONNX model directory")
    ap.add_argument("--out", required=True, help="output semdup ONNX model directory")
    ap.add_argument(
        "--mode",
        choices=["int8-dynamic"],
        default="int8-dynamic",
        help="quantization mode to apply",
    )
    ap.add_argument(
        "--op",
        action="append",
        dest="ops",
        default=["MatMul"],
        help="ONNX op type to quantize; repeat to add more (default: MatMul)",
    )
    ap.add_argument("--no-per-channel", action="store_true")
    args = ap.parse_args()

    src = Path(args.input)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    from onnxruntime.quantization import QuantType, quantize_dynamic

    quantize_dynamic(
        src / "model.onnx",
        out / "model.onnx",
        op_types_to_quantize=args.ops,
        per_channel=not args.no_per_channel,
        weight_type=QuantType.QInt8,
    )
    copy2(src / "tokenizer.json", out / "tokenizer.json")

    meta = json.loads((src / "semdup-model.json").read_text())
    meta["quantization"] = {
        "mode": args.mode,
        "op_types": args.ops,
        "per_channel": not args.no_per_channel,
        "weight_type": "QInt8",
    }
    (out / "semdup-model.json").write_text(json.dumps(meta, indent=2) + "\n")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
