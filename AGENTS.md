These rules apply across the repository. More specific guides under `src/` add source-level and
subsystem rules.

# Keep builds and deployment secure and reproducible

Run the production image as a non-root user unless a narrowly documented capability is required
for a privileged port. Prefer capability-based port binding or an outer proxy over running the
whole application as root.

Pin git dependencies by revision or tag in addition to committing the lockfile. Production
documentation and examples must name configuration keys the application actually reads and must
not promise validation that startup does not perform.

CI must exercise formatting, offline compilation, migrations, and the database-backed test suite.
When adding or changing a concurrency protocol, include a test with competing claimants rather
than relying only on sequential mocks.

# A raised limit must leave something that fails when it is approached again

Raising a bound to clear a failure also removes the signal that the bound was being approached, and
the next report of it is a crash rather than a red build. So when you raise one, say what now
fails early instead, and keep that in CI.

The worked example is stack size. `.cargo/config.toml` gives test threads 16 MiB so the agent chain
cannot overflow them — which on its own would let that chain grow twelvefold unnoticed, and it had
already grown silently once. `scripts/stack-budget.sh` re-runs the suite at the stock 2 MiB and is
the thing that actually catches it. When it fails, shrink the chain before raising
`STACK_BUDGET_KIB`, and record the reason either way. Its threshold is calibrated per platform, so
treat a first failure on new hardware as calibration, not regression.

Settings that bound the same resource for different threads move together:
`RUNTIME_THREAD_STACK_BYTES` covers the threads the process spawns, `RUST_MIN_STACK` covers the
ones libtest spawns, and neither covers the other.

<!-- graft:start -->
## Graft — repo context graph

This repo is indexed in `graft/`: small linked markdown nodes that explain each
system and carry exact file:line spans, kept in sync with the code through git.

For ANY task here — understanding how something works, finding where code lives,
or scoping a change — get context from the graph before grepping or opening
source files. Re-ask freely (it's cheap) and reuse literal identifiers you
already have (symbol, error string, file name) as the query. New to this repo?
Run `graft map` first — a token-budgeted orientation (dir clusters, hubs,
hotspots), no LLM, no key.

- Run `graft ask "<your question>" --source` → ranked nodes with the relevant
  code spans inlined (each hit's ≤8-line crux by default; `--full` for whole
  definitions when the crux isn't enough). Match the tool to the task shape:
  for understanding or editing, the top node IS the answer — cite its
  `covers:` file:line spans and edit straight from `--source`. For
  exhaustive tasks ("every occurrence / every caller of this pattern"), ranked
  results are top-N, not complete — run `graft grep "<literal>"` instead
  (exhaustive over indexed files, grouped by enclosing symbol), falling back
  to raw `grep -rn` only for unindexed files.
- `graft skeleton <file>` → every definition's signature + span, ~10× cheaper
  than reading the file; use it to skim an API surface.
- `graft callers <symbol>` gives precomputed, exact edges — who calls this.
  Add `--direction out` for what it calls, or `--depth N` to walk
  transitively for the full blast radius. For structural questions, skip
  ranking and use this directly.
- Or browse: `graft/INDEX.md` lists every node; follow the links.
- Monorepos and folders of multiple repos rank fairly across sub-projects —
  hits carry `[scope/]` labels naming which one they're from. Narrow with
  `graft ask "<task>" --in <scope>/` once you know where you're working.

If a returned span is truncated ("+N more lines"), open the file at that exact
range before finalizing. Only open source files when a node genuinely lacks a
needed detail, and then at the exact file:line the node points to — never
re-read whole files.

After big code changes, refresh the graph with `graft build` (deterministic,
no API key, $0).
<!-- graft:end -->
