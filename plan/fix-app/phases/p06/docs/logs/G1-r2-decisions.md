# FIXAPP-P06-G1 r2 — workflow storage and application-wide retention

Original requirements: `plan/fix-app/06-workflow-storage-growth.md`, README and authoritative
COORDINATION r20, especially I005/r2. Whole phase is one cohesive implementation/review batch.
No backward compatibility: edit the init migration in place and reset only the p06 databases.
No dependent implementation precedes an independent PASS for this decision revision.

## D1 — Transient witnesses, bounded cleanup without weakening current proofs

The three witness tables are `workflow_action_state_witnesses`,
`workflow_action_schedule_witnesses`, and `workflow_action_claim_retirement_witnesses`.
Every reader requires the current transaction ID. Contrary to the old plan description,
state witnesses already have PK `(company_id,run_id,transaction_id)`; retain all existing PKs.
Claim-retirement witnesses have their own owner trigger; the other two use fact immutability.

- Delete only visible rows whose `transaction_id <> pg_current_xact_id()`. MVCC excludes another
  transaction's uncommitted rows; own current witnesses are explicitly excluded. A visible foreign
  transaction's row cannot be needed by any future current-transaction proof.
- Each table gets a bounded DELETE over a `SELECT ... FOR UPDATE SKIP LOCKED LIMIT $1` selection
  of its exact PK. Default 256 rows per table, hard maximum 1024 per table per invocation.
- Permit exactly this obsolete-witness deletion in both applicable trigger functions. Keep all
  current-transaction delete/update, source ownership and append-only protections. Other fact
  tables retain their existing rules. Cascade deletion of an absent owner remains allowed.
- Retain permanent refusal/audit protection: a witness with a linked audit is already required to
  have the matching permanent `workflow_action_claim_budget_refusals` fact by its deferred guard.
  Sweeping the witness does not remove that fact or audit.
- One transaction for a witness sweep; cancellation/error rolls it back. No capture-skipping
  optimization: whether an action will reconcile later in the same transaction is not local proof.

## D2 — One immutable tenant-scoped copy of each run bundle

Measurement before schema changes is saved in `development-bundle-ratio.json`: the private fresh
p06 development database has zero runs/bytes, so the empirical ratio is unavailable. No production
ratio or measured improvement is claimed. Controlled admission tests will demonstrate sharing.

- Add `workflow_bundles(company_id,digest,bundle)` with PK `(company_id,digest)`, company cascade FK,
  `digest bytea` of exactly 32 bytes, and CHECK `digest = sha256(bundle)` plus 1..4 MiB payload bound.
- Replace `workflow_runs.bundle` with non-null `bundle_digest bytea` and composite tenant FK to
  that table, DEFERRABLE INITIALLY DEFERRED for company/cascade ordering. No fallback column,
  view, dual representation, backfill or legacy decoder.
- Admission serializes the existing frozen bundle once and derives SHA256 on the same transaction/
  connection. It uses `INSERT ... ON CONFLICT(company_id,digest) DO UPDATE SET bundle=EXCLUDED.bundle
  WHERE false`: PostgreSQL locks the conflicting tuple through transaction end WITHOUT updating it
  or firing UPDATE triggers. This is a locking statement, not an immutability exception or a no-op
  payload update. After it holds ownership, read the same tenant/digest row and compare the exact
  stored bytes with the serialized input, then insert the run. A mismatch is an internal error and
  rollback, never reuse of other content. The binding/budget/link commit remains atomic.
- If admission wins existing-row ownership, GC's FOR UPDATE SKIP LOCKED skips that row until the
  reference commits. If GC deletes first, the upsert waits for that transaction and inserts the
  payload again after deletion commits; admission still succeeds. No conflict-DO-NOTHING followed
  by an unprotected gap, no reliance on the deferred run FK alone, and no unbounded retry loop.
- All actual run-bundle readers explicitly join by both company and digest, keeping their existing
  typed byte payload and restore/validation behavior. The exhaustive 292-hit inventory (66 files,
  including versions/bindings/tests that do not change) is `bundle-sites.txt`; use exact SQL owner
  to distinguish run payloads from independent publication/binding snapshots.
