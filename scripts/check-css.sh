#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
generated_css=$(mktemp "${TMPDIR:-/tmp}/openbot-css.XXXXXX")
trap 'rm -f "$generated_css"' EXIT
node_modules/.bin/tailwindcss -i assets/app.css.in -o "$generated_css" --minify
if ! cmp -s assets/app.css "$generated_css"; then
    echo 'assets/app.css is stale; run npm run build:css' >&2
    exit 1
fi
