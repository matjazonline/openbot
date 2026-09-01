#!/usr/bin/env bash

set -euo pipefail

statistics_arguments=(
  -c shared_preload_libraries=pg_stat_statements
  -c compute_query_id=on
  -c pg_stat_statements.track=top
  -c pg_stat_statements.track_utility=off
  -c log_parameter_max_length=0
  -c log_parameter_max_length_on_error=0
)

slow_query_arguments=()
case "${DATABASE_SLOW_QUERY_LOGGING_ENABLED:-false}" in
  false)
    ;;
  true)
    slow_query_arguments=(
      -c log_min_duration_statement=200ms
    )
    ;;
  *)
    echo "DATABASE_SLOW_QUERY_LOGGING_ENABLED must be 'true' or 'false'" >&2
    exit 64
    ;;
esac

# A non-server command such as `psql --version` must retain the official image's behavior. Fly's
# process command begins with `postgres`, which is the only case that receives server settings.
if [[ "${1:-}" == "postgres" ]]; then
  set -- "$@" "${statistics_arguments[@]}" "${slow_query_arguments[@]}"
fi

exec /usr/local/bin/docker-entrypoint.sh "$@"
