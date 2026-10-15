# 04.8 implementation record (D)

Developer: thread for assignment [D-001](assign-D-001.md), claude-code / claude-opus-5-5 / high.
Original requirements, batch order and acceptance live in [TASK.md](TASK.md) and
[BRIEF-04.8](../BRIEF-04.8.md) (SHA256 `3f76f318…cd04`, unchanged).

## Group A — runtime/static verification (D-001, 2026-10-15)

No source edits were made. The group A artifact is unchanged: HEAD
`8533a792a195bf4e2b0d48f1c2f1bc706fc3d006`, base `5b3d7dc`. The seven files match
`source-manifest.json` SHA256 `9148f4c1…5b57` ([reconcile](logs/groupA-source-reconcile.txt)).
`git diff --name-only 5b3d7dc HEAD` touches only those seven Rust files plus non-Rust
`.agents/`/`.claude/helpers` content and `plan/`. All logs are under [logs/groupA/](logs/groupA/).

### Fixture identity (task-owned only)

| Item | Value | Log |
| --- | --- | --- |
| Container | `workflow-foundation-048-20261013`, ID `f4e50996cf2af336bd0e36e2e933b5f0a819da473945959f7024fb3d7a58dc40`; was `exited`, started 2026-10-15T10:14:01Z via escalated `newgrp docker <<'EOF' … EOF` | [pre-start](logs/groupA/fixture-pre-start.log), [start](logs/groupA/fixture-start.log) |
| Image | `postgres:18.6-bookworm`, image ID = repo digest `sha256:afc7e2d441324c0388fa80c3d24f733b4194a4eb7f47dd8ee2b08eb1a24a647c` | pre-start |
| Volume | `31aa992f4f7f…37cd9` → `/var/lib/postgresql`; port 127.0.0.1:55448 | pre-start |
| Server | system_identifier `7696100747480141867`, PostgreSQL 18.6; max_connections 300, pg_stat_statements preloaded, track=top, track_utility=off, compute_query_id=on, both log_parameter limits 0 | [identity](logs/groupA/fixture-identity.log) |
| Connections at start | only the verifying psql session (plus server background processes) | identity |
| Host containers | only this container exists on the host (`docker ps -a`) | pre-start |

Leftover inventory before cleanup: four `mail_agents_own_<uuid>` databases (17 MB each, files
stamped 2026-10-14 03:19:06Z, which is the interrupted dispatch run at the user stop). They match
the `own_database` naming (`test_support.rs:302`) inside the task-owned container. All four were
dropped. After the drop: only `postgres`, `template0/1` and `workflow048_test` remain
([inventory](logs/groupA/leftover-db-inventory.log), [cleanup](logs/groupA/leftover-db-cleanup.log)).
The shared `workflow048_test` database was not reset. Shared-pool tests are written to tolerate
foreign rows (persistence `AGENTS.md`).

### Test binary

