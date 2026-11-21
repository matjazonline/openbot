#!/usr/bin/env bash
set -euo pipefail
# Executed by PostgreSQL only on a fresh, empty data volume. No reset or backfill.
psql --username postgres --dbname postgres --set ON_ERROR_STOP=1 \
  --set app_password="$(cat /run/secrets/database_application)" <<'SQL'
CREATE ROLE attribution LOGIN PASSWORD :'app_password' NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
CREATE DATABASE attribution OWNER attribution;
REVOKE ALL ON DATABASE attribution FROM PUBLIC;
SQL
