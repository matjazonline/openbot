#!/usr/bin/env bash
# The one-off paid memory-service smoke check (plan 06, decision U9): the standalone journey
# (services/memory/scripts/standalone-journey.sh) against the real Anthropic Messages API, with
# the journey tenant's daily generation cap at 50,000 tokens, so the service itself refuses any
# spend beyond that budget. It makes one generation call. Never run by CI.
#
# The test API key is read from this repository's git-ignored .env, and only the line
# MEMORY_SMOKE_ANTHROPIC_API_KEY=...; nothing else in .env is read, and the key is never printed.
#   MEMORY_TEST_DATABASE_URL  superuser maintenance URL of a pgvector server (required)
#   MEMORY_PSQL, MEMORY_MODEL_DIR, MEMORY_EXTRACT_UNTRACKED  as the journey
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${MEMORY_TEST_DATABASE_URL:?memory-paid-smoke: MEMORY_TEST_DATABASE_URL is required}"
[[ -f "$root/.env" ]] || { echo "memory-paid-smoke: $root/.env is missing" >&2; exit 2; }
line="$(grep -E '^MEMORY_SMOKE_ANTHROPIC_API_KEY=' "$root/.env" | tail -n 1 || true)"
key="${line#MEMORY_SMOKE_ANTHROPIC_API_KEY=}"
key="${key%\"}"; key="${key#\"}"; key="${key%\'}"; key="${key#\'}"
[[ -n "$key" ]] || { echo "memory-paid-smoke: set MEMORY_SMOKE_ANTHROPIC_API_KEY in .env" >&2; exit 2; }
unset line
MEMORY_JOURNEY_ANTHROPIC_API_KEY="$key" MEMORY_JOURNEY_PROVIDER=anthropic MEMORY_JOURNEY_CAP=50000 \
  exec "$root/services/memory/scripts/standalone-journey.sh"