- Preserve run snapshot immutability on the new digest and make bundle payload/key immutable.
  Bundle deletion is permitted only without referencing runs, or during company removal. Bounded
  unused-bundle GC locks bundle rows `FOR UPDATE SKIP LOCKED` and rechecks absence of run refs.
  The explicit conflict-row locking protocol above serializes reuse with GC; no pool reacquisition
  or detached work. `G1-r2-author-probes.json` demonstrates both controlled orderings on PostgreSQL
  18.6, including successful admission, exact payload and an UPDATE trigger that rejects all writes.
- Same company/same bytes share one row. Tenant keys never resolve through another company.
  Concurrent duplicate admissions and unused-blob GC must not cause a dangling or mismatched run.
- Adapt physical-schema fixtures and intentional-corruption tests to store a correctly hashed blob
  and reference it explicitly. Preserve fail-closed restore tests; do not mutate an immutable run
  snapshot or add a production fixture bypass.

## D3 — Exact usage/cost history survives deleting runtime provenance

`model_usage_calls` currently cascades through run, execution, job and budget links; leaving that
unchanged without preservation would destroy reports. Reports use arbitrary timestamp ranges,
per-call completion/source/token categories, agent/workflow filters, a bounded chronological scan,
current model prices and exact integer cost aggregation. Coarse daily rollups would change those
contracts, so preserve the reporting inputs per call.

- Add immutable `model_usage_history`, holding company/request identity, agent/workflow/version
  identity, provider/model, original request timestamp, reserved/reported tokens, usage source and
  settlement timestamp. Omit runtime run/execution/root/job links; those records are being removed.
- Retain tenant company FK (cascade), tenant agent FK when present and tenant workflow/version FK
  `(company_id,workflow_id,version_id)` to the published version. Keep the latter FKs deferred so
  a company cascade remains atomic. Existing referenced-entity lifetime protections remain.
- An archive insert must exactly match its still-present live usage row for every retained field;
  it cannot fabricate another tenant/request or edit costs. Archive rows are immutable. A deferred
  constraint refuses an archive row whose live counterpart still exists at commit, preventing
  committed double counting. Company deletion may cascade archive rows; run deletion cannot.
- After locking the eligible tree and its accounting rows, archive all its live call inputs and
  delete its whole runtime tree in ONE transaction. No partial archive/partial tree result may
  commit; failure/cancellation leaves both sources as before. No conflict-ignore that hides a
  mismatched archive row.
- The report loader reads live and historical reporting inputs with UNION ALL in one statement,
  preserving the existing `(requested_at,request_id)` ordering and 10001-row detection BEFORE
  optional filters, current pricing, grouping and truncation. Authorization/filter validation and
  aggregate arithmetic stay unchanged. This is permanent live/history storage, not compatibility
  reads for old representations.
- Verify exact report equality before/after deletion for reported, estimated, mixed and unsettled
  reserved usage; arbitrary time boundaries, model/workflow grouping, agent/workflow filtering,
  current price changes and scan-limit behavior. Archive/live duplication and forged archives must
  be rejected, and a failed tree deletion must roll back archive insertion.
- Downstream p08 disposition: active `model_usage_calls` remain wholly workflow-owned with all
  original live-provenance invariants. Their five workflow IDs can still become NOT NULL; historical
  reporting rows have their separate explicit contract and no detached live run IDs.

## D4 — Bounded global terminal-tree retention and concurrency

The 90-day window applies across ALL companies (I005/r2), measured from the terminal top-level
run's `finished_at`. No company setting or user-facing unscoped delete endpoint is added.

### Discovery and progress

- Discover up to 4 old terminal top-level roots per page across companies, ordered by
  `(finished_at,company_id,id)`, using a partial terminal-root index and a typed keyset cursor.
  The discovery cursor is a hint, not authority, and advances past inspected/skipped/failed roots.
  Wrap after exhausting the range. A protected/error prefix cannot monopolize the first page.
- Discovery applies age/top-level/terminal shape; eligibility is checked under ownership inside
  each root's own transaction. One bad root rolls back only itself, not previously completed roots.
  A missing or newly protected root is a skipped result; errors remain observable and delayed.
- The immutable `workflow_run_budgets` parent/root FK and insertion trigger give the complete tree
  membership. Read all same-company links for this root, validating root identity and terminal run
  rows; the existing root activation bound is 100000. Add an indexed root lookup; do not duplicate
  or change the runtime budget limit. No newly invented smaller size cap silently exempts a tree.