`target/debug/deps/mail_agents-43f563ba6b6c24ac`, SHA256
`f14fb5cda77c66735354f34141597cdc9ef718038eed2901c121d9340168517e` (= recorded value).
Freshness check: `env -u RUST_MIN_STACK SQLX_OFFLINE=true cargo test --locked --offline --lib --no-run -v`.
It ran with the repo's normal 16 MiB compiler stack from `.cargo/config.toml`, reported every
unit `Fresh`, finished in 0.87 s and named the same executable with the same hash
([log](logs/groupA/test-build-freshness.log)). Toolchain: rustc/cargo/clippy 1.99.0 (b940084d7),
sqlx-cli 0.8.6. The stock 2 MiB limit applies to the test threads (`RUST_MIN_STACK=2097152`), not to
rustc (see the handoff's rustls compiler-overflow evidence).

**Binary identity changed after the runs (my error).** The A/B baseline build (below) shared the
target directory and wrote the same artifact name, replacing `f14fb5cd…`. I moved the baseline
binary aside and rebuilt group A from repo HEAD (clean worktree):
`env -u RUST_MIN_STACK SQLX_OFFLINE=true cargo test --locked --offline --lib --no-run`
([log](logs/groupA/test-rebuild-groupA.log)). The result is the same path with new SHA256
`c7491b3a5d7a69944754a1035350cc45daa1f0d321fd91b2ae8c40e1424b8953`: same source, but the debug
build is not bit-reproducible.
- Every group A runtime result in this record was produced by `f14fb5cd…`, which cargo had
  verified fresh for this source.
- `f14fb5cd…` no longer exists on disk.
- Future runs must pass `GROUPA_SHA=c7491b3a…` to the runner (its default still asserts
  `f14fb5cd…` and will refuse the new binary).

### Runtime filters at stock 2 MiB

Runner: [run-groupA.py](logs/groupA/run-groupA.py). It was SHA256 `efd6f079…3e69` for the
group A runs; later parameterized with env `GROUPA_BINARY/GROUPA_SHA/GROUPA_LABEL/GROUPA_CWD` for
the A/B run, giving `fdc4a4a7…f5bc2` with defaults and behaviour unchanged. It asserts the binary
hash and sets `DATABASE_URL=TEST_DATABASE_URL=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test`
and `RUST_MIN_STACK=2097152`. It removes `ALLOW_MISSING_DATABASE_URL`, runs `<binary> <filter>`
with default libtest parallelism, and requires `passed == <binary> <filter> --list` count, `> 0`,
0 ignored and exit 0. After each filter it records leftover own DBs and foreign client backends.
Results: [runtime-results.json](logs/groupA/runtime-results.json).

| Filter | Listed | Result | Time | Residue after (own DBs / clients) | Log |
| --- | --- | --- | --- | --- | --- |
| `workflow_action_dispatch_` (interrupted on 10-14) | 20 | **PASS** 20/20 | 37.6 s | 0/0 | [log](logs/groupA/runtime-workflow-action-dispatch.log) |
| `workflow_action_receipt_return_authority_errors_propagate_after_commit` | 1 | **PASS** | 6.8 s | 0/0 | [log](logs/groupA/runtime-workflow-action-receipt-return-authority-errors-propagate-after-commit.log) |
| `workflow_action_receipt_refused_return_releases_run_before_returning` | 1 | **PASS** | 1.8 s | 0/0 | [log](logs/groupA/runtime-workflow-action-receipt-refused-return-releases-run-before-returning.log) |
| `workflow_action_receipt` (all) | 12 | **PASS** 12/12 | 15.8 s | 0/0 | [log](logs/groupA/runtime-workflow-action-receipt.log) |
| `workflow_action_` (all action: authority, cancellation, dispatch, receipt, uncertainty, history, replay, evidence, 231 reconciliation) — run 1 | 309 | **FAIL** 308/309 | 665.9 s | 0/0 | [log](logs/groupA/runtime-workflow-action.run1.log) |
| `workflow_control_` | 29 | **PASS** 29/29 | 33.4 s | 0/0 | [log](logs/groupA/runtime-workflow-control.log) |
| `control_tests` (module; includes budget-runtime control races) | 37 | **PASS** 37/37 | 52.1 s | 1/5 * | [log](logs/groupA/runtime-control-tests.log) |
| `::controls::` (prior-bounds) | 7 | **PASS** 7/7 | 11.2 s | 1/5 * | [log](logs/groupA/runtime-controls.log) |

\* Counts taken while my isolated high-bounds diagnosis reruns (below) were running at the same
time. That was my sequencing error: those three control filters partly overlapped with them. They
still passed, under extra load. After everything stopped, residue was 0 own DBs / 0 clients
([log](logs/groupA/residue-after-runner.log)).

The prior 10-14 results remain valid for the unchanged binary and fixture:
`workflow_action_cancellation_` 2 PASS and `workflow_action_authority_` 3 PASS. Both are also
re-covered by the `workflow_action_` filter above.

#### Failure: `workflow_action_reconciliation_high_bounds_native_owners_and_supported_entry`

- Run 1 panicked at `action_reconciliation_high_bounds_native.rs:261:45`,
  `writer.enter_remote(*reservation).await.unwrap()` →
  `Err(Conflict("Workflow lifecycle revision or command conflict"))`. That is after
  `reserve_remote` succeeded in `supported_positive`. The test had been running over 60 s under
  the 309-test parallel load.
- Isolated reruns (`--exact`, same binary, same env, stock 2 MiB): **PASS** twice, 92.9 s and
  53.9 s ([1](logs/groupA/high-bounds-isolated-1.log), [2](logs/groupA/high-bounds-isolated-2.log)).
- Uncontended rerun of the whole `workflow_action_` filter (run 2, same binary `f14fb5cd…`,
  nothing else running): **FAIL 307/309**, 663.2 s, residue 0/0
  ([log](logs/groupA/runtime-workflow-action.run2.log), [stdout](logs/groupA/runner-rerun-stdout.log)).
  - Same test again, now at `action_reconciliation_high_bounds_native.rs:237:10`
    (`reserve_remote(...).unwrap()` → `Conflict`).
  - Plus `workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses`
    at `action_reconciliation_prior_bounds_tests.rs:403:27` → `Timeout("Workflow reconciliation
    verification")`.
- Diagnosis. The claims in these suites use a 3 s lease
  (`lease_tests.rs:12-14`, `LeasePolicy::new(Duration::from_secs(3))`, via `new_claim`,
  `action_reconciliation_receipt_tests.rs:253-307`).
  - Between that claim and `enter_remote`, `supported_positive` runs `clean_current`,
    `available`, two whole-table `all_tables` snapshots, `reserve_remote`, a provenance query and
    `subject_counts`.
  - `enter_remote` (`action_dispatch.rs:481-530`) returns `conflict()` when `lock_fence`
    fails (run inactive or past deadline), when `live_window` finds the lease
    expired (`lease.rs:303-316`), when the evidence predicate is false, or after group A, when
    the locked run's actor differs from the prelock hint.
  - The run's actor never changes in this test, so the actor branch cannot diverge. A lapsed 3 s
    lease under load is a fail-closed `Conflict` by design, and the file's own
    `provider_timeout` doc acknowledges "host load outran the lease" for the source phase.
    `supported_positive`, however, `unwrap()`s with no lapse guard.
  - Neither the test file nor its fixture changed in group A (last touched `8ed25b4`). The 04.7
    full suite passed with it.
  - Group A's added work before `lock_fence` is one unlocked `actor_id` SELECT plus the company
    authority lock it previously took after the run lock.
- **A/B on this host (decisive).** Baseline = `5b3d7dc` plus only group A's five explicit
  `invalid` imports. Without them, base cannot compile on Rust 1.99 (author `check-baseline.log`).
  The baseline has no lock-order change: its `action_dispatch.rs` is byte-identical to base.
  - Patch: [ab-baseline-imports.patch](logs/groupA/ab-baseline-imports.patch). Source tree:
    `/tmp/claude-1000/baseA` (disposable).
  - Built with the normal compiler stack:
    `CARGO_TARGET_DIR=/home/mh/dev/openbot/target SQLX_OFFLINE=true cargo test --locked --offline --lib --no-run`
    ([build](logs/groupA/ab-baseline-build.log); the first attempt misplaced one import under a
    `//!` comment and failed to compile, then was corrected).
  - Binary moved to `target/ab-baseline/mail_agents-baseA`, SHA256
    `aa9bfa2f96246d5753354c664240ebed320307a10879f896c0890e0b2544c351`.
  - Same runner, fixture, stock 2 MiB and filter (`GROUPA_LABEL=.ab-baseline`, cwd = baseline tree):
    **FAIL 305/307**, 826.4 s, residue 0/0
    ([log](logs/groupA/runtime-workflow-action.ab-baseline.log),
    [results](logs/groupA/runtime-results.ab-baseline.json)).
    The first launch failed before running any test (unset `$TMPDIR` outside the sandbox; [log](logs/groupA/runner-ab-baseline-stdout.attempt0-cwd-error.log)).
    - The same `high_bounds_native_owners_and_supported_entry` failed at
      `action_reconciliation_high_bounds_assertions.rs:79:9`, `assert_eq!(state["live"], true)`
      → `false`. That is a direct observation that the claim's lease had already lapsed under
      load, without any group A code.
    - Plus `workflow_action_reconciliation_runtime_mixed_invalid_then_same_genuine_result`
      (`action_reconciliation_runtime_mixed_truth_tests.rs:444:10`, `Option::unwrap()` on `None`
      from completion).
- **Conclusion.** The `workflow_action_` failures are a pre-existing, load-dependent lease/bound
  sensitivity of the heavy reconciliation tests on this Linux host. The 04.7 full-suite pass was
  recorded on the earlier macOS host, so this is the first Linux run of these suites. Group A
  neither introduces nor (on this sample) worsens the failures:
  - group A: 1 and 2 failures in two runs;
  - baseline: 2 failures in one run;
  - the failing set varies per run, and the high-bounds test fails in all three.
  - Per AGENTS.md and BRIEF §C, this is a first-failure-on-new-hardware calibration question, not
    a reason to raise a bound automatically.
  - Sample is small (3 full runs); no claim of equal failure rates.
- Not changed by D. Options such as making these tests lapse-aware, serializing or isolating the
  heavy reconciliation tests, or recalibrating fixture lease length are test or bound decisions
  outside the group A artifact. They need root/S direction.

### Static and SQLx gates

| Check | Command (env) | Result | Log |
| --- | --- | --- | --- |
| Formatting | `cargo fmt --all -- --check` | **PASS** | [fmt](logs/groupA/fmt.log) |
| Whitespace | `git diff --check` (clean worktree) and `git diff --check 5b3d7dc HEAD` (group A delta) | **PASS** both | [diff-check](logs/groupA/diff-check.log) |
| Offline compile | `SQLX_OFFLINE=true cargo check --locked --offline --all-targets` | **PASS** 2m13s; only warnings are 5 inherited Rust 1.99 `fetch_update` deprecations | [check](logs/groupA/check-offline.log) |
| Strict Clippy (CI: `cargo clippy --locked --all-targets -- -D warnings`, env `SQLX_OFFLINE=true`; run with `--offline` added) | as stated | **FAIL** — see below | [clippy](logs/groupA/clippy.log) |
| SQLx metadata | `DATABASE_URL=<task URL> SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets` | **PASS** exit 0; `.sqlx/` 45 files byte-identical before/after and to the 10-14 author manifest (no cache delta) | [check](logs/groupA/sqlx-prepare-check.log), [before](logs/groupA/sqlx-manifest-before-prepare.json), [after](logs/groupA/sqlx-manifest-after-prepare.json) |
| Migration state of task DB | `cargo sqlx migrate info` | 53/53 installed, none pending or mismatched | [info](logs/groupA/migrate-info.log) |

#### Clippy failure (inherited toolchain drift, not group A)

- 406 errors in lib test (402 in lib):
  - 401 × `clippy::double_must_use`, all originating in the `#[async_trait]` macro expansion
    (Clippy 1.99 lint; reported at `#[async_trait]` line:1/:15 across ~80 files);
  - 5 × deprecated `Atomic::fetch_update` (renamed `try_update`), e.g.
    `task_worker.rs:2014`, `runtime_metrics.rs:294`.
- The only hits in group A files are `action_dispatch.rs:24`, `:35` and
  `action_reconciliation/mod.rs:20`. All three are `#[async_trait]` lines blamed to `8ed25b4`,
  before group A. Group A introduces no Clippy diagnostic.
- CI uses `dtolnay/rust-toolchain@stable` (now 1.99), so CI would fail the same way.
- Fixing it means a crate-level `allow(clippy::double_must_use)`, an `async-trait` dependency
  change, or roughly 400 edits outside group A, plus 5 `try_update` renames. That is a
  repo-policy/dependency decision, so it is returned to root rather than applied as a routine lint
  fix.

### Group A verification status

- PASS: dispatch, receipt (operational errors, refused return, all 12), control (29 + 37 + 7),
  prior cancellation (2) and authority (3), fmt, diff-check, offline check, SQLx check, migrations.
- FAIL / blocking acceptance (both reproduce without group A; root/S decision needed):
  1. `workflow_action_` full filter: 308/309 and 307/309 for group A; 305/307 for the baseline.
     Load-dependent lease/bound failures in the heavy reconciliation tests (high-bounds every run).
  2. Strict Clippy: Rust/Clippy 1.99 `double_must_use` on `#[async_trait]` (401) plus
     `fetch_update` deprecations (5).
- No source edits; no new artifact for S recheck. Group A artifact remains HEAD `8533a79` /
  manifest `9148f4c1…`.
- Fixture left running for group B: container up, `workflow048_test` migrated 53/53, 0 own DBs,
  0 clients.
- Retained disposable A/B artifacts: `target/ab-baseline/mail_agents-baseA` (about 1 GB, gitignored),
  plus `/tmp/claude-1000/baseA` and `/tmp/claude-1000/base0` source trees. Delete when root no
  longer needs the A/B.

## D-002 — strict Clippy remedy (user decision, 2026-10-15)

Root relayed the user decision: add a crate-level `allow(clippy::double_must_use)` and rename the
five deprecated `fetch_update` calls. The `workflow_action_` load failures are excluded (S-002).
This creates a **new source artifact** on top of HEAD `8533a79` that needs an S recheck.

### Changes

Manifest: [source-manifest-D002.json](logs/groupA/source-manifest-D002.json), SHA256 `9a617cd6…01aa`.
Diff: [source-D002.diff](logs/groupA/source-D002.diff), SHA256 `826c44be…c620`. Nothing is staged,
there are no untracked project files, and the seven group A files are unchanged.

| File | Change | SHA256 |
| --- | --- | --- |
| `src/lib.rs` | `#![allow(clippy::double_must_use)]` with a comment: it fires on `#[async_trait]` expansions under Clippy 1.99; remove once async-trait stops triggering it | `325667bf…88f2` |
| `src/adapters/harness/rig/budget.rs:24` | `fetch_update` → `try_update` | `57408d91…9fc7` |
| `src/adapters/http/routes/ui_dashboard.rs:447` | same | `df31b370…1962` |
| `src/adapters/mcp/http.rs:65` | same | `48ff8d61…145a` |
| `src/application/services/runtime_metrics.rs:294` | same | `be18f9dd…5a57` |
| `src/application/services/task_worker.rs:2014` (test) | same | `e9f71c87…92a1` |

- Only one crate root needs the allow. The package has two targets, lib `src/lib.rs` and bin
  `src/main.rs`, and strict Clippy over all targets passes with the allow in `src/lib.rs` alone.
  The lib-test target shares the lib root; the bin emitted nothing.
- `try_update` semantics are identical. In the 1.99 std source
  (`library/core/src/sync/atomic.rs:3408-3419`), the deprecated integer-atomic `fetch_update` body
  is exactly `self.try_update(set_order, fetch_order, f)`, with the same argument order and the same
  `Result<T, T>` return. The only signature difference is `impl FnMut` instead of a named generic,
  which matters only with turbofish; no call site uses one. `try_update` has been stable since
  1.95.0, and the package declares no `rust-version`.
- No SQL changed, so SQLx metadata is unaffected (the group A `prepare --check` result stands).

### Checks

| Check | Command (env) | Result | Log |
| --- | --- | --- | --- |
| Strict Clippy | `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings` | **PASS**, 0 diagnostics, 2m22s | [log](logs/groupA/clippy-D002.log) |
| Formatting | `cargo fmt --all -- --check` | **PASS** | [log](logs/groupA/fmt-D002.log) |
| Whitespace | `git diff --check` | **PASS** | [log](logs/groupA/diff-check-D002.log) |
| Offline compile | `SQLX_OFFLINE=true cargo check --locked --offline --all-targets` | **PASS**, 0 warnings (the deprecations are gone), 1m52s | [log](logs/groupA/check-offline-D002.log) |
| Test build | `env -u RUST_MIN_STACK SQLX_OFFLINE=true cargo test --locked --offline --lib --no-run` | PASS; binary `target/debug/deps/mail_agents-43f563ba6b6c24ac`, SHA256 `d769bca7fdcde79d492e83ab775ee5f5e6170b0c64887b3613c80421db887972` | [log](logs/groupA/test-build-D002.log) |

Tests covering the renamed sites ran at stock 2 MiB with
`DATABASE_URL=TEST_DATABASE_URL=<task fixture>`, using the same runner with
`GROUPA_SHA=d769bca7… GROUPA_LABEL=.D002`
([results](logs/groupA/runtime-results.D002.json), [stdout](logs/groupA/runner-D002-stdout.log)):

| Filter | Site covered | Result | Residue |
| --- | --- | --- | --- |
| `adapters::harness::rig::` | `RunBudget::charge` via compile, tools and skills tests | **PASS** 52/52 | 0/0 |
| `adapters::http::routes::ui_dashboard::` | `DashboardSseConnection::drop` (`dashboard_connection_gauge_increments_and_cleans_up_on_drop`) | **PASS** 5/5 | 0/0 |
| `adapters::mcp::` | `bounded` response stream through every `post_message` | **PASS** 7/7 | 0/0 |
| `application::services::runtime_metrics::` | test persistence `probe_and_record` | **PASS** 6/6 | 0/0 |
| `bounded_task_loop_runs_up_to_its_limit_and_refills_finished_slots` | the site itself (test code) | **PASS** 1/1 | 0/0 |

Coverage limit: no test I found drives the MCP response stream past its byte limit, so the
`Err` branch of `mcp/http.rs:65` has no direct coverage. The rename cannot change it (see the
semantics note above).

### Housekeeping

The disposable A/B artifacts were deleted as instructed: `target/ab-baseline/` (the baseline
binary) and `/tmp/claude-1000/baseA` and `/tmp/claude-1000/base0`. Their logs remain under
`logs/groupA/`. The fixture is still running, with 0 own DBs and 0 clients.

### Status after D-002

- Strict Clippy: now PASS on the D-002 artifact (supersedes the group A Clippy FAIL above).
- Still open: the `workflow_action_` load-dependent failures (S-002 / root).
- S recheck is needed for the D-002 delta.

## D-003 — reconciliation test corrections under load (DEC-009..011, 2026-10-15)

Test-side only. **No production code, limit, policy or CI guard changed.** Unchanged: the
three-second fixture `policy()`, `LeasePolicy` bounds, the five-second reconciliation command
cap, the native 128/129 limits, the three-fresh-fixture ceiling (`fresh_proof`, `high_source`),
`withheld_timeout` and its negative controls, every native-rejection oracle,
`scripts/lease-budget.sh`, `scripts/stack-budget.sh` and `.github/workflows/ci.yml`
(`git diff -- scripts .github` is empty). No test was excluded, serialized or made to retry more.

### Measurements first (DEC-010/011 diagnostics)

Read-only phase probes (`lease_phase`: elapsed monotonic time, exact-owner match, DB-clock lease
and run windows) and a prior-bounds timeline were added before any restructuring. The verifier is
timestamped when entered, ready, released and returned; settlement entry and return are recorded
by a delegating `TimedSettlement` wrapper around the real adapter, with the same service and
budget and no production port. Measurement run on binary `27660a97…`
([log](logs/groupA/runtime-workflow-action.D003-measure1.log)): FAIL 305/309.

| Test | Measured cause |
| --- | --- |
| high-bounds | Lease expired cumulatively: −183 ms at `supported_before_snapshot`, 3152 ms after the claim with no renewal. Each native phase took only 0.2–0.55 s. Confirms DEC-009. |
| mixed (`missing` variant this run) | Completion began with 87 ms of lease left; `owner=false` afterwards. `live_recovery` alone took about 1.1 s. Confirms DEC-009's expiry hypothesis by direct observation. |
| prior-bounds | Deliberate fixture delay inside the verifier pause, not slow settlement. Each whole-database snapshot took 1.3–2.0 s; the competitor reached 7.2 s (budget 5 s) and the 129 lease expired before park (renewal panic). |
| **new:** `workflow_action_reconciliation_live_return_conflict_vetoes_genuine_new_result` | `assert!(release_io(...))` failed; same single-lease shape with several full snapshots. Not in DEC-009, so measured before any change (below). |

Where the snapshot cost comes from (temporary instrumentation, removed;
[logs](logs/groupA/) `D003-temp-*.log`):
- `all_tables` made 121 sequential round trips, one per public table.
- On the prior-bounds history a full snapshot is 4.8 MB, of which `workflow_action_evidence_coverage`
  is 3.8 MB (8,256 rows).
- Its cost is the locale-collated sort of each row's text (about 700 ms) versus a 21 ms scan.
- Settlement with a promptly released verifier took **23–46 ms** (isolated and under full load), so
  slow settlement and production starvation have no supporting evidence.

### Corrections

1. **`all_tables` in one statement** (`action_reconciliation_proof_tests.rs`).
   - It keeps the same catalog list, the same per-relation
     `jsonb_agg(to_jsonb(record) ORDER BY to_jsonb(record)::text)` and the same repeatable-read
     read-only snapshot, and returns an identical `Value`, in one round trip instead of about 120.
   - The old per-relation read is kept as `per_relation_tables`, the oracle for the new
     `workflow_action_reconciliation_all_tables_matches_per_relation_reads`. That test checks
     equality on a populated fixture, plus `some_tables`, `relation` and `digested_tables` against
     the full form.
   - High-bounds isolated time fell from 54–93 s to 31 s; the full filter from about 660 s to
     467–538 s.
   - New helpers: `some_tables` (named relations), `relation` (one relation) and
     `digested_tables`. The digest helper reports the named relations as row count plus SHA-256 of
     the same canonical row text, ordered with `COLLATE "C"`: a canonical, bytewise order is all a
     digest needs. Equality of two digested snapshots is equality of those rows, and everything
     else stays full rows. Plain `all_tables` keeps its original locale ordering because tests
     index rows by position.
2. **`renew_phase`** (`action_reconciliation_receipt_tests.rs`), DEC-010's genuine heartbeat.
   - It calls the production `renew_io` with `policy()` at a quiescent boundary. A lapse before or
     at renewal fails as `LEASE LAPSE scenario=… phase=…`; a refusal while the exact owner is live
     is a separate hard failure. A definite NotApplied requires the old window to still be live.
   - It asserts the run revision is unchanged.
   - It rebases earlier snapshots by exactly one field, that job's `lock_expires_at`, read back as
     committed via `to_jsonb(job)`. Any other difference still fails.
3. **High-bounds.**
   - It renews (rebasing `before`) before `invalid_exclusions`, before each of the four attacks
     and before `supported_positive`, and again immediately before `reserve_remote`, because the
     reservation freezes its own window.
   - Between reserve and entry there is now only a one-relation read of the entries table.
     After entry, the full snapshot proves reserve and entry together added exactly entry 130
     (`reservation_delta`), and the entries relation equals the read taken between them, so entry
     wrote nothing. This is equivalent to the old post-entry `all_tables == committed`.
   - Provenance, `subject_counts` (130/0/128), 128 calls and 0 effects are unchanged and all
     native oracles are kept.
   - Probes now assert liveness, so a lapse fails labelled instead of at the next assertion.
     Reserve or entry refusals report `LEASE LAPSE` or `LIVE-OWNER REFUSAL` and still fail.
4. **Mixed truth.**
   - It renews after `live_recovery` (rebasing `claimed`), before the supported dispatch
     (rebasing `claimed`) and before completion (rebasing `applied`).
   - Completion `None` fails labelled instead of on `unwrap()`.
   - Genuine completion is still required, and all boundary checks are unchanged.
5. **Prior-bounds (DEC-011).**
   - The scheduled-128 premise checks (full snapshot, availability with exclusions, read-only
     recheck) moved before the command; the command only reads until settlement.
   - Inside the pause there is only the genuine claim, renewal, 129 consumption, park
     (`park_observed_as`, the same refused-release backstop over a coverage-digested snapshot),
     barrier and the indispensable pre-settlement snapshot.
   - The competitor's two full snapshots, which were only used for six named relations plus a
     count, became reads of exactly those relations (coverage digested).
   - `only_refusal` compares the same relations as full rows, with coverage by digest. Its
     structural checks (commands, runs) still see full rows.
   - The stale 128 snapshot, the real competing claim, consumption and retirement, the required
     `BoundExceeded`, `withheld_timeout` (unchanged), the void backstop and the three-attempt
     ceiling are all retained. No returned-but-late classifier was added, because that race was
     not observed after the correction.
   - `park_observed`, used by the controls, is unchanged in behaviour: it is now
     `park_observed_as(.., &[])`.
6. **Live-return conflict test:** probes only, no behaviour change. With snapshots cheaper its
   single lease kept at least 1834 ms of headroom under full load, so no restructuring was needed.

### Verification

Final binary `target/debug/deps/mail_agents-43f563ba6b6c24ac`, SHA256
`c973c76d0cd5858ec03b3471b11b3b0ace0ca366db4d14c0dea156f8145b84a8`. Cargo reported it up to date
after the runs ([log](logs/groupA/test-binary-D003-freshness.log)).

Isolated, stock 2 MiB, `--exact`, all PASS (logs `D003-isolated-*.log`, `D003-isolated-v3-*.log`):
the equivalence test, high-bounds (30.9 s), mixed missing and mixed invalid, prior-bounds
(149.9 s; pause released at 827 ms, settlement 34 ms) and live-return conflict.

Full `workflow_action_` filter, stock 2 MiB, default parallelism, task fixture; the runner
requires passed == listed (310):

| Run | Result | Time | Lowest lease while owner (ms): high / mixed / prior / live-conflict | Prior pause release / settlement | Voids | Residue | Log |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | **PASS 310/310** | 467.3 s | 2761 / 2759 / 2463 / 1834 | 1591 / 36 ms | 0 | 0/0 | [log](logs/groupA/runtime-workflow-action.D003-full1.log) |
| 2 | **PASS 310/310** | 507.4 s | 2835 / 2728 / 2543 / 2651 | 1595 / 44 ms | 0 | 0/0 | [log](logs/groupA/runtime-workflow-action.D003-full2.log) |
| 3 | **PASS 310/310** | 538.3 s | 2707 / 2698 / 2433 / 2483 | 1600 / 44 ms | 0 | 0/0 | [log](logs/groupA/runtime-workflow-action.D003-full3.log) |

Static checks on the same source: `cargo fmt --all -- --check` **PASS**
([log](logs/groupA/fmt-D003.log)); `git diff --check` **PASS** ([log](logs/groupA/diff-check-D003.log));
`SQLX_OFFLINE=true cargo check --locked --offline --all-targets` **PASS**, 0 diagnostics
([log](logs/groupA/check-offline-D003.log)); strict Clippy
`SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings` **PASS**, 0
diagnostics ([log](logs/groupA/clippy-D003.log)). No SQL in `.sqlx` scope changed. Fixture after:
0 own DBs, 0 clients, system identifier `7696100747480141867`
([log](logs/groupA/residue-after-D003.log)).

A failed run during measurement was caused by my temporary per-relation timing loop. It ran EXPLAIN
inside the pause and pushed the 129 lease past expiry; the existing classifier correctly made the
resulting `Conflict` a hard failure ([log](logs/groupA/D003-temp-relation-measure2.log)). The loop
was removed before verification.

### Artifact

Combined manifest (every file changed since `8533a79`, i.e. D-002 plus D-003):
[source-manifest-D003.json](logs/groupA/source-manifest-D003.json), SHA256
`e2a41cc3b06aabaaefb0939de2916f94f8f657aee91d5ec403d6401643cc93cb`; diff
[source-D003.diff](logs/groupA/source-D003.diff). There are 15 files: the 6 D-002 files with
unchanged hashes and 9 test files. Nothing is staged and nothing is untracked.

### For S review

- The live-return conflict test is outside DEC-009; it got diagnostics only.
- The `all_tables` single-statement rewrite affects all 513 call sites (performance only, with an
  equivalence test).
- The coverage digest applies inside the prior-bounds pause only.
- Not run here: the full stock-2 MiB library suite, `scripts/lease-budget.sh` and
  `scripts/stack-budget.sh` (group C gates).

## D-004 — corrections for F-001..F-003 (R-004, 2026-10-15)

The corrected artifact is HEAD `8533a79` plus
[source-manifest-D004.json](logs/groupA/source-manifest-D004.json), SHA256
`64ce8860703195366dd8c14b838ac81951d040971fa7183079f457c40bd4935f`
(15 files; diff [source-D004.diff](logs/groupA/source-D004.diff) SHA256
`9bf348297edca3bee77afe5fad634a325d2dba336469db3d6504609fcf6068ef`). Nothing is staged or
untracked. Two files changed since D-003, both test-only:
`action_reconciliation_high_bounds_native.rs` (`20d3f6e6…`) and
`action_reconciliation_proof_tests.rs` (`288d2b8a…`). No limit, policy, script or CI file changed.
Test binary SHA256 `72cca654c30c76bea9c63216da8672c7e914145a8949ca943727c2d1ff852984`. A
post-Clippy rebuild of the same source reproduced the identical hash
([log](logs/groupA/test-binary-D004-freshness.log)).

### F-001 — separate all-relation reserve and entry oracles restored

- **New helpers** in `action_reconciliation_proof_tests.rs`:
  - `fingerprint(f)`: every public relation as row count plus SHA-256 of its canonical rows
    (`relation_digest`, bytewise order), from one repeatable-read snapshot.
  - `tables_and_fingerprint(f)`: the full `all_tables` rows and that fingerprint, from **one**
    repeatable-read snapshot.
  - Shared plumbing: `read_snapshot` and `relations_on`.
- **`supported_positive`:**
  - Renew immediately before the real `reserve_remote` (as in D-003), then
    `reserved = fingerprint(f)`, covering every relation, then the real `enter_remote`.
  - After entry, `(committed, entered) = tables_and_fingerprint(f)`.
  - **Entry-only no-write:** `entered == reserved`, for every public relation.
  - **Reservation-only delta:** `reservation_delta(before, committed)`. `committed` is the
    post-reservation state, because it comes from the same snapshot as `entered` and
    `entered == reserved`.
  - The window is still frozen by a renewal immediately before reserve, the entry call is real,
    and lapse or refusal labelling is unchanged.
  - The example S gave (a non-entry row changed by reservation and restored by entry) now fails
    `entered == reserved`.
- **Proving the fingerprint is faithful:** `workflow_action_reconciliation_all_tables_matches_per_relation_reads`
  now also asserts:
  - `tables_and_fingerprint` rows equal `all_tables`;
  - `fingerprint` equals the snapshot-pair fingerprint and `digested_tables` over all names;
  - every relation's row count matches.
  - After a real fixture write (`barrier`), each relation's digest differs **exactly when** its
    full rows differ, and at least one relation does differ.
- **Interval cost:** reserve plus fingerprint takes 50 ms isolated. Under full load the lowest
  lease while owner for high-bounds was 2757 / 2712 / 2644 ms.
- **Isolated runs:** PASS ([equivalence](logs/groupA/D004-isolated-workflow_action_reconciliation_all_tables_matches_per_relation_reads.log),
  [high-bounds](logs/groupA/D004-isolated-workflow_action_reconciliation_high_bounds_native_owners_and_supported_entry.log)).
  Two intermediate builds were superseded: the first lacked the sensitivity check, and the
  second's sensitivity check failed because the plain fixture has no ledger relations for
  `barrier` to write. The fix creates the ledger fixture first; the test oracle was not weakened.

### F-002 — prior-bounds negative controls on the current artifact

`prior_bounds_` lists exactly **9** tests ([list](logs/groupA/D004-prior-bounds-list.txt)): the
seven `controls::prior_bounds_*` tests, `support::prior_bounds_timeout_discriminator_rejects_prompt_release_and_wrong_errors`,
and the positive `…bounds_128_prior_consuming_129_total_then_lost_refuses`, selected through its
module path. The run used the same runner (requires passed == listed), stock 2 MiB, the task
fixture and default parallelism, on binary `72cca654…`.

**PASS 9/9, 0 ignored, residue 0/0**
([log](logs/groupA/runtime-prior-bounds.D004-prior-controls.log),
[results](logs/groupA/runtime-results.D004-prior-controls.json)). The exceptional paths really ran:
- `BACKSTOP … interrupted=lapsed_exact_fence`;
- `reserve_remote=unknown`, with the entry committed and with it not committed;
- four `release=false lapsed_exact_fence; no durable delta` backstops;
- `three_void_fixtures_are_not_a_proof` reached VOID attempts 1–3 and failed as required;
- `foreign_release_refusal_is_not_void` and `unexpected_dispatch_error_is_not_void` panicked as
  required (`should panic … ok`);
- `withheld_verifier_budget_writes_nothing` and `early_command_failure_drains_pause_waiter`
  exercised the D-003 `PausedVerifier::run` / `TimedSettlement` path, and `lapsed_observation_and_refused_release_preserve_state`
  exercised `park_observed` (now `park_observed_as(.., &[])`).

### F-003 — SQLx prepare/check after the D-003/D-004 query changes

`DATABASE_URL=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets`
(sqlx-cli 0.8.6): **PASS**, exit 0, 0 warnings or errors
([log](logs/groupA/sqlx-prepare-check-D004.log)). The `.sqlx/` cache is 45 files, byte-identical
before and after ([before](logs/groupA/sqlx-manifest-before-D004.json),
[after](logs/groupA/sqlx-manifest-after-D004.json)) and identical to the D-001 manifest, so there is
no cache delta. That is expected: the new snapshot, digest and lease-phase SQL are runtime
`sqlx::query*` strings, which produce no cache entries and are not verified by this command. They
are verified by execution instead: the equivalence test (row, subset, digest and fingerprint
assertions) and every full-filter run below execute them on the task database.
`cargo sqlx migrate info`: 53/53 installed, none pending or different
([log](logs/groupA/migrate-info-D004.log)).

### Checks on the final artifact

Full `workflow_action_` filter, stock 2 MiB, default parallelism, task fixture, binary `72cca654…`.
It ran three times because F-001 adds work in the reserve-to-entry interval.

| Run | Result | Time | Lowest lease while owner (ms): high / mixed / prior / live-conflict | Prior release / settlement | Voids/backstops | Residue | Log |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | **PASS 310/310** | 584.2 s | 2757 / 2853 / 2338 / 1924 | 2258 / 115 ms | 0 | 0/0 | [log](logs/groupA/runtime-workflow-action.D004-full1.log) |
| 2 | **PASS 310/310** | 561.1 s | 2712 / 2713 / 1824 / 2462 | 2838 / 74 ms | 0 | 0/0 | [log](logs/groupA/runtime-workflow-action.D004-full2.log) |
| 3 | **PASS 310/310** | 624.4 s | 2644 / 2685 / 2362 / 1731 | 1797 / 101 ms | 0 | 0/0 | [log](logs/groupA/runtime-workflow-action.D004-full3.log) |

These runs were slower than D-003's (467–538 s). The host load average was 9–13 during them,
from other workloads, and the F-001 fingerprint adds only about 50 ms to one test. The
prior-bounds release time was correspondingly later (1.8–2.8 s versus about 1.6 s), still well
inside the unchanged 5 s budget.

Static checks: `cargo fmt --all -- --check` **PASS** ([log](logs/groupA/fmt-D004.log));
`git diff --check` **PASS** ([log](logs/groupA/diff-check-D004.log));
`SQLX_OFFLINE=true cargo check --locked --offline --all-targets` **PASS**, 0 diagnostics
([log](logs/groupA/check-offline-D004.log)); strict Clippy
`SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings` **PASS**, 0
diagnostics ([log](logs/groupA/clippy-D004.log)). Fixture after: 0 own DBs, 0 clients
([log](logs/groupA/residue-after-D004.log)).

Findings and notes were not edited; S updates their dispositions after recheck.

## Group B1 — expansion and implementation (D-005, 2026-10-15)

Scope per [GUIDANCE.md](GUIDANCE.md) DEC-007 item 1: prepared/protected, post-reservation,
pending provider and token-only semantics, and action-bearing cancellation rollback. Decisions
used: DEC-001 coverage map, DEC-003 token versus durable, DEC-006 rollback, and DEC-008 reopen rule.
DEC-002 race rules are not exercised in B1: B1 has no concurrent contender pairs, because its
gates are sequential service seams. Original requirements are [BRIEF-04.8 §B](../BRIEF-04.8.md)
and phase 04 execution item 7.

### Guidance checked against current code (no contradiction; DEC-008 not triggered)

- **Durable cancel is real.** `CancelCommand` goes through `WorkflowRunTransitions::cancel`
  (`controls.rs:29–41`, the owner named by BRIEF §A.1). That path covers company authority,
  head `FOR UPDATE OF run`, association recheck, CAS revision, `cancel_on`, audit and the
  command receipt. It is the same path group A and 04.7 used; no test calls `cancel_on`.
- **The writes a cancellation makes, as observed:**
  - runs → `cancelled`;
  - the current attempt → `failed` with `workflow_retirement='cancel'` and `workflow.cancelled`;
  - the job → `failed`, ownership cleared;
  - one `control_cancel` run event and one control-command receipt;
  - one `workflow_action_state_witnesses` row recording the prior state;
  - a `workflow_action_reconciliations` row when a marker exists.

  Nothing else changes. The test helper `retired_once` enforces exactly this set.
- **Service sequencing** (`application/workflow/actions/dispatch.rs`):
  - a pre-cancelled token returns before reservation;
  - after `enter_remote`, the token/deadline check precedes the first poll;
  - while the provider is pending, `owns_remote` heartbeats every `lease.heartbeat()` and drops
    the future on lost ownership.

  This matches DEC-003. A cancelled fence is refused at `authorize_on`/`lock_fence` with the exact
  `Conflict("Workflow lifecycle revision or command conflict")`, before any approval or policy
  check, so protected intent is refused the same way.

### Coverage map (DEC-001 rows in B1 scope)

| Obligation | Reused exact tests (re-run, unchanged) | New B1 test |
| --- | --- | --- |
| Prepared/undispatched, including protected | `workflow_action_schema_fk_immutable_and_cancelled_new_intent_fail_closed` (SQL negative; not a CancelCommand test), `workflow_action_dispatch_stale_fences_scopes_protection_and_errors`, group A `workflow_action_cancellation_company_before_run_all_callers` | `workflow_action_cancellation_prepared_and_protected_refuse_after_command` |
| Lock competition (group A) | `workflow_action_cancellation_company_before_run_all_callers`, `workflow_action_cancellation_return_lost_owner_precedes_denied_authority` | none (not duplicated, per DEC-001) |
| Marker before entry | `workflow_action_dispatch_service_post_marker_revoke_cancel_zero_provider_polls`, `workflow_action_dispatch_post_marker_revocation_cancel_and_expiry_refuse_entry` (SQL state) | `workflow_action_cancellation_after_reservation_refuses_entry_zero_polls` |
| Work in flight, durable (DEC-003) | `workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline` (SQL mode), `workflow_control_cancel_actual_handler_future_is_dropped_by_heartbeat` | `workflow_action_cancellation_pending_provider_dropped_by_ownership_supervision` (never-applied and applied-then-pending ledger variants) |
| Token-only (DEC-003) | same pending test (token mode), `workflow_action_dispatch_delayed_first_poll_checks_monotonic_window` | `workflow_action_cancellation_token_precancelled_never_reserves`, `workflow_action_cancellation_token_after_entry_suppresses_first_poll`, `workflow_action_cancellation_token_pending_drop_keeps_ownership` |
| Rollback (DEC-006) | `workflow_control_cancel_deferred_faults_roll_back_complete_state`, `workflow_control_wait_and_retry_deferred_faults_roll_back_complete_state`, `workflow_action_receipt_deferred_rollback_returns_no_success` (converse) | `workflow_action_cancellation_rollback_keeps_prepared_work_usable`, `workflow_action_cancellation_rollback_keeps_accepted_effect_settleable` |

Out of B1 and left for B2/B3 (DEC-007): receipt/cancel orders and the observed-result token case
(DEC-002/004), ordinary cancelled duplicate/restart, both shared-completion orders (DEC-005),
SafeRepeat/ProviderIdempotency terminal refusal, and the parked/reconciled rechecks.

### New tests and their oracles

File `src/adapters/persistence/workflow/action_cancellation_boundary_tests.rs` (new), module
`cancellation_boundary_tests` under `action_receipt_tests`. All tests use `receipt_fixture`
(own migrated DB, Reconcile recovery, the real dispatch writer, `fixture_remote_effects` ledger)
and the real `ActionService`. A provider result valid for both the action contract and the step
output is used wherever completion is proven. "Whole-DB" below means `all_tables`.

- **Prepared/protected** (4 cases: local, remote, protected local, protected remote, protection
  via `fixture_action_policies.approval=true`):
  - genuine heartbeat, baseline, real cancel, `retired_once`;
  - each public service path from the original and a fresh service: exact cancelled-fence
    `Conflict` and a whole-DB snapshot unchanged;
  - 0 provider calls, 0 effects, `(markers, receipts, local effects) = (0,0,0)`;
  - prepared intent retained: `workflow_action_intents` is unchanged by `retired_once`;
  - `inert`: `complete_io` on the old fence returns `None`, `claim_io` returns `None`, no write.
- **After reservation:** `CancelAfterReservation` delegates the real `reserve_remote`, then commits
  the real cancel before returning, so the service's final entry authorization follows durable
  cancellation.
  - The service gets the exact `Conflict`, 0 polls and 0 effects.
  - The marker and reserved entry are the only ones present (ids asserted), with no receipt.
  - The unresolved marker gets its reconciliation row.
  - The whole DB equals the post-cancel snapshot.
  - A fresh service retry is refused with 0 polls, and the case is `inert`.
- **Durable pending** (DEC-003), invocation token never cancelled:
  - after the genuine first poll, real cancel (`retired_once` against the post-entry snapshot);
  - dispatch returns `Interrupted` with exactly 1 poll and the future dropped (`dropped==1`
    read at the moment `dispatch_remote` returns);
  - the whole DB equals the post-cancel snapshot: no fabricated receipt, marker and entry retained,
    reconciliation row present;
  - `fixture_remote_effects` is 0 for never-applied and 1 for applied-then-pending;
  - fresh dispatch is refused with no further poll, and the case is `inert`.
- **Token-only:**
  - Pre-cancelled token: `Interrupted`, 0 calls, whole DB unchanged (no reservation).
  - Token cancelled inside `enter_remote` after the real final authorization: `Interrupted`, no
    first poll, marker and entry present. `still_owned`: runs, executions, jobs, attempts,
    events, control commands and reconciliations are unchanged, the run is `running` and the
    job is `processing` under the same worker.
  - Pending then token: `Interrupted`, 1 poll, dropped before return, whole DB equals the
    post-entry snapshot, `still_owned`.
  - In both latter cases a live retry under the original Reconcile contract returns
    `PossibleDispatchExists` with no write and no extra poll; token cancellation grants no
    replay.
- **Rollback (DEC-006):** the fault is a deferred constraint trigger on `workflow_control_commands`
  INSERT, so it fires at COMMIT after every cancellation write. Baseline whole DB, real cancel
  returns the injected error, and the whole DB equals the baseline, including no reconciliation
  row, witness or attempt change. Then the fault is removed, and no fresh cancellation is issued.
  - Prepared: the **same** fence dispatches remote once (`Committed`, replay `Committed`,
    1 provider call, 1 effect), then `complete_io` on the same fence completes with that output.
    There is 1 attempt, no reclaim, and the run is `succeeded`.
  - Accepted: real reserve and enter, then the provider's accepted effect (1). After the
    rolled-back cancel, `finish_remote(entry)` commits the receipt, `(1,1,0)`. `complete_io`
    then completes with the receipt output, still 1 provider call, 1 effect and 1 attempt.

Test-only support change: the whole-database snapshot helpers (`all_tables`, `some_tables`,
`digested_tables`, `fingerprint`, `tables_and_fingerprint`, `relation`, `per_relation_tables`)
moved from `action_reconciliation_proof_tests.rs` to the new `action_snapshot_support.rs`,
included by `action_dispatch_tests.rs`. Before the move, the B1 module could not reach them.
- The code is byte-identical to accepted D-004 apart from the module header and
  `per_relation_tables` becoming `pub(super)`; this was verified line by line against
  `source-D004.diff` and HEAD.
- `proof_tests` re-exports `all_tables` (`pub(super) use super::all_tables;`) for its nine
  existing path imports, and its equivalence test stays there.

Two failures during development were test-expectation corrections, not product behaviour: the
cancellation's state-witness row (added to the explicit allowed delta with exact assertions), and
the terminal run state is `succeeded`, not `completed`.

### Verification

Binary `target/debug/deps/mail_agents-43f563ba6b6c24ac`, SHA256
`596408e77e589d68078de72a0e061d77465032bbd5fb0787f73f0d10db638a72`. A post-Clippy rebuild
reproduced it ([log](logs/groupB/test-binary-B1-freshness.log)). Stock 2 MiB,
`DATABASE_URL=TEST_DATABASE_URL=<task fixture>`, `ALLOW_MISSING_DATABASE_URL` unset; the runner
requires passed == `--list` ([script](logs/groupB/verify-B1.sh), [runner](logs/groupB/run-groupB.py),
[stdout](logs/groupB/verify-B1-stdout.log)).

| Check | Result | Log |
| --- | --- | --- |
| Each of 8 B1 tests, `--exact`, isolated | **PASS** 8/8 | `logs/groupB/B1-isolated-*.log` |
| `cancellation_boundary_tests` ×3 | **PASS** 8/8 each (10.3, 10.1, 10.2 s), residue 0/0 | `runtime-cancellation-boundary-tests.B1-repeat{1,2,3}.log` |
| `workflow_action_` (318 = 310 + 8) | **PASS** 318/318, 405.5 s, residue 0/0 | [log](logs/groupB/runtime-workflow-action.B1-full.log) |
| `prior_bounds_` (moved snapshot helpers) | **PASS** 9/9 | [log](logs/groupB/runtime-prior-bounds.B1-full.log) |
| `workflow_control_` (reused control/cancellation tests) | **PASS** 29/29 | [log](logs/groupB/runtime-workflow-control.B1-full.log) |
| `cargo fmt --all -- --check` | **PASS** | [log](logs/groupB/fmt-B1.log) |
| `git diff --check` | **PASS** | [log](logs/groupB/diff-check-B1.log) |
| `SQLX_OFFLINE=true cargo check --locked --offline --all-targets` | **PASS**, 0 diagnostics | [log](logs/groupB/check-offline-B1.log) |
| Strict Clippy (`SQLX_OFFLINE=true … --all-targets -- -D warnings`) | **PASS**, 0 diagnostics | [log](logs/groupB/clippy-B1.log) |
| `cargo sqlx prepare --check -- --all-targets` (live task URL) | **PASS**; `.sqlx` 45 files byte-identical before and after, and equal to D-004 | [log](logs/groupB/sqlx-prepare-check-B1.log) |

### Artifact

[source-manifest-B1.json](logs/groupB/source-manifest-B1.json), SHA256
`822434bc4593d9d78bd32f367374e07e2ee10a6d2a0608395cc04647581719ac`. It covers 19 files changed or
added since `8533a79`: the 15 accepted group A/D-004 files, of which `action_reconciliation_proof_tests.rs`
changed again because of the move, plus `action_dispatch_tests.rs` and `action_receipt_tests.rs`
(module registrations) and the 2 new files. Nothing is staged; the 2 new files are untracked and
included. Tracked diff: `source-B1-tracked.diff`.

### Open questions for S

1. The post-reservation gate is sequential (cancel commits inside the delegating writer after the
   real reservation returns). That is DEC-001's "service gate … then release", with no concurrent
   contender, so DEC-002 blocker evidence does not apply. Confirm that this is the intended form.
2. The snapshot-helper move touches an accepted group A file (pure move plus one re-export).
   Confirm that it is acceptable rather than a third local copy.
3. Applied-then-pending durable interruption leaves the accepted effect (1), no receipt and a
   reconciliation row; the marker is unresolved. Recovery of that history belongs to the
   existing reconciliation contracts (B3), not B1.

### B1 correction (F-004, R-006; D-006, 2026-10-15)

S's answers to the three B1 questions were all "yes" (R-006). Only
`action_cancellation_boundary_tests.rs` changed (`6c3e4567…`). No production code or bound changed:
the 3 s fixture lease, the frozen entry deadline, `supervision_guard` and heartbeat are untouched.

**Problem (F-004):** the durable-pending test allowed 8 s, so it would also pass if the frozen
3 s entry deadline, rather than cancellation-driven ownership refusal, ended the invocation. Its
"still pending" check also ran after the cancel, where a prompt heartbeat may already have dropped
the future.

**Correction:**
- **`Supervised`:** a delegating writer around the real adapter. It records the deadline the real
  `enter_remote` froze, and each real `owns_remote` probe's start time and result (`None` on
  error). The negative-control mode `always_owned` affirms ownership without asking the database,
  which is supervision disabled.
