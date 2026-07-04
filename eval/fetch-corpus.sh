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
NEWTONSOFT_SHA=0a2e291c0d9c0c7675d445703e51750363a549ef # tag 13.0.3
GUZZLE_SHA=d281ed313b989f213357e3be1a179f02196ac99b     # tag 7.9.2
SINATRA_SHA=7b50a1bbb5324838908dfaa00ec53ad322673a29    # tag v4.1.1
JQ_SHA=71c2ab509a8628dbbad4bc7b3f98a64aa90d3297         # tag jq-1.7.1
FMT_SHA=123913715afeb8a437e6388b4473fcc4753e1c9a        # tag 11.1.4

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

fetch ripgrep    https://github.com/BurntSushi/ripgrep.git      "$RIPGREP_SHA"
fetch vue-core   https://github.com/vuejs/core.git              "$VUE_SHA"
fetch flask      https://github.com/pallets/flask.git           "$FLASK_SHA"
fetch fzf        https://github.com/junegunn/fzf.git            "$FZF_SHA"
fetch gson       https://github.com/google/gson.git             "$GSON_SHA"
fetch newtonsoft https://github.com/JamesNK/Newtonsoft.Json.git "$NEWTONSOFT_SHA"
fetch guzzle     https://github.com/guzzle/guzzle.git           "$GUZZLE_SHA"
fetch sinatra    https://github.com/sinatra/sinatra.git         "$SINATRA_SHA"
fetch jq         https://github.com/jqlang/jq.git               "$JQ_SHA"
fetch fmt        https://github.com/fmtlib/fmt.git              "$FMT_SHA"
