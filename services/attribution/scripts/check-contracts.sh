#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
exec "${ATTRIBUTION_CONTRACT_PYTHON:-python3}" contract/test_contracts.py "$@"
