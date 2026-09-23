#!/usr/bin/env bash
#
# Deploy to Fly.io, per docs/deploy.md.
#
# Usage:
#   scripts/deploy.sh                 # deploy db, then app
#   scripts/deploy.sh db              # deploy openbots-db only
#   scripts/deploy.sh app             # deploy openbots only
#   scripts/deploy.sh db app          # same as no args, explicit order
#   scripts/deploy.sh --bootstrap db  # first-time db setup, then deploy
#
# --bootstrap creates the openbots-db app, its pgdata volume, and sets
# POSTGRES_PASSWORD, before deploying; for the app target it creates the
# openbots app, allocates a dedicated IPv4 (required for inbound
# SMTP on port 25), and prompts for its secrets (DATABASE_URL, JWT_SECRET,
# SMTP_USERNAME/PASSWORD and credential-encryption keys) before deploying. It's a no-op
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
  if fly status --app openbots-db >/dev/null 2>&1; then
    echo "==> openbots-db already exists, skipping bootstrap"
    return
  fi

  local region
  region="$(awk -F'"' '/^primary_region/ {print $2; exit}' "$DB_CONFIG")"

  echo "==> Creating openbots-db"
  fly apps create openbots-db

  echo "==> Creating pgdata volume ($region, 5GB)"
  fly volumes create pgdata --app openbots-db --region "$region" --size 5 --yes

  echo "==> Setting POSTGRES_PASSWORD"
  read -rsp "Postgres password for mail_agents (leave empty to generate one): " PG_PASSWORD
  echo
  if [[ -z "$PG_PASSWORD" ]]; then
    PG_PASSWORD="$(openssl rand -base64 32)"
    echo "Generated password (save this — it will not be shown again):"
    echo "$PG_PASSWORD"
  fi
  fly secrets set POSTGRES_PASSWORD="$PG_PASSWORD" --app openbots-db
}

bootstrap_app() {
  if fly status --app openbots >/dev/null 2>&1; then
    echo "==> openbots already exists, skipping bootstrap"
    return
  fi

  echo "==> Creating openbots"
  fly apps create openbots

  echo "==> Allocating dedicated IPv4 (required for inbound SMTP on port 25)"
  fly ips allocate-v4 --app openbots

  local database_url
  if [[ -n "$PG_PASSWORD" ]]; then
    database_url="postgres://mail_agents:${PG_PASSWORD}@openbots-db.internal:5432/mail_agents"
    echo "==> Building DATABASE_URL from the POSTGRES_PASSWORD just set on openbots-db"
  else
    read -rp "DATABASE_URL (postgres://mail_agents:<POSTGRES_PASSWORD>@openbots-db.internal:5432/mail_agents): " database_url
  fi

  echo "==> Generating JWT_SECRET"
  local jwt_secret credential_key
  jwt_secret="$(openssl rand -base64 48)"
  credential_key="$(openssl rand -base64 32 | tr -d '\n')"

  local smtp_username smtp_password
  read -rp "SMTP relay username: " smtp_username
  read -rsp "SMTP relay password: " smtp_password
  echo

  echo "==> Setting secrets"
  local secrets_args=(
    "DATABASE_URL=$database_url"
    "JWT_SECRET=$jwt_secret"
    "SMTP_USERNAME=$smtp_username"
    "SMTP_PASSWORD=$smtp_password"
    "CREDENTIAL_ENCRYPTION_KEYS=1:$credential_key"
    "CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=1"
  )
  fly secrets set "${secrets_args[@]}" --app openbots

  echo "==> Before deploying, edit fly.toml: APP_DOMAIN_NAME, CORS_ALLOWED_ORIGINS, primary_region"
}

validate_app_secrets() {
  local secret_list
  if ! secret_list="$(fly secrets list --app openbots --json)"; then
    echo "Unable to inspect openbots secrets; refusing to deploy." >&2
    exit 1
  fi
  secret_list="$(printf '%s' "$secret_list" | tr -d '[:space:]')"

  local missing=()
  local required
  for required in DATABASE_URL JWT_SECRET CREDENTIAL_ENCRYPTION_KEYS CREDENTIAL_ENCRYPTION_ACTIVE_VERSION; do
    if [[ "$secret_list" != *"\"Name\":\"$required\""* && "$secret_list" != *"\"name\":\"$required\""* ]]; then
      missing+=("$required")
    fi
  done
  if [[ ${#missing[@]} -gt 0 ]]; then
    echo "openbots is missing required secret names: ${missing[*]}" >&2
    echo "Set them with 'fly secrets set' before deploying. Secret values cannot be recovered from Fly." >&2
    exit 1
  fi
}

deploy_db() {
  if [[ "$BOOTSTRAP" == true ]]; then
    bootstrap_db
  fi
  echo "==> Deploying openbots-db"
  fly deploy -c "$DB_CONFIG"
}

deploy_app() {
  if [[ "$BOOTSTRAP" == true ]]; then
    bootstrap_app
  fi

  echo "==> Verifying required production secret names"
  validate_app_secrets

  echo "==> Verifying .sqlx/ is up to date (offline build check)"
  (cd "$ROOT_DIR" && env -u DATABASE_URL SQLX_OFFLINE=true cargo check --locked) || {
    echo "Offline build check failed. Regenerate .sqlx/ and commit it:" >&2
    echo "  DATABASE_URL=\"postgres://\$(whoami)@localhost:5432/mail_agents\" cargo sqlx prepare -- --all-targets" >&2
    exit 1
  }

  echo "==> Deploying openbots"
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
