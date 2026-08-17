#!/usr/bin/env bash
#
# Deploy to Fly.io, per docs/deploy.md.
#
# Usage:
#   scripts/deploy.sh                 # deploy db, then app
#   scripts/deploy.sh db              # deploy mail-agents-db only
#   scripts/deploy.sh app             # deploy mail-agents-server only
#   scripts/deploy.sh db app          # same as no args, explicit order
#   scripts/deploy.sh --bootstrap db  # first-time db setup, then deploy
#
# --bootstrap creates the mail-agents-db app, its pgdata volume, and sets
# POSTGRES_PASSWORD, before deploying; for the app target it creates the
# mail-agents-server app, allocates a dedicated IPv4 (required for inbound
# SMTP on port 25), and prompts for its secrets (DATABASE_URL, JWT_SECRET,
# SMTP_USERNAME/PASSWORD, OPENAI_API_KEY) before deploying. It's a no-op
# (skipped with a message) once the app already exists, so it's safe to
# leave on. Remember to edit fly.toml (APP_DOMAIN_NAME,
# CORS_ALLOWED_ORIGINS, primary_region) before the app's first deploy; see
# docs/deploy.md "First deploy" > "2. Application".

set -euo pipefail

usage() {
  sed -n '2,18p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DB_CONFIG="$ROOT_DIR/deploy/postgres/fly.toml"

BOOTSTRAP=false
# Captured when this run bootstraps the db, so a same-run app bootstrap can
# build DATABASE_URL without asking the password twice.
PG_PASSWORD=""
targets=()
for arg in "$@"; do
  case "$arg" in
    -h|--help)
      usage
      exit 0
      ;;
    --bootstrap) BOOTSTRAP=true ;;
    *) targets+=("$arg") ;;
  esac
done

if ! command -v fly >/dev/null 2>&1; then
  echo "flyctl not found. Install it and run 'fly auth login' first." >&2
  exit 1
fi

bootstrap_db() {
  if fly status --app mail-agents-db >/dev/null 2>&1; then
    echo "==> mail-agents-db already exists, skipping bootstrap"
    return
  fi

  local region
  region="$(awk -F'"' '/^primary_region/ {print $2; exit}' "$DB_CONFIG")"

  echo "==> Creating mail-agents-db"
  fly apps create mail-agents-db

  echo "==> Creating pgdata volume ($region, 5GB)"
  fly volumes create pgdata --app mail-agents-db --region "$region" --size 5 --yes

  echo "==> Setting POSTGRES_PASSWORD"
  read -rsp "Postgres password for mail_agents (leave empty to generate one): " PG_PASSWORD
  echo
  if [[ -z "$PG_PASSWORD" ]]; then
    PG_PASSWORD="$(openssl rand -base64 32)"
    echo "Generated password (save this — it will not be shown again):"
    echo "$PG_PASSWORD"
  fi
  fly secrets set POSTGRES_PASSWORD="$PG_PASSWORD" --app mail-agents-db
}

bootstrap_app() {
  if fly status --app mail-agents-server >/dev/null 2>&1; then
    echo "==> mail-agents-server already exists, skipping bootstrap"
    return
  fi

  echo "==> Creating mail-agents-server"
  fly apps create mail-agents-server

  echo "==> Allocating dedicated IPv4 (required for inbound SMTP on port 25)"
  fly ips allocate-v4 --app mail-agents-server

  local database_url
  if [[ -n "$PG_PASSWORD" ]]; then
    database_url="postgres://mail_agents:${PG_PASSWORD}@mail-agents-db.internal:5432/mail_agents"
    echo "==> Building DATABASE_URL from the POSTGRES_PASSWORD just set on mail-agents-db"
  else
    read -rp "DATABASE_URL (postgres://mail_agents:<POSTGRES_PASSWORD>@mail-agents-db.internal:5432/mail_agents): " database_url
  fi

  echo "==> Generating JWT_SECRET"
  local jwt_secret
  jwt_secret="$(openssl rand -base64 48)"

  local smtp_username smtp_password
  read -rp "SMTP relay username: " smtp_username
  read -rsp "SMTP relay password: " smtp_password
  echo

  local llm_api_key
  read -rsp "OPENAI_API_KEY (leave empty to set later with 'fly secrets set'): " llm_api_key
  echo

  echo "==> Setting secrets"
  local secrets_args=(
    "DATABASE_URL=$database_url"
    "JWT_SECRET=$jwt_secret"
    "SMTP_USERNAME=$smtp_username"
    "SMTP_PASSWORD=$smtp_password"
  )
  if [[ -n "$llm_api_key" ]]; then
    secrets_args+=("OPENAI_API_KEY=$llm_api_key")
  fi
  fly secrets set "${secrets_args[@]}" --app mail-agents-server

  echo "==> Before deploying, edit fly.toml: APP_DOMAIN_NAME, CORS_ALLOWED_ORIGINS, primary_region"
}

deploy_db() {
  if [[ "$BOOTSTRAP" == true ]]; then
    bootstrap_db
  fi
  echo "==> Deploying mail-agents-db"
  fly deploy -c "$DB_CONFIG"
}

deploy_app() {
  if [[ "$BOOTSTRAP" == true ]]; then
    bootstrap_app
  fi

  echo "==> Verifying .sqlx/ is up to date (offline build check)"
  (cd "$ROOT_DIR" && env -u DATABASE_URL SQLX_OFFLINE=true cargo check --locked) || {
    echo "Offline build check failed. Regenerate .sqlx/ and commit it:" >&2
    echo "  DATABASE_URL=\"postgres://\$(whoami)@localhost:5432/mail_agents\" cargo sqlx prepare -- --all-targets" >&2
    exit 1
  }

  echo "==> Deploying mail-agents-server"
  (cd "$ROOT_DIR" && fly deploy)
}

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
