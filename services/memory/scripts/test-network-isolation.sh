#!/usr/bin/env bash
# Deny external network access to the wrapped command; loopback (local PostgreSQL) stays open.
# Adapted from OpenBot's scripts/test-network-isolation.sh. Before the wrapped command runs,
# scripts/network-isolation.py proves the guard: loopback works, external IPv4/IPv6 is refused,
# and the PostgreSQL server named by MEMORY_TEST_DATABASE_URL (if set) is reachable.
# Linux rules select this user's sockets; Docker's loopback-published PostgreSQL is admitted by
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
    chain="MEMORY_TEST_$$"; uid="$(id -u)"
    cleanup() { for fw in iptables ip6tables; do sudo "$fw" -w -D OUTPUT -m owner --uid-owner "$uid" -j "$chain" 2>/dev/null || true; sudo "$fw" -w -F "$chain" 2>/dev/null || true; sudo "$fw" -w -X "$chain" 2>/dev/null || true; done; }
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    for fw in iptables ip6tables; do sudo "$fw" -w -N "$chain"; done
    sudo iptables -w -A "$chain" -d 127.0.0.0/8 -j ACCEPT
    sudo iptables -w -A "$chain" -p tcp -m conntrack --ctdir ORIGINAL \
      --ctorigdst 127.0.0.1 --ctorigdstport 5432 -j ACCEPT
    sudo ip6tables -w -A "$chain" -d ::1/128 -j ACCEPT
    for fw in iptables ip6tables; do sudo "$fw" -w -A "$chain" -j REJECT; sudo "$fw" -w -I OUTPUT 1 -m owner --uid-owner "$uid" -j "$chain"; done
    python3 -I scripts/network-isolation.py
    "$@" ;;
  *) echo "network isolation requires Linux or macOS; refusing unguarded tests" >&2; exit 2 ;;
esac
