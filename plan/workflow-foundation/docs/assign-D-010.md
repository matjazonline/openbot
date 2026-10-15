# Assignment D-010 — group C: final combined 04.8 gates

Same role and settings (claude-code / claude-opus-5-5 / high).

B3 is root-accepted (REVIEW.md; manifest `c3379ee3…`). Execute group C per
BRIEF-04.8 §C and the S2 checklist "Group C integration-review checklist" in
`docs/GUIDANCE.md` (line ~405). Expected: no source edits. If any gate fails,
diagnose with measurements and return evidence to root before changing source,
bounds, scripts or CI (AGENTS.md: first failure on new hardware is calibration
evidence, not an automatic bound increase).

Runtime (task fixture, missing-DB allowance unset, counts vs `--list`):
- full library suite at stock 2 MiB (`RUST_MIN_STACK=2097152`), default parallelism;
- `scripts/lease-budget.sh` and `scripts/stack-budget.sh` unchanged, run as CI does
  (read `.github/workflows/ci.yml` for exact invocation/env);
- confirm F-004/F-005/R-008 negative controls and `prior_bounds_` are in the run.
Static/schema:
- fmt, `git diff --check`, `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`,
  strict Clippy, live `cargo sqlx prepare --check -- --all-targets`;
- fresh migration/schema/checksum verification on a **new task-owned database**
  in the same fixture container (create, migrate, capture identity/checksums,
  compare to 53-migration catalog, drop it after); do not touch other DBs;
- `graft build` at repo root; record the graph delta summary.
Bind every log to toolchain, binary SHA256, DB identity and final source manifest
`logs/groupC/source-manifest-C.json` (all changed files since `8533a79` incl.
untracked; should equal B3 unless graft/sqlx metadata changed — list any delta).

Record a "Group C" section in IMPLEMENTATION.md with the boundary→test coverage
map (BRIEF §B rows → exact test names, new and reused), logs under logs/groupC/.
Stop the long processes you started when done; leave the fixture container
running. Final response: gate table, manifest SHA256, any failures/deltas.
