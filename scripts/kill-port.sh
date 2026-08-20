#!/usr/bin/env bash

set -euo pipefail

PORT="${1:-3001}"

PID=$(lsof -ti :"$PORT" 2>/dev/null || true)

if [[ -n "$PID" ]]; then
  echo "Killing process $PID on port $PORT..."
  kill -9 $PID
  echo "Killed process $PID on port $PORT."
else
  echo "No process found running on port $PORT."
fi
