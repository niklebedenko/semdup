#!/usr/bin/env python3
"""Emit GitHub annotations and a Markdown summary for semdup diff JSON."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path


UNIT_RE = re.compile(r"^(?P<path>.*):(?P<start>\d+)-(?P<end>\d+)\s+(?P<name>.*)$")


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: semdup-github-report.py REPORT_JSON STEP_SUMMARY", file=sys.stderr)
        return 2

    report_path = Path(sys.argv[1])
    summary_path = Path(sys.argv[2]) if sys.argv[2] else None
    if not report_path.exists():
        append_summary(
            summary_path,
            "## semdup\n\nNo diff report was produced. This is expected for scan-only runs or empty diffs.\n",
        )
        return 0

    try:
        reports = json.loads(report_path.read_text())
    except json.JSONDecodeError as exc:
        print(f"could not parse {report_path}: {exc}", file=sys.stderr)
        return 1

    for item in reports:
        emit_annotation(item)
    append_summary(summary_path, render_summary(reports))
    return 0


def emit_annotation(item: dict) -> None:
    verdict = item.get("verdict")
    if verdict not in {"DUP", "REVIEW"}:
        return
    parsed = parse_unit(item.get("unit", ""))
    if parsed is None:
        return
    level = "error" if verdict == "DUP" else "warning"
    top = top_neighbor(item)
    message = f"{verdict}: nearest duplicate {top}" if top else verdict
    print(
        f"::{level} file={escape_prop(parsed['path'])},"
        f"line={parsed['start']},"
        f"endLine={parsed['end']},"
        f"title=semdup {verdict.lower()}::{escape_message(message)}"
    )


def render_summary(reports: list[dict]) -> str:
    total = len(reports)
    dup = sum(1 for r in reports if r.get("verdict") == "DUP")
    review = sum(1 for r in reports if r.get("verdict") == "REVIEW")
    ok = total - dup - review
    lines = [
        "## semdup diff",
        "",
        f"{dup} duplicate, {review} review, {ok} ok across {total} touched function(s).",
        "",
    ]
    if not reports:
        return "\n".join(lines) + "\n"

    lines.extend(
        [
            "| Verdict | Unit | Top neighbor |",
            "| --- | --- | --- |",
        ]
    )
    for item in reports[:50]:
        verdict = item.get("verdict", "")
        unit = markdown_escape(item.get("unit", ""))
        neighbor = markdown_escape(top_neighbor(item))
        lines.append(f"| `{verdict}` | `{unit}` | {neighbor} |")
    if len(reports) > 50:
        lines.append(f"\n... {len(reports) - 50} more touched function(s) omitted.")
    lines.append("")
    return "\n".join(lines)


def top_neighbor(item: dict) -> str:
    neighbors = item.get("neighbors") or []
    if not neighbors:
        return ""
    first = neighbors[0]
    unit = first.get("unit", "")
    cosine = first.get("cosine")
    if isinstance(cosine, (int, float)):
        return f"{cosine:.4f} {unit}"
    return unit


def parse_unit(unit: str) -> dict | None:
    match = UNIT_RE.match(unit)
    if not match:
        return None
    return match.groupdict()


def append_summary(path: Path | None, text: str) -> None:
    if path is None:
        return
    with path.open("a", encoding="utf-8") as f:
        f.write(text)


def markdown_escape(text: str) -> str:
    return text.replace("|", "\\|").replace("\n", " ")


def escape_prop(text: str) -> str:
    return (
        text.replace("%", "%25")
        .replace("\r", "%0D")
        .replace("\n", "%0A")
        .replace(":", "%3A")
        .replace(",", "%2C")
    )


def escape_message(text: str) -> str:
    return text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")


if __name__ == "__main__":
    raise SystemExit(main())
