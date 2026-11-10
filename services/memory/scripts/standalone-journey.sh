#!/usr/bin/env bash
# The standalone journey (plan 06B.1): copy only the service's files to a fresh directory outside
# any checkout, build it there with its own lockfile and SQLx metadata, provision a throwaway
# database and role, serve on loopback, and walk scripts/standalone_client.py (a generic,
# stdlib-only client) through provisioning, save, search, fetch, timeline, context, MCP,
# generation, correction, deletion, purge and tenant deletion. Every step must report `ok`.
#
#   MEMORY_TEST_DATABASE_URL  superuser maintenance URL of a PostgreSQL server with pgvector
#                             (required); the journey's database and role are dropped on exit
#   MEMORY_PSQL               psql binary (default: psql)
#   MEMORY_MODEL_DIR          the verified model (default: this service's assets/model)
#   MEMORY_JOURNEY_PORT       service port on 127.0.0.1 (default 18095)
#   MEMORY_JOURNEY_FAKE_PORT  Messages API stand-in port on 127.0.0.1 (default 18096)
#   MEMORY_JOURNEY_PROVIDER   `stand-in` (default) or `anthropic`: the real Messages API with the
#                             key in MEMORY_JOURNEY_ANTHROPIC_API_KEY (a paid call; the tenant's
#                             cap is MEMORY_JOURNEY_CAP, required in that mode). The key is
#                             removed from the environment at once and given only to `serve`.
#   MEMORY_EXTRACT_UNTRACKED=1  also copy untracked, non-ignored files (local runs before the
#                             service is committed; as scripts/extracted-build.sh)
# It never reads DATABASE_URL or any .env file and never touches another database.
set -euo pipefail
: "${MEMORY_TEST_DATABASE_URL:?standalone-journey: MEMORY_TEST_DATABASE_URL is required}"
service="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
psql="${MEMORY_PSQL:-psql}"
port="${MEMORY_JOURNEY_PORT:-18095}"
fake_port="${MEMORY_JOURNEY_FAKE_PORT:-18096}"
provider="${MEMORY_JOURNEY_PROVIDER:-stand-in}"
model="${MEMORY_MODEL_DIR:-$service/assets/model}"
case "$provider" in
  stand-in) cap=1000000; generated=saved ;;
  anthropic)
    : "${MEMORY_JOURNEY_ANTHROPIC_API_KEY:?standalone-journey: the anthropic provider needs MEMORY_JOURNEY_ANTHROPIC_API_KEY}"
    : "${MEMORY_JOURNEY_CAP:?standalone-journey: the anthropic provider needs MEMORY_JOURNEY_CAP}"
    cap="$MEMORY_JOURNEY_CAP"; generated=any ;;
  *) echo "standalone-journey: MEMORY_JOURNEY_PROVIDER must be stand-in or anthropic" >&2; exit 2 ;;
esac
# The provider key stays in an unexported variable: no build script, client or psql sees it.
provider_key="${MEMORY_JOURNEY_ANTHROPIC_API_KEY:-journey-fake-key}"
unset MEMORY_JOURNEY_ANTHROPIC_API_KEY MEMORY_ANTHROPIC_API_KEY
[[ -d "$model" ]] || { echo "standalone-journey: no model at $model (scripts/prepare-model.py)" >&2; exit 2; }
model="$(cd "$model" && pwd)"

work="$(mktemp -d "${TMPDIR:-/tmp}/memory-journey.XXXXXX")"
suffix="$(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')"
database="memory_journey_$suffix"; role="memory_journey_$suffix"
password="$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')"
serve_pid=""; fake_pid=""
cleanup() {
  status=$?
  set +e
  [[ -n "$serve_pid" ]] && kill "$serve_pid" 2>/dev/null && wait "$serve_pid" 2>/dev/null
  [[ -n "$fake_pid" ]] && kill "$fake_pid" 2>/dev/null && wait "$fake_pid" 2>/dev/null
  "$psql" "$MEMORY_TEST_DATABASE_URL" -X -q -c "DROP DATABASE IF EXISTS \"$database\" WITH (FORCE)" \
    -c "DROP ROLE IF EXISTS \"$role\"" >/dev/null 2>&1 || echo "warning: could not drop $database/$role" >&2
  if [[ $status -ne 0 && -f "$work/serve.log" ]]; then
    echo "--- service log (last 40 lines) ---" >&2; tail -n 40 "$work/serve.log" >&2
  fi
  rm -rf "$work"
  exit "$status"
}
trap cleanup EXIT

