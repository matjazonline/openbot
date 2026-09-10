#!/usr/bin/env bash
# Exercise the real gate against disposable inner-layer source, including test-only exceptions.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/scripts" "$fixture/src/application/test_support" "$fixture/src/domain/entities" \
    "$fixture/src/adapters/protocols/email" "$fixture/migrations"
cp "$repo_root/scripts/transport-boundary-check.sh" "$fixture/scripts/"
touch "$fixture/src/domain/entities/"{message,thread,participant}.rs
for runtime in rig rig_core rig_agent; do
    for layer in application domain; do
        printf 'use %s::completion::Message;\n' "$runtime" > "$fixture/src/$layer/leak.rs"
        if bash "$fixture/scripts/transport-boundary-check.sh" > "$fixture/result" 2>&1; then
            echo "gate accepted $runtime in $layer" >&2
            exit 1
        fi
        grep -q "$layer boundary violation" "$fixture/result"
        rm "$fixture/src/$layer/leak.rs"
    done
    printf 'use %s::completion::Message;\n' "$runtime" > "$fixture/src/application/test_support/wire.rs"
    printf '#[cfg(test)]\nmod tests {\nuse %s::completion::Message;\n}\n' "$runtime" > "$fixture/src/application/inline.rs"
    bash "$fixture/scripts/transport-boundary-check.sh" > "$fixture/result" 2>&1
    # A cfg(test) on an item must not hide production imports below it.
    printf '#[cfg(test)]\nfn helper() {}\nuse %s::completion::Message;\n' "$runtime" > "$fixture/src/application/inline.rs"
    if bash "$fixture/scripts/transport-boundary-check.sh" > "$fixture/result" 2>&1; then
        echo "gate truncated production source after a test helper" >&2
        exit 1
    fi
    rm "$fixture/src/application/inline.rs"
done
echo 'runtime boundary gate regression tests passed'
