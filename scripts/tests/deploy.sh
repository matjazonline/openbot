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
if [[ "${1:-}" == "status" ]]; then
  [[ "${FLY_APP_EXISTS:-1}" == "1" ]]
  exit
fi
if [[ "${1:-}" == "secrets" && "${2:-}" == "list" ]]; then
  printf '%s\n' "$FLY_SECRET_JSON"
fi
EOF
cat >"$TEST_DIR/bin/cargo" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$TEST_DIR/bin/fly" "$TEST_DIR/bin/cargo"

export PATH="$TEST_DIR/bin:$PATH"
export FLY_TEST_LOG="$TEST_DIR/fly.log"

export FLY_APP_EXISTS=1
export FLY_SECRET_JSON='[{"Name":"DATABASE_URL"},{"Name":"JWT_SECRET"},{"Name":"CREDENTIAL_ENCRYPTION_KEYS"}]'
if "$ROOT_DIR/scripts/deploy.sh" app >"$TEST_DIR/missing.out" 2>&1; then
  echo "deploy unexpectedly accepted a missing active-version secret" >&2
  exit 1
fi
grep -q "CREDENTIAL_ENCRYPTION_ACTIVE_VERSION" "$TEST_DIR/missing.out"
if grep -q '^deploy' "$FLY_TEST_LOG"; then
  echo "deploy ran after required-secret preflight failed" >&2
  exit 1
fi

: >"$FLY_TEST_LOG"
export FLY_APP_EXISTS=0
export FLY_SECRET_JSON='[{"Name":"DATABASE_URL"},{"Name":"JWT_SECRET"},{"Name":"CREDENTIAL_ENCRYPTION_KEYS"},{"Name":"CREDENTIAL_ENCRYPTION_ACTIVE_VERSION"}]'
printf 'postgres://mail_agents:password@db/mail_agents\nrelay-user\nrelay-password\n' | \
  "$ROOT_DIR/scripts/deploy.sh" --bootstrap app >"$TEST_DIR/bootstrap.out" 2>&1
grep -q 'CREDENTIAL_ENCRYPTION_KEYS=<redacted>' "$FLY_TEST_LOG"
grep -q 'CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=1' "$FLY_TEST_LOG"
grep -q '^deploy$' "$FLY_TEST_LOG"

echo "deploy script tests passed"
