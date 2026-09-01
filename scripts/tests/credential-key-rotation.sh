#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf "$TEST_DIR"' EXIT

mkdir -p "$TEST_DIR/bin"
cat >"$TEST_DIR/bin/fly" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
rendered=()
for arg in "$@"; do
  case "$arg" in
    CREDENTIAL_ENCRYPTION_KEYS=*) rendered+=("CREDENTIAL_ENCRYPTION_KEYS=<redacted>") ;;
    *) rendered+=("$arg") ;;
  esac
done
printf '%s\n' "${rendered[*]}" >>"$FLY_TEST_LOG"
if [[ "${1:-}" == "machine" && "${2:-}" == "list" ]]; then
  if [[ "${FLY_FAIL_CHECKS:-0}" == "1" ]]; then
    printf '%s\n' '[{"state":"started","host_status":"ok","image_ref":{"digest":"sha256:release"},"checks":[{"status":"critical"}]}]'
  else
    printf '%s\n' '[{"state":"started","host_status":"ok","image_ref":{"digest":"sha256:release"},"checks":[{"status":"passing"}]}]'
  fi
fi
EOF
chmod +x "$TEST_DIR/bin/fly"

line_of() {
  local pattern="$1"
  awk -v pattern="$pattern" 'index($0, pattern) { print NR; exit }' "$FLY_TEST_LOG"
}

assert_before() {
  local first second
  first="$(line_of "$1")"
  second="$(line_of "$2")"
  [[ -n "$first" && -n "$second" && "$first" -lt "$second" ]]
}

export PATH="$TEST_DIR/bin:$PATH"
export FLY_TEST_LOG="$TEST_DIR/fly.log"
export CREDENTIAL_ENCRYPTION_KEYS="1:old-key,2:new-key"

"$ROOT_DIR/scripts/credential-key-rotation.sh" 1 2 >"$TEST_DIR/success.out"
assert_before "credentials status --require-version 1" "CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=1"
assert_before "CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=1" "CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=2"
assert_before "CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=2" "credentials rotate"
assert_before "credentials rotate" "credentials status --require-version 2"

: >"$FLY_TEST_LOG"
if FLY_FAIL_CHECKS=1 "$ROOT_DIR/scripts/credential-key-rotation.sh" 1 2 >"$TEST_DIR/failure.out" 2>&1; then
  echo "rotation unexpectedly continued after rollout verification failed" >&2
  exit 1
fi
if awk 'index($0, "CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=2") || index($0, "credentials rotate") { found=1 } END { exit !found }' "$FLY_TEST_LOG"; then
  echo "activation or convergence ran after distribution verification failed" >&2
  exit 1
fi

: >"$FLY_TEST_LOG"
export RETAINED_CREDENTIAL_ENCRYPTION_KEYS="2:new-key"
"$ROOT_DIR/scripts/credential-key-rotation.sh" 1 2 --retire --backup-retention-confirmed >"$TEST_DIR/retire.out"
retirement_secret_line="$(awk 'index($0, "secrets set") { print NR; exit }' "$FLY_TEST_LOG")"
status_lines="$(awk 'index($0, "credentials status --require-version 2") { print NR }' "$FLY_TEST_LOG")"
pre_retirement_status="$(printf '%s\n' "$status_lines" | sed -n '1p')"
post_retirement_status="$(printf '%s\n' "$status_lines" | sed -n '2p')"
[[ -n "$retirement_secret_line" && "$pre_retirement_status" -lt "$retirement_secret_line" ]]
[[ -n "$post_retirement_status" && "$retirement_secret_line" -lt "$post_retirement_status" ]]
if grep -q 'CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=1\|credentials rotate' "$FLY_TEST_LOG"; then
  echo "retirement unexpectedly repeated distribution or convergence" >&2
  exit 1
fi

echo "credential key rollout script tests passed"