- **`run_pending`:** awaits the genuine first poll, then checks the **required premise before
  cancellation**: 1 poll, 0 drops, before the frozen deadline, and the real `owns_remote` returns
  true. It then commits the real CancelCommand (`retired_once`) with the invocation token never
  cancelled, and samples the RAII drop count at the instant `dispatch_remote` returns.
- **`ownership_interrupted`, the causal oracle**, requires all of:
  1. at least one real probe started after the cancel committed;
  2. every such probe returned `Some(false)`, a real refusal;
  3. the last refusal preceded the return;
  4. dispatch returned strictly before the frozen entry deadline.

  A deadline-only end fails (4) and (2). A supervision-guard timeout records no result and fails
  (1). A probe error fails (2). Probes started before the commit are ignored, because they may
  legitimately see the old state.
- **`workflow_action_cancellation_pending_provider_dropped_by_ownership_supervision`:** both ledger
  variants (never applied, and applied then hung), with the oracle plus all previous durable
  checks: `Interrupted`, 1 poll, dropped before return, whole DB equals the post-cancel snapshot,
  marker and entry retained, no receipt, reconciliation row, actual effect count, refused fresh
  dispatch, `inert`.
- **New negative control `workflow_action_cancellation_pending_oracle_rejects_deadline_only_interruption`:**
  the same live scenario with `always_owned`. It still ends `Interrupted` with the drop, but only
  at or after the frozen deadline, and the oracle **rejects** it. Observed:
  `a post-cancel probe did not refuse ownership: [(…, Some(true)), (…, Some(true))]`.
