#!/usr/bin/env bash
set -euo pipefail
readonly owned_cluster_root=/Users/mac02/openbot-workflow-roles-tjdv6uy3
exec /opt/homebrew/opt/postgresql@18/bin/pg_ctl -D "$owned_cluster_root/pgdata"   -l "$owned_cluster_root/postgres.log"   -o "-h127.0.0.1 -p60242 -k$owned_cluster_root -cmax_connections=200" start
