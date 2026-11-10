#!/usr/bin/env bash
# Run one focused test prefix: boundary check, list the selection, refuse an empty selection,
# then run it. DB-backed tests read MEMORY_TEST_DATABASE_URL and fail (never skip) without it.
#   scripts/focused.sh memory_contract_ [--network-isolated]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
prefix="${1:?usage: scripts/focused.sh PREFIX [--network-isolated]}"
scripts/boundary-check.sh
listed="$(cargo test --locked --offline --all-targets "$prefix" -- --list 2>/dev/null | grep -c ': test$' || true)"
echo "focused: $prefix selects $listed tests"
if [[ "$listed" -eq 0 ]]; then
  echo "focused: refusing an empty selection for $prefix" >&2
  exit 1
fi
runner=()
if [[ "${2:-}" == "--network-isolated" ]]; then runner=(scripts/test-network-isolation.sh); fi
"${runner[@]}" cargo test --locked --offline --all-targets "$prefix"