- **New `workflow_action_cancellation_pending_oracle_discriminates_interruption_causes`:** the
  oracle over constructed observations. It accepts only refusal-before-deadline, including when a
  pre-cancel affirmation precedes it, and rejects a guard timeout with no result, a probe error,
  a still-affirmed probe, deadline-only, a refusal after the return, and a refusal with return at
  the deadline.

**Verification.** Binary `de27a83b2cbfffc16c3928c75dab6620828599a2f30f59385b56d17e71dd123a`, up to
date ([log](logs/groupB/test-binary-B1r1-freshness.log)). Stock 2 MiB, task fixture, runner requires
passed == `--list` ([script](logs/groupB/verify-B1r1.sh), [stdout](logs/groupB/verify-B1r1-stdout.log)).

| Check | Result | Log |
| --- | --- | --- |
| Corrected pending test, `--exact` ×3 | **PASS** ×3 | `B1r1-isolated-pending-{1,2,3}.log` |
| Negative control, `--exact` | **PASS** (oracle rejected the deadline-only run) | [log](logs/groupB/B1r1-isolated-negative-control.log) |
| Synthetic oracle test, `--exact` | **PASS** | [log](logs/groupB/B1r1-isolated-oracle.log) |
| `cancellation_boundary_tests` (10 = 8 + 2) | **PASS** 10/10, residue 0/0 | [log](logs/groupB/runtime-cancellation-boundary-tests.B1r1.log) |
| `workflow_action_` (320) | **PASS** 320/320, 404.4 s, residue 0/0 | [log](logs/groupB/runtime-workflow-action.B1r1.log) |
| fmt / `git diff --check` | **PASS** / **PASS** | [fmt](logs/groupB/fmt-B1r1.log), [diff](logs/groupB/diff-check-B1r1.log) |
| `SQLX_OFFLINE=true cargo check --locked --offline --all-targets` | **PASS**, 0 diagnostics | [log](logs/groupB/check-offline-B1r1.log) |
| Strict Clippy | **PASS**, 0 diagnostics | [log](logs/groupB/clippy-B1r1.log) |

No SQL changed (test-only Rust), so the B1 SQLx check stands.

**Artifact:** [source-manifest-B1r1.json](logs/groupB/source-manifest-B1r1.json), SHA256
`ef2e0ecd1ef49a5ebea6c0ddb60f42a92cb92d56b1a3300575b4513c5c9aafdb`. It covers 19 files changed or
added since `8533a79`, the same set as B1, with only the boundary test file's hash changed. Nothing
is staged; the 2 new files are untracked and included. Tracked diff: `source-B1r1-tracked.diff`.

## Group B2 — expansion and implementation (D-007, 2026-10-15)

Scope per [GUIDANCE.md](GUIDANCE.md) DEC-007 item 2: both receipt/cancel orders, the
observed-result token case, the ordinary cancelled duplicate/restart, and reuse of the 04.7
conflict cases. Decisions used: DEC-002 (race and oracle rules), DEC-004 (observed truth settles
before output suppression), the DEC-001 coverage map and the DEC-008 reopen rule. Originals:
BRIEF-04.8 §B rows "Result/receipt/cancel" and "Duplicate/restart", and phase 04 item 7 and
acceptance.

### Guidance checked against current code (no contradiction; DEC-008 not triggered)

- **Receipt settlement.** `commit_once` (`action_receipts.rs:199–254`) runs inside a
  `WriteBound` attempt. It locks the run first (`SELECT id FROM workflow_runs WHERE company_id…
  FOR UPDATE`) with no company authority and no live-fence guard. It then inserts the actual
  observation and the canonical receipt (`ON CONFLICT DO NOTHING`) and commits. After that,
  `return_authorized` runs in its own read-only transaction with group A's company-before-run
  prelock. This matches DEC-002's statement that receipt and completion paths lock run first.
- **Cancellation.** `control` takes company authority, then `HEAD … FOR UPDATE OF run`
  (statement text `… FROM workflow_runs AS run …`), then `cancel_on` locks executions, under
  1000 ms lock and statement timeouts. `action_uncertainty::record` inserts a reconciliation row
  only for a marker **without** a receipt, which serves as the observed order witness.
- **Revision.** Receipt and observation insertion leave the run revision unchanged when they
  create no evidence conflict. A command built from the pre-race head is `Applied` in both orders,
  as DEC-004 expects.

### Coverage map (DEC-001 rows in B2 scope)

| Obligation | Reused exact tests (re-run, unchanged) | New B2 test (`workflow_action_cancellation_…`) |
| --- | --- | --- |
| Receipt/cancel, cancel commits first (DEC-002/004) | `workflow_action_receipt_late_cancel_and_stale_truth_without_advancement` (SQL cancel) | `receipt_race_cancel_first` |
| Receipt/cancel, receipt first (DEC-002/004) | `workflow_action_receipt_resolution_stalled_commit_against_completion` (gate precedent; completion, not cancel) | `receipt_race_receipt_first` |
| Ungated live positive (DEC-002) | — | `receipt_without_cancel_commits` |
| Observed result, then token (DEC-004) | `workflow_action_receipt_return_authority_errors_propagate_after_commit`, `workflow_action_receipt_refused_return_releases_run_before_returning` (operational errors stay errors) | `observed_result_token_settles_then_interrupts` |
| Ordinary cancelled duplicate/restart (DEC-001/004) | `workflow_action_receipt_committed_ambiguous_ack_restart_zero_io` (live restart) | extended into both race tests (`late_truth_is_final`) |
| Conflicting truth, canonical-first, append-only (04.7, reused per DEC-001/004) | `workflow_action_reconciliation_old_actual_after_new_receipt_keeps_same_and_different_truth`, `workflow_action_reconciliation_late_truth_cancelled_retains_conflicting_actual_receipt` | none (no duplicate conflict matrix) |
| Lock-order authority/return callers (group A) | `workflow_action_cancellation_company_before_run_all_callers`, `workflow_action_cancellation_return_lost_owner_precedes_denied_authority` | none |

All reused names were verified present in the current binary's `--list`.

### New tests and oracles

New file `src/adapters/persistence/workflow/action_cancellation_receipt_tests.rs`, module
`receipt_tests`, a child of B1's `cancellation_boundary_tests`. That reuses B1's
`cancel_command`, `inert`, `refused`, `still_owned`, `provider`, `heartbeat` and `interrupted`.
`accepted()` performs a real reservation, real final entry and the provider's real accepted
effect (`fixture_remote_effects` = 1), all before settlement.

- **`receipt_race_cancel_first`** (DEC-002 cancel-first):
  - A gate transaction holds only the exact **execution** row. The run probe reads **free**
    (`FOR UPDATE NOWAIT`, rolled back).
  - The actual `cancel(command)` is observed waiting at `SELECT id FROM workflow_executions`
    behind the gate PID, after acquiring company and run. The run probe then reads **held**
    (`55P03`).
  - The actual `finish_remote(entry, result)` is observed waiting at
    `SELECT id FROM workflow_runs WHERE company_id` with `pg_blocking_pids` = the **cancel PID**.
    The cancel future is polled throughout and must not complete early.
  - On gate rollback both are awaited under the hang guard: `CancelResult::Applied` and
    `Interrupted` (late truth, no caller success). The run probe reads free again and both
    backends are drained.
- **`receipt_race_receipt_first`** (DEC-002 receipt-first):
  - The lease is `gated_policy()` (renewed through `renew_io`), so the established resolution
    gate (`Gate::arm(workflow_action_receipts INSERT, Stall::Commit)`) can be released before
    the 2 s attempt bound.
  - Run probe free. The actual receipt transaction stalls **inside COMMIT** after writing the
    observation and receipt (`gate.stalled`, observed in 11–39 ms), with the receipt future
    polled. Run probe **held**.
  - The actual cancel (company already acquired) is observed waiting at `FROM workflow_runs AS run`
    with `pg_blocking_pids` = the **receipt PID**.
  - Gate release, then both awaited: `Applied` and `Interrupted` (late truth persisted, no
    caller success once cancellation had retired the fence; no separate return-authorization
    wait is observed or claimed). Run probe free, both backends drained.
- **Settled truth, identical in both orders** (`settled_truth`):
  - exactly one canonical receipt and one actual observation, both with the exact entry,
    invocation, digest and result;
  - one marker, one entry, no evidence conflict, 1 effect;
  - the run `cancelled`, the attempt retired `cancel`, no completion, route, output or successor;
  - one control command, `cancel` / `applied`.

  The order witness is `workflow_action_reconciliations` = **1** for cancel-first (the marker was
  unresolved at cancel) and **0** for receipt-first.
- **Duplicate/restart** (`late_truth_is_final`, run after both races):
  - The **identical** accepted observation is delivered again for the exact same entry.
    `redelivery` rebuilds that entry in-crate: same marker, entry and request, the frozen action
    read while live, and `prepare_provider(…, None)` for the Reconcile/unregistered invocation.
    The real `verify_entry_on` re-verifies every identity.
  - It returns `Interrupted`, and the whole DB is unchanged: no new observation, receipt, entry,
    conflict or effect.
  - A fresh service retry is refused with the exact cancelled-fence `Conflict` and 0 extra polls,
    then `inert`.
- **`receipt_without_cancel_commits`:** the same accepted settlement with no cancellation returns
  `Committed` with the provider result, and the run stays `running`. A refusal-only
  implementation therefore cannot satisfy the race oracles.
- **`observed_result_token_settles_then_interrupts`** (DEC-004): `CancelAtSettlement` asserts at
  `finish_remote` entry that the token is still live, records the validated result, cancels the
  token, then delegates to the real `finish_remote`.
  - The real `finish_remote` must return `Committed`: settlement is neither dropped nor cancelled.
    The service then returns `Interrupted`.
  - There is 1 provider call and 1 effect, and one receipt and one observation with the exact
    entry, digest and result.
  - `still_owned`: no durable cancellation; run, executions, job, attempts, events, control
    commands and reconciliations are unchanged, and the run is running under the same worker.
  - A fresh service on the live fence returns the canonical receipt (`Committed`) with no
    further call or write.

