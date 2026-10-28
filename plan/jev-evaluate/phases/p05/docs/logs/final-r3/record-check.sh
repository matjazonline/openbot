#!/usr/bin/env bash
set -uo pipefail

if [[ $# -lt 2 ]]; then
    echo 'usage: record-check.sh label command...' >&2
    exit 2
fi

label="$1"
shift
record_dir="$(cd "$(dirname "$0")" && pwd)"
log="$record_dir/$label.log"
command_text="$(printf '%q ' "$@")"
command_text="${command_text% }"

export DATABASE_URL='postgres://mac02@localhost:5432/openbot_jev_final_r3_20261028'
export TEST_DATABASE_URL="$DATABASE_URL"
export SQLX_OFFLINE=true
export RUST_TEST_THREADS=4
export CHROME_PATH='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'

started="$(date +%s)"
"$@" >"$log" 2>&1
status=$?
elapsed=$(( $(date +%s) - started ))
printf '%s\t%s\t%s\t%s\n' "$label" "$status" "$elapsed" "$command_text" >> "$record_dir/results.tsv"
printf '%s: exit=%s elapsed=%ss log=%s\n' "$label" "$status" "$elapsed" "$log"
tail -n 3 "$log"
exit "$status"
