#!/usr/bin/env bash
# Create the service's own role and database, and pgvector (a superuser-only extension).
#   MEMORY_PROVISION_ADMIN_URL   superuser URL on the target server (required)
#   MEMORY_PROVISION_DATABASE    database name (required)
#   MEMORY_PROVISION_ROLE        owning LOGIN role (required)
#   MEMORY_PROVISION_PASSWORD    role password (required; never echoed)
#   MEMORY_PSQL                  psql binary (default: psql)
set -euo pipefail
: "${MEMORY_PROVISION_ADMIN_URL:?}" "${MEMORY_PROVISION_DATABASE:?}" "${MEMORY_PROVISION_ROLE:?}" "${MEMORY_PROVISION_PASSWORD:?}"
psql="${MEMORY_PSQL:-psql}"
ident='^[a-z_][a-z0-9_]{0,62}$'
[[ "$MEMORY_PROVISION_DATABASE" =~ $ident && "$MEMORY_PROVISION_ROLE" =~ $ident ]] || { echo "names must match $ident" >&2; exit 2; }
"$psql" "$MEMORY_PROVISION_ADMIN_URL" -X -q -v ON_ERROR_STOP=1 -v role="$MEMORY_PROVISION_ROLE" -v db="$MEMORY_PROVISION_DATABASE" -v pw="$MEMORY_PROVISION_PASSWORD" <<'SQL'
CREATE ROLE :"role" LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD :'pw';
CREATE DATABASE :"db" OWNER :"role";
REVOKE CONNECT ON DATABASE :"db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"db" TO :"role";
SQL
# Same server, the new database: replace the (optional) database path, keep the query.
base="${MEMORY_PROVISION_ADMIN_URL%%\?*}"; query=""
[[ "$MEMORY_PROVISION_ADMIN_URL" == *\?* ]] && query="?${MEMORY_PROVISION_ADMIN_URL#*\?}"
scheme="${base%%://*}"; authority="${base#*://}"; authority="${authority%%/*}"
"$psql" "${scheme}://${authority}/${MEMORY_PROVISION_DATABASE}${query}" -X -q -v ON_ERROR_STOP=1 -c "CREATE EXTENSION vector"
echo "provisioned database $MEMORY_PROVISION_DATABASE owned by $MEMORY_PROVISION_ROLE"