Test-only edits to accepted files:
- `action_cancellation_order_tests.rs` (group A): `blocked`, `RunLock`, `probe_run` and
  `assert_drained` became `pub(super)` so B2 reuses the calibrated race helpers. Visibility only.
- `action_cancellation_boundary_tests.rs` (B1): registers the `receipt_tests` child module
  (2 lines).

### Verification

Binary `78998780a0d13a8a965214747bce7db459116d22a4f13bff4c7cee0c206a2901`, reproduced by the
post-Clippy rebuild ([log](logs/groupB/test-binary-B2-freshness.log)). Stock 2 MiB, task fixture;
the runner requires passed == `--list` ([script](logs/groupB/verify-B2.sh),
[stdout](logs/groupB/verify-B2-stdout.log)).

| Check | Result | Log |
| --- | --- | --- |
| Each of 4 new tests, `--exact`, ×3 | **PASS** 12/12; races showed the exact blocker statements every time | `B2-isolated-*-{1,2,3}.log` |
| `cancellation_boundary_tests::receipt_tests` | **PASS** 4/4 | [log](logs/groupB/runtime-cancellation-boundary-tests-receipt-tests.B2.log) |
| `cancellation_boundary_tests` (B1 + B2) | **PASS** 14/14 | [log](logs/groupB/runtime-cancellation-boundary-tests.B2.log) |
| `workflow_action_` (324), including all reused receipt, reconciliation and group A tests | **PASS** 324/324, 408.8 s | [log](logs/groupB/runtime-workflow-action.B2.log) |
| `workflow_control_` | **PASS** 29/29 | [log](logs/groupB/runtime-workflow-control.B2.log) |
| fmt / `git diff --check` | **PASS** / **PASS** | [fmt](logs/groupB/fmt-B2.log), [diff](logs/groupB/diff-check-B2.log) |
| Offline all-target check | **PASS**, 0 diagnostics | [log](logs/groupB/check-offline-B2.log) |
| Strict Clippy | **PASS**, 0 diagnostics | [log](logs/groupB/clippy-B2.log) |
| SQLx prepare/check (live task URL) | **PASS**; cache unchanged and equal to B1 | [log](logs/groupB/sqlx-prepare-check-B2.log) |

Residue after every filter: 0 own DBs, 0 clients.

### Artifact

[source-manifest-B2.json](logs/groupB/source-manifest-B2.json), SHA256
`19abba9a05d1f619339b6a50772ab83fda7de2ee5145a36288523d5b27198a28`. It covers 21 files changed or
added since `8533a79`: accepted B1r1's 19 files, of which the B1 module and group A order tests
changed, plus the new `action_cancellation_receipt_tests.rs`. Nothing is staged; the 3 untracked
new files are included. Tracked diff: `source-B2-tracked.diff`.

### Open questions for S

1. **Identical redelivery construction.** No production path can produce a second `RemoteEntry`
   for the same entry: entry consumption is non-cloneable by design. The duplicate is therefore
   rebuilt in-crate from durable identities and re-verified by the real writer. Confirm that this
   satisfies DEC-004's "repeat identical accepted observation delivery".
2. **Receipt-first lease.** The receipt-first race uses `gated_policy()` (6 s lease, renewed
   through `renew_io`) as the established gate tests do, because a COMMIT stall plus the cancel
   observation must stay inside one attempt bound. Cancel-first uses the common 3 s `policy()`.
   No production bound changed.
3. **Return-authorization wait.** Superseded by R-008: the receipt-first `Interrupted` outcome is
   not claimed to prove a literal return-authorization wait behind cancellation, and no third
   blocker is observed or required.

### B2 correction (F-005, R-008, DEC-012; D-008, 2026-10-15)

S's answers (R-008): (1) the in-crate identical redelivery is accepted. (2) The 6 s fixture lease is
acceptable under DEC-012 once F-005 is corrected. (3) No third blocker is required. Wording that
treated `Interrupted` as proof of a return-authorization wait was corrected in the B2 section above
and in its open question 3, and the test's assert message no longer claims it. Only
`action_cancellation_receipt_tests.rs` changed (`3bb882e2…`). No production code or bound changed;
`gated_policy()` is used only by this receipt-first fixture, as DEC-012 permits.

**Problem (F-005):** receipt-first checked only `Applied`, `Interrupted` and durable truth. A
release after the 2 s primary attempt, followed by ambiguous-COMMIT **resolution**, would have
produced the same outcome and passed.

**Correction:**
- **`receipt_first(f, request, injected, late)`** is shared by the ordinary test and the control,
  and keeps every earlier B2 check:
  - free/held/free run probes;
  - the receipt stalled in COMMIT via `gate.stalled`;
  - the cancel's exact run wait (`blocked`);
  - drained backends;
  - `Applied` / `Interrupted`;
  - `settled_truth`;
  - `late_truth_is_final`.
- **Primary-attempt evidence:** the test-only recorder (`Injected::mark()`, current-thread test
  runtime) captures every bounded operation finished during the race, sliced to the race only.
  `deadline` is a conservative primary-attempt deadline: `gated_policy().attempt_bound()` from an
  instant read **before** the settlement future exists, so never later than the real attempt's
  deadline. `released` is read after `pg_advisory_unlock` returns, so never earlier than the
  actual release.
- **`ordinary_receipt`, the ordinary-race oracle:** exactly one `action.commit_remote` event with
  outcome `completed`, and `released < deadline`. It rejects `resolved_applied`,
  `resolved_adopted`, `unknown`, a `not_applied` attempt followed by a retry, a missing event, and
  a late release.
- **`workflow_action_cancellation_receipt_race_receipt_first`:** the oracle must pass.
- **Negative control `workflow_action_cancellation_receipt_race_late_release_fails_ordinary_oracle`:**
  `late = true` keeps the gate until `resolution_behind` observes the primary attempt's resolution
  transaction queued on the run behind the stalled receipt. That proves the attempt elapsed: it
  was observed at about 1984 ms.
  - Calibration detail: in this state the resolution transaction is first in the row-lock queue
    and holds the tuple lock, so the cancel's **direct** blocker is the resolution PID. The test
    observes exactly that.
  - Then cancel, release, drain. The business outcome is identical: `Applied`, `Interrupted`, the
    same `settled_truth`, and `late_truth_is_final` passes.
  - The recorder shows `action.commit_remote = resolved_applied`, and the ordinary oracle
    **rejects** it.
- **`workflow_action_cancellation_receipt_ordinary_oracle_discriminates_attempt_paths`:** the
  oracle over constructed recorder events, covering every rejected path above and the one accepted
  path.

**Verification.** Binary `b36b7b85efce7d17c9a8223fcf45e17828a7071de36d97c7a1b388eae9255a1b`, up to
date ([log](logs/groupB/test-binary-B2r1-freshness.log)). Stock 2 MiB, task fixture, runner requires
passed == `--list` ([script](logs/groupB/verify-B2r1.sh), [stdout](logs/groupB/verify-B2r1-stdout.log)).

| Check | Result | Log |
| --- | --- | --- |
| Receipt-first, `--exact` ×3 | **PASS** ×3; one `commit_remote` `completed`; released 1968 / 1976 / 1968 ms before the conservative deadline | `B2r1-isolated-receipt-first-{1,2,3}.log` |
| Late-release negative control, `--exact` ×3 | **PASS** ×3; `resolved_applied` each time and the oracle rejected it; business outcome matched | `B2r1-isolated-late-release-{1,2,3}.log` |
| Synthetic oracle test | **PASS** | [log](logs/groupB/B2r1-isolated-oracle.log) |
| `cancellation_boundary_tests` (B1 + B2 = 16) | **PASS** 16/16, residue 0/0 | [log](logs/groupB/runtime-cancellation-boundary-tests.B2r1.log) |
| `workflow_action_` (326) | **PASS** 326/326, 408.6 s, residue 0/0 | [log](logs/groupB/runtime-workflow-action.B2r1.log) |
| fmt / `git diff --check` | **PASS** / **PASS** | [fmt](logs/groupB/fmt-B2r1.log), [diff](logs/groupB/diff-check-B2r1.log) |
| Offline all-target check | **PASS**, 0 diagnostics | [log](logs/groupB/check-offline-B2r1.log) |
| Strict Clippy | **PASS**, 0 diagnostics | [log](logs/groupB/clippy-B2r1.log) |

No SQL changed (test-only Rust), so the B2 SQLx check stands. `scripts/lease-budget.sh` and its CI
invocation are unchanged (DEC-012's retained work-phase gate).

**Artifact:** [source-manifest-B2r1.json](logs/groupB/source-manifest-B2r1.json), SHA256
`1b109cc661ccf7dd6b692f06f2882e7d8152325355931bd23d1d2184951eeace`. It covers the same 21 files as
B2, with only the receipt test file's hash changed. Nothing is staged; the 3 untracked new files
are included. Tracked diff: `source-B2r1-tracked.diff`.

## Group B3 — expansion and implementation (D-009, 2026-10-15)

Developer: D2, thread `thr_2n9nrfy6ra` (claude-code / claude-opus-5-5 / high), replacing the
retired D. Scope per [GUIDANCE.md](GUIDANCE.md) DEC-007 item 3: both shared-completion orders,
SafeRepeat/ProviderIdempotency terminal refusal after cancellation, and the exact accepted
uncertainty/reconciliation rechecks, including the applied-then-hung recovery deferred from B1
(R-006 Q3). Decisions used: DEC-001 (coverage map), DEC-002 (race rules), DEC-005 (completion
winner determines result priority), DEC-012 (ordinary attempt versus ambiguity recovery, carried by
R-008), DEC-008 (reopen rule), and S2's B3 readiness note (R-010; no new decision). Originals:
BRIEF-04.8 §B rows "Completion boundary" and "Parked/reconciled", phase 04 item 7 and acceptance.

Starting artifact: HEAD `8533a79` plus accepted B2r1 (`source-manifest-B2r1.json`, SHA256
`1b109cc6…`). Before any edit all 21 hashes were re-verified against the worktree (0 mismatches;
`git diff --name-only 8533a79` plus the 3 untracked files equals the manifest set; index empty).

### Guidance checked against current code (no contradiction; DEC-008 not triggered)

- **Completion** (`completion.rs:23–173`) uses a `WriteBound` named `completion`. `complete_on`
  locks the run (`SELECT binding_id,max_steps FROM workflow_runs … FOR UPDATE`) with no company
  authority, then the execution (`SELECT completed_at,committed_output,… FROM workflow_executions
  … FOR UPDATE`), then job/attempt (`lock_job`, status `processing`). A completed execution
  replays before the live-window check (`:97–105`). An ambiguous COMMIT is resolved read-only
  through `completed()`/`complete_on` replay and therefore returns `CommitDisposition::Replayed`
  (`:44–67`), recorded as `resolved_applied`; the ordinary attempt records `completed`
  (`persistence_bound.rs:263–295`). This is the R-008/DEC-012 discriminator for B3.
- **Cancellation** (`controls.rs:64–129`) sets 1000 ms lock/statement timeouts, authorizes
  company, locks `HEAD … FOR UPDATE OF run`, compares revision **before** terminal state
  (`:96–100`), then `cancel_on` locks executions (`SELECT id FROM workflow_executions … ORDER BY id
  FOR UPDATE`) and jobs, retires processing and pending jobs (`maintenance.rs` `retire_jobs`), and
  records uncertainty only for a marker without receipt. A conflict or terminal result writes only
  the control-command row (no audit event, no run change).
- **Receipt** settlement does not change the run revision (DEC-004, verified in B2), so a command
  built after the receipt carries the revision both race orders compete on.
- **Replay contracts** (`action_dispatch.rs:85–146`): `authorize_prelocked_on` checks
  `lock_fence` and the processing attempt before any replay/receipt handling, so a cancelled fence
  is refused with the exact lifecycle `Conflict` whatever the recovery mode.
  `workflow_action_retry_safe` depends only on action facts (marker, receipt, evidence, replay
  support), so the live contract stays "safe" after cancellation; only the terminal fence refuses.

### Coverage map (DEC-001 rows in B3 scope)

| Obligation | Reused exact tests (re-run, unchanged) | New B3 test |
| --- | --- | --- |
| Completion boundary, cancel commits first (DEC-002/005) | `workflow_control_cancel_completion_competitors_preserve_winning_result` (barrier, not deterministic), `workflow_action_receipt_actual_result_precedes_shared_completion_writer` (no cancel) | `workflow_action_cancellation_completion_race_cancel_first` (end and successor routes) |
| Completion boundary, completion commits first (DEC-002/005) | same | `workflow_action_cancellation_completion_race_completion_first` (end route: terminal command; successor route: fresh cancel cannot erase the result) |
| Ordinary completion versus ambiguity recovery (R-008, DEC-012) | `workflow_lease_resolution_completion_replays_or_expires_once`, `workflow_lease_resolution_completion_retirement_owns_nothing`, `workflow_action_receipt_resolution_stalled_commit_against_completion` (the separate ambiguity contract) | oracle in both races; live negative control `workflow_action_cancellation_completion_race_resolved_commit_fails_ordinary_oracle`; synthetic `workflow_action_cancellation_completion_ordinary_oracle_discriminates_attempt_paths` |
| SafeRepeat/ProviderIdempotency cancelled history (DEC-001 parked row) | `workflow_action_receipt_lost_actual_effect_supported_reclaim_and_competing_replay` (live positive: same history replays after reclaim) | `workflow_action_cancellation_replay_contract_cannot_override_terminal` (both modes; cancelled before reclaim and after reclaim) |
| Applied-then-hung recovery (R-006 Q3) | B1 `workflow_action_cancellation_pending_provider_dropped_by_ownership_supervision` (applied variant: effect kept, no receipt, unresolved marker); `workflow_action_reconciliation_control_serialization_cancel_before_command` Applied (verifier recovery of a parked applied history after cancel) | `workflow_action_cancellation_applied_then_hung_history_reconciles_after_cancel` (the never-parked history that cancel-while-hung leaves, recovered through the real verifier) |
| Parked/reconciled terminal contracts (DEC-001) | `workflow_action_uncertainty_parked_deadline_cancel_and_late_receipt_truth`; `workflow_action_reconciliation_control_serialization_{command_before_cancel,cancel_before_command,command_before_expire,expire_before_command}` (Applied/Final/Unknown each); `workflow_action_reconciliation_claim_cancellation_{cancel_first,claim_first,dropped_usage_wait,aborted_task_usage_wait}`; `workflow_action_reconciliation_late_truth_{pending_before_consumption,pending_after_consumption,cancelled_retains_conflicting_actual_receipt,expired_retains_conflicting_actual_receipt}`; terminal audit `workflow_action_reconciliation_revision_{failed_receipt_unknown_is_audit_only,failed_receipt_applied_replay_and_stale_refusal,failed_unconsumed_final_replay_and_stale_refusal,succeeded_unknown_is_audit_only,succeeded_applied_replay_and_stale_refusal}` | none (no duplicate 04.7 matrix) |
| Proven-not-applied history cannot authorize new calls | control-serialization Final variants (terminal dispatch polls nothing, adds nothing) | none |

The R-006 Q3 composition is made explicit by the new combined test rather than left to inference:
B1 ends with the run cancelled while the provider was hung (never parked: the reconciliation row
comes from `cancel_on`, not from `release_io`), whereas the control-serialization Applied history
is parked first. The new test starts from B1's exact durable shape and recovers it.

### New tests and oracles

New file `src/adapters/persistence/workflow/action_cancellation_completion_tests.rs`, module
`completion_tests`, a child of B1's `cancellation_boundary_tests` (beside B2's `receipt_tests`);
it reuses B1's `cancel_command`, `heartbeat`, `retired_once`, `inert`, `refused`, `provider`,
`step_output`, `rows`, and group A's calibrated `blocked`/`probe_run`/`assert_drained`.

- **Receipt premise (`receipted`)**: genuine heartbeat; the real `ActionService::dispatch_remote`
  commits one canonical receipt and one actual observation for a provider result valid for the
  step's output schema; 1 call, 1 effect, the execution not completed. The cancel command is then
  built from the current revision. Two fixtures: `receipt_fixture` (step routes to `$end`) and a
  two-step source whose `start` routes `success → second` (`successor_fixture`), so cancel-first's
  "no successor" is a meaningful assertion and completion-first schedules one.
