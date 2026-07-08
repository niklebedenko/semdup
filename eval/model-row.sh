#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: eval/model-row.sh --model <alias-or-model-id> [options]

Print one Markdown row for the eval model matrix. Known aliases live in
eval/models.tsv; unknown --model values are passed directly to semdup.

Options:
  --db <path>          SQLite DB to use (default: semdup.sqlite)
  --manifest <path>    Eval manifest (default: eval/manifest.json)
  --models <path>      Alias table (default: eval/models.tsv)
  --provider <name>     Override ONNX provider for this row: auto, cpu, cuda
  --unit-kind <kind>   Eval unit kind: function or block (default: function)
  --no-extract         Do not refresh eval/corpus + eval/injected rows first
  --header             Print the Markdown table header before the row
  -h, --help           Show this help

Set SEMDUP_BIN to choose the executable, e.g.
  SEMDUP_BIN="target/release/semdup" eval/model-row.sh --model coderankembed-fp32
USAGE
}

model=""
db="semdup.sqlite"
manifest="eval/manifest.json"
models="eval/models.tsv"
extract=1
header=0
provider="-"
unit_kind="function"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --model)
      model="${2:-}"
      shift 2
      ;;
    --db)
      db="${2:-}"
      shift 2
      ;;
    --manifest)
      manifest="${2:-}"
      shift 2
      ;;
    --models)
      models="${2:-}"
      shift 2
      ;;
    --provider)
      provider="${2:-}"
      shift 2
      ;;
    --unit-kind)
      unit_kind="${2:-}"
      shift 2
      ;;
    --no-extract)
      extract=0
      shift
      ;;
    --header)
      header=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$model" ]]; then
  echo "--model is required" >&2
  usage >&2
  exit 2
fi

if [[ -n "${SEMDUP_BIN:-}" ]]; then
  read -r -a semdup <<<"$SEMDUP_BIN"
elif [[ -f Cargo.toml ]]; then
  semdup=(cargo run --quiet --)
elif [[ -x target/release/semdup ]]; then
  semdup=(target/release/semdup)
elif command -v semdup >/dev/null 2>&1; then
  semdup=(semdup)
else
  echo "semdup binary not found; run from the repo root or set SEMDUP_BIN" >&2
  exit 1
fi

label="$model"
model_key="$model"
backend="-"
model_dir="-"
script="-"

if [[ -f "$models" ]]; then
  while IFS=$'\t' read -r alias tsv_label tsv_model_key tsv_backend tsv_provider tsv_model_dir tsv_script; do
    [[ -z "${alias:-}" || "${alias:0:1}" == "#" ]] && continue
    if [[ "$alias" == "$model" ]]; then
      label="$tsv_label"
      model_key="$tsv_model_key"
      backend="$tsv_backend"
      if [[ "$provider" == "-" ]]; then
        provider="$tsv_provider"
      fi
      model_dir="$tsv_model_dir"
      script="$tsv_script"
      break
    fi
  done <"$models"
fi

if [[ "$model_dir" != "-" && ! -d "$model_dir" ]]; then
  echo "model dir '$model_dir' does not exist for '$model'" >&2
  echo "first create it, e.g. python3 scripts/quantize_onnx.py --mode nbits-int4-asym --input models/coderankembed-fp32 --out $model_dir" >&2
  exit 1
fi

if [[ "$extract" -eq 1 ]]; then
  eval/fetch-corpus.sh >/dev/null
  extract_args=(--granularity function)
  if [[ "$unit_kind" == "block" ]]; then
    extract_args+=(--granularity block)
  fi
  "${semdup[@]}" --db "$db" extract --root eval/corpus --corpus main "${extract_args[@]}" >/dev/null
  "${semdup[@]}" --db "$db" extract --root eval/injected --corpus injected "${extract_args[@]}" >/dev/null
fi

embed_args=(--db "$db" embed --model "$model_key")
if [[ "$backend" != "-" ]]; then
  embed_args+=(--backend "$backend")
fi
if [[ "$provider" != "-" ]]; then
  embed_args+=(--provider "$provider")
fi
if [[ "$model_dir" != "-" ]]; then
  embed_args+=(--model-dir "$model_dir")
fi
if [[ "$script" != "-" ]]; then
  embed_args+=(--script "$script")
fi
"${semdup[@]}" "${embed_args[@]}" >/dev/null

if [[ "$header" -eq 1 ]]; then
  echo "| model | n | L1 R@1 | L1 R@5 | L2 R@1 | L2 R@5 | L3 R@1 | L3 R@5 | macro F1@1 | L3 margin |"
  echo "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
fi

"${semdup[@]}" --db "$db" inject-eval \
  --model "$model_key" \
  --label "$label" \
  --manifest "$manifest" \
  --unit-kind "$unit_kind" \
  --summary-row