### Locking and eligibility

- Take the company row FOR KEY SHARE first (same company-first order as application commands;
  prevents company-delete/archive-FK inversion without locking owner configuration), then the root
  FOR UPDATE SKIP LOCKED, then the complete tree's run rows in stable order with NOWAIT. A child
  owner conflict skips this candidate and advances discovery. Re-read eligibility after locks.
- Every member must be terminal. Keep a tree with any pending/processing workflow job, any
  unsubmitted decision whose wait remains `waiting`, or any message action in
  `pending`/`uncertain`/`stalled`. These are protected work even if the root is terminal.
- Keep any tree with evidence conflicts or unsettled action reconciliations. A reconciliation is
  settled only by its exact receipt, or unconflicted conclusive applied evidence, or (remote only)
  validated grant-eligible not-applied evidence covering EVERY remote-entry fact and no processing
  attempt. Historical not-applied closure does not require a still-current retry grant: expiration
  prevents new retries, not the historical fact. A consumed old proof cannot cover its subsequent
  entry; an uncovered later entry keeps the tree. Mere replay support is not settlement.
- A retained rerun referencing any tree member blocks deletion. Reruns are independent roots:
  reclaim an eligible referring root first, then its original on a later cursor cycle.
- Lock owned execution/job/accounting/usage rows in the runtime run→execution→job→root accounting→
  usage order before copying/deleting; no provider or network call is part of cleanup. Terminal
  checks plus row/FK locks fence new children, retries, decisions and late evidence/settlement.
- Capture and lock the exact `(company_id,job_id,execution_id)` set before removing its owning
  executions; no workflow job has an execution-delete cascade. After archiving usage, DELETE all
  tree runs in one statement (execution/run-owned cascades occur), THEN explicitly DELETE those
  captured workflow jobs in the SAME transaction, checking the expected removal set. Keep the job
  FK deferred NO ACTION; do not change execution deletion into a new automatic job cascade.
- Ordering is deliberate: owner runs must be absent before explicit job deletion cascades attempts,
  harness checkpoints and remaining job-owned effect facts, so existing append-only guards see an
  absent run owner and permit their normal cascade path. The deferred job FK is temporarily pending
  between these two statements and must be satisfied before COMMIT. A surviving/differently-scoped
  job or any failure rolls back runs, jobs and archive together. Unrelated jobs stay untouched.
- Internal lineage edges disappear with the tree; binding revision and external rerun protection
  remain. The measured
  inbound-FK inventory is `tree-retention-fk-inventory.json` (68 edges). Make its 11 currently
  nondeferred, tree-INTERNAL NO ACTION edges deferred initially deferred for atomic cascade rounds:
  background_tasks_workflow_execution_fk; workflow_budget_receipts_company_id_run_id_root_run_id_fkey;
  workflow_successor_fk; workflow_run_budgets_company_id_parent_run_id_root_run_id_fkey;
  workflow_run_budgets_company_id_root_run_id_fkey; workflow_run_event_execution_fk;
  workflow_wakeup_execution_fk; workflow_parent_execution_fk; workflow_run_parent_link_fk;
  workflow_run_terminal_execution_fk; workflow_wait_event_consumer_execution.
  Keep `workflow_rerun_original_fk` unchanged; block retained referents before delete. Existing
  deferred edges stay deferred, with all tenant/identity checks enforced at commit.
- Run-scoped audit/command/history rows follow their existing cascades; runtime history and those
  scoped replay records have this retention horizon. Live operation idempotency stays unchanged;
  historical usage does not recreate executable commands or serve as a fallback run snapshot.

### Bounds and observation

- Every write operation uses transaction-local statement timeout 5s, lock timeout 500ms and idle
  timeout 5s; application wrapper deadline 6s drops the actual operation/transaction on timeout.
  No detached database or provider task. On timeout/uncertain acknowledgement, retry only via fresh
  discovery and fenced idempotent deletion; never report an unobserved commit as a confirmed delete.
- Separate required port operations: sweep obsolete witnesses, discover a retention page, retire
  ONE tree atomically, sweep unused bundles, read oldest remaining terminal-root finish time.
  This keeps per-tree progress/metrics/cancellation explicit without a generic SQL abstraction.