- **Race (`race`)**, policy `policy()` (3 s, no fixture bound change): a gate transaction holds
  only the exact execution row; the run probe reads free (`FOR UPDATE NOWAIT`, rolled back).
  - Cancel-first: the actual cancellation is observed at `SELECT id FROM workflow_executions`
    behind the gate PID; the run probe reads held (`55P03`); the actual `complete_io` is observed
    at `SELECT binding_id,max_steps FROM workflow_runs` with the **cancel PID** in
    `pg_blocking_pids`, the cancel future polled throughout and rejected if it completes.
  - Completion-first: the actual `complete_io` is observed at `SELECT completed_at,committed_output`
    behind the gate PID; run probe held; the actual cancellation (company already authorized) is
    observed at `FROM workflow_runs AS run` with the **completion PID** as blocker.
  - Gate rollback, both real results awaited under the hang guard, run probe free again, both
    backends drained. A timeout fails.
- **Ordinary-attempt oracle (`ordinary_completion`)**: exactly one recorder event `completion`
  with outcome `completed`, sliced to the race; for the completion winner additionally
  `CommitDisposition::Committed` (a resolved ambiguous COMMIT returns `Replayed`).
- **Cancel commits first (`cancel_won`)**: `Applied`; completion `None`; receipt, observation and
  effect remain; `completed_at`, output, route and successor absent; exactly one execution and one
  job (no successor scheduled even on the successor route); no `io_step_completed` event; one
  `cancel`/`applied` command; then the same fence and a fresh claim are inert.
- **Completion commits first (`completion_won`)**: `Committed` with the receipt result, `success`
  route; end route → target `$end`, run `succeeded`; successor route → target `second`, a new
  execution and pending job, run `running`. The stale command is exactly `RevisionConflict` with
  `current_revision` = the run's revision; its only write is one `cancel`/`conflict` command row
  (no `control_cancel` event, run not cancelled). Then:
  - the identical completion replays (`Replayed`, same output/route/target/successor) and the
    whole database is unchanged;
  - end route: a command at the current revision returns `AlreadyTerminalOrApplied` and adds only
    its `terminal` command row;
  - successor route: a fresh, correctly revisioned cancel is `Applied` and retires the pending
    successor job, while the predecessor execution and its completed attempt stay byte-identical.
- **Negative control**: completion-first with the established COMMIT gate
  (`Gate::arm(task_attempts UPDATE processing→completed, Stall::Commit)`) under `gated_policy()`,
  held until `resolution_behind` observes the completion's resolution queued on the run; the
  actual cancel is then observed behind that resolution PID; release. Business outcome is the same
  (committed step with the receipt output, stale command `RevisionConflict`), but the recorder
  shows `resolved_applied` and the winner is `Replayed`, and the ordinary oracle must reject it.
  The six-second fixture policy is confined to this control (DEC-012); the ordinary races keep 3 s.
- **Synthetic oracle test**: accepts one `completed` attempt with `Committed`; rejects
  `resolved_applied`, `unknown`, `not_applied` then retry, no attempt, and a `Replayed` winner.
- **Replay contract cannot override terminal** (SafeRepeat and ProviderIdempotency; cancelled
  before reclaim and after reclaim): the provider applies its effect and loses the response; the
  live premise holds (same-fence `PossibleDispatchExists`, `workflow_action_retry_safe` = true, and
  in the after-reclaim case a genuine competing reclaim to attempt 2, i.e. live replay permission).
  The real cancel then retires exactly that fence (`retired_once`); `retry_safe` is still true; the
  original and a fresh service are refused with the cancelled-fence `Conflict`; 1 call, 1 effect,
  the same entries and attempts; the whole database unchanged; the same fence and a fresh claim are
  inert.

New file `src/adapters/persistence/workflow/action_cancellation_hung_reconciliation_tests.rs`,
module `hung_reconciliation_tests`, a child of 04.7's `control_serialization_tests` (where the
ledger verifier, reconciliation service and 04.7 assertion helpers live):

- **Applied then hung, cancelled, reconciled**: a ledger provider applies the effect (ledger row
  with recoverable result, provider effect) and then never returns; with the invocation token
  uncancelled the real CancelCommand commits after the genuine first poll, and ownership
  supervision drops the future (`Interrupted`, 1 poll, dropped before return). Premise checked:
  run cancelled, marker and entry kept, no receipt, one reconciliation row, effect 1. Then a real
  verified reconciliation command at the current revision returns `AppliedRecorded { receipt: true }`
  with linked evidence/audit (04.7 `linked_result`), a recovered receipt bound to the evidence, no
  scheduled job, unchanged job/execution/attempt/budget rows and run still `cancelled`; terminal
  dispatch polls nothing and adds nothing; no claim.

Coverage-map addendum: the `workflow_action_reconciliation_control_serialization_` filter also
lists the existing calibration test `workflow_action_reconciliation_control_serialization_probe_negative_controls`
(04.7 free/held probe negative controls); it is reused and re-run, not new.

Test-only edits to accepted files (registration only):
- `action_cancellation_boundary_tests.rs` (B1): registers the `completion_tests` child (3 lines).
- `action_reconciliation_control_serialization_tests.rs` (04.7, HEAD): registers the
  `hung_reconciliation_tests` child (3 lines). Nothing else in either file changed.

No production code, policy, bound, script or CI file changed. The ordinary races use the common
3 s `policy()`; `gated_policy()` (6 s) is used only by the resolved-COMMIT negative control, as the
existing completion-resolution tests do (DEC-012). The completion races' outer hang guard is the
existing `HANG_GUARD` (10 s); a timeout fails.

### Verification

Binary `target/debug/deps/mail_agents-43f563ba6b6c24ac`, SHA256
`18d7c86ab7d56031d38450f7a7748d1198ff5f7a8a49ed978b780dc440584bb4`
([build](logs/groupB/test-build-B3.log)); reproduced after Clippy and SQLx prepare
([freshness](logs/groupB/test-binary-B3-freshness.log)). Fixture: container
`workflow-foundation-048-20261013`, PostgreSQL 18.6, system identifier `7696100747480141867`,
0 own DBs / 0 clients before the runs. Stock 2 MiB (`RUST_MIN_STACK=2097152`),
`DATABASE_URL=TEST_DATABASE_URL=<task fixture>`, `ALLOW_MISSING_DATABASE_URL` unset; the runner
requires passed == `--list`, > 0, 0 ignored, exit 0
([script](logs/groupB/verify-B3.sh), [runner](logs/groupB/run-groupB.py),
[stdout](logs/groupB/verify-B3-stdout.log), [results](logs/groupB/runtime-results.B3.json)).
The DB runs execute outside the Bash sandbox, which refuses localhost TCP.

| Check | Result | Log |
| --- | --- | --- |
| Each of 6 new tests, `--exact`, ×3 | **PASS** 18/18 | `B3-isolated-*-{1,2,3}.log` |
| — cancel-first race | blockers: cancel at `SELECT id FROM workflow_executions` behind gate; completion at `SELECT binding_id,max_steps FROM workflow_runs` behind cancel PID; free/held/free probes; `Applied`; one `completion`/`completed`; gate released 19–22 ms, drained 39–69 ms after start | |
| — completion-first race | completion at `SELECT completed_at,committed_output` behind gate; cancel at `FROM workflow_runs AS run` behind completion PID; `RevisionConflict` (revision 10 end / 11 successor); one `completion`/`completed`, `Committed`; released 12–21 ms, drained 43–70 ms | |
| — resolved-COMMIT control | resolution observed behind the stalled completion at 1974–1976 ms; cancel behind the resolution PID; `Replayed` + `resolved_applied`, `RevisionConflict`; ordinary oracle **rejected** it each run | |
| `workflow_action_cancellation_` (B1 10 + B2 6 + B3 6 + group A 2) | **PASS** 24/24 | [log](logs/groupB/runtime-workflow-action-cancellation.B3.log) |
| `cancellation_boundary_tests` (B1 + B2 + B3 completion module) | **PASS** 21/21 | [log](logs/groupB/runtime-cancellation-boundary-tests.B3.log) |
| `workflow_action_reconciliation_control_serialization_` | **PASS** 5/5 | [log](logs/groupB/runtime-workflow-action-reconciliation-control-serialization.B3.log) |
| `workflow_action_reconciliation_claim_cancellation_` | **PASS** 4/4 | [log](logs/groupB/runtime-workflow-action-reconciliation-claim-cancellation.B3.log) |
| `workflow_action_reconciliation_late_truth_` | **PASS** 4/4 | [log](logs/groupB/runtime-workflow-action-reconciliation-late-truth.B3.log) |
| `workflow_action_reconciliation_revision_` (terminal audit, 9 incl. the 5 mapped) | **PASS** 9/9 | [log](logs/groupB/runtime-workflow-action-reconciliation-revision.B3.log) |
| `workflow_action_uncertainty_` | **PASS** 10/10 | [log](logs/groupB/runtime-workflow-action-uncertainty.B3.log) |
| `workflow_action_receipt_` (SafeRepeat live positive, receipt-before-completion, resolution) | **PASS** 12/12 | [log](logs/groupB/runtime-workflow-action-receipt.B3.log) |
| `workflow_lease_resolution_completion_` (completion ambiguity contract) | **PASS** 2/2 | [log](logs/groupB/runtime-workflow-lease-resolution-completion.B3.log) |
| `prior_bounds_` (negative controls outside `workflow_action_`) | **PASS** 9/9 | [log](logs/groupB/runtime-prior-bounds.B3.log) |
| `workflow_control_` (incl. `workflow_control_cancel_completion_competitors_preserve_winning_result`) | **PASS** 29/29 | [log](logs/groupB/runtime-workflow-control.B3.log) |
| `workflow_action_` (326 + 6) | **PASS** 332/332, 417.9 s | [log](logs/groupB/runtime-workflow-action.B3.log) |
| `cargo fmt --all -- --check` | **PASS** | [log](logs/groupB/fmt-B3.log) |
| `git diff --check` (tracked) plus trailing-whitespace scan of the 2 new files | **PASS** | [log](logs/groupB/diff-check-B3.log) |
| `SQLX_OFFLINE=true cargo check --locked --offline --all-targets` | **PASS**, 0 diagnostics | [log](logs/groupB/check-offline-B3.log) |
| Strict Clippy (`SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`) | **PASS**, 0 diagnostics | [log](logs/groupB/clippy-B3.log) |
| `cargo sqlx prepare --check -- --all-targets` (live task URL) | **PASS**; `.sqlx` 45 files unchanged before/after and hash-identical to B2 | [log](logs/groupB/sqlx-prepare-check-B3.log), [before](logs/groupB/sqlx-manifest-before-B3.json), [after](logs/groupB/sqlx-manifest-after-B3.json) |

Residue after every filter: 0 own DBs, 0 clients. The new SQL is runtime `sqlx::query*` test
strings (gate/probe, `workflow_action_retry_safe`), which produce no cache entries; they are
verified by execution above.

### Artifact

[source-manifest-B3.json](logs/groupB/source-manifest-B3.json), SHA256
`c3379ee330dd4795f1f78a4758aa80c4a832b001ee28c845b015e5881f115131`. It covers 24 files changed or
added since `8533a79`: accepted B2r1's 21 files, of which only the B1 boundary file changed
(registration), plus the 04.7 control-serialization file (registration) and the 2 new test files.
Nothing is staged; the 5 untracked new files are included. Tracked diff:
[source-B3-tracked.diff](logs/groupB/source-B3-tracked.diff), SHA256
`2423cf9841ad85c6f8e6c4062f7cba029ef21016f46693231bb884afe0e1b7e7`.

### Open questions for S

1. **Combined applied-then-hung test.** R-010 allowed composed coverage; I added the combined
   test because B1's history is cancelled while running (uncertainty row from `cancel_on`, never
   `release_io`-parked), which the control-serialization Applied variant does not start from.
   It passed with no production change. Confirm it is wanted, or whether the mapping alone suffices.
