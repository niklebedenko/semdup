#!/usr/bin/env bash
# Fetch the pinned public corpora that eval/manifest.json's originals live in.
# The checkouts are gitignored; run this before `semdup inject-eval`.
set -euo pipefail
cd "$(dirname "$0")/corpus" 2>/dev/null || { mkdir -p "$(dirname "$0")/corpus" && cd "$(dirname "$0")/corpus"; }

RIPGREP_SHA=4649aa9700619f94cf9c66876e9549d83420e16c   # tag 14.1.1
VUE_SHA=6eb29d345aa73746207f80c89ee8b37ff7b949c9        # tag v3.5.13

fetch() {
  local dir=$1 url=$2 sha=$3
  if [ ! -d "$dir" ]; then
    git init -q "$dir"
    git -C "$dir" remote add origin "$url"
  fi
  git -C "$dir" fetch -q --depth 1 origin "$sha"
  git -C "$dir" checkout -q "$sha"
  echo "$dir @ $sha"
}

fetch ripgrep  https://github.com/BurntSushi/ripgrep.git "$RIPGREP_SHA"
fetch vue-core https://github.com/vuejs/core.git         "$VUE_SHA"
