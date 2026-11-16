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
        suffix="$$_$(od -An -N4 -tx4 /dev/urandom | tr -d ' ')"
        chain="RIG_TEST_$suffix"
        test_group="obnet_$suffix"
        test_uid="$(id -u)"
        test_groups="$(id -G | tr ' ' ',')"
        created_firewalls=()
        group_created=false
        cleanup() {
            for firewall in "${created_firewalls[@]}"; do
                sudo "$firewall" -w -D OUTPUT -m owner --gid-owner "$test_gid" -j "$chain" 2>/dev/null || true
                sudo "$firewall" -w -F "$chain" 2>/dev/null || true
                sudo "$firewall" -w -X "$chain" 2>/dev/null || true
            done
            if [[ "$group_created" == true ]]; then
                sudo groupdel "$test_group" 2>/dev/null || true
            fi
        }
        trap cleanup EXIT
        trap 'exit 130' INT
        trap 'exit 143' TERM
        sudo groupadd --system "$test_group"
        group_created=true
        test_gid="$(getent group "$test_group" | cut -d: -f3)"
        [[ "$test_gid" =~ ^[0-9]+$ ]] || { echo 'cannot resolve test group' >&2; exit 1; }
        for firewall in iptables ip6tables; do
            sudo "$firewall" -w -N "$chain"
            created_firewalls+=("$firewall")
        done
        sudo iptables -w -A "$chain" -d 127.0.0.0/8 -j ACCEPT
        sudo iptables -w -A "$chain" -p tcp -m conntrack --ctdir ORIGINAL \
            --ctorigdst 127.0.0.1 --ctorigdstport 5432 -j ACCEPT
        sudo ip6tables -w -A "$chain" -d ::1/128 -j ACCEPT
        for firewall in iptables ip6tables; do
            sudo "$firewall" -w -A "$chain" -j REJECT
            sudo "$firewall" -w -I OUTPUT 1 -m owner --gid-owner "$test_gid" -j "$chain"
        done
        # Keep the caller's UID, supplementary groups, environment and cache permissions.
        # Only the command's primary group changes, so unrelated runner sockets stay usable.
        guarded() {
            sudo --preserve-env env "PATH=$PATH" "HOME=$HOME" \
                setpriv --reuid "$test_uid" --regid "$test_gid" --groups "$test_groups" -- "$@"
        }
        guarded python3 scripts/tests/network-isolation.py
        guarded "$@"
        ;;
    Darwin)
        # The enclosing sandbox also covers Cargo's children and every provider/tool client.
        profile='(version 1)(allow default)(deny network*)(allow network-bind (local ip "localhost:*"))(allow network-inbound (local ip "localhost:*"))(allow network-outbound (remote ip "localhost:*"))'
        sandbox-exec -p "$profile" python3 scripts/tests/network-isolation.py
        exec sandbox-exec -p "$profile" "$@"
        ;;
    *) echo 'network isolation requires Linux or macOS; refusing unguarded tests' >&2; exit 2 ;;
esac
