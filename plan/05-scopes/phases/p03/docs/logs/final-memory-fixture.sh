#!/usr/bin/env bash
# Phase-owned same-server pgvector fixture for the unchanged real integration protocol.
set -euo pipefail
source /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa05-p03-live-env.sh
memory_base="${MEMORY_INTEGRATION_ADMIN_URL%%\?*}"
memory_query=""
if [[ "$MEMORY_INTEGRATION_ADMIN_URL" == *\?* ]]; then memory_query="?${MEMORY_INTEGRATION_ADMIN_URL#*\?}"; fi
memory_db="pa05_p03_live_$(python3 -c 'import uuid; print(uuid.uuid4().hex[:12])')"
psql "$MEMORY_INTEGRATION_ADMIN_URL" -X -q -v ON_ERROR_STOP=1 -c "CREATE DATABASE \"$memory_db\""
trap 'psql "$MEMORY_INTEGRATION_ADMIN_URL" -X -q -v ON_ERROR_STOP=1 -c "DROP DATABASE \"$memory_db\" WITH (FORCE)" >/dev/null' EXIT
export TEST_DATABASE_URL="${memory_base%/*}/$memory_db$memory_query"
export DATABASE_URL="$TEST_DATABASE_URL"
echo "owned same-server memory integration base: $memory_db"
read -r MEMORY_INTEGRATION_PORT MEMORY_INTEGRATION_FAKE_PORT < <(python3 - <<'PORTS'
import socket
sockets = [socket.socket(), socket.socket()]
for s in sockets: s.bind(('127.0.0.1', 0))
print(*(s.getsockname()[1] for s in sockets))
for s in sockets: s.close()
PORTS
)
export MEMORY_INTEGRATION_PORT MEMORY_INTEGRATION_FAKE_PORT
bash scripts/memory-integration.sh
