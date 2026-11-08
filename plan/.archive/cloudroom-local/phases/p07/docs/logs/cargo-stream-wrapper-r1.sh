#!/bin/bash
set -o pipefail
"$OPENBOT_REAL_CARGO" "$@" 2>&1 | tee -a "$OPENBOT_STACK_PROGRESS"