- A process-wide storage maintenance worker runs once per minute, with a positive delay even for
  full/protected/error pages. It increments committed deleted-tree counts, swept-witness/bundle
  counts, skipped/error counters and publishes the oldest remaining terminal-root finish time.
  Counters describe observed commits on this process, not durable billing/accounting totals.
- Wire one worker into WorkflowEngine's existing workflow worker lifecycle. Increase its reserved
  DB connection accounting from 9 to 10 (pool maximum unchanged); preflight/worker_slots tests must
  fail early if slots plus reservations exhaust the pool. Retain cancellation and shutdown joining.
  This is one additional bounded maintenance connection, no resource-limit increase to mask failure.

## D5 — Verification and implementation boundaries

- One architecture gate for D1–D4, then all dependent phase implementation before one whole-batch
  code review. Focused checks during each feature, not repeated full workflow suites per edit.
- `witness_sweep_*`: all three obsolete tables drain in bounded pages; same-transaction witness
  use and direct-delete refusal survive; a concurrent sweeper cannot delete another transaction's
  invisible/current witness; two sweepers are safe. Rerun affected `action_reconciliation_` guards.
- `bundle_store_*`: two runs share one exact payload; tenant scope/hash/immutability/size guards;
  all run readers/restore and admission/recovery interfaces; competing inserts and blob GC/admission.
- `run_retention_*` on own_database: a whole old root/child tree and runtime rows disappear;
  young/live/open-decision/unsettled/conflicting/rerun-referenced trees remain; settled evidence
  can become eligible, including expired historical proof and uncovered later-entry controls;
  exact usage/cost reporting survives. Fault after archive rolls back both parts. Retry versus GC,
  concurrent sweepers and cross-company pages use actual competing transactions/barrier evidence.
  Protected full pages advance to another company's eligible roots without advancing time.
- Add a real admitted terminal tree with its workflow jobs, attempts and checkpoints; assert all
  expected runtime rows disappear at commit, unrelated/protected jobs remain, and an injected
  failure between run deletion and explicit job cleanup restores the complete original tree and
  leaves no archive copy. The minimized F1 protocol probe is supporting architecture evidence,
  not a substitute for this production-writer/real-fixture test.
- Bundle GC/admission tests must start from an existing UNUSED bundle and prove admission succeeds
  in BOTH ownership orders. Keep the immutable payload trigger active; assert exact payload and
  one committed reference, not merely absence of dangling rows after an admission rollback.
- Native deferred FK tests must reject invalid scope at COMMIT, preserving rollback assertions.
  Metadata changes do not permit dangling committed references. Use actual runtime fixtures where
  possible; do not bypass guards or normalize corrupted fixture data to obtain a pass.
- Final affected union includes workflow persistence/admission/recovery/reconciliation, model
  usage/report consumers, lifecycle/worker slots and affected HTTP run readers. Run SQLx prepare,
  offline all-target compile/clippy-Dwarnings, fmt, whitespace and fresh migration checksum checks.
  No full suite/check.sh/stack-budget under explicit I004. No external-provider claim.
- Refresh graft after major source changes. Source snapshot excludes ONLY
  `plan/fix-app/phases/p06/docs`; freeze the entire phase source during independent review.

## R1 finding corrections and evidence

- FIXAPP-P06-G1-F1: explicit captured-job cleanup after run-owner removal completes the deletion
  closure while retaining deferred NO ACTION and existing fact guards. The isolated protocol probe
  includes attempt/checkpoint/job-owned guarded facts, archived/live inputs and an unrelated owner.
- FIXAPP-P06-G1-F2: nonmodifying ON CONFLICT locking gives the reused tuple durable ownership;
  controlled PostgreSQL probes prove GC skips admission-owned reuse and GC-first admission waits,
  recreates the row and commits. The immutable UPDATE trigger remains enabled and never fires.
- Evidence `G1-r2-author-probes.py/.json`, both corrected protocol assertions PASS and scratch schema
  removed. The initial probe's overlong PGAPPNAME hid its barrier (not a protocol failure); preserve
  `G1-r2-author-probes-r1-failed.json` and the original script, corrected with a short app name.
- No production source/schema change yet and no Rust suite run. Real production-writer coverage
  remains mandatory during implementation and final code review. Other D1–D5 decisions unchanged.
