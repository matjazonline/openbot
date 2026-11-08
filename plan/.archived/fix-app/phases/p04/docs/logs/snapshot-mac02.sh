#!/usr/bin/env bash
set -euo pipefail
realpath() {
  python3 -c 'import os,sys; print(os.path.relpath(os.path.realpath(sys.argv[-1]), os.getcwd()))' "$@"
}
export -f realpath
bash /Users/mac02/.codex/worktrees/b109/openbot/.agents/skills/implement-astra-reviewed-2-2/scripts/snapshot.sh plan/fix-app/phases/p04/docs
