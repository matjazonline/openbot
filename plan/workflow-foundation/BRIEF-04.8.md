# 04.8 — cancellation at the shared action boundary

Status: preparation only, 2026-10-13; independent architecture review and root acceptance pending.
Author A owns this brief and implementation. No production/test code changed in preparation.

## Original requirement and baseline

[04 actions](04-actions-http-and-delivery.md), execution item 7: cancellation suppresses
undispatched work, cannot retract accepted remote effects, and retains auditable late receipts
without advancing cancelled runs. Its acceptance also requires competing dispatchers, stale
workers and duplicate callbacks not to create new logical effects. [EXPANSION](EXPANSION.md)
maps this obligation to 04.8. Accepted 04.1–04.7 remain foundations; see
[latest handoff](HANDOFF-04.7-COMPLETE-20261012.md). The new continuation instruction supersedes
that handoff's STOP, not its retained limitations. Production HTTP/MCP cleanup is 04.9/04.10,
messaging sibling delivery 04.11, real Rig integration 06 and bypass removal 08. No migration
squash, provider implementation, deployment or production-data work is part of this point.

Current HEAD is `5b3d7dc08340d7b516740b9fcd71b753183ced16`. Initial status/diff and protected
helper hashes are under `/tmp/workflow-foundation-20261013-04.8/`. Preserve modified
`.claude/helpers/graft-hooks.cjs`, `.claude/helpers/graft-statusline.cjs`, untracked `.agents/`
and root-owned `CONTINUATION-20261013-04.8.md`. Historical macOS database/evidence paths are
not current Linux resources or fresh verification.

## Contracts and consequential choices

1. Keep `WorkflowControl::cancel` as the durable, authorized, revision-checked cancellation
   owner (`controls.rs:29–41,64–129,163–183`). Cancellation retires existing jobs/attempts,
   cancels waits and preserves immutable intent/dispatch/receipt/evidence/audit facts. It
   creates no new action state machine, lease, queue or retry permission. A cancellation token
   only interrupts this invocation's actual future; it is not a durable CancelCommand.
2. **Align action authority with the command lock order before adding the full race matrix.**
   Actual controls take company authority before the run, but
   `action_dispatch.rs:74–133` takes the run/execution/job before calling
   `action_authority.rs:30–94`, which then takes that same company's `FOR NO KEY UPDATE`
   lock (`authority.rs:9–36`). A cancel holding company while dispatch holds run can cycle.
   This is a concrete existing lock-order risk; no reproduced failure is claimed yet.
   Keep command/admission company-before-run ordering. Add a narrow action-authority helper
   that first resolves the persisted actor by exact company/run without a row lock, locks
   current company/principal authority, then locks/reloads the run and verifies the actor
   agrees. That preliminary read is only a lock-order hint, never authorization or a grant.
   Preserve active state/deadline, frozen bundle/digest/scope, actor/resource/policy and
   exact current attempt/fence validation under the same transaction before any effect.
   Apply this ordering to the standalone authority loader and every action dispatch caller;
   `return_authorized` currently acquires the run before `authorize_on` and must participate.
   Keep its known cancelled/stale ownership result `Interrupted`, operational failures errors,
   and successful late truth persistence independent of current access. Do not turn missing
   authority into success or swallow arbitrary errors. Trace callers again before edits.
   Run-only receipt settlement, ambiguous-outcome observation, ownership heartbeat and intent
   preparation do not acquire company authority and need no speculative lock rewrite.
3. Linearization remains explicit: local effect/receipt commit together under the run lock;
   remote permission linearizes at final locked entry authorization, followed by the existing
   monotonic/token check immediately before provider polling. Cancellation committed before
   that authorization prevents provider entry. Cancellation after authorization cannot retract
   an already authorized/accepted effect; heartbeat/token stops actual pending work. Do not
   promise cross-database/network atomicity or that every post-authorization cancel wins.
4. Once the service observes a valid provider result, settlement preserves bounded accepted
   truth even after durable cancellation or token cancellation. `action_receipts.rs:168–351`
   already writes actual observations/canonical receipt under run lock without requiring a
   live fence, then independently suppresses caller success through current authority.
   Retain exact entry/tenant/digest validation, duplicate idempotency and conflicting-truth
   audit. No cancelled execution output, successor, job reopening or tool success may result.
   A provider future dropped before its result is observed supplies no fabricated receipt:
   its marker/uncertainty remains, and cancellation is never proof of not-applied.
5. Keep all existing bounds, ownership supervision, SQL constraints and append-only facts.
   The expected production change is the focused authority lock-order correction only;
   further behavior findings reopen the affected architecture gate before dependent edits.
   Prefer typed fixture variants and small helpers; no new public port solely for testing.

## Existing evidence versus work still needed

Paths in this section are under `src/adapters/persistence/workflow/` unless qualified.
Existing tests are source coverage, not new execution evidence on this host.

