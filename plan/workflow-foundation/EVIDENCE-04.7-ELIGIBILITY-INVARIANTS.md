# 04.7 coupled eligibility invariants and absent action/attempt history

Implementation subgroup only; independent review and root acceptance remain due.
Base HEAD `9cff5164fa0694a4e7664f6ff8c69e473d4c2a97`; preserve all prior WIP.
Scope: BRIEF-04.7:194–240, Execution6:29–39, archive RESUME:355–377 and accepted
CONTRACT-04.7-ELIGIBILITY-REMAINING's coupled shapes/activation, unactivated and
exhausted-pending rows, with independent expansion-check.md. No row2/04.8.

New `action_reconciliation_eligibility_invariant_tests.rs` is nested under accepted
remaining_tests, whose only source change is module registration. Production/schema,
retained data, resource bounds, accepted history sources, PROGRESS/RESUME and commits
remain unchanged. No subagent created. Assigned skill/context/PostgreSQL references
and root/source/application/persistence repository guides were applied.

Coverage in four tests:

- Coupled output/route shapes: genuinely admitted unactivated row, then genuine
  activation. Each SQL attack rolls back every public table and asserts exact23514
  workflow_execution_output_shape or workflow_route_shape. Output without completion,
  completion without output, completion/output without activation, and route/successor
  without completion are separate. Successor attack creates a same-run matching-step
  next-ordinal row within the rolled-back transaction, so the earlier progression
  trigger permits that association. A genuine provider receipt and complete_io prove
  the valid activated/completed/output/end-route shape with no successor.
- Activated/frozen-input clearing: genuine unsafe provider entry, ordinary owner
  retirement and barrier. Two shape-valid attempted activation/input rewrites assert
  exact23514/workflow_activation_immutable and full rollback. The unchanged genuine
  retired history then schedules through actual final proof.
- Unactivated public refusal: admitted data.map projection input is explicitly changed
  to invalid `{}`. Ordinary activate returns BadRequest without activation, debit,
  attempt or action facts; every public row remains equal. No genuine scoped intent
  or marker exists. A syntactically valid input using identifiers from a separate
  genuine control is submitted at the public SQL reconciliation snapshot boundary;
  its scope/actor/revision name this admitted execution. It returns exact
  NotFound("Workflow resource") at absent scoped intent, before marker lookup, and
  writes nothing. No marker/proof/intent/attempt is inserted or copied into target
  history. SQL reopen predicate is false. Separate activated/parked control schedules.
- Exhausted pending: explicit adversarial retry_count=max_retries projection matches
  existing pending owner tests. Actual poll_work classifies ExhaustedPending; barrier
  competitors call retire_exhausted_work, exactly one succeeds. No attempt/action/
  budget history is invented. Full snapshot permits only ordinary job status/time,
  run terminal state/revision, one attempts_exhausted audit and one exactly scoped
  state witness preserving initial queued state/NULL waiting reason and matching
  the terminal owning run row's transaction ID. Every prior witness is retained
  before normalizing only that asserted append. Original allowance,
  execution, frozen bytes and budgets remain unchanged; repeated retirement/poll/claim
  preserve every row. The same exact absent-intent public refusal adds no evidence;
  predicate false is explicitly coupled exhausted/unactivated/missing-attempt coverage.
  Ordinary RetryCommand returns Unsafe and writes only its immutable refusal receipt.
  Separate genuine unsafe-action retirement/final-proof control schedules.

These cases explain coupled schema/owner invariants and actual public rejection;
they do not claim independent persisted-field reconciliation conjunct sensitivity.
No rejected SQL setup is followed by a target reconciliation call.

## Freeze, resources and verification

Source freeze: invariant SHA256
`10848af16796b392f31d3036ca66e622cc3487400ec9a0bd0256e62a8f9feacd`;
remaining_tests SHA256
`dd2ff4a7ca464c889ee1f9d045534b6fb66c1b93964ce15f345793ecc90b3035`.
Self-audit completed: exact constraint prerequisites, real provider/owner controls,
post-setup revision, honest absent-intent boundary, no invented history, complete
rollback and stock-stack boxed admission/action/provider seams. Source frozen during
independent review; any runtime correction will be explicitly re-frozen.

Retained PostgreSQL identity verified at check startup: system7691265791172745923,
data/private/tmp/workflow-admission-pg-e3aa, port55439, mac03/workflow_admission,
max_connections200. Both URLs explicitly set; AdmissionFixture owns isolated migrated
databases. Exclusive build/SQLx/fixture ownership, defaultparallel, SQLX_OFFLINE=true,
RUST_MIN_STACK=2097152. No bounds raised.

