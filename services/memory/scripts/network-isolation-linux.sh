#!/usr/bin/env bash
# Shared Linux mechanism. Callers set their working directory and pass their own preflight.
# Lives in the standalone service so its extracted gate has no parent dependency.
set -euo pipefail
[[ $# -ge 2 ]] || { echo "usage: $0 preflight.py command [args...]" >&2; exit 2; }
preflight="$1"; shift
suffix="$$_$(od -An -N4 -tx4 /dev/urandom | tr -d ' ')"
chain="OB_TEST_$suffix"; test_group="obnet_$suffix"
test_uid="$(id -u)"; test_groups="$(id -G | tr ' ' ',')"
test_gid=""; created_firewalls=(); hooked_firewalls=()
group_created=false; pending_signal=0
# Bound sudo itself as well as xtables lock contention. Cleanup has at most
# seven calls, each with a five-second deadline plus one second to force exit.
# Cleanup ignores signals in its owner, but its children must remain cancellable.
privileged() { env --default-signal=INT,TERM timeout --kill-after=1s 5s sudo -n "$@"; }
cleanup_command() {
    local result
    if privileged "$@"; then return 0; else result=$?; fi
    printf 'network isolation cleanup failed (%s): %s\n' "$result" "$*" >&2
    return 1
}
cleanup() {
    local command_status=$? cleanup_failed=0 firewall
    trap - EXIT
    trap '' INT TERM
    for firewall in "${hooked_firewalls[@]}"; do
        cleanup_command "$firewall" -w 1 -D OUTPUT -m owner --gid-owner "$test_gid" -j "$chain" || cleanup_failed=1
    done
    for firewall in "${created_firewalls[@]}"; do
        cleanup_command "$firewall" -w 1 -F "$chain" || cleanup_failed=1
        cleanup_command "$firewall" -w 1 -X "$chain" || cleanup_failed=1
    done
    if [[ "$group_created" == true ]]; then
        if [[ "$cleanup_failed" == 0 ]]; then
            cleanup_command groupdel "$test_group" || cleanup_failed=1
        else
            echo "network isolation retained group $test_group: firewall cleanup incomplete" >&2
        fi
    fi
    # Preserve command/signal failures; a successful command cannot hide failed cleanup.
    if [[ "$command_status" != 0 ]]; then exit "$command_status"; fi
    exit "$cleanup_failed"
}
check_interruption() {
    if [[ "$pending_signal" != 0 ]]; then exit "$pending_signal"; fi
}
trap cleanup EXIT
# Defer exit until successful creation has published ownership. Bash delivers a
# pending signal before the assignment following an external command can execute.
trap 'pending_signal=130' INT
trap 'pending_signal=143' TERM
privileged groupadd --system "$test_group"
group_created=true
check_interruption
test_gid="$(getent group "$test_group" | cut -d: -f3)"
[[ "$test_gid" =~ ^[0-9]+$ ]] || { echo 'cannot resolve test group' >&2; exit 1; }
for firewall in iptables ip6tables; do
    privileged "$firewall" -w 1 -N "$chain"
    created_firewalls+=("$firewall")
    check_interruption
done
privileged iptables -w 1 -A "$chain" -d 127.0.0.0/8 -j ACCEPT
privileged iptables -w 1 -A "$chain" -p tcp -m conntrack --ctdir ORIGINAL \
    --ctorigdst 127.0.0.1 --ctorigdstport 5432 -j ACCEPT
privileged ip6tables -w 1 -A "$chain" -d ::1/128 -j ACCEPT
for firewall in iptables ip6tables; do
    privileged "$firewall" -w 1 -A "$chain" -j REJECT
    privileged "$firewall" -w 1 -I OUTPUT 1 -m owner --gid-owner "$test_gid" -j "$chain"
    hooked_firewalls+=("$firewall")
    check_interruption
done
trap 'exit 130' INT
trap 'exit 143' TERM
check_interruption
# Keep the invoking UID/groups/environment and therefore checkout/cache permissions.
guarded() {
    sudo -n --preserve-env env "PATH=$PATH" "HOME=$HOME" \
        setpriv --reuid "$test_uid" --regid "$test_gid" --groups "$test_groups" -- "$@"
}
guarded python3 -I "$preflight"
guarded "$@"
