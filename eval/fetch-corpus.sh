#!/usr/bin/env bash
# Fetch the pinned public corpora that eval/manifest.json's originals live in.
# The checkouts are gitignored; run this before `semdup inject-eval`.
set -euo pipefail
cd "$(dirname "$0")/corpus" 2>/dev/null || { mkdir -p "$(dirname "$0")/corpus" && cd "$(dirname "$0")/corpus"; }

RIPGREP_SHA=4649aa9700619f94cf9c66876e9549d83420e16c   # tag 14.1.1
VUE_SHA=6eb29d345aa73746207f80c89ee8b37ff7b949c9        # tag v3.5.13
FLASK_SHA=ab8149664182b662453a563161aa89013c806dc9      # tag 3.1.0
FZF_SHA=3347d6159156f2c3e269a54b7fb34aa905a3fd2d        # tag v0.60.0
GSON_SHA=29e3d1d2cc0ce4175378e511a87f538561625515       # tag gson-parent-2.12.1

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
fetch flask    https://github.com/pallets/flask.git      "$FLASK_SHA"
fetch fzf      https://github.com/junegunn/fzf.git       "$FZF_SHA"
fetch gson     https://github.com/google/gson.git        "$GSON_SHA"
