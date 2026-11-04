#!/usr/bin/env bash
set -euo pipefail
# Adapt only GNU realpath invocation; invoke the installed snapshot script unchanged.
# Excluded path is an existing, non-symlink phase-record directory.
realpath() {
  python3 -c 'import os,sys; print(os.path.relpath(os.path.realpath(sys.argv[-1]), os.getcwd()))' "$@"
}
export -f realpath
bash /Users/mac03/dev/openbot/.agents/skills/implement-astra-reviewed-2-2/scripts/snapshot.sh plan/fix-app/phases/p03/docs
