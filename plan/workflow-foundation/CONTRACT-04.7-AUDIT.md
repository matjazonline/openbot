# 04.7 affected expansion — one actor audit per evidence command

Status: expansion ready for independent Sol check; not implementation acceptance.
Scope is only the reopened audit cardinality/provenance contract. Original execution
item 6, BRIEF-04.7 sections 1–5/required checks and accepted attribution/witness
corrections remain authoritative. Preserve 04.1–6; do not start 04.8. All applied
migrations through 20260930183000 remain immutable. No historical rewriting,
compatibility backfill, fixture bypass, audit omission, or raised bound.

## Observed mismatch and dependency surface

`action_reconciliation/settlement.rs:5–79` already allocates a fresh event after
evidence/truth/optional continuation and before its immutable command receipt.
`20260928133100_workflow_progression_commit_guards.sql:16–20` instead makes
`(company_id,run_id,execution_id,event_kind)` globally unique. Thus the second
legitimate command on the same execution fails even with a different key, evidence
and revision. Provider scenarios Unknown→Applied, Unknown→final barrier and
Applied-without-output→contradictory final evidence cannot finish. See
EVIDENCE-04.7-GAPS.md and `/private/tmp/workflow-04.7-resume-provider-r6.log`.

Graph exhaustive searches of `workflow_run_events` and `action_reconciled`, plus
callers of `settle`, `audit`, and parent wakeups, identified these live consumers:

- `action_reconciliation/mod.rs:66–145,228–274`: authorized snapshot/settle delegate,
  exact replay and refusal. Replay precedes new facts; refusal has no evidence/audit.
- `action_reconciliation/settlement.rs:5–79,154–202`: actor audit and optional job/run
  continuation. Keep the existing ordering and run lock; no event deduplication.
- `20260930180000_workflow_action_evidence.sql:47–80`: immutable command key,
  evidence identity, event FK, and evidence/audit null shape.
- `20260930181000_workflow_action_evidence_binding.sql:128–202`: bidirectional exact
  command/evidence guards and deferred `check_workflow_control_retry`. Its audit join
  checks company/run/sequence/actor/kind, but omits audit execution equality.
- Other event writers are `admission_write.rs:4–27`, `controls.rs:154–161`,
  `waits.rs:136–144`, `recovery.rs:152–168`, `batch_commit.rs:5–67` and the parent
  wakeup SQL triggers in migrations 20260928145500/20260928150500. Their sequence
  allocation and event uniqueness remain unchanged. Recovery uses attempt-specific
  kinds; controls/admission/wakeups retain nullable execution semantics.
- `wakeups.rs:14–42` reads only `parent_wakeup`, ordered by sequence. SQL command
  FKs reference event primary key, not the replaced execution/kind constraint.
  Graph also finds event snapshots/counts and injected-failure tests, but no reader
  treating `action_reconciled` as an execution singleton. Migration SQL is unindexed;
  its references were searched directly. Ambiguous graph callers were supplemented
  with literal `settlement::settle`, `replay_on`, and `parent_wakeups` searches.

Paths above are under `src/adapters/persistence/workflow/` unless stated otherwise.

## Chosen additive SQL contract

1. Add one migration after 20260930183000, transactionally replacing the old UNIQUE
   constraint with a unique partial index on the **same four columns**, predicate
   `event_kind <> 'action_reconciled'`. Retain name
   `workflow_run_event_execution_kind` for the replacement index if practical.
   All existing kinds, including future non-reconciliation kinds, keep the old
   uniqueness; PostgreSQL's existing NULL execution behavior stays unchanged.
   Event primary key and execution FK stay. This is a targeted cardinality change,
   not removal of the execution audit integrity constraint.
2. Use the existing immutable command's `audit_sequence` as the authoritative
   command→audit identity. Add UNIQUE `(company_id,run_id,audit_sequence)` to
   `workflow_action_evidence_commands` (ordinary NULL-distinct semantics). Two
   commands can never claim the same event. No new audit ID column/table or copied
   command identity is needed: the sequence primary key plus this unique reference
   already supplies an unambiguous reverse mapping.
3. Extend `workflow_action_command_scope_guard` in the new migration, preserving
   its full exact evidence checks. For every command with evidence/audit, require
   an event with the exact company/run/sequence, nonnull **equal execution_id**,
   equal actor_id and literal `action_reconciled` kind. Keep refusal null-shape
   semantics unchanged. Existing evidence link binds command id/key/request digest,
   invocation/digest/marker/actor/scope, so audit derives that full provenance through
   the single immutable command; no caller-provided note can substitute for it.
4. Add an initially deferred AFTER INSERT constraint trigger on reconciliation
   events requiring exactly one matching evidence-bearing command at commit, with
   the same company/run/sequence/execution/actor and nonnull evidence. Event may be
   inserted before command, as the writer already does. Require nonnull execution
   for reconciliation events with a narrowly scoped CHECK. Reject orphan events,
   event reuse, wrong actors/executions and linkage to refusal commands. Event and
   command guards together enforce the relationship in both directions.
