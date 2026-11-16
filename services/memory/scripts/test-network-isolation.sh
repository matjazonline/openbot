#!/usr/bin/env bash
# Deny external network access to the wrapped command; loopback (local PostgreSQL) stays open.
# Adapted from OpenBot's scripts/test-network-isolation.sh. Before the wrapped command runs,
# scripts/network-isolation.py proves the guard: loopback works, external IPv4/IPv6 is refused,
# and the PostgreSQL server named by MEMORY_TEST_DATABASE_URL (if set) is reachable.
# Linux rules select a dedicated command group, leaving the CI runner's sockets alone.
# Docker's loopback-published PostgreSQL is admitted by
# its original destination (OUTPUT DNAT happens before the filter table). No public DNS.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
[[ $# -gt 0 ]] || { echo "usage: $0 command [args...]" >&2; exit 2; }
export CARGO_NET_OFFLINE=true NO_PROXY='*' no_proxy='*'
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY http_proxy https_proxy all_proxy
case "$(uname -s)" in
  Darwin)
    # The enclosing sandbox also covers Cargo's children. It cannot be nested: run this script
    # from an unsandboxed shell.
    profile='(version 1)(allow default)(deny network*)(allow network-bind (local ip "localhost:*"))(allow network-inbound (local ip "localhost:*"))(allow network-outbound (remote ip "localhost:*"))(allow network-outbound (remote unix-socket))'
    sandbox-exec -p "$profile" python3 -I scripts/network-isolation.py
    exec sandbox-exec -p "$profile" "$@" ;;
  Linux)
        exec bash scripts/network-isolation-linux.sh scripts/network-isolation.py "$@"
        ;;
  *) echo "network isolation requires Linux or macOS; refusing unguarded tests" >&2; exit 2 ;;
esac
