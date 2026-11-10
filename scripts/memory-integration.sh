#!/usr/bin/env bash
# Live memory-service integration (phase 05 E11): OpenBot's adapter, workers and freezes against
# the real service, built from services/memory, with a loopback stand-in for the Messages API.
#
#   MEMORY_INTEGRATION_ADMIN_URL  superuser URL of a PostgreSQL server with pgvector (required);
#                                 a throwaway service database and role are created there and
#                                 dropped on exit
#   TEST_DATABASE_URL             OpenBot's test database (required; tests make their own), on
#                                 the same server as the admin URL: the isolation check creates
#                                 and drops a throwaway table in it
#   MEMORY_INTEGRATION_PORT       service port on 127.0.0.1 (default 18085)
#   MEMORY_INTEGRATION_FAKE_PORT  Messages API stand-in port on 127.0.0.1 (default 18086)
#   MEMORY_PSQL                   psql binary (default: psql)
#   MEMORY_MODEL_DIR              the verified model (default services/memory/assets/model;
#                                 prepare it with services/memory/scripts/prepare-model.py)
#
# Builds the service in its own directory, provisions its database with provision-db.sh,
# bootstraps an operator, issues a provisioner credential, serves on loopback, then runs the
# ignored `memory_integration_live_` and `memory_e2e_` tests and requires a nonzero count of each.
set -euo pipefail
: "${MEMORY_INTEGRATION_ADMIN_URL:?superuser URL of a pgvector server}" "${TEST_DATABASE_URL:?OpenBot test database}"
# Checked before anything connects: a socket-style URL (no host) is refused rather than letting
# psql or the driver fall back to a default server. Query options (sslmode) carry over.
base="${MEMORY_INTEGRATION_ADMIN_URL%%\?*}"; query=""
[[ "$MEMORY_INTEGRATION_ADMIN_URL" == *\?* ]] && query="?${MEMORY_INTEGRATION_ADMIN_URL#*\?}"
authority="${base#*://}"; authority="${authority%%/*}"; hostport="${authority##*@}"
# The admin URL's server with another database path (the query options carry over).
admin_base="${base%%://*}://$authority"; admin_query="$query"
[[ -n "$hostport" && "$hostport" != /* ]] \
  || { echo "MEMORY_INTEGRATION_ADMIN_URL must name a TCP host" >&2; exit 2; }
root="$(cd "$(dirname "$0")/.." && pwd)"
service="$root/services/memory"
port="${MEMORY_INTEGRATION_PORT:-18085}"
fake_port="${MEMORY_INTEGRATION_FAKE_PORT:-18086}"
psql="${MEMORY_PSQL:-psql}"
model="${MEMORY_MODEL_DIR:-$service/assets/model}"
work="$(mktemp -d)"
suffix="$(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')"
database="memory_live_$suffix"
role="memory_live_$suffix"
probe="openbot_probe_$suffix"
probe_table="memory_isolation_probe_$suffix"
openbot_db="${TEST_DATABASE_URL%%\?*}"; openbot_db="${openbot_db##*/}"
password="$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')"
serve_pid=""
fake_pid=""

cleanup() {
  status=$?
  # Cleanup must finish whatever fails here (a killed child's `wait` is non-zero).
  set +e
  [[ -n "$serve_pid" ]] && kill "$serve_pid" 2>/dev/null && wait "$serve_pid" 2>/dev/null
  [[ -n "$fake_pid" ]] && kill "$fake_pid" 2>/dev/null && wait "$fake_pid" 2>/dev/null
  "$psql" "$MEMORY_INTEGRATION_ADMIN_URL" -X -q -v ON_ERROR_STOP=1 \
    -c "DROP DATABASE IF EXISTS \"$database\" WITH (FORCE)" -c "DROP ROLE IF EXISTS \"$role\"" \
    -c "DROP ROLE IF EXISTS \"$probe\"" \
    >/dev/null 2>&1 || echo "warning: could not drop $database/$role/$probe" >&2
  "$psql" "$admin_base/$openbot_db$admin_query" -X -q -c "DROP TABLE IF EXISTS public.\"$probe_table\"" \
    >/dev/null 2>&1 || echo "warning: could not drop the probe table $probe_table" >&2
  if [[ $status -ne 0 && -f "$work/serve.log" ]]; then
    echo "--- service log (last 40 lines) ---" >&2
    tail -n 40 "$work/serve.log" >&2
  fi
  rm -rf "$work"
  exit "$status"
}
trap cleanup EXIT

echo "building the memory service in $service"
(cd "$service" && cargo build --locked --bin memory-service)
binary="$service/target/debug/memory-service"

MEMORY_PSQL="$psql" MEMORY_PROVISION_ADMIN_URL="$MEMORY_INTEGRATION_ADMIN_URL" \
  MEMORY_PROVISION_DATABASE="$database" MEMORY_PROVISION_ROLE="$role" \
  MEMORY_PROVISION_PASSWORD="$password" "$service/scripts/provision-db.sh"

# The owning role's URL on the same server (never printed).
export MEMORY_DATABASE_URL="postgres://$role:$password@$hostport/$database$query"

# 06B.3: the two databases are isolated by role, both ways. The service's role cannot read a table
# in OpenBot's database (a throwaway table created here, so the check does not depend on that
# database being migrated yet), and a non-superuser OpenBot-side role cannot connect to the
# service database.
"$psql" "$admin_base/$openbot_db$admin_query" -X -q -v ON_ERROR_STOP=1 \
  -c "CREATE TABLE public.\"$probe_table\" (id integer)" >/dev/null
if "$psql" "postgres://$role:$password@$hostport/$openbot_db$query" -X -q -tA \
    -c "SELECT count(*) FROM public.\"$probe_table\"" >"$work/isolation.log" 2>&1; then
  echo "isolation: the service role read OpenBot's tables" >&2; exit 1
fi
grep -Eq "permission denied" "$work/isolation.log" \
  || { echo "isolation: unexpected answer for the service role" >&2; cat "$work/isolation.log" >&2; exit 1; }
probe_password="$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')"
"$psql" "$MEMORY_INTEGRATION_ADMIN_URL" -X -q -v ON_ERROR_STOP=1 -v probe="$probe" -v pw="$probe_password" >/dev/null <<'SQL'
CREATE ROLE :"probe" LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD :'pw';
SQL
if "$psql" "postgres://$probe:$probe_password@$hostport/$database$query" -X -q -tA \
    -c "SELECT count(*) FROM observations" >"$work/isolation.log" 2>&1; then
  echo "isolation: another role read the service's tables" >&2; exit 1
fi
grep -Eq "permission denied" "$work/isolation.log" \
  || { echo "isolation: unexpected answer for the probe role" >&2; cat "$work/isolation.log" >&2; exit 1; }
echo "isolation: neither role can read the other's database"
export MEMORY_MODEL_DIR="$model" MEMORY_LOG=warn MEMORY_BIND_ADDR="127.0.0.1:$port"
export MEMORY_ANTHROPIC_API_KEY="live-fake-key" MEMORY_ANTHROPIC_BASE_URL="http://127.0.0.1:$fake_port"
export MEMORY_GENERATION_WORKERS=1

operator="$("$binary" admin bootstrap --label "live integration" 2>"$work/bootstrap.log" | tail -n 1)"
[[ "$operator" == msk_adm_* ]] || { echo "bootstrap printed no operator token" >&2; cat "$work/bootstrap.log" >&2; exit 1; }

python3 -I "$service/scripts/fake-messages.py" "$fake_port" "$work/messages.jsonl" &
fake_pid=$!
"$binary" serve >"$work/serve.log" 2>&1 &
serve_pid=$!
for _ in $(seq 1 240); do
  curl -fsS "http://127.0.0.1:$port/readyz" >/dev/null 2>&1 && break
  kill -0 "$serve_pid" 2>/dev/null || { echo "the service exited during startup" >&2; exit 1; }
  sleep 0.5
done
curl -fsS "http://127.0.0.1:$port/readyz" >/dev/null || { echo "the service never became ready" >&2; exit 1; }

# The operator token goes to curl on stdin (`-H @-`), never on its command line.
provisioner="$(printf 'authorization: Bearer %s\n' "$operator" \
  | curl -fsS -X POST "http://127.0.0.1:$port/admin/v1/admin-credentials/issue" \
  -H @- -H 'content-type: application/json' \
  -d '{"scope":"provisioner","label":"openbot live integration"}' \
  | python3 -I -c 'import json, sys; print(json.load(sys.stdin)["token"])')"

export MEMORY_LIVE_URL="http://127.0.0.1:$port"
export MEMORY_LIVE_PROVISIONER_TOKEN="$provisioner" MEMORY_LIVE_OPERATOR_TOKEN="$operator"
unset DATABASE_URL
cd "$root"
SQLX_OFFLINE=true cargo test --locked --lib -- --ignored --test-threads=1 \
  memory_integration_live_ memory_e2e_ | tee "$work/test.log"
grep -Eq '^test result: ok\. [1-9][0-9]* passed' "$work/test.log" \
  || { echo "the live tests did not pass" >&2; exit 1; }
for prefix in memory_integration_live_ memory_e2e_; do
  grep -Eq "^test .*::${prefix}[a-z0-9_]+ \.\.\. ok$" "$work/test.log" \
    || { echo "no $prefix test ran" >&2; exit 1; }
done
grep -q '"path": "/v1/messages"' "$work/messages.jsonl" \
  || { echo "the service never called the Messages API stand-in" >&2; exit 1; }
echo "memory integration: live tests passed against the real service"
