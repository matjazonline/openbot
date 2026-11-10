#!/usr/bin/env bash
# Service independence gate: parent Cargo config, path/git dependencies, parent-directory
# reach and DATABASE_URL/.env use. `--probe` adds the poisoned-parent build probe.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec python3 -I scripts/boundary_check.py "$@"
