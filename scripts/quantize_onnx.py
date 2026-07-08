#!/usr/bin/env python3
"""Create smaller ONNX variants for semdup model experiments.

This wraps ONNX Runtime quantizers used by the hosted semdup model variants.
The output directory keeps semdup's tokenizer.json and semdup-model.json next
to the quantized model.onnx so it can be passed directly to `semdup embed`.
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
        choices=["int8-dynamic", "nbits-int4-asym"],
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

    meta = json.loads((src / "semdup-model.json").read_text())
    if args.mode == "int8-dynamic":
        from onnxruntime.quantization import QuantType, quantize_dynamic

        quantize_dynamic(
            src / "model.onnx",
            out / "model.onnx",
            op_types_to_quantize=args.ops,
            per_channel=not args.no_per_channel,
            weight_type=QuantType.QInt8,
        )
        meta["quantization"] = {
            "mode": args.mode,
            "op_types": args.ops,
            "per_channel": not args.no_per_channel,
            "weight_type": "QInt8",
        }
    elif args.mode == "nbits-int4-asym":
        from onnxruntime.quantization.matmul_nbits_quantizer import MatMulNBitsQuantizer

        quantizer = MatMulNBitsQuantizer(
            str(src / "model.onnx"),
            bits=4,
            block_size=128,
            is_symmetric=False,
            op_types_to_quantize=tuple(args.ops),
        )
        quantizer.process()
        quantizer.model.save_model_to_file(
            str(out / "model.onnx"), use_external_data_format=True
        )
        meta["quantization"] = {
            "mode": "nbits",
            "bits": 4,
            "block_size": 128,
            "symmetric": False,
            "op_types": args.ops,
        }
    else:
        raise AssertionError(args.mode)

    copy2(src / "tokenizer.json", out / "tokenizer.json")

    (out / "semdup-model.json").write_text(json.dumps(meta, indent=2) + "\n")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
