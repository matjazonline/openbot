#!/usr/bin/env bash
# Shared Linux mechanism. Callers set their working directory and pass their own preflight.
# Lives in the standalone service so its extracted gate has no parent dependency.
set -euo pipefail
[[ $# -ge 2 ]] || { echo "usage: $0 preflight.py command [args...]" >&2; exit 2; }
preflight="$1"; shift
suffix="$$_$(od -An -N4 -tx4 /dev/urandom | tr -d ' ')"
chain="OB_TEST_$suffix"; test_group="obnet_$suffix"
test_uid="$(id -u)"; test_groups="$(id -G | tr ' ' ',')"
test_gid=""; owned_firewalls=(); hook_intents=()
group_owned=false; pending_signal=0
# Every cleanup probe/mutation is bounded; at most eleven native calls are needed.
# The owner ignores repeated signals during cleanup; its children must not.
privileged() {
    # The inner timer has the same privilege as the operation it must terminate.
    # The outer timer also bounds noninteractive sudo; existing5s+1s stays unchanged.
    env --default-signal=INT,TERM timeout --kill-after=1s 5s \
        sudo -n env --default-signal=INT,TERM timeout --kill-after=1s 3s "$@"
}
cleanup_command() {
    local result
    if privileged "$@"; then return 0; else result=$?; fi
    printf 'network isolation cleanup failed (%s): %s\n' "$result" "$*" >&2
    return 1
}
chain_in() {
    # A successful full listing distinguishes absence from a failed named-chain lookup.
    [[ $'\n'"$1"$'\n' == *$'\n-N '"$chain"$'\n'* ]]
}
cleanup() {
    local command_status=$? cleanup_failed=0 firewall result current_rules group_error
    trap - EXIT
    trap '' INT TERM
    for firewall in "${hook_intents[@]}"; do
        if privileged "$firewall" -w 1 -C OUTPUT -m owner --gid-owner "$test_gid" -j "$chain" >/dev/null 2>&1; then
            cleanup_command "$firewall" -w 1 -D OUTPUT -m owner --gid-owner "$test_gid" -j "$chain" || cleanup_failed=1
        else
            result=$?
            if [[ "$result" != 1 ]]; then
                echo "network isolation cannot inspect owned $firewall jump ($result)" >&2
                cleanup_failed=1
            fi
        fi
    done
    for firewall in "${owned_firewalls[@]}"; do
        if current_rules=$(privileged "$firewall" -w 1 -S); then
            if chain_in "$current_rules"; then
                cleanup_command "$firewall" -w 1 -F "$chain" || cleanup_failed=1
                cleanup_command "$firewall" -w 1 -X "$chain" || cleanup_failed=1
            fi
        else
            echo "network isolation cannot inspect owned $firewall chain $chain" >&2
            cleanup_failed=1
        fi
    done
    if [[ "$group_owned" == true ]]; then
        if [[ "$cleanup_failed" == 0 ]]; then
            if group_error=$(privileged groupdel "$test_group" 2>&1); then :; else
                result=$?
                # groupdel status6 is documented absence, including acquisition that never applied.
                if [[ "$result" != 6 ]]; then
                    printf 'network isolation cleanup failed (%s): groupdel %s: %s\n' "$result" "$test_group" "$group_error" >&2
                    cleanup_failed=1
                fi
            fi
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
# Each supported caller execs a helper with a distinct live PID.
# Reject existing identities, then publish intent before an uncertain mutation.
trap 'pending_signal=130' INT
trap 'pending_signal=143' TERM
if privileged getent group "$test_group" >/dev/null; then
    echo "network isolation group identity already exists: $test_group" >&2
    exit 1
else
    result=$?
    [[ "$result" == 2 ]] || exit "$result"
fi
group_owned=true
check_interruption
privileged groupadd --system "$test_group"
check_interruption
test_gid="$(privileged getent group "$test_group" | cut -d: -f3)"
[[ "$test_gid" =~ ^[0-9]+$ ]] || { echo 'cannot resolve test group' >&2; exit 1; }
for firewall in iptables ip6tables; do
    current_rules=$(privileged "$firewall" -w 1 -S)
    if chain_in "$current_rules"; then
        echo "network isolation chain identity already exists: $firewall $chain" >&2
        exit 1
    fi
    owned_firewalls+=("$firewall")
    check_interruption
    privileged "$firewall" -w 1 -N "$chain"
    check_interruption
done
privileged iptables -w 1 -A "$chain" -d 127.0.0.0/8 -j ACCEPT
privileged iptables -w 1 -A "$chain" -p tcp -m conntrack --ctdir ORIGINAL \
    --ctorigdst 127.0.0.1 --ctorigdstport 5432 -j ACCEPT
privileged ip6tables -w 1 -A "$chain" -d ::1/128 -j ACCEPT
for firewall in iptables ip6tables; do
    privileged "$firewall" -w 1 -A "$chain" -j REJECT
    hook_intents+=("$firewall")
    check_interruption
    privileged "$firewall" -w 1 -I OUTPUT 1 -m owner --gid-owner "$test_gid" -j "$chain"
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