Logs: `/private/tmp/workflow-eligibility-finish-20261002/`. Initial two compile runs
failed on local imports and non-Debug unwrap_err respectively; neither is runtime
evidence. Final exact-four-test run is compiling (`invariants-targeted-final.log`).
Format and whitespace PASS (`invariants-fmt.log`, `invariants-diff-check.log`).
Offline all-target, strict Clippy, migration inspection, SQLx prepare/check and graft
refresh remain due for this new subgroup. No full04.7 integration claimed.

Context UUID01a0fc48-d72f-7373-893c-a8bac43be5ab: preimplementation60,669/258,400=23.48%
at11:08:10Z; preparation78,573=30.41% at11:10:01Z; implementation108,251=41.89%
at11:13:59Z; freeze114,162=44.18% at11:16:27Z. Sources token_usage_record.usage /
task_started.model_context_window; depth0 latest-input estimate.

Runtime update: final run compiled in1m18s, executed the exact4 tests at default
parallelism/stock2MiB, and finished2passed/2failed in4.30s. Activation immutability
and unactivated/no-scoped-action cases PASS. Coupled shapes reached the completion
positive then failed because fixture_provider_operations was not initialized by
ledger(); exhausted competing retirement passed its owner checks but its full
snapshot assertion missed the legitimate queued→failed state-witness append.
Independent reviewer and worker compact log diagnostics agree the only remaining
snapshot delta is workflow_action_state_witnesses length0→1, carrying exact company,
run, initial_state queued, NULL initial_waiting_reason and transaction ID. Source
remains frozen during independent review. Both corrections and rerun remain due;
no failed constructor is reported as passed. No active test process remains.

Fresh runtime milestone122,456/258,400=47.39% at11:19:06.274Z, same UUID/sources.
Worker will stop substantive work by50%; a fresh worker may need to complete the
grouped runtime/review corrections and remaining gates.

## Reviewed findings corrected, combined verification

Both findings in `/private/tmp/workflow-eligibility-finish-20261002/invariants-review.md`
were corrected by fresh Sol UUID01a0fc59-8473-7600-af1a-fedebb481c84. The genuine
completion control initializes provider ledger tables/operation via ledger() before
dispatch. The synchronous retirement_delta helper asserts the exact one new
company/run/transaction/queued/NULL witness while preserving all prior witnesses;
the transaction ID is independently read from the retired owner row's xmin. Only
that verified append is normalized for the whole-public-table comparison.

Corrected frozen invariant SHA256
`2b46f55d0d756a278d545eabfd8ca4e54ae114747929840dd6e500f72146e6c2`;
remaining_tests registration remains
`dd2ff4a7ca464c889ee1f9d045534b6fb66c1b93964ce15f345793ecc90b3035`.
No other source, production, migration, resource-limit or retained-data changes.

Combined `cargo test --locked --offline --lib workflow_action_reconciliation_eligibility`
PASS12/0failed/0ignored, default parallelism, SQLX_OFFLINE=true, stock2MiB
RUST_MIN_STACK=2097152, 22.21s. This includes all four corrected invariant cases
and accepted poison/runnable/lease/root-budget/completed-history subgroups.
Both URLs explicitly target the retained authorized endpoint. Complete logs and
closing evidence live under `/private/tmp/workflow-eligibility-corrections-20261002/`.

The retained cluster was verified system7691265791172745923,
data/private/tmp/workflow-admission-pg-e3aa, port55439, mac03/workflow_admission,
max200. Disposable workflow_eligibility_corrections_20261002 applies all53 migrations
fresh; its schema inspection records output/route/lease checks, activation/state
witness triggers, and exact reconciliation reopen/root-budget predicates. All53
SHA384 migration checksums match repository bytes in retained and disposable DBs.
Prior production behavior and unchanged sibling-suite evidence remain reusable;
the new fixtures themselves ran in their own fresh migrated AdmissionFixture DBs.
Root acceptance and independent corrected-source/integration review remain due.

Closing checks all PASS on the frozen corrected source: format/whitespace, locked
offline all-target compilation39.09s, strict Clippy1m06s, SQLx prepare36.72s,
prepare --check35.92s, and graft build. No SQLx-cache/migration/limit diff. Task-only
fresh schema database was dropped after inspection; retained cluster identity and
data preserved. Exact commands/results/logs/resource and evidence-reuse decisions:
`/private/tmp/workflow-eligibility-corrections-20261002/evidence.md`.
No active process/pending permission remains; exclusive build/DB/SQLx ownership
returns to root. No row2 edits until assigned; no full04.7 acceptance implied.