| Boundary | Existing coverage | Required 04.8 addition/recheck |
| --- | --- | --- |
| Prepared/undispatched | `action_tests.rs:101–131` rejects new intent after raw SQL cancellation; `action_dispatch_tests.rs:394–417` rejects post-marker entry | Real CancelCommand before local/remote dispatch; zero provider polls/local effects/new entries; prepared truth retained; protected action remains undispatched. |
| Lock competition | `control_acceptance_tests.rs`, `control_tests.rs:303–351` exercise control/claim/failure/completion; no action/company-run inversion test found | Reproduce inversion constructor with exact row blockers; corrected local, reserve, enter and receipt-return callers must avoid cycle. Both deterministic cancel-first and action-first outcomes, not timeout-as-success. |
| Marker before entry | `action_dispatch_remote_tests.rs:69–98` uses raw SQL state change | Gate actual service after durable reservation; commit actual cancellation; no provider polling, marker retained, attempt retired once, replay cannot dispatch. |
| Work in flight | `action_dispatch_remote_tests.rs:201–282` drops pending provider on raw SQL cancel/token/ownership loss/deadline | Real command while actual provider is pending, prove future dropped before return, one first poll, durable uncertainty retained; independent token-only case must not pretend run cancelled. |
| Result/receipt/cancel | `action_receipt_tests.rs:277–310` late truth after raw SQL cancel; `action_receipt_resolution_tests.rs:18–114` receipt commit versus completion | Real cancel versus receipt commit in both ordered overlaps, plus token cancellation after result observation/before receipt settlement returns. Accepted provider ledger remains one effect; receipt/actual observation persists and no cancelled-run advancement/output. |
| Duplicate/restart | 04.5 receipt replay and 04.7 late-conflict tests | After cancel and late receipt, fresh service retry cannot poll provider or advance; repeated same actual observation is idempotent, conflicting truth remains append-only without resurrection. Use existing 04.7 conflict tests where exact coverage suffices. |
| Completion boundary | Existing control/completion race and receipt-before-completion tests | Action receipt followed by real competing cancellation/shared completion, in both serialized orders: cancel winner blocks step/output/successor; completion winner retains its committed result, stale cancel reports revision conflict/terminal rather than rewriting it. |
| Parked/reconciled | `action_uncertainty_tests.rs:281–351`, real terminal audit tests; 04.7 control serialization, claim cancellation and late truth suites | Reuse/re-run these exact accepted contracts; no duplicate whole 04.7 matrix. Cancelled safe-replay or proven-not-applied history cannot authorize new calls. |

Preparation does not assert that every row needs a new test. Implementation records exact old
and new named tests against each obligation and fills only uncovered combinations. Public action
ports currently have fixture callers; this point establishes their shared foundation, not later
production HTTP, MCP or Rig integration.

## Ordered implementation and verification

A. First bounded group: authority ordering helper, all affected callers, deterministic regression
for opposite company/run acquisition and cancel-first refusals. Inspect actor stability and
error semantics under the locked reload; preserve authority-revocation tests. B reviews actual
code plus concurrency evidence before the next group. No dependency or schema change expected.

B. Boundary matrix in cohesive test children: undispatched/marker/pending; then observed result,
late settlement/restart/duplicate; then shared completion and accepted uncertainty/reconciliation
integration. Each group reuses real isolated migrated fixtures (`own_database`), genuine service
and CancelCommand operations, recorded provider polls/effects and exact durable snapshots.
For races, establish both contenders with explicit gates and observed actual backend blockers;
use the existing calibrated probes/negative controls from
[04.7 cancellation serialization](CONTRACT-04.7-CANCEL-SERIALIZATION.md). Company contention
alone is not evidence of the exact run lock. Observe the real blocking site for the chosen pair,
retain bounds, release/await all tasks, and ensure clean connections/no claimable residue.
Use full relevant run/execution/job/attempt/wait/receipt/evidence/audit assertions; account for
expected control audit/revision writes rather than treating all state as byte-immutable.
Deferred transaction failure tests must show full rollback of cancellation and continued valid
work under the old fence, not a partially cancelled state. Existing control rollback coverage
can be reused if inspection proves it includes the affected facts.

C. Final combined 04.8 gate: focused new cases, accepted action/control/completion/lease/recovery/
budget and affected reconciliation cases; full stock-2MiB library suite/default parallelism,
unchanged lease-budget gate; `cargo fmt --all -- --check`, `git diff --check`,
`SQLX_OFFLINE=true cargo check --locked --offline --all-targets`, strict Clippy,
SQLx prepare/check after any query edits, fresh migration/schema/checksum verification and
`graft build`. Capture commands, full logs, exact source manifest and DB identity. A test that
skips DB execution is not a pass. First Linux budget failures need measured diagnosis/calibration,
not an automatic raised bound. Independent final actual-code/criteria/integration pass and root
acceptance are required; 04.9 does not start merely because this brief exists.

## Current prerequisites and ownership

Read-only inventory: Linux x86_64, Rust/Cargo 1.99.0. `psql`/Postgres client utilities and Docker
CLI exist; `postgres`, `initdb`, `pg_ctl` absent on PATH (`pg_config --bindir` is `/usr/bin`).
`cargo sqlx` is absent. DATABASE_URL/TEST_DATABASE_URL/SQLX_OFFLINE/RUST_MIN_STACK/CARGO_TARGET_DIR
are unset. No repository `target/` or default Cargo registry/git checkout cache was observed;
`cargo metadata --offline --locked --no-deps` succeeds but does not establish dependency
availability or compilation. Sandbox Docker socket read is denied, so available host containers
are unknown, not absent. No database was contacted or mutated and no test/build was started.

Before implementation verification, establish a task-owned isolated PostgreSQL 18.6 fixture with
create-database privileges and CI pg_stat_statements configuration, explicit identical test/schema
URLs, sufficient connections, verified system/database identity and zero unrelated clients.
Current `.github/workflows/ci.yml` specifies `postgres:18.6-bookworm` and sqlx-cli 0.8.6.
Check host Docker availability with necessary sandbox escalation; if no suitable local resources
exist, provision only task-owned resources and locked tool/dependency downloads as needed.
Author owns build/test/DB processes; reviewer reads artifacts and runs no competing mutation.
Historical damaged55439 and macOS retained55440 identities are not touched. Root decides when
resource work starts; fresh Linux verification remains open.
