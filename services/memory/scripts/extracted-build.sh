#!/usr/bin/env bash
# Copy only the tracked service files (plus prepared model assets, when present) to a fresh
# directory and run the full service gate there. A root build is not independence evidence.
#   MEMORY_EXTRACT_UNTRACKED=1  also copy untracked, non-ignored files: only for a local run
#                               before the service is committed; CI never sets it.
set -euo pipefail
service="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest="$(mktemp -d "${TMPDIR:-/tmp}/memory-extracted.XXXXXX")/memory"
mkdir -p "$dest"
select=(--cached)
if [[ "${MEMORY_EXTRACT_UNTRACKED:-}" == 1 ]]; then select+=(--others --exclude-standard); fi
( cd "$service" && git ls-files -z "${select[@]}" . ) | ( cd "$service" && xargs -0 -I{} rsync -R "{}" "$dest/" )
[[ -f "$dest/Cargo.toml" ]] || { echo "extracted: no tracked service files (set MEMORY_EXTRACT_UNTRACKED=1 before the first commit)" >&2; exit 2; }
if [[ -d "$service/assets/model" ]]; then rsync -a "$service/assets/model" "$dest/assets/"; fi
echo "extracted: $dest (${select[*]})"
cd "$dest"
exec scripts/verify.sh "$@"
