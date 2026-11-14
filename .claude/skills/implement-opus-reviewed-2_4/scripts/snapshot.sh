#!/usr/bin/env bash
# Print a reproducible digest of the working tree relative to HEAD: staged,
# unstaged, and untracked (non-ignored) content. Run from the repository root.
# Extra arguments are paths to exclude, normally the phase's record directory,
# e.g. plans/<topic>/phases/p07/docs (TASK.md, REVIEW.md, HANDOFF.md, logs/,
# agent-review-notes.md, agent-review-findings.md).
# Exclude only workflow artifacts, not deliverables.
# Portable to macOS (bash 3.2, BSD userland) as well as GNU systems.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
top=$(pwd -P)

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum; }
else
  sha256() { shasum -a 256; }
fi

# Print the repository-relative form of an exclude path. Relative paths are
# taken from the repository root; symlinks in existing ancestors are resolved.
relative_exclude() {
  local path=$1 dir rest=""
  [[ "$path" == /* ]] || path="$top/$path"
  while [[ "$path" == */ && "$path" != / ]]; do path=${path%/}; done
  dir=$path
  until [[ -d "$dir" ]]; do
    rest="/${dir##*/}$rest"
    dir=${dir%/*}
    [[ -n "$dir" ]] || dir=/
  done
  if [[ "$rest/" == */./* || "$rest/" == */../* ]]; then
    printf 'Exclude must not use . or .. below a missing directory: %s\n' "$1" >&2
    return 1
  fi
  path="$(cd "$dir" && pwd -P)$rest"
  if [[ "$path" != "$top"/?* ]]; then
    printf 'Exclude must be inside the repository: %s\n' "$1" >&2
    return 1
  fi
  printf '%s\n' "${path#"$top"/}"
}

pathspec=(.)
for path in "$@"; do
  path=$(relative_exclude "$path")
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
            printf 'symlink\0%s\0' "$(readlink -- "$file")"
          elif [[ -f "$file" ]]; then
            if [[ -x "$file" ]]; then printf 'executable\0'; else printf 'file\0'; fi
            sha256 < "$file"
          else
            printf 'Unsupported untracked file: %s\n' "$file" >&2
            exit 1
          fi
        done
  } | sha256 | cut -d' ' -f1
)

echo "base:   $base"
echo "digest: $digest"
echo "files:"
{
  git diff --cached HEAD --name-only -- "${pathspec[@]}"
  git diff --name-only -- "${pathspec[@]}"
  git ls-files --others --exclude-standard -- "${pathspec[@]}"
} | sort -u | sed 's/^/  /'
