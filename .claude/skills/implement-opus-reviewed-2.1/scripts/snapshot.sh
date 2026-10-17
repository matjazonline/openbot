#!/usr/bin/env bash
# Print a reproducible digest of the working tree relative to HEAD: staged,
# unstaged, and untracked (non-ignored) content. Run from the repository root.
# Extra arguments are paths to exclude, normally the plan's docs directory,
# e.g. plans/<topic>/docs (TASK.md, REVIEW.md, logs/, agent-review-notes.md,
# agent-review-findings.md). Agent tooling (.agents, .claude) is always excluded.
set -euo pipefail

excludes=(.agents .claude "$@")
pathspec=(.)
for path in "${excludes[@]}"; do
  pathspec+=(":(exclude)$path")
done

base=$(git rev-parse HEAD)
digest=$(
  {
    echo "base $base"
    git diff HEAD --binary -- "${pathspec[@]}"
    git ls-files --others --exclude-standard -z -- "${pathspec[@]}" \
      | sort -z \
      | xargs -0 -r sha256sum
  } | sha256sum | cut -d' ' -f1
)

echo "base:   $base"
echo "digest: $digest"
echo "files:"
{
  git diff HEAD --name-only -- "${pathspec[@]}"
  git ls-files --others --exclude-standard -- "${pathspec[@]}"
} | sort -u | sed 's/^/  /'