5. Make reconciliation actor events immutable with a narrow BEFORE UPDATE/DELETE
   guard: reject mutation/deletion of an OLD reconciliation event and UPDATE of
   another kind into reconciliation. Do not change other kinds' rules or the
   independent parent-wakeup guard. This prevents an initially valid audit from
   being changed later to launder provenance. Event insertion has no revision
   trigger; evidence/conflict/run/job owners still generate revisions.
6. Replace `check_workflow_control_retry` additively, changing only its reconciliation
   audit match to include exact execution equality. Keep the ordinary retry branch
   byte-for-byte behavior, shared safety predicate, exact scheduled job/outcome,
   evidence scope, final generated revision and first OLD state/reason witness for
   `pg_current_xact_id()`. An unrelated or historical audit cannot substitute for
   the scheduled command's unique audit. Historical scheduled command revision
   cannot equal a later reopen's generated revision; do not weaken that comparison
   or replace the current-transaction witness with command previous-state labels.

The migration must validate existing reconciliation command/event linkage without
changing any historical row (including kind, sequence, actor, timestamps, revisions
or evidence). Perform a fail-closed invariant preflight for pre-existing orphan or
mismatched audits/commands before installing guards; normal history created by the
current writer satisfies it. Malformed history is an integrity failure, not a
license to backfill or silently grandfather a forged witness. New unique/check
constraints validate normally. Fresh schema and populated upgrade tests are required.

## Delivery order and compatibility

Sol first independently checks this document against originals and frozen code.
After root records that gate, Sol owns migration/code/test changes and retained PG.
Apply additive migration before runtime tests/new deployment. Existing writer SQL
requires no format or API change and satisfies the new deferred order. Extract a
helper only if local edits require it under src/AGENTS.md; do not redesign settlement.
Existing saved command decoding/replay remains unchanged, and historical events and
their sequences retain identity. Do not use ON CONFLICT DO NOTHING for actor audit.

Then add focused DB tests in a reconciliation audit test module (or existing narrow
modules) and wire it in the workflow test module. Re-run the actual provider tests
without bypasses. Sol chooses function/index/test names and local organization.
No new application/domain port or production verifier is required by this correction.

## Discriminatory acceptance

- Sequential distinct authorized commands on one execution: Unknown→actual Applied,
  Unknown→genuine final barrier, Applied missing/invalid output→contradictory final
  proof, and later distinct usable Applied output. Each committed evidence-bearing
  command has a distinct sequence, exact command actor/execution linkage and its own
  immutable evidence. Assert truth/conflict/reopen behavior, not just command success.
- Exact replay (including after restart/proof expiry) reauthorizes and adds zero
  evidence/events/revision/reopen/provider verification. Same-key changed actor,
  input or revision conflicts without an extra audit. Refusal remains audit-free.
- Real competing same-key and distinct-key commands on the same execution. Owning
  run lock serializes sequence allocation and settlement. Same-key exact contenders
  yield one evidence/audit; distinct same-snapshot contenders leave the loser a
  stale refusal, not a uniqueness exception or fresh grant. A newly snapshotted later
  distinct command can commit its own audit. Keep genuine two-claimant proof-entry
  tests in the larger 04.7 matrix; this correction does not replace them.
- Direct SQL negatives isolate duplicate command audit reference, orphan audit,
  wrong company/run/execution/actor/kind, NULL reconciliation execution, refusal
  linkage, audit UPDATE/DELETE and other-kind→reconciliation laundering. Arrange
  otherwise valid evidence/command scope so the intended guard fails; assert SQLSTATE
  and named constraint/diagnostic. Other-kind duplicate execution/kind must still
  fail; nullable-execution existing admission/control/wakeup behavior must still pass.
- Exact deferred reopen witness: mismatched audit execution/actor or another command's
  audit cannot enable pending work. Preserve actual first OLD terminal/human-wait
  laundering negatives, historical-command/final-revision rejection and normal valid
  waiting/reconciliation reopen. Ordinary retry remains independent and passes.
- Inject deferred command/audit linkage failure after proposed reopen. Compare all
  scoped evidence/coverage/receipt/conflict/command/events plus run/job/revision before
  and after: all transactional writes roll back, while the real provider effect stays.
- Populated upgrade preserves pre-existing legitimate command/audit bytes and exact
  replay; pre-existing malformed linkage fails explicitly without mutation. Fresh
  all-migrations schema has both uniqueness policies and deferred/immutable guards.
  Migration checksums through 183000 remain unchanged.
- Required broader 04.7 checks remain those in BRIEF and EVIDENCE-04.7-GAPS: stock
  2 MiB action and affected suites, formatting/whitespace, locked offline all-target
  compilation, strict Clippy, fresh schema, SQLx prepare/check and graph refresh.
  Passing these focused tests is not whole-04.7 acceptance; full original-criteria/
  actual-code/integration Astra review and root acceptance remain separate gates.

No unresolved product contract was identified. The independent expansion check,
implementation, all runtime acceptance and full 04.7 review remain pending. This
assignment changes this document only and performs no migration or DB operation.
