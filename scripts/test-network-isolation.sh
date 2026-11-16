#!/usr/bin/env bash
# Scope egress denial to the test command. Fetch/build dependencies before entering this phase.
# Linux rules select a dedicated command group; the CI runner keeps its network access.
# Docker's loopback-published PostgreSQL is allowed by
# its original destination (OUTPUT DNAT happens before the filter table). No public DNS is allowed.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $# -eq 0 ]]; then
    echo "usage: $0 command [arguments...]" >&2
    exit 2
fi
export CARGO_NET_OFFLINE=true
export NO_PROXY='*'
export no_proxy='*'
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy

case "$(uname -s)" in
    Linux)
        exec bash services/memory/scripts/network-isolation-linux.sh scripts/tests/network-isolation.py "$@"
        ;;
    Darwin)
        # The enclosing sandbox also covers Cargo's children and every provider/tool client.
        profile='(version 1)(allow default)(deny network*)(allow network-bind (local ip "localhost:*"))(allow network-inbound (local ip "localhost:*"))(allow network-outbound (remote ip "localhost:*"))'
        sandbox-exec -p "$profile" python3 scripts/tests/network-isolation.py
        exec sandbox-exec -p "$profile" "$@"
        ;;
    *) echo 'network isolation requires Linux or macOS; refusing unguarded tests' >&2; exit 2 ;;
esac
