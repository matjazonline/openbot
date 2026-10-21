#!/usr/bin/env bash
# Print a reproducible digest of the working tree relative to HEAD: staged,
# unstaged, and untracked (non-ignored) content. Run from the repository root.
# Extra arguments are paths to exclude, normally the phase's record directory,
# e.g. plans/<topic>/phases/p07/docs (TASK.md, REVIEW.md, HANDOFF.md, logs/,
# agent-review-notes.md, agent-review-findings.md).
# Exclude only workflow artifacts, not deliverables.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
excludes=("$@")
pathspec=(.)
for path in "${excludes[@]}"; do
  path=$(realpath -m --relative-to="$PWD" -- "$path")
  if [[ "$path" == . || "$path" == .. || "$path" == ../* ]]; then
    printf 'Exclude must be inside the repository: %s\n' "$path" >&2
    exit 1
  fi
  pathspec+=(":(top,exclude,literal)$path")
done

base=$(git rev-parse HEAD)
digest=$(
  {
    echo "base $base"
    git diff --no-ext-diff --no-textconv --cached HEAD --binary -- "${pathspec[@]}"
    git diff --no-ext-diff --no-textconv --binary -- "${pathspec[@]}"
    git ls-files --others --exclude-standard -z -- "${pathspec[@]}" \
      | sort -z \
      | while IFS= read -r -d '' file; do
          printf '%s\0' "$file"
          if [[ -L "$file" ]]; then
            printf 'symlink\0'
            readlink -z -- "$file"
          elif [[ -f "$file" ]]; then
            if [[ -x "$file" ]]; then printf 'executable\0'; else printf 'file\0'; fi
            sha256sum < "$file"
          else
            printf 'Unsupported untracked file: %s\n' "$file" >&2
            exit 1
          fi
        done
  } | sha256sum | cut -d' ' -f1
)

echo "base:   $base"
echo "digest: $digest"
echo "files:"
{
  git diff --cached HEAD --name-only -- "${pathspec[@]}"
  git diff --name-only -- "${pathspec[@]}"
  git ls-files --others --exclude-standard -- "${pathspec[@]}"
} | sort -u | sed 's/^/  /'