2. **Completion-race lease.** Both ordinary races use the common 3 s policy (1 s attempt; the
   cancel's own 1000 ms lock timeout also bounds the wait). Observed gate-release-to-drain was
   under 70 ms isolated and the full filter passed, so no DEC-012-style exception is requested.
3. **Recorder-based oracle.** The ordinary-completion oracle requires exactly one `completion`
   recorder event `completed` (and `Committed` for the winner). In cancel-first the completion's
   `None` is likewise required to come from that ordinary attempt (a resolved retirement would
   record `resolved_applied`). Confirm this is the intended R-008 discriminator for the refusal side.
4. **Successor fixture.** The two-step source is a local 6-line copy of the claim-SQL fixture's
   `two_step_source` (that one is private several modules deep); I did not widen its visibility.

## Group C — final combined 04.8 gates (D-010, 2026-10-15)

Developer D2 (`thr_2n9nrfy6ra`, claude-code / claude-opus-5-5 / high). Scope: BRIEF-04.8 §C and
S2's "Group C integration-review checklist" (GUIDANCE.md). **No source, bound, script or CI edit
was made.** Logs are under [logs/groupC/](logs/groupC/).

### Identity

| Item | Value |
| --- | --- |
| Source | HEAD `8533a792a195bf4e2b0d48f1c2f1bc706fc3d006` + [source-manifest-C.json](logs/groupC/source-manifest-C.json), SHA256 `6e4f93aac25f2263de6938b1d70b1b9e1a21e5a247ebd1c80fdc5928f49ddadd`: 24 files (19 tracked diffs + 5 untracked), nothing staged. File set and every hash **equal accepted B3** (`c3379ee3…`); tracked diff SHA256 `2423cf98…` unchanged. No delta. |
| Toolchain | rustc/cargo 1.99.0 (b940084d7), clippy 0.1.99, sqlx-cli 0.8.6, graft 0.21.1, psql/pg_dump 18.6 |
| Lib test binary | `target/debug/deps/mail_agents-43f563ba6b6c24ac`, SHA256 `18d7c86ab7d56031d38450f7a7748d1198ff5f7a8a49ed978b780dc440584bb4` (the B3-verified binary; [build](logs/groupC/test-build-C.log)); unchanged before/after stack-budget and lease-budget (recorded in their logs) |
| Bin test binary | `target/debug/deps/mail_agents-3e0cdae7a94f5271` (`src/main.rs`, 5 tests), SHA256 `217fdee314c474b552cadcb6fbb80f9a2dddf232df732e793ab9a2aa4fd57b7e` ([build](logs/groupC/test-build-bins-C.log)) |
| DB | container `workflow-foundation-048-20261013`, PostgreSQL 18.6, system identifier `7696100747480141867`, test DB `workflow048_test` (53/53 migrations); 0 own DBs / 0 clients before and after ([residue](logs/groupC/residue-after-C.log)) |

### Gate table

| Gate | Command / env | Result | Log |
| --- | --- | --- | --- |
| Formatting | `cargo fmt --all -- --check` | **PASS** | [log](logs/groupC/fmt-C.log) |
| Whitespace | `git diff --check` + trailing-whitespace scan of untracked `src` files | **PASS** | [log](logs/groupC/diff-check-C.log) |
| Offline compile | `SQLX_OFFLINE=true cargo check --locked --offline --all-targets` | **PASS**, 0 diagnostics | [log](logs/groupC/check-offline-C.log) |
| Strict Clippy | `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings` | **PASS**, 0 diagnostics | [log](logs/groupC/clippy-C.log) |
| SQLx metadata | `DATABASE_URL=<task> SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets` | **PASS**; `.sqlx` 45 files unchanged and equal to B3 | [log](logs/groupC/sqlx-prepare-check-C.log), [before](logs/groupC/sqlx-manifest-before-C.json), [after](logs/groupC/sqlx-manifest-after-C.json) |
| Task DB migrations | `cargo sqlx migrate info` | 53/53 installed, none pending/different | [log](logs/groupC/migrate-info-task-db-C.log) |
| Fresh migration/schema/checksums | new task-owned DB `wf048_groupc_fresh_202610151721` (oid 45372257) in the same container: create, `cargo sqlx migrate run`, `migrate info`, catalog and `pg_dump --schema-only`, then **dropped** | **PASS**: 53 rows, all `success`, every checksum = SHA-384 of its `migrations/*.sql` file; catalog (version, checksum, success) identical to `workflow048_test`; schema dumps byte-identical (SHA256 `cfd8c4ba…`, 0 diff lines); catalog digest `ca78e4be…` | [migrate](logs/groupC/fresh-migrate-C.log), [checksums](logs/groupC/migration-checksums-C.log), [diff](logs/groupC/schema-diff-fresh-vs-task.txt), [drop](logs/groupC/fresh-drop-C.log) |
| Full library suite, stock 2 MiB, default parallelism | runner ([run-groupC.py](logs/groupC/run-groupC.py)), filter `""`; passed must equal listed (2579) − listed-ignored (22), ignored must equal 22 | **FAIL** 2550 passed / **7 failed** / 22 ignored, 1608 s, residue 0/0 | [log](logs/groupC/runtime-all-lib.C.log), [results](logs/groupC/runtime-results.C.json) |
| Bin tests, stock 2 MiB | same runner, bin binary | **PASS** 5/5 | [log](logs/groupC/runtime-all-lib.C-bins.log) |
| `scripts/stack-budget.sh --offline` (unchanged, SHA256 `95102fb0…`) | `SQLX_OFFLINE=true DATABASE_URL=TEST_DATABASE_URL=<task>`; script exports 2048 KiB | **Stack budget not exceeded** (0 overflow); script exit **101** "tests failed for a reason unrelated to the stack budget": 2551 passed / **6 failed** / 22 ignored, 1521.9 s | [log](logs/groupC/stack-budget-C.log) |
| `scripts/lease-budget.sh --offline` (unchanged, SHA256 `d3787db4…`) | same env; `RUST_MIN_STACK` from `.cargo/config.toml` as in CI; `LEASE_BUDGET_RECORD` kept | **PASS** 87/87 (= `--list` of its filters); all required ops measured; highest work share `budget.reserve` 350/1000 ms, `lease.renew` 362/2000, `lease.claim` 264/2000, `completion` 232/2000; every `action.*` ≤ 86 ms | [log](logs/groupC/lease-budget-C.log), [record](logs/groupC/lease-budget-record-C.jsonl) |
| `graft build` (repo root) | `graft build` | **PASS** exit 0: 16605 nodes, 26519 edges, 862 cards (all replayed from cache). Graph delta: 879 files before/after, **0 added/removed/changed**. `graft/` is git-ignored, so no manifest delta | [log](logs/groupC/graft-build-C.log), [delta](logs/groupC/graft-delta-C.txt) |

Negative controls present and passing in the full-library run (and in the stack-budget run):
F-004 `workflow_action_cancellation_pending_oracle_rejects_deadline_only_interruption` and
`…_pending_oracle_discriminates_interruption_causes`; F-005
`…_receipt_race_late_release_fails_ordinary_oracle` and `…_receipt_ordinary_oracle_discriminates_attempt_paths`;
R-008 `…_completion_race_resolved_commit_fails_ordinary_oracle` and
`…_completion_ordinary_oracle_discriminates_attempt_paths`; `prior_bounds_` 9/9 (9 `should panic … ok`).

Deviations from CI, recorded rather than hidden:
- CI wraps the DB suites in `scripts/test-network-isolation.sh`, which needs `sudo iptables`. I did
  not use sudo on this host; the same commands, arguments (`--offline`) and env ran without the
  egress block. Egress denial is therefore unverified here.
- CI's main step `cargo test --locked --offline --all-targets` runs at the 16 MiB config stack; the
  stock-2 MiB lib and bin runs above plus stack-budget cover the same tests at the stricter stack.
- The 22 `#[ignore]` tests (18 optional HydraDB adapter, 1 HydraDB live, 2 live-DNS smoke, 1
  developer aid; [list](logs/groupC/ignored-tests-list.txt)) are skipped exactly as in CI; the runner
  requires exactly those to be reported ignored.
- DB-backed runs execute outside the Claude Bash sandbox, which refuses localhost TCP.

### Failure diagnosis (no change made; returned to root)

Isolated reruns: `--exact`, ×3 each, stock 2 MiB, same binary, load average about 5
([script](logs/groupC/rerun-failures-C.sh), [stdout](logs/groupC/rerun-failures-C-stdout.log),
logs `C-isolated-*`).

**1. Timestamp precision on Linux: 5 tests, deterministic, outside 04.8.** They fail in both full
runs and in every isolated rerun (15/15 failures), with identical mechanism:
- `attention::tests::agent_work_stays_out_until_a_human_decision_is_required` (`attention_tests.rs:796`)
- `attention::tests::the_queue_ages_the_generation_rather_than_the_row` (`:1794`)
- `task::tests::extending_an_expired_outreach_serializes_with_the_timeout_sweep` (`task/tests.rs:6800`)
- `wait_tests::workflow_wait_competing_park_signal_resume_and_lost_ack` (`wait_tests.rs:46`)
- `schedule::tests::claiming_anchors_the_next_run_on_the_slot_not_the_clock` (`schedule.rs:813`)

Each compares a Rust `Utc::now()`-derived value carrying nanoseconds (e.g. `…24.166672393Z`) with
the value PostgreSQL stored and returned at microsecond precision (`…24.166672Z`). The schedule case
is the same truncation: `(next − missed_slot)` is 4 h minus < 1 µs, so `num_seconds() % 3600 = 3599`.
Linux's clock exposes nanoseconds; the earlier macOS host's clock is microsecond-resolution, which
hides the mismatch. None of these test or production files differs from `8533a79` or from the 04.8
base `5b3d7dc` (last touched `0ace8dc`, 04.7). This is a pre-existing Linux portability defect in
those tests (it would also fail CI's `ubuntu-latest`), not a 04.8 regression. A fix (truncate the
expected value to microseconds, or compare at DB precision) is a test-source edit outside 04.8 scope
and needs root's decision.

**2. Load-dependent fixture windows: 2 tests, 04.7 action dispatch, outside the 04.8 diff.**
- `workflow_action_dispatch_delayed_first_poll_checks_monotonic_window`
  (`action_dispatch_remote_tests.rs:125`): failed in the full-library run only, with
  `Conflict("Workflow lifecycle revision or command conflict")`. The test sets the lease to expire
  `clock_timestamp()+0.3 s` and expects the post-entry monotonic check to interrupt; a `Conflict`
  means the lapse already happened before final authorization, the fail-closed outcome.
- `workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline` (`:271`):
  `Elapsed` from its 4 s outer timeout in both full runs; which of its 4 modes timed out is not logged.
- Both pass isolated ×3 (6/6), the `workflow_action_dispatch_` filter passes 20/20
  ([results](logs/groupC/runtime-results.C-rerun.json)), and both passed in every earlier
  `workflow_action_` run (B1–B3, 318–332 tests) and every dispatch-filter run.
- Host load: another project on this host (`/home/mh/dev/bizi44`, `cargo test --test
  company_access …`, not task-owned and not touched) was compiling and testing during the runs; load
  average was 5.4 at the start and about 20 during stack-budget. The full library run is about 8×
  the action filter's test count on 8 cores.
- The test file is unchanged since `0ace8dc`. Group A's lock-order correction adds an unlocked actor
  read and moves company authority ahead of the run lock on the dispatch path; on this evidence it
  is not excluded as a minor contributor to the 0.3 s window, nor established as one. These match the
  DEC-009 class (fixture lease windows under full parallel load, first Linux run). Measuring the exact
  phase (DEC-010 style probes) or changing the windows requires test edits, which await root/S.

The `workflow_wait_` filter rerun fails 15/16 on the precision test above, as expected.

### BRIEF §B boundary → test coverage (final)

All 64 names below are present in the final binary's `--list`
([names](logs/groupC/coverage-names-C.txt)) and ran in the full-library run.

| Boundary | New 04.8 tests | Reused exact tests |
| --- | --- | --- |
| Prepared/undispatched | `workflow_action_cancellation_prepared_and_protected_refuse_after_command` | `workflow_action_schema_fk_immutable_and_cancelled_new_intent_fail_closed`, `workflow_action_dispatch_stale_fences_scopes_protection_and_errors`, `workflow_action_cancellation_company_before_run_all_callers` |
| Lock competition | group A: `workflow_action_cancellation_company_before_run_all_callers`, `workflow_action_cancellation_return_lost_owner_precedes_denied_authority` | control/claim/completion races in `workflow_control_` (incl. `workflow_control_cancel_completion_competitors_preserve_winning_result`) |
| Marker before entry | `workflow_action_cancellation_after_reservation_refuses_entry_zero_polls` | `workflow_action_dispatch_service_post_marker_revoke_cancel_zero_provider_polls`, `workflow_action_dispatch_post_marker_revocation_cancel_and_expiry_refuse_entry` |
| Work in flight | `workflow_action_cancellation_pending_provider_dropped_by_ownership_supervision`, F-004 controls `…_pending_oracle_rejects_deadline_only_interruption`, `…_pending_oracle_discriminates_interruption_causes`; token-only `…_token_precancelled_never_reserves`, `…_token_after_entry_suppresses_first_poll`, `…_token_pending_drop_keeps_ownership` | `workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline`, `workflow_control_cancel_actual_handler_future_is_dropped_by_heartbeat`, `workflow_action_dispatch_delayed_first_poll_checks_monotonic_window` |
| Result/receipt/cancel | `…_receipt_race_cancel_first`, `…_receipt_race_receipt_first`, F-005 `…_receipt_race_late_release_fails_ordinary_oracle`, `…_receipt_ordinary_oracle_discriminates_attempt_paths`, `…_receipt_without_cancel_commits`, `…_observed_result_token_settles_then_interrupts` | `workflow_action_receipt_late_cancel_and_stale_truth_without_advancement`, `workflow_action_receipt_resolution_stalled_commit_against_completion`, `workflow_action_receipt_return_authority_errors_propagate_after_commit`, `workflow_action_receipt_refused_return_releases_run_before_returning` |
| Duplicate/restart | identical redelivery and fresh-service retry inside both receipt races (`late_truth_is_final`) | `workflow_action_receipt_committed_ambiguous_ack_restart_zero_io`, `workflow_action_reconciliation_old_actual_after_new_receipt_keeps_same_and_different_truth`, `workflow_action_reconciliation_late_truth_cancelled_retains_conflicting_actual_receipt` |
| Completion boundary | `…_completion_race_cancel_first`, `…_completion_race_completion_first` (end and successor routes), R-008 `…_completion_race_resolved_commit_fails_ordinary_oracle`, `…_completion_ordinary_oracle_discriminates_attempt_paths` | `workflow_control_cancel_completion_competitors_preserve_winning_result`, `workflow_action_receipt_actual_result_precedes_shared_completion_writer`, `workflow_lease_resolution_completion_replays_or_expires_once`, `workflow_lease_resolution_completion_retirement_owns_nothing` |
| Parked/reconciled | `…_replay_contract_cannot_override_terminal` (SafeRepeat, ProviderIdempotency), `…_applied_then_hung_history_reconciles_after_cancel` (R-006 Q3) | `workflow_action_receipt_lost_actual_effect_supported_reclaim_and_competing_replay` (live positive), `workflow_action_uncertainty_parked_deadline_cancel_and_late_receipt_truth`, control serialization ×5 (`…_command_before_cancel`, `…_cancel_before_command`, `…_command_before_expire`, `…_expire_before_command`, `…_probe_negative_controls`), claim cancellation ×4 (`…_cancel_first`, `…_claim_first`, `…_dropped_usage_wait`, `…_aborted_task_usage_wait`), late truth ×4 (`…_pending_before_consumption`, `…_pending_after_consumption`, `…_cancelled_retains_conflicting_actual_receipt`, `…_expired_retains_conflicting_actual_receipt`), terminal audit ×5 (`workflow_action_reconciliation_revision_{failed_receipt_unknown_is_audit_only,failed_receipt_applied_replay_and_stale_refusal,failed_unconsumed_final_replay_and_stale_refusal,succeeded_unknown_is_audit_only,succeeded_applied_replay_and_stale_refusal}`) |
| Rollback | `…_rollback_keeps_prepared_work_usable`, `…_rollback_keeps_accepted_effect_settleable` | `workflow_control_cancel_deferred_faults_roll_back_complete_state`, `workflow_control_wait_and_retry_deferred_faults_roll_back_complete_state`, `workflow_action_receipt_deferred_rollback_returns_no_success` |

(`…` = `workflow_action_cancellation_` for new tests and the corresponding family prefix for reused.)

### Status

- PASS: fmt, diff-check, offline check, strict Clippy, SQLx prepare/check, task and fresh
  migrations/schema/checksums, bin tests, lease-budget, graft build, stack budget itself
  (no overflow), all 04.8 tests and negative controls.
- **FAIL (blocking a green full suite): 7 full-library failures** — 5 deterministic
  timestamp-precision tests outside 04.8, and 2 load-dependent 04.7 dispatch fixture windows. Because
  of them, `stack-budget.sh` also exits non-zero. Root decides the disposition; no edit was made.
- Long processes stopped; the fixture container is left running with 0 own DBs and 0 clients.

## Batch C1 — precision fixes, F-006, timing stabilization (D-011, 2026-10-15)

Developer D2 (`thr_2n9nrfy6ra`, claude-code / claude-opus-5-5 / high). Decisions: DEC-013, DEC-014
and F-006 (R-012); user decisions in TASK.md (precision fixes in 04.8, F-006 production fix
included, timing made load-independent where S says feasible). Starting artifact: manifest C
(`6e4f93aa…`), re-verified (24 files, 0 mismatches) before editing. No bound, policy, script, CI,
schema or migration changed.

**Commit note (not by D):** during this batch the user committed the worktree as `cfef3d8`
("workflow-foundation 4.8", 21:01:39 +0200), which captured C1 edits in progress (the F-006 fix
and pre-rustfmt precision edits), and later amended it to `94c77e0`, which contains the complete C1
source. Verified: `cfef3d8`'s blobs for the 24 manifest-C files equal manifest C; after the runs the
worktree source equals `94c77e0` (no source diff, nothing untracked), and cargo reports the crate
`Fresh` with the tested binary hash ([freshness](logs/groupC/test-binary-C1-freshness.log)). The
committed `waits.rs` is the fixed version (SHA256 `3975761c…`), not the negative-check state.

### Changes

| File | Change |
| --- | --- |
| `src/adapters/persistence/attention_tests.rs` | DEC-013: the outreach expiry and both generation-open instants are `trunc_subsecs(6)` (microsecond-exact, as timestamptz stores them; SQLx binds integer microseconds). Hidden-agent-work, single delegation item, priority/responsibility/earlier-expiry and ordered-ID/age oracles unchanged. |
| `src/adapters/persistence/task/tests.rs` | DEC-013: `new_expiry` microsecond-exact. Genuine competing extension/sweep, exact saved expiry and waiting states unchanged. |
| `src/adapters/persistence/schedule.rs` (test module only) | DEC-013: `missed_slot` microsecond-exact; new assertion that the claimed run's `scheduled_for` equals that persisted slot; rhythm is now an exact positive advance with `num_microseconds() % 3_600_000_000 == 0` (no whole-second truncation). Future and about-half-hour checks unchanged. |
| `src/adapters/persistence/workflow/waits.rs` (**production, F-006**) | `park_on` inserts the wait with `RETURNING deadline` and answers the first park with that stored value, so first, competing and replayed parks return the persisted deadline. Capping, guards, completion and audit are unchanged. Callers traced: only `park_wait`; its production user `application/workflow/worker.rs` checks `is_some()`. Runtime query (no `.sqlx` entry). |
| `src/adapters/persistence/workflow/wait_tests.rs` | F-006 regression in `workflow_wait_competing_park_signal_resume_and_lost_ack`: the deadline is µs-exact + 477 ns on every platform; asserts the input keeps sub-µs digits, stored == `min(requested, run deadline)` truncated to µs, first park == stored, both competing parks equal, and a replay park == the first response. Signal/resume/lost-ack assertions unchanged. |
| `src/adapters/persistence/workflow/action_dispatch_remote_tests.rs` | DEC-014. `Intercept` loses its `entry_delay` (now unused). New delegating `Recorded` seam (real writer; records the real entry time, frozen deadline, hand-back time and every real ownership probe). `delayed_first_poll` now renews genuinely and holds the **genuine** entry until its own captured deadline before handing it back; oracle `expired_entry_never_polled` requires the entry was reached and expired, token live, 0 polls, `Interrupted` (a refusal before entry fails as a premise). New synthetic `workflow_action_dispatch_expired_entry_oracle_rejects_polled_provider`. The pending test runs four named modes (`OwnershipTaken`, `RunCancelled`, `RunDeadline`, `Token`), fails immediately and labelled if dispatch returns before the first poll, prints a phase timeline on every failure, and checks a causal witness per mode (refused probe before the frozen deadline; entry bounded by the run deadline and returned at it with ownership affirmed; token return before the deadline with no refusal). The run-deadline mode sets a 1.5 s window after a fresh renewal (was 0.4 s before renewal, the likely source of the earlier `Elapsed`); the 4 s watchdog is unchanged. |

F-006 negative check ([log](logs/groupC/F006-negative-check-C1.log)): with the pre-fix INSERT
temporarily restored, the regression fails (`deadline: …611631477Z` vs `…611631Z`); the fix was then
restored byte-identical.

### Verification

Binary `target/debug/deps/mail_agents-43f563ba6b6c24ac`, SHA256
`8fd635ae795e0f8e60c78b4746a5ae1a13635d99e7dd4878564fccaa86d4a4f4` ([build](logs/groupC/test-build-C1.log)),
unchanged before/after stack-budget and lease-budget and fresh afterwards. Bin target
`mail_agents-3e0cdae7a94f5271`, SHA256 `b5b4ea27…a134` ([build](logs/groupC/test-build-bins-C1.log)).
Fixture as in group C (system identifier `7696100747480141867`); toolchain rustc 1.99.0, sqlx-cli 0.8.6.

| Gate | Result | Load (1/5/15 min) start → end | Log |
| --- | --- | --- | --- |
| Changed tests, `--exact` ×3 (8 tests) | **PASS** 24/24 | 4.27 → 7.43 (whole targeted script) | `C1-isolated-*-{1,2,3}.log`, [stdout](logs/groupC/verify-C1-targeted-stdout.log) |
| Filters: `persistence::attention::` 19, `persistence::task::` 113, `persistence::schedule::` 2, `workflow_wait_` 16, `workflow_action_dispatch_` 21, `workflow_control_` 29, `workflow_poll` 13, `workflow_maintenance_` 12, `workflow_poison_` 12, `workflow_budget_activation_` 9 | **PASS** all (= `--list`) | same run | [results](logs/groupC/runtime-results.C1-targeted.json) |
| fmt / `git diff --check` (+ untracked scan) | **PASS** / **PASS** | — | [fmt](logs/groupC/fmt-C1.log), [diff](logs/groupC/diff-check-C1.log) |
| `SQLX_OFFLINE=true cargo check --locked --offline --all-targets` | **PASS**, 0 diagnostics | — | [log](logs/groupC/check-offline-C1.log) |
| Strict Clippy | **PASS**, 0 diagnostics | — | [log](logs/groupC/clippy-C1.log) |
| `cargo sqlx prepare --check -- --all-targets` (live) | **PASS**; `.sqlx` 45 files unchanged, equal to C (the new query is runtime SQL) | — | [log](logs/groupC/sqlx-prepare-check-C1.log) |
| **Full library suite, stock 2 MiB, default parallelism** | **PASS** 2558 passed / 0 failed / 22 ignored (2580 listed, 22 listed-ignored), 1322.8 s, residue 0/0 | 3.93 / 5.02 / 4.08 → 2.84 / 7.81 / 9.59 | [log](logs/groupC/runtime-all-lib.C1.log), [results](logs/groupC/runtime-results.C1.json) |
| Bin tests, stock 2 MiB | **PASS** 5/5 | — | [log](logs/groupC/runtime-all-lib.C1-bins.log) |
| `scripts/stack-budget.sh --offline` (unchanged) | **PASS** exit 0: 2558 passed / 0 failed / 22 ignored at 2048 KiB, 0 overflow, 1021.1 s | 2.84 / 7.81 / 9.59 → 5.25 / 7.35 / 8.77 | [log](logs/groupC/stack-budget-C1.log) |
| `scripts/lease-budget.sh --offline` (unchanged) | **PASS** exit 0: 88/88 (= `--list`; +1 is the new oracle test), all required ops, highest work share `budget.reserve` 352/1000 ms, `lease.renew` 333/2000, `completion` 232/2000, `lease.claim` 232/2000; every `action.*` ≤ 45 ms | 5.25 / 7.35 / 8.77 → 2.47 / 4.41 / 7.03 | [log](logs/groupC/lease-budget-C1.log), [record](logs/groupC/lease-budget-record-C1.jsonl) |

Load was recorded at each run boundary ([load log](logs/groupC/load-C1.log)); no foreign
cargo/rustc process was present at any boundary (bizi44 stopped, per root). The 5- and 15-minute
figures include this batch's own preceding suite. The first, load-gated launch was stopped during
its initial 90 s pause, before any test ran, when root confirmed a quiet host.

All 8 changed or new tests passed in the full-library run. Negative controls ran and passed in it:
F-004 deadline-only, F-005 late-release, R-008 resolved-commit, and `prior_bounds_` 9/9.

Deviations unchanged from group C: no `sudo iptables` network-isolation wrapper (egress denial
unverified); 22 `#[ignore]` tests skipped as in CI and counted exactly.

### Artifact

[source-manifest-C1.json](logs/groupC/source-manifest-C1.json), SHA256
`e59bb3f02615d685854ae03bae674f57017acc0b66bbdb9ee89c12d230be7dfd`: base `8533a79`, HEAD
`94c77e0893248a45f338de5701649cea8f9b3c3d`, 30 project files changed since the base (the 24 of
manifest C, all hashes unchanged, plus the 6 C1 files above), nothing staged, no untracked project
file (the user's commit includes the former untracked test files), `.sqlx` unchanged. Tracked diff
since base: [source-C1-tracked.diff](logs/groupC/source-C1-tracked.diff), SHA256 `18eb1398…d0ce`.

## C1 correction (F-007/F-008/F-009) — C2 (D-012..D-014, 2026-10-15/16)

Developer D2 (`thr_2n9nrfy6ra`, claude-code / claude-opus-5-5 / high). Findings: F-007 (R-014),
F-008 (R-015), F-009 (R-016; S3 also ruled that a clean `stack-budget.sh` run counts as the
full-suite gate). Starting artifact: C1 manifest `e59bb3f0…` at HEAD `94c77e0`, re-verified (30/30)
before editing. Test-only source changes; no production code, bound, policy, script, CI, schema,
migration or `.sqlx` change.

### Changes

| File | Change |
| --- | --- |
| `src/adapters/persistence/workflow/action_dispatch_remote_tests.rs` | **F-007.** The `Recorded` seam now reads the genuine persisted run and lease remainders in one statement right after the real entry returns (`persisted_windows`, monotonic instants before and after the read). New `run_limits_entry` oracle, used by the `RunDeadline` mode: rejects a lapsed window and a lease that ends no later than the run; requires the frozen entry deadline ≤ `read_after + run` (no earlier than the true run expiry) and that this is at least `RUN_MARGIN` (250 ms) before `read_at + lease` (no later than the true lease expiry). The existing post-update bound, first-poll-while-live, drop-before-return, affirmed probes, live token, 4 s watchdog and 1.5 s window are kept. `pending_case` is split into `pending_run` (all non-causal assertions) plus the oracle; new `RunPremise::{RunFirst, LeaseFirst}`. New control `workflow_action_dispatch_pending_run_deadline_oracle_rejects_lease_limited_entry`: a live run of S's counterexample (renew, wait 2 s, then set the run window, so the lease ends first) that passes every other assertion and must be rejected, plus constructed cases (lease first with empty probes, equal windows, within the margin, run or lease lapsed, entry later than the run deadline, no windows). |
| `src/adapters/persistence/task/tests.rs` | **F-009.** `stop_task_serializes_with_final_dispatch_without_duplicate_customer_reply` now runs both orders deterministically on an `own_database`: a gate transaction holds the outreach row (both operations lock outreach before task), the first contender and then the second are lined up behind it with the existing `wait_until_a_backend_is_blocked` / `wait_until_backends_are_blocked` helpers (either finishing early fails), and releasing the gate decides the winner. **StopFirst:** stop `Ok`, dispatch `LeaseLost`, task `stopped`, 0 customer replies. **DispatchFirst:** dispatch `Committed`, stop `Conflict("Delegation changed from version 2 to 3…")`, task still `processing` with the same worker and generation, outreach `completed`, exactly 1 reply; then the real fenced closeout `mark_task_completed(lease)` succeeds, the task is `Completed`, still 1 reply. The old expectation of `Completed` straight after dispatch is gone (it was never the dispatch-only contract). Each run logs both outcomes; fixture and payload unchanged. |

F-007 first-run evidence: the corrected RunDeadline case read run 1.414 s vs lease 2.905 s and
returned at the run-bound 1.467 s deadline; the control read run 1.415 s vs lease 0.908 s, had no
probes, passed every other assertion and was rejected ("the lease ends no later than the run
deadline").

### Static checks (final source)

`cargo fmt --all -- --check`, `git diff --check` (+ untracked scan), `SQLX_OFFLINE=true cargo check
--locked --offline --all-targets` and strict Clippy all exit 0, 0 diagnostics
([fmt](logs/groupC/fmt-C2.log), [diff](logs/groupC/diff-check-C2.log),
[check](logs/groupC/check-offline-C2.log), [clippy](logs/groupC/clippy-C2.log)). Final lib test
binary SHA256 `44ada14f77c70b30889bd0b1bea51a8047664e9b944b7faa8dea323dac13afff`
([build](logs/groupC/test-build-C2.log)); bin test binary unchanged `b5b4ea27…a134`
([build](logs/groupC/test-build-bins-C2.log)). After the gates, cargo recompiled because the two B3
test files' mtimes had been touched (content unchanged, hashes equal to the manifest) and produced
the identical hash `44ada14f…` ([freshness](logs/groupC/test-binary-C2-freshness.log)).

### Run accounting (all logs retained)

| Run | Binary | Disposition | Evidence |
| --- | --- | --- | --- |
| D-012 first C2 pipeline (20:31Z) | `94a422dc…` (F-007 only) | **Load-invalidated, aborted by root (D-013)** — not a pass, not a regression verdict. bizi44 `cargo clippy/check` started during the targeted phase (load about 16–20); the dispatch filter had 7 timing failures and the partial full-library log 16 when stopped. One leftover own DB was dropped. | `*-loaded-aborted.*`, [cleanup](logs/groupC/residue-cleanup-C2.log) |
| D-013 attempt a1 (21:57Z) | `94a422dc…` | **Superseded by F-009.** Targeted F-007 tests and dispatch filter 22/22 passed; the full library run failed 1 test (`stop_task_serializes_with_final_dispatch_without_duplicate_customer_reply`, `Processing` vs `Completed` — the F-009 defect, not load); stack-budget 2559/0/22 and lease-budget exited 0 under foreign load (122 foreign samples). The orchestrator was stopped during the a2 quiet wait. | [load](logs/groupC/load-C2.log), `*-C2-a1*` |
| D-014 attempt f1 (23:25Z) | `44ada14f…` (final) | **Load-invalidated.** Started quiet (load 2.11, no foreign); bizi44 resumed within 2 min (84 of the samples had foreign processes; load up to about 8). Targeted 11/11, task 113/113, dispatch 22/22 and lease-budget 89/89 passed; stack-budget failed 1 test: `workflow_action_reconciliation_snapshot_restores_terminal_and_parked_full_provenance`, setup `Conflict("Workflow lease.claim commit outcome unknown")` (an unresolved setup claim, not an assertion of the changed code). Rerun once when quiet, per D-014. | [load](logs/groupC/load-C2-final.log), [stack](logs/groupC/stack-budget-C2-f1.log), [samples](logs/groupC/load-samples-C2-f1.log) |
| **D-014 attempt f2 (00:04Z)** | `44ada14f…` (final) | **PASS — the C2 gate evidence (below).** | [stdout](logs/groupC/final-gates-C2-stdout.log), [load](logs/groupC/load-C2-final.log), [samples](logs/groupC/load-samples-C2-f2.log) |

### Final gates — attempt f2 (stock 2 MiB for test threads, task fixture, counts vs `--list`)

| Gate | Result | Load 1/5/15 min (start → end); foreign cargo/rustc at boundaries | Log |
| --- | --- | --- | --- |
| F-009 test `--exact` ×5 | **PASS** 5/5 (StopFirst and DispatchFirst outcomes logged each run) | targeted phase: 0.49/1.17/3.74 → 6.85/2.79/3.98; 0 → 0 | `C2-f2-isolated-stop_task…-{1..5}.log` |
| F-007 corrected test ×3, control ×3 | **PASS** 6/6 (control rejected the lease-limited entry each run) | same | `C2-f2-isolated-workflow_action_dispatch_pending_*-{1,2,3}.log` |
| `persistence::task::` / `workflow_action_dispatch_` | **PASS** 113/113 / 22/22 | same | [results](logs/groupC/runtime-results.C2-f2-targeted.json) |
| `scripts/stack-budget.sh --offline` (full suite, counts as the full-suite gate) | **PASS** exit 0: 2559 passed / 0 failed / 22 ignored, 0 overflow, 968.3 s; lib binary unchanged before/after | 6.85/2.79/3.98 → 1.70/5.08/5.80; 0 → 0 | [log](logs/groupC/stack-budget-C2-f2.log) |
| Bin tests | **PASS** 5/5, residue 0/0 | — | [results](logs/groupC/runtime-results.C2-f2-bins.json) |
| `scripts/lease-budget.sh --offline` | **PASS** exit 0: 89/89, all required ops; highest work share `budget.reserve` 349/1000 ms, `lease.renew` 323/2000, `lease.claim` 264/2000, `completion` 237/2000; every `action.*` ≤ 49 ms | 1.70/5.08/5.80 → 2.82/3.59/4.85; 0 → 0 | [log](logs/groupC/lease-budget-C2-f2.log) |
| Live `cargo sqlx prepare --check -- --all-targets` | **PASS** exit 0; `.sqlx` unchanged | — | [log](logs/groupC/sqlx-prepare-check-C2-f2.log) |

f2 started on a quiet host and every run boundary recorded 0 foreign processes, but the 15 s
sampler saw a foreign cargo/rustc in 15 of 100 samples (00:14–00:29Z, during stack-budget and
lease-budget); the peak 1-min load was 10.74, partly this suite's own. No test failed, so f2
stands; the foreign presence is recorded rather than hidden.

### F-008 — graph refresh on the final source

`graft build` at repo root after the final source was frozen (HEAD `94c77e0` + the two corrected
files): exit 0, 16639 nodes (11544 functions, 1708 structs), 26557 edges, 862 cards (group C:
16605 / 26519). Delta against the pre-build cache: 880 files before and after, 0 added/removed,
3 changed — the task-tests card (`graft/src/adapters/persistence/task/tests.md`, F-009) and two
extraction/fingerprint cache files. The dispatch-tests card already carried the F-007 symbols from
query-time refresh. `graft/` is git-ignored, so it is not in the manifest
([log](logs/groupC/graft-build-C2.log), [delta](logs/groupC/graft-delta-C2.txt)).

### Artifact

[source-manifest-C2.json](logs/groupC/source-manifest-C2.json), SHA256
`5f8181076864e1fe0e1184380bdac8949987b8685faca8333e7bc51c389debf0`: base `8533a79`, HEAD `94c77e0`
(the user's commit; no new commit during C2), 30 project files changed since the base — C1's 30
with only `task/tests.rs` and `action_dispatch_remote_tests.rs` changed, both uncommitted on top of
HEAD; nothing staged or untracked; `.sqlx` unchanged. Tracked diff since base:
[source-C2-tracked.diff](logs/groupC/source-C2-tracked.diff), SHA256 `ebf95bfc…7b7e`.
Deviation unchanged: no `sudo iptables` network-isolation wrapper (egress denial unverified).
