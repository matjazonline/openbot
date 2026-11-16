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
    suffix="$$_$(od -An -N4 -tx4 /dev/urandom | tr -d ' ')"
    chain="MEMORY_TEST_$suffix"; test_group="obnet_$suffix"
    test_uid="$(id -u)"; test_groups="$(id -G | tr ' ' ',')"
    created_firewalls=(); group_created=false
    cleanup() {
      for fw in "${created_firewalls[@]}"; do
        sudo "$fw" -w -D OUTPUT -m owner --gid-owner "$test_gid" -j "$chain" 2>/dev/null || true
        sudo "$fw" -w -F "$chain" 2>/dev/null || true
        sudo "$fw" -w -X "$chain" 2>/dev/null || true
      done
      if [[ "$group_created" == true ]]; then sudo groupdel "$test_group" 2>/dev/null || true; fi
    }
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    sudo groupadd --system "$test_group"
    group_created=true
    test_gid="$(getent group "$test_group" | cut -d: -f3)"
    [[ "$test_gid" =~ ^[0-9]+$ ]] || { echo 'cannot resolve test group' >&2; exit 1; }
    for fw in iptables ip6tables; do
      sudo "$fw" -w -N "$chain"
      created_firewalls+=("$fw")
    done
    sudo iptables -w -A "$chain" -d 127.0.0.0/8 -j ACCEPT
    sudo iptables -w -A "$chain" -p tcp -m conntrack --ctdir ORIGINAL \
      --ctorigdst 127.0.0.1 --ctorigdstport 5432 -j ACCEPT
    sudo ip6tables -w -A "$chain" -d ::1/128 -j ACCEPT
    for fw in iptables ip6tables; do
      sudo "$fw" -w -A "$chain" -j REJECT
      sudo "$fw" -w -I OUTPUT 1 -m owner --gid-owner "$test_gid" -j "$chain"
    done
    # Kept self-contained so the extracted service needs no parent scripts or writable caches.
    guarded() {
      sudo --preserve-env env "PATH=$PATH" "HOME=$HOME" \
        setpriv --reuid "$test_uid" --regid "$test_gid" --groups "$test_groups" -- "$@"
    }
    guarded python3 -I scripts/network-isolation.py
    guarded "$@" ;;
  *) echo "network isolation requires Linux or macOS; refusing unguarded tests" >&2; exit 2 ;;
esac