# The extracted copy: only the service's own files, outside any checkout.
dest="$work/memory"; mkdir -p "$dest"
select=(--cached)
[[ "${MEMORY_EXTRACT_UNTRACKED:-}" == 1 ]] && select+=(--others --exclude-standard)
( cd "$service" && git ls-files -z "${select[@]}" . ) | ( cd "$service" && xargs -0 -I{} rsync -R "{}" "$dest/" )
[[ -f "$dest/Cargo.toml" ]] || { echo "standalone-journey: no service files (MEMORY_EXTRACT_UNTRACKED=1 before the first commit)" >&2; exit 2; }
( cd "$dest" && git rev-parse --show-toplevel >/dev/null 2>&1 ) && { echo "standalone-journey: the copy is inside a checkout" >&2; exit 2; }
# The model travels with the copy, so the running service reads nothing outside it.
mkdir -p "$dest/assets" && rsync -a "$model/" "$dest/assets/model/"
model="$dest/assets/model"
echo "standalone-journey: building the extracted copy in $dest"
( cd "$dest" && scripts/boundary-check.sh && cargo build --locked --offline --bin memory-service ) >"$work/build.log" 2>&1 \
  || { tail -n 40 "$work/build.log" >&2; exit 1; }
binary="$dest/target/debug/memory-service"

MEMORY_PSQL="$psql" MEMORY_PROVISION_ADMIN_URL="$MEMORY_TEST_DATABASE_URL" \
  MEMORY_PROVISION_DATABASE="$database" MEMORY_PROVISION_ROLE="$role" \
  MEMORY_PROVISION_PASSWORD="$password" "$dest/scripts/provision-db.sh" >/dev/null
base="${MEMORY_TEST_DATABASE_URL%%\?*}"; query=""
[[ "$MEMORY_TEST_DATABASE_URL" == *\?* ]] && query="?${MEMORY_TEST_DATABASE_URL#*\?}"
authority="${base#*://}"; authority="${authority%%/*}"; hostport="${authority##*@}"
export MEMORY_DATABASE_URL="postgres://$role:$password@$hostport/$database$query"
export MEMORY_MODEL_DIR="$model" MEMORY_LOG=warn MEMORY_BIND_ADDR="127.0.0.1:$port"
export MEMORY_GENERATION_WORKERS=1
if [[ "$provider" == stand-in ]]; then
  python3 -I "$dest/scripts/fake-messages.py" "$fake_port" "$work/messages.jsonl" &
  fake_pid=$!
fi

operator="$(cd "$dest" && "$binary" admin bootstrap --label "standalone journey" 2>"$work/bootstrap.log" | tail -n 1)"
[[ "$operator" == msk_adm_* ]] || { echo "bootstrap printed no operator token" >&2; cat "$work/bootstrap.log" >&2; exit 1; }
# Only `serve` gets the provider settings, as shell assignments (never argv, so `ps` cannot show
# the key).
if [[ "$provider" == stand-in ]]; then
  ( cd "$dest" && MEMORY_ANTHROPIC_API_KEY="$provider_key" \
      MEMORY_ANTHROPIC_BASE_URL="http://127.0.0.1:$fake_port" exec "$binary" serve ) >"$work/serve.log" 2>&1 &
else
  ( cd "$dest" && MEMORY_ANTHROPIC_API_KEY="$provider_key" exec "$binary" serve ) >"$work/serve.log" 2>&1 &
fi
serve_pid=$!
for _ in $(seq 1 240); do
  curl -fsS "http://127.0.0.1:$port/readyz" >/dev/null 2>&1 && break
  kill -0 "$serve_pid" 2>/dev/null || { echo "the service exited during startup" >&2; exit 1; }
  sleep 0.5
done
curl -fsS "http://127.0.0.1:$port/readyz" >/dev/null || { echo "the service never became ready" >&2; exit 1; }

MEMORY_URL="http://127.0.0.1:$port" MEMORY_OPERATOR_TOKEN="$operator" MEMORY_JOURNEY_CAP="$cap" \
  MEMORY_JOURNEY_GENERATED="$generated" python3 -I "$dest/scripts/standalone_client.py" | tee "$work/journey.log"
steps=(provision save search classification fetch timeline context mcp generation correct delete purge tenant_delete)
for step in "${steps[@]}"; do
  grep -qx "memory_standalone_${step} ok" "$work/journey.log" || { echo "standalone-journey: step $step did not pass" >&2; exit 1; }
done
if [[ "$provider" == stand-in ]]; then
  grep -q '"path": "/v1/messages"' "$work/messages.jsonl" || { echo "the stand-in was never called" >&2; exit 1; }
fi
# What the provider reported using (per-process counters, so the tenant deletion keeps them).
printf 'authorization: Bearer %s\n' "$operator" \
  | curl -fsS "http://127.0.0.1:$port/admin/v1/metrics" -H @- \
  | grep -E '^memory_provider_tokens_total|^memory_generation_settled_total' || true
echo "standalone-journey: ${#steps[@]} steps passed ($provider provider)"
