# 04.7 bounds correction — first bounded implementation

Status: scoped implementation/tests complete; independent actual-code review and
whole 04.7 acceptance remain pending. No 04.8 work. Original Execution 6,
BRIEF-04.7 sections 2/4/5 and acceptance, accepted attribution/witness refinements,
CONTRACT-04.7-AUDIT and CONTRACT-04.7-BOUNDS remain authoritative. Root recorded
the preparation gate after `/private/tmp/workflow-04.7-bounds-expansion-recheck.md`
independently passed contract SHA256
`9ef2a82ccd275db90453a0f3a8e2b04604227c9110afe2736c4b153374000d5b`.

## Changed owner and criterion mapping

- Additive `migrations/20261001090000_workflow_action_proof_bounds.sql` replaces only
  `workflow_action_not_applied_available(uuid,uuid,uuid)`, preserving its signature
  and NULL-on-unavailable convention. It counts up to 129 dispatch markers for the
  exact company/execution and refuses above 128, including receipted/local siblings.
  No Rust production owner, ordinary-dispatch ceiling, retry-safe recursion or bound
  increase was added. Existing reservation, provider-entry, deferred entry/consumption,
  recovery/scheduling and projection callers inherit the shared decision.
- The prior-entry count excludes only the caller-named exact row provisionally; proof
  return still requires its matching consumption joined to the actual consuming entry
  with full company/run/execution/invocation/digest/marker provenance and matching
  evidence. Existing receipt/applied/conflict vetoes, expiry, complete prior coverage,
  no covered consuming row and no older processing attempt remain. Deferred live-fence
  and current transaction identity owners are unchanged.
- Existing three sibling bounds tests now pass: paused 128-to-129 snapshot/settlement
  refusal, overflow after proof scheduling before reserve (zero new entry/consumption),
  and overflow after reserve before enter (committed consumption preserved). Their
  all-table equality assertions and genuine ordinary 129th sibling receipt remain.
- Added `workflow_action_reconciliation_bounds_exclusion_requires_exact_consumption`
  in the existing bounds module: at exactly 128 siblings, proof is available with NULL
  exclusion; an old covered but unmatched row, unrelated invocation's real entry and
  absent entry each return NULL. After genuine reservation, only its exact consumed
  entry returns that proof; NULL and invalid exclusions refuse. Genuine provider entry
  succeeds without altering committed facts. This tests matching/exclusion authority
  with one prior entry, **not** the pending 128-prior/129-total history discriminator.

## Verification and preservation

Full command logs, initial WIP/diff, exact own-before delta, frozen artifacts,
dependency and migration manifests, active function definition and HANDOFF are in
`/private/tmp/workflow-04.7-bounds-correction-01a0f656/`.
Both URLs were explicitly `postgres://mac03@127.0.0.1:55439/workflow_admission`.
No skipped/ignored tests or missing-DB waiver.

| Check | Result / log |
| --- | --- |
| New additive migration | PASS, 4.664542ms; `migrate-run.log` |
| Prior immutable migrations and retained facts | All 49 prior file SHA256 and stored SQLx checksums unchanged; complete retained data dump excluding SQLx metadata unchanged across migration; `migration-preservation.log` / `facts-before.sql` / `facts-after.sql` |
| Stock `RUST_MIN_STACK=2097152`, `SQLX_OFFLINE=true`, locked/offline `--lib workflow_action_reconciliation_bounds_` | 4 PASS, 0 FAIL, 0 ignored; compile 45.88s, test 4.88s; `bounds-r1.log` |
| Same settings, combined `--lib workflow_action_reconciliation_` | 47 PASS, 0 FAIL, 0 ignored, 18.07s; `reconciliation-combined-r1.log`; retains proof competing commands/claimants/loss, clock/expiry, sequential all-proof/mixed siblings, authority and runtime conflict regressions |
| `cargo fmt --all -- --check`, `git diff --check`, own untracked whitespace | PASS; `fmt-check-final.log`, `whitespace-check-final.log`, `own-whitespace-and-wip-preservation.log` |
| `cargo sqlx prepare -- --all-targets --locked --offline` | PASS, 15.43s; `sqlx-prepare.log`; no `.sqlx` diff |
| `cargo sqlx prepare --check -- --all-targets --locked --offline` | PASS, 11.31s; `sqlx-prepare-check.log` |
| `SQLX_OFFLINE=true` locked/offline all-target `cargo check` | PASS, 0.48s; `offline-all-targets.log` |
| `graft build` | PASS; `graft-build.log` |

Before restart, cluster was cleanly shut down with system ID 7691265791172745923,
PG18, data `/private/tmp/workflow-admission-pg-e3aa`. After explicit authorized
startup, verified database workflow_admission, user mac03, PG18.6, port 55439,
socket `/private/tmp`, max_connections 200. All 50 migrations applied. Preexisting
HEAD `c9cb4b434258b6d33ca26def4ff6b5728dd5a6bc` and unrelated source WIP preserved.
No retained-data reset/drop, history rewrite, backfill, shim, stage, commit or deploy.
Tests use existing isolated genuine owner fixtures; no disposable database remains.

## Required next bounded substeps

Pending: genuine 128-prior/129-total successful snapshot/proof/reserve/enter and
129-prior refusal cycles; overflow plus invalid exclusions at Rust/deferred/current-xid
boundaries; populated proof/command upgrade and exact immutable replay discriminator;
all remaining eligibility and RESUME groups 2–4. Fresh schema from isolated tests is
not the explicit populated-upgrade/final-schema gate. Required broader action/runtime
suites, strict Clippy and final full 04.7 checks/integration review remain open.
Astra must independently review this frozen correction before root accepts even
this bounded implementation. Passing 47 scoped tests is not full 04.7 acceptance.

Worker `/root/bounds_implementation`, verified own CODEX_THREAD_ID
`01a0f656-50d9-7341-8ec3-3519639ab745`, requested Sol6.1/high; no spawning.
Startup context sample 26679/258400 = 10.32%; verification sample 86638/258400 =
33.53% at `2026-10-01T07:26:47.456Z`, usage/window estimate from the bundled
context helper. Fresh completion sample and resource lifecycle are in HANDOFF.
