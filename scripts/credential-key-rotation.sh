#!/usr/bin/env bash
# Safely roll a credential-encryption key across all Fly Machines.

set -euo pipefail

usage() {
  echo "Usage: CREDENTIAL_ENCRYPTION_KEYS='<old-and-new-ring>' $0 <old-version> <new-version>" >&2
  echo "       RETAINED_CREDENTIAL_ENCRYPTION_KEYS='<new-only-ring>' $0 <old-version> <new-version> --retire --backup-retention-confirmed" >&2
}

APP_NAME="${FLY_APP_NAME:-mail-agents-server}"
RETIRE=false
BACKUP_RETENTION_CONFIRMED=false
versions=()
for arg in "$@"; do
  case "$arg" in
    --retire) RETIRE=true ;;
    --backup-retention-confirmed) BACKUP_RETENTION_CONFIRMED=true ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) versions+=("$arg") ;;
  esac
done

if [[ ${#versions[@]} -ne 2 ]]; then
  usage
  exit 2
fi
OLD_VERSION="${versions[0]}"
NEW_VERSION="${versions[1]}"
if [[ ! "$OLD_VERSION" =~ ^[1-9][0-9]*$ || ! "$NEW_VERSION" =~ ^[1-9][0-9]*$ || "$OLD_VERSION" == "$NEW_VERSION" ]]; then
  echo "Old and new versions must be distinct positive integers." >&2
  exit 2
fi
if ! command -v fly >/dev/null 2>&1; then
  echo "flyctl not found. Install it and run 'fly auth login' first." >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required to verify every Fly Machine in the rollout." >&2
  exit 1
fi

key_ring_has_version() {
  local key_ring="$1"
  local wanted="$2"
  local entry
  local -a entries=()
  IFS=',' read -r -a entries <<< "$key_ring"
  for entry in "${entries[@]}"; do
    if [[ "${entry%%:*}" == "$wanted" && "${entry#*:}" != "$entry" && -n "${entry#*:}" ]]; then
      return 0
    fi
  done
  return 1
}

run_status() {
  local version="$1"
  fly ssh console --app "$APP_NAME" \
    --command "/app/mail_agents credentials status --require-version $version"
}

verify_rollout() {
  local machines
  machines="$(fly machine list --app "$APP_NAME" --json)"
  if ! jq -e '
      length > 0
      and ([.[].image_ref.digest] | unique | length == 1)
      and all(.[];
        .state == "started"
        and (.host_status // "ok") == "ok"
        and ((.checks // []) | length > 0)
        and all((.checks // [])[]; .status == "passing")
      )
    ' <<<"$machines" >/dev/null; then
    echo "Fly rollout verification failed: every Machine must be started, healthy, and on one image." >&2
    return 1
  fi
  fly checks list --app "$APP_NAME" >/dev/null
}

roll_out() {
  local key_ring="$1"
  local active_version="$2"
  local phase="$3"
  echo "==> $phase rollout: active credential version $active_version"
  fly secrets set --stage \
    "CREDENTIAL_ENCRYPTION_KEYS=$key_ring" \
    "CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=$active_version" \
    --app "$APP_NAME" >/dev/null
  fly deploy --app "$APP_NAME" --strategy rolling
  verify_rollout
}

if [[ "$RETIRE" == true ]]; then
  if [[ "$BACKUP_RETENTION_CONFIRMED" != true ]]; then
    echo "Refusing retirement without --backup-retention-confirmed." >&2
    exit 2
  fi
  RETAINED_KEY_RING="${RETAINED_CREDENTIAL_ENCRYPTION_KEYS:-}"
  if ! key_ring_has_version "$RETAINED_KEY_RING" "$NEW_VERSION"; then
    echo "RETAINED_CREDENTIAL_ENCRYPTION_KEYS must contain the new version." >&2
    exit 2
  fi
  if key_ring_has_version "$RETAINED_KEY_RING" "$OLD_VERSION"; then
    echo "RETAINED_CREDENTIAL_ENCRYPTION_KEYS still contains the retiring version." >&2
    exit 2
  fi

  echo "==> Pre-retirement database verification"
  run_status "$NEW_VERSION"
  roll_out "$RETAINED_KEY_RING" "$NEW_VERSION" "Retirement"
  echo "==> Post-retirement database verification"
  run_status "$NEW_VERSION"
  exit 0
fi

KEY_RING="${CREDENTIAL_ENCRYPTION_KEYS:-}"
if ! key_ring_has_version "$KEY_RING" "$OLD_VERSION" || ! key_ring_has_version "$KEY_RING" "$NEW_VERSION"; then
  echo "CREDENTIAL_ENCRYPTION_KEYS must contain non-empty entries for both requested versions." >&2
  exit 2
fi

echo "==> Preflight: database converged on version $OLD_VERSION"
run_status "$OLD_VERSION"

roll_out "$KEY_RING" "$OLD_VERSION" "Distribution"
roll_out "$KEY_RING" "$NEW_VERSION" "Activation"

echo "==> Converging stored credentials on version $NEW_VERSION"
fly ssh console --app "$APP_NAME" --command "/app/mail_agents credentials rotate"
run_status "$NEW_VERSION"

echo "==> Rotation converged. Retain version $OLD_VERSION through the documented backup/recovery window."
