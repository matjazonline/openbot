#!/usr/bin/env bash
set -euo pipefail
cd /Users/mac03/dev/openbot
# BSD realpath lacks the GNU flags used by the unchanged skill script.
# These five exclusions exist; normalize them with Python's equivalent realpath/relpath.
realpath() {
  python3 -c 'import os, sys; print(os.path.relpath(os.path.realpath(sys.argv[-1]), os.getcwd()))' "$@"
}
export -f realpath
bash .agents/skills/implement-astra-reviewed-2.1/scripts/snapshot.sh \
  plan/workflow-foundation/docs/TASK.md \
  plan/workflow-foundation/docs/REVIEW.md \
  plan/workflow-foundation/docs/agent-review-notes.md \
  plan/workflow-foundation/docs/agent-review-findings.md \
  plan/workflow-foundation/docs/logs/04.9
