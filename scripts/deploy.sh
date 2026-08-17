#!/usr/bin/env bash
#
# Deploy to Fly.io, per docs/deploy.md.
#
# Usage:
#   scripts/deploy.sh          # deploy db, then app
#   scripts/deploy.sh db       # deploy mail-agents-db only
#   scripts/deploy.sh app      # deploy mail-agents-server only
#   scripts/deploy.sh db app   # same as no args, explicit order

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v fly >/dev/null 2>&1; then
  echo "flyctl not found. Install it and run 'fly auth login' first." >&2
  exit 1
fi

deploy_db() {
  echo "==> Deploying mail-agents-db"
  fly deploy -c "$ROOT_DIR/deploy/postgres/fly.toml"
}

deploy_app() {
  echo "==> Deploying mail-agents-server"
  (cd "$ROOT_DIR" && fly deploy)
}

targets=("$@")
if [[ ${#targets[@]} -eq 0 ]]; then
  targets=(db app)
fi

for target in "${targets[@]}"; do
  case "$target" in
    db) deploy_db ;;
    app) deploy_app ;;
    *)
      echo "Unknown target '$target' (expected 'db' and/or 'app')" >&2
      exit 1
      ;;
  esac
done
