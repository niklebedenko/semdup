#!/usr/bin/env python3
"""Benchmark exact scan scoring with CUDA matrix multiplication.

This is a development probe, not a semdup runtime dependency. It loads cached
embeddings from semdup.sqlite, copies the matrix to CUDA, computes blocked
X @ X.T scores, thresholds them, and applies the same same-file containment
and larger-overlapping-block exclusions as the Rust scan path.
"""

from __future__ import annotations

import argparse
import sqlite3
import time
from dataclasses import dataclass

import numpy as np
import torch


@dataclass(frozen=True)
class Unit:
    path: str
    kind: str
    start_line: int
    end_line: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--db", default="semdup.sqlite")
    parser.add_argument("--model", default="nomic-ai/CodeRankEmbed")
    parser.add_argument("--threshold", type=float, default=0.7)
    parser.add_argument("--min-lines", type=int, default=8)
    parser.add_argument("--unit-kind", choices=["function", "block"])
    parser.add_argument("--block-size", type=int, default=4096)
    parser.add_argument("--include-tests", action="store_true")
    parser.add_argument("--renormalize", action="store_true")
    parser.add_argument("--tf32", action="store_true")
    return parser.parse_args()


def contains(a: Unit, b: Unit) -> bool:
    return (a.start_line <= b.start_line <= b.end_line <= a.end_line) or (
        b.start_line <= a.start_line <= a.end_line <= b.end_line
    )


def load_embeddings(args: argparse.Namespace) -> tuple[list[Unit], np.ndarray]:
    conn = sqlite3.connect(args.db)
    rows = conn.execute(
        """
        SELECT u.path, u.unit_kind, u.start_line, u.end_line, u.ignored, u.is_test, e.vec
        FROM units u JOIN embeddings e ON e.hash = u.hash AND e.model = ?
        WHERE u.corpus = 'main'
          AND u.ignored = 0
          AND (u.end_line - u.start_line + 1) >= ?
          AND (? = 1 OR u.is_test = 0)
          AND (? IS NULL OR u.unit_kind = ?)
        """,
        (args.model, args.min_lines, args.include_tests, args.unit_kind, args.unit_kind),
    ).fetchall()
    units: list[Unit] = []
    vectors: list[np.ndarray] = []
    for path, kind, start, end, _ignored, _is_test, blob in rows:
        vec = np.frombuffer(blob, dtype="<f4").astype(np.float32, copy=True)
        if args.renormalize:
            norm = np.linalg.norm(vec)
            if norm > 0:
                vec /= norm
        units.append(Unit(path=path, kind=kind, start_line=start, end_line=end))
        vectors.append(vec)
    keep = drop_larger_overlapping_blocks(units)
    units = [unit for unit, keep_unit in zip(units, keep, strict=True) if keep_unit]
    vectors = [vec for vec, keep_unit in zip(vectors, keep, strict=True) if keep_unit]
    return units, np.stack(vectors)


def drop_larger_overlapping_blocks(units: list[Unit]) -> list[bool]:
    by_path: dict[str, list[int]] = {}
    for i, unit in enumerate(units):
        if unit.kind == "block":
            by_path.setdefault(unit.path, []).append(i)
    keep = [True] * len(units)
    for indices in by_path.values():
        for i in indices:
            unit = units[i]
            unit_lines = unit.end_line - unit.start_line + 1
            if any(
                i != j
                and unit_lines > units[j].end_line - units[j].start_line + 1
                and unit.start_line <= units[j].end_line
                and units[j].start_line <= unit.end_line
                for j in indices
            ):
                keep[i] = False
    return keep


def main() -> None:
    args = parse_args()
    if not torch.cuda.is_available():
        raise SystemExit("CUDA is not available to PyTorch")
    torch.backends.cuda.matmul.allow_tf32 = args.tf32

    t0 = time.perf_counter()
    units, matrix = load_embeddings(args)
    load_s = time.perf_counter() - t0

    device = torch.device("cuda")
    torch.cuda.synchronize()
    t0 = time.perf_counter()
    x = torch.from_numpy(matrix).to(device)
    torch.cuda.synchronize()
    copy_s = time.perf_counter() - t0

    n = len(units)
    pair_count = 0
    t0 = time.perf_counter()
    with torch.no_grad():
        for i0 in range(0, n, args.block_size):
            i1 = min(i0 + args.block_size, n)
            left = x[i0:i1]
            for j0 in range(i0, n, args.block_size):
                j1 = min(j0 + args.block_size, n)
                scores = left @ x[j0:j1].T
                mask = scores >= args.threshold
                if i0 == j0:
                    mask = torch.triu(mask, diagonal=1)
                hits = mask.nonzero().cpu().numpy()
                for local_i, local_j in hits:
                    i = i0 + int(local_i)
                    j = j0 + int(local_j)
                    if units[i].path == units[j].path and contains(units[i], units[j]):
                        continue
                    pair_count += 1
    torch.cuda.synchronize()
    search_s = time.perf_counter() - t0

    print(f"device={torch.cuda.get_device_name(0)}")
    kind = args.unit_kind or "all"
    print(f"shape={matrix.shape} kind={kind} threshold={args.threshold} block={args.block_size}")
    print(f"pairs={pair_count}")
    print(f"load_s={load_s:.4f} copy_s={copy_s:.4f} gpu_search_s={search_s:.4f}")


if __name__ == "__main__":
    main()
