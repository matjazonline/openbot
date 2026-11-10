# p06 agent review findings

Severity scale (as in p05):
- Major: a wrong decision or a missing safety property; blocks.
- Minor (actionable): the decision is right in shape, but a rule, a contract or a check it rests on
  is missing or wrong; fix it before the dependent code.
- Optional suggestion: may be declined with a reason.

Every gate below was reviewed as a decision record before any dependent code exists. "Evidence"
cites the gate text (TASK.md, sha256 59505897…bead4 at review) and the code it was checked against.

## MEM-P06-A1 findings — common context

2026-11-10; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p06/docs/TASK.md (`## p06-A1 architecture gate
(revision 1)`, lines 63-181); review record
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p06/docs/REVIEW.md; batch p06-A1 (service retention,
purge, forgetting, tenant reassignment, credential listing); stage architecture gate; round note MEM-P06-A1-R1
(agent-review-notes.md); reviewer claude-opus-5-5, effort xhigh (B abdd866fb428eea05); phase `p06`; worktree
/Users/mac03/.codex/worktrees/60a0/openbot (detached HEAD a6bed48a); artifact digest
7ce18038c31f60d29ebd6570f3b65c2868b4dc80cfc1f7ca7d10dc295cbe21e6 (snapshot excluding p05/p06 docs and
COORDINATION.md); gate text TASK.md sha256 59505897c00674dd6c6b42cec0d4eccd9e7b7817d3012a78921a0c741a4bead4.

## MEM-P06-A1-F1 — the removal's marking steps lock a parent after its children, and the stated invariant says they never do

- Context: round MEM-P06-A1-R1; decision A1-1 and the "Failure modes and invariants" paragraph.
- Classification: Minor, actionable; concurrency protocol (D13 lock direction).
- Evidence:
  - TASK.md:88-93 ends the removal with "namespaces with no remaining rows get `content_removed_at`".
    A deleted tenant "loses its grants, idempotency records, spend days and schedule row, then gets
    `content_removed_at`".
  - Both marks are UPDATEs of the parent row, which take `FOR NO KEY UPDATE` on it. If the tenant step
    deletes its children and marks the tenant in one transaction, it acquires child locks and then the
    tenant row. D13 forbids that: "A transaction may skip levels but never acquires upward"
    (implementation-decisions.md:643-644).
  - TASK.md:155 claims the sweep "locks only content rows, never the namespace or tenant `FOR NO KEY
    UPDATE`". That is false for the marking statements.
  - No deadlock is shown today. The transactions that lock a deleted tenant's row do not then wait on
    those children: revoke_credential (services/memory/src/adapters/postgres/mod.rs:1168-1230) and the
    delete no-op (mod.rs:453-548). The protocol should not rest on that.
- Required:
  - Run each mark in its own transaction that locks only the parent. For example:
    `UPDATE namespaces SET content_removed_at = now() WHERE state = 'deleted' AND content_removed_at IS
    NULL AND NOT EXISTS (…content of it…)`.
  - Treat the tenant the same way, after its child deletes have committed in their own step
    transactions.
  - Correct the invariant to: the sweep locks content rows, and locks a deleted namespace or tenant row
    only in a marking statement that holds no child lock.
  - Include the marking in the two-sweeper test.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A1-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): FIXED. Each content step runs without a parent lock. Each mark is a separate transaction that locks only its namespace or tenant row and checks NOT EXISTS, and the invariant (TASK.md:155-157) is corrected. The tenant mark's predicate omits label blanking; see new F7.

## MEM-P06-A1-F2 — two new races have no competing-claimant test, and one has no rule

- Context: round MEM-P06-A1-R1; decisions A1-2 (purge replay) and A1-5 (reassignment); the verification
  plan.
- Classification: Minor, actionable; concurrency protocol and verification.
- Evidence:
  - Purge.
    - The idempotency claim comes after the namespace lock (TASK.md:99-101; D13,
      implementation-decisions.md:654).
    - Of two identical purges, the loser blocks on the namespace row. It then finds the namespace
      `deleted` inside its own transaction, before its claim, and gets the writer re-check's 403
      (services/memory/src/adapters/postgres/memory.rs:111-121).
    - The replay rule (TASK.md:104-106) is triggered by "an authorization refusal", which reads as the
      pre-transaction grant check. So the loser answers 403, although D5 says a concurrent identical
      request replays (implementation-decisions.md:268-270).
    - The plan (TASK.md:162-169) tests replay only sequentially.
  - Reassignment.
    - "A request the old provisioner already holds a tenant lock for finishes first; its next request
      is `not_found`" (TASK.md:136-137) is a claim about a race.
    - The plan checks the old provisioner only after the move.
  - AGENTS.md:24: "When adding or changing a concurrency protocol, include a test with competing
    claimants rather than relying only on sequential mocks."
- Required:
  - State that purge's in-transaction "namespace not active" refusal runs the same same-credential
    lookup, as a fresh read after the transaction rolls back.
  - Add a test with two identical purges on separate connections: one executes, the other answers
    `replayed: true`, and a different key gets 403.
  - Add a reassign test that runs while an old-provisioner request holds the tenant row on a second
    connection. That request commits first; the tenant then moves; the old provisioner's next call is
    404 (`tenants/create` answers 409).
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A1-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): FIXED. Purge claims right after its locks and before re-checking state and grant, so the loser of two identical purges replays; the pre-lock refusal consults the record for the same credential and hash only. Three tests on separate connections are named (identical purges; reassign waiting on an in-flight request; reassign racing old-provisioner writes). A concurrent identical purge by another credential is unspecified; see new F8 (optional).

## MEM-P06-A1-F3 — the limits of deletion omit copies in other namespaces and restore from backup

- Context: round MEM-P06-A1-R1; decision A1-7, which 06A.2 requires ("Document the limits of deletion
  after information has already been consumed").
- Classification: Minor, actionable; documentation, plus a forgetting decision the gate should state.
- Evidence:
  - Copies in other namespaces.
    - A save may cite a parent observation in another granted namespace
      (services/memory/src/adapters/postgres/memory.rs:371-407).
    - The parent link cascades on delete (services/memory/migrations/0001_init.sql:361-362). Deleting,
      expiring or purging the parent therefore removes only the derived observation's link row. Its
      title, body and label stay in the other namespace.
    - A1-7 (TASK.md:145-151) lists text returned to clients, prompts, transcripts, frozen preparations
      and the provider. It does not list derivations inside the service.
  - Restore from backup.
    - A service database restored from a backup taken before a purge or delete brings that content
      back as active.
    - After 30 days OpenBot's settled-absent lifecycle rows are removed (A3-2), so nothing re-applies
      the deletion.
    - A1-7 does not mention backups.
- Required:
  - Decide and document whether a derived observation in another namespace survives its parent's
    deletion. Expected: it survives, because it is that namespace's own memory under its own label.
  - Test it: purge and sweep the parent namespace; the copy still fetches, and its parent link is gone.
  - In the limits of deletion, add backups. Deleted data persists in backups until they rotate. A
    restore must re-apply the deletions committed after the backup; the audit's `tenant.delete`,
    `namespace.delete` and `namespace.purge` rows, kept 400 days, list them.
  - Carry the restore procedure to A4.
  - Optionally note that row removal becomes physical only after vacuum.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A1-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): FIXED. Copies keep their own text and label (implementation-decisions.md:763-768, verified), documented as a limit and tested. Restore: correctly reasoned that the restored audit log cannot list later deletions; OpenBot's absent rows (kept 30 days) or an operator-held list must be re-applied before serving; the procedure is carried to A4. Optional additions in new F8.

## MEM-P06-A1-F4 — free-text credential labels are kept forever as if they were opaque ids

- Context: round MEM-P06-A1-R1; decisions A1-1 (credential rows of deleted tenants) and A1-4 (retention
  table).
- Classification: Minor, actionable; retention of possibly sensitive data. 06A.1 requires retention
  "without making sensitive data impossible to delete"; D9 keeps only opaque ids forever.
- Evidence:
  - TASK.md:94-95 keeps deleted tenants' credential rows, including an "opaque label".
  - The label is free text chosen by an admin. It is checked only for 1–128 non-blank characters
    (services/memory/src/application/service.rs:415-421; migrations/0001_init.sql:77).
  - OpenBot's label is `ob:<namespace>`, but another client's label can name a person or a device.
  - The uniform `credential_revoked` answer needs the id, verifier and revoked state, not the label.
  - D9 keeps forever only "Tombstones (opaque ids only)" (implementation-decisions.md:533).
  - Revoked credentials of live tenants are also kept forever (implementation-decisions.md:1910), and
    A1-4 has no row for credentials.
- Required:
  - Either overwrite labels when a tenant's content is removed (for example with a fixed `removed`), or
    restrict labels to the external-id character set and document them as retained data.
  - Add a credentials row to A1-4's retention table: rows are kept forever for the uniform refusal, and
    grants are removed with their namespace.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A1-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): FIXED. Labels become `removed` at tenant removal, and credentials have their own README row. See new F7: the mark must wait for the blanking.

## MEM-P06-A1-F5 — finished operations and source metadata of a live namespace grow without a bound

- Context: round MEM-P06-A1-R1; decision A1-4 ("tombstones and finished operation rows: kept until the
  namespace is removed").
- Classification: optional suggestion; retention.
- Evidence:
  - TASK.md:128 keeps finished operation rows until the namespace is removed.
  - Sources keep their metadata after the 30-day null-out (implementation-decisions.md:1071-1079).
  - Every OpenBot capture adds one operation, two operation-source rows and two source rows to the
    agent's namespace (per-part `source_ref`, p05 B6), for the namespace's whole life.
  - Failed, cancelled, expired and skipped operations are referenced by no observation.
- Suggestion:
  - Give operation and source metadata an explicit row in the README table, with the reason
    (provenance of generated observations, the one-revision-one-content rule, `generations.status`)
    and the growth per capture.
  - Optionally sweep finished operations that no observation references once their request is nulled,
    together with their operation-source rows. Keep sources, which hold the revision rule.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A1-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): TAKEN. Unreferenced finished operations, with their op-source rows, go 30 days after finishing; unreferenced sources go 30 days after their null-out. Checked closely: acceptance and save resolve sources under row locks, and the sweep locks candidates SKIP LOCKED and re-checks references. A revision is immutable while anything references it, and a removed revision re-records as a fresh row. That is no new oracle and no laundering path: a client can always record the same text under a new revision. This amends D9's provenance sentence; see new F8 (optional).

## MEM-P06-A1-F6 — small contract gaps in reassignment and listing

- Context: round MEM-P06-A1-R1; decisions A1-5 and A1-6.
- Classification: optional suggestion; contract precision and efficiency.
- Evidence and suggestions:
  - (a) The type of `remaining` is not stated. A1-5 returns `{moved, remaining}` (TASK.md:131-133),
    while A3-7 loops "until `remaining = false`" (TASK.md:248). Pick one; a count is more useful.
  - (b) Behaviour for `from = to` is not stated. Refuse with 409 `invalid_target`, or define it as a
    no-op.
  - (c) Lock the two admin credential rows in id order.
  - (d) Listing cost.
    - A label filter scans the tenant's whole credential set through the `(tenant_id, id)` key, and
      that set only grows. An index on `(tenant_id, label, id)` serves `label` together with `after`.
    - `grants_for` runs once per listed credential (mod.rs:1149-1159), so a 1,000-row page issues up to
      1,000 queries while holding the tenant `FOR SHARE`. One grants query per page would do.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A1-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): TAKEN, all five points.

## MEM-P06-A1-F7 — the tenant mark does not wait for label blanking, so a large tenant can keep labels

- Context: round MEM-P06-A1-R2; the fixes for F1 (tenant marking) and F4 (label blanking).
- Classification: Minor, actionable; the F4 property can fail silently.
- Evidence:
  - The revision-2 tenant step (TASK.md:185-193) runs the child deletions and the label blanking in
    their own bounded transactions, at most 500 rows each. It then marks the tenant `WHERE NOT EXISTS
    any unremoved namespace or child row`.
  - Credentials are child rows that are kept, so "no child row" cannot include them. Nothing in the
    predicate says the labels are all blanked.
  - Candidates are deleted tenants that are not yet marked (A1-1, TASK.md:92-93). A tenant with more
    credentials than one batch can therefore be marked after a partial blanking. Once marked, it is no
    longer a candidate, and the remaining labels are never blanked.
  - OpenBot alone holds one credential per namespace plus revoked strays, so a large company can have
    more than 500.
- Required:
  - Add `NOT EXISTS (credentials of the tenant whose label <> 'removed')` to the tenant mark, or key the
    blanking on the tenant's deleted state rather than on the mark.
  - Test it: a deleted tenant with more credentials than one batch has every label blanked before it is
    marked.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A1-R3 (digest 7ce18038…21e6, TASK.md sha256 b5c3606b…): FIXED. Blanking is keyed on the tenant's deleted state, not on the mark: its candidates are any deleted tenant's credentials with `label <> 'removed'`, at most 500 per tick, SKIP LOCKED. The tenant mark's NOT EXISTS adds "no unblanked label". The named test uses 1,201 credentials over three ticks, and the mark comes only after the last batch.

## MEM-P06-A1-F8 — three small points left open by revision 2

- Context: round MEM-P06-A1-R2; the fixes for F2, F3 and F5.
- Classification: optional suggestion; contract and documentation precision.
- Evidence and suggestions:
  - (a) A concurrent identical purge by another credential.
    - Purge now claims before re-checking state and grant (TASK.md:194-200). If it uses the generic
      `claim`, an identical request from another credential that also held the `purge` grant runs D5's
      `replay_authority` (memory.rs:182-184) and replays.
    - A1-2 and the plan say another caller gets "the ordinary refusal".
    - Either is safe: that credential was authorized when the request arrived. State which one applies,
      and test it.
  - (b) Restore and other clients' deletions. A restore also undoes observation deletes and corrections
    made after the backup. OpenBot makes none, but other clients do, and only they can re-apply them.
    Say so in A4's restore procedure.
  - (c) Taking F5 changes D9 ("Identity, revision, level and digest stay for provenance",
    implementation-decisions.md:531-533). Record that amendment, together with "a nulled revision is
    refused at acceptance until removed, then accepted as new evidence", where the D9 retention rules
    are documented.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A1-R3 (digest 7ce18038…21e6, TASK.md sha256 b5c3606b…): TAKEN. (a) Another credential's concurrent identical purge is refused, with a test, but the chosen code `not_found` is not what a fresh execution gets; see new F9 (optional). (b) Carried to A4's restore procedure. (c) The D9 amendment is recorded in a new 06A "As implemented" section.

## MEM-P06-A1-F9 — another credential's refusal code differs from what a fresh execution gets

- Context: round MEM-P06-A1-R3; the fix for F8(a).
- Classification: optional suggestion; refusal uniformity (D3).
- Evidence:
  - A1 revision 3 (TASK.md:252-257) refuses another credential's concurrent identical purge with
    `not_found`, "exactly as a fresh execution by that credential would be".
  - A fresh purge by a credential without the grant is refused with 403 `forbidden`. That holds in
    both places it can be refused:
    - before the transaction, by authorization (`denied`, services/memory/src/application/memory.rs:36-45);
    - inside the transaction, by the namespace re-check (memory.rs:111-121, the writer path).
  - D3 makes ungranted, unknown and deleted namespaces one indistinguishable 403
    (implementation-decisions.md:191-193).
  - Reaching this case requires guessing the other credential's exact key and `expires_at`, so it is
    no practical oracle. But the decision text contradicts itself, and the named test would fix the
    inconsistent code in place.
- Suggestion: answer 403 `forbidden` in this race, and have the test assert it.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).

## MEM-P06-A3 findings — common context

2026-11-10; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p06/docs/TASK.md (`## p06-A3 architecture gate
(revision 1)`, lines 183-263); review record REVIEW.md; batch p06-A3 (OpenBot retention, maintenance driver, rotation
adoption, absent steps, credential paging); stage architecture gate; round note MEM-P06-A3-R1; reviewer claude-opus-5-5,
effort xhigh (B abdd866fb428eea05); phase `p06`; HEAD a6bed48a; digest and gate text as for A1. The A3 carries already
in the tree (B6-F1–F3, F16) are not part of this gate; they are reviewed with A3's code.

## MEM-P06-A3-F1 — `failed` is not a final run state; the retention of a save rests on an unstated run-deadline bound

- Context: round MEM-P06-A3-R1; decision A3-1 and the invariant "a removed intent can never be
  re-enqueued".
- Classification: Minor, actionable; correctness of the retention predicate.
- Evidence:
  - A3-1 (TASK.md:196-204) treats `succeeded`, `failed` and `cancelled` as terminal.
  - A control retry reopens a `failed` run, keeping the same run and execution ids
    (src/adapters/persistence/workflow/control_retry.rs:11-17, 57-60). The schema says so too:
    `stamp_workflow_run_finished`, "leaving one (retry) clears it"
    (migrations/20260817000000_init_schema.sql:9182-9198).
  - A re-executed memory.save derives the same key `ob.s.<execution>`
    (src/adapters/persistence/workflow/memory_steps.rs:279-280).
  - If that intent had been removed, the step would enqueue a new intent with a new `expires_at`. Once
    the service's idempotency record was swept (A1-4), the save would execute a second time.
  - Two things prevent this today, and A3-1 states neither:
    - `workflow_control_retry_safe` requires `owner.deadline > clock_timestamp()` (schema line 2919);
    - a run's deadline is its admission + 24 h, or inherited (admission.rs:6; admission_write.rs:146-156).
  - A longer deadline would silently break the invariant. AGENTS.md: a raised limit must leave something
    that fails.
- Required:
  - Put the fact in the predicate. For example: `run.state IN ('succeeded','cancelled') OR (run.state =
    'failed' AND run.deadline < now())`, or require `run.deadline < now()` wherever a run is checked.
    Give the reason in A3-1.
  - Add a test: a failed run whose deadline is still in the future keeps its 8-day-old intent.
  - Add a mutation probe that drops the deadline term.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A3-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): FIXED. A save intent is removed only when its run is retired, or terminal with `run.deadline < now()`. Tests on both sides of the deadline and a mutation probe are named.

## MEM-P06-A3-F2 — the documented rotation order makes lifecycle passes in the window fail definitively

- Context: round MEM-P06-A3-R1; decisions A3-4 and A3-7 (with A1-5).
- Classification: Minor, actionable; operational protocol and documentation.
- Evidence:
  - A3-7 (TASK.md:247-250) documents this order: issue the new provisioner, run `tenants/reassign`
    until done, then switch OpenBot's token.
  - Between the reassign and the switch, OpenBot's rows record the old id and the old token is still
    configured. `pass` (src/application/memory/lifecycle.rs:246-265) therefore sees no mismatch.
  - The service, however, now treats the old token as not the manager:
    - `tenants/create` answers 409, and the tenant steps `Fail("tenant_conflict")`
      (lifecycle.rs:342-346);
    - `tenant_absent`'s delete answers 404, giving `Fail("tenant_unmanageable")` (lifecycle.rs:386-396);
    - A3-5's namespace probe answers 409, giving `Fail("tenant_unmanageable")`.
  - Each of these is a definite failure that needs an owner's "Retry failed provisioning", company by
    company.
  - Switching the token first lets A3-4 absorb the window. Rows recorded under the old id are postponed
    (`provisioner_mismatch`) while the probe answers 409, and adopt the new id once it answers 200.
  - One gap remains in that order. A namespace row created after the switch for a company not yet
    reassigned records the new id and gets 404 from `create_namespace`. That is retried as `not_found`
    on the bounded budget (lifecycle.rs:179-199 with `RetryBudget::of`), so the reassign must follow
    promptly, or such steps must postpone while the company's tenant is recorded under another
    provisioner.
- Required:
  - Document the safe order: issue the new provisioner; switch every OpenBot process to it; reassign
    until done; let rows adopt; revoke the old credential.
  - Handle the new-namespace case in the switch-first order.
  - Say what an operator does after the reverse order: "Retry failed provisioning".
  - Test the documented sequence against the contract fake: a reassign in the middle of it leaves no
    failed row.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A3-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): FIXED. The safe order is documented. Every "not ours" answer postpones `provisioner_mismatch` without spending an attempt, which covers namespaces created in the window and makes a rolling restart harmless; it is visible through `memory_lifecycle_postponed_rows{outcome}`, and the full sequence is tested against the fake. The adoption probe still cannot confirm a tombstoned tenant; see new F5.

## MEM-P06-A3-F3 — monitoring counts scan whole tables, and the sweep order defeats its index

- Context: round MEM-P06-A3-R1; decisions A3-1, A3-3 and the verification plan.
- Classification: optional suggestion; efficiency and test design.
- Evidence and suggestions:
  - Full-table counts.
    - `memory_deliveries_rows{state}` and `memory_lifecycles_rows{state}` (TASK.md:213-220) count
      every row by state every 5 minutes, including all terminal intents of the retained week and all
      settled lifecycle rows.
    - Only the open, failed and stuck counts and the backlog are actionable, and existing or new
      partial indexes serve those.
    - Gauge those, rather than every state.
  - Sweep order.
    - A3-1 locks candidates in id order ("the E9 order"), but the new index is on `(created_at, id)`.
      With a backlog, every pass sorts all eligible rows.
    - The E9 order prevents deadlocks between settlements that wait. A `SKIP LOCKED` sweep never waits,
      so `ORDER BY created_at, id` is just as safe and follows the index.
  - EXPLAIN checks: on a small test table the planner prefers a sequential scan. Run the check with
    `SET LOCAL enable_seqscan = off`, or with enough seeded rows, so that it proves the index can serve
    the query.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A3-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): TAKEN (sweep ordered by `(created_at, id)`; gauges count open rows and the backlog; EXPLAIN with `enable_seqscan = off`).

## MEM-P06-A3-F4 — document the open-ended waits and the adoption edges

- Context: round MEM-P06-A3-R1; decisions A3-4, A3-5 and A3-7.
- Classification: optional suggestion; documentation of admin and misconfiguration edges.
- Evidence and suggestions:
  - (a) Waits on a disabled tenant (A3-5).
    - The namespace absent step of a tenant that stays disabled waits indefinitely, re-checked every
      `TENANT_WAIT`.
    - A3-2 never removes it, because it is not settled.
    - Say so in docs/deploy.md, and make such rows visible next to the failed and stuck gauges.
  - (b) An operator credential configured as OpenBot's provisioner.
    - `tenants/create` answers 200 for every tenant (`may_manage`,
      services/memory/src/domain/admin_policy.rs:32-36).
    - Adoption would then record every row under the operator's id.
    - Document that OpenBot must hold a provisioner credential.
  - (c) A probe 200 does not always mean an earlier reassignment. It can also mean the probe created a
    tenant whose old create never landed, which is intended under the create-first rule. State this
    where A3-4 says a 200 "proves" management.
  - (d) The rotation documentation should also say that rows already `failed` need "Retry failed
    provisioning".
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A3-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): TAKEN, all four points documented.

## MEM-P06-A3-F5 — the adoption probe cannot confirm a tombstoned tenant, so its rows stay postponed after rotation

- Context: round MEM-P06-A3-R2; A3-4 with the revision-2 rule that every "not ours" answer postpones
  `provisioner_mismatch`.
- Classification: Minor, actionable; a lifecycle row with no path to completion.
- Evidence:
  - The A3-4 probe is `tenants/create`. For an existing tenant it answers 409 whenever the tenant is
    deleted, whoever manages it (services/memory/src/adapters/postgres/mod.rs:904-905: `existing.state
    == Deleted || !may_manage`).
  - A1-5 moves tombstones too, so after a complete reassignment a tombstoned tenant is managed by the
    new credential, yet the probe still answers 409.
  - Rows in this state:
    - a company whose deletion is in progress at rotation (namespace absent rows still pending or
      postponed, or a tenant row whose delete landed but whose settle did not);
    - recorded under the old id;
    - claimed by a process holding the new token.
  - Each such row probes, gets 409, and is postponed every 5 minutes, indefinitely.
  - Without rotation the same rows complete at once: `namespace_absent`'s create answers 409 for a
    deleted tenant, which settles them as absent.
  - A3-2 never removes them (they are not settled). The new `memory_lifecycle_postponed_rows` gauge shows
    them, but nothing completes them.
- Required: probe management with a call that answers for tombstones.
  - `admin.credentials.list` with a page limit of 1 goes through `lock_tenant` without `require_active`
    (mod.rs:1134-1166). It answers 200 for any tenant the caller manages, deleted included, and 404
    otherwise.
  - Fall back to `tenants/create` only on that 404, so that a missing tenant is created and a foreign
    one answers 409.
  - Alternatively, settle namespace absent rows directly once the company's tenant row has settled
    absent.
  - Test: rotation while a deleted company still has pending absent rows; they complete after the
    reassignment.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A3-R3 (digest 7ce18038…21e6, TASK.md sha256 b5c3606b…): FIXED. One `manages_tenant` probe serves both A3-4 and A3-5. It calls `credentials/list {limit: 1}` first: `lock_tenant` without `require_active` (mod.rs:1134-1166), so a 200 proves management in any state, tombstones included. Only on that 404 does it fall back to `tenants/create`, where a 200 is also proof; a remaining 409 or 404 postpones. The rotation-mid-deletion test is named.

## MEM-P06-A2 findings — common context

2026-11-10; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p06/docs/TASK.md (`## p06-A2 architecture gate
(revision 1)`, lines 265 to the end); review record REVIEW.md; batch p06-A2 (service metrics, correlation, log hygiene,
OpenBot memory metrics); stage architecture gate; round note MEM-P06-A2-R1; reviewer claude-opus-5-5, effort xhigh
(B abdd866fb428eea05); phase `p06`; HEAD a6bed48a; digest and gate text as for A1.

## MEM-P06-A2-F1 — the log-hygiene test can pass without capturing anything

- Context: round MEM-P06-A2-R1; decision A2-3 and the verification plan.
- Classification: Minor, actionable; whether the verification is decisive.
- Evidence:
  - A2-3 (TASK.md:303-309) asserts that a marker and the bearer token never appear in captured tracing
    output.
  - The service logs both from request handlers and from tasks spawned on the runtime: the generation
    worker's claim, dispatch and settle, the indexer and maintenance.
  - A thread-local subscriber in a multi-threaded test runtime does not see events from other threads.
    A capture that missed them is indistinguishable from clean output.
  - Refusal bodies embed serde's message (`ServiceError::invalid(format!("Invalid request: {error}"))`,
    services/memory/src/application/service.rs:204, application/memory.rs:49). For type and variant
    errors that message quotes the offending value, so a marker there stays out of the logs only while
    no log prints the message.
- Required:
  - Give the test a positive control. For every scripted step, assert first that the expected lines
    (`request_id`/`correlation_id`/`operation_id`, including the worker's claim, dispatch and settle
    lines) are present in the capture, and only then that the marker is absent.
  - Capture with a process-wide subscriber, or run the workers inline on a current-thread runtime.
  - Include a refused request whose marker sits in a value serde would quote: a wrong type or an
    unknown enum variant.
  - Apply the same to OpenBot's test.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A2-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): FIXED. A process-wide subscriber, or a current-thread runtime with the worker inline. The positive control requires a request line, an operation line and a worker settle line, each carrying the correlation id. Markers go in quoted serde refusals (an unknown field and a wrong type). OpenBot's test follows the same pattern.

## MEM-P06-A2-F2 — the affected contracts are not listed, and three of them are underspecified

- Context: round MEM-P06-A2-R1; decisions A2-1 and A2-2.
- Classification: Minor, actionable; contract stability. Unlike A1 and A3, A2 has no "Affected
  contracts" section.
- Evidence:
  - A2 changes these contracts:
    - `openapi.json`: the new operation, the `x-correlation-id` request header and the `x-request-id`
      response header on every operation;
    - the `memory_contract_` snapshot;
    - two schema columns and their CHECKs;
    - OpenBot's `MemoryService` port. `search`, `context_pack` and `fetch` take no run or execution
      today (src/application/memory/ports.rs:394-438), so `ob.r.<run>.<execution>` needs new
      parameters through recall and tool reads;
    - the contract fake, which must record headers.
  - Three points are underspecified:
    - (a) `/mcp`: one HTTP request can carry tool calls, and `mcp_gate` (services/memory/src/adapters/http/mod.rs:137-138)
      and `call_tool` (services/memory/src/adapters/mcp/mod.rs:85) each mint their own request id. The
      gate does not say whether `x-correlation-id` is read on `/mcp` or how it reaches each call.
    - (b) `generation_operations.correlation_id` must join the immutable column list of
      `generation_requests_are_immutable` (migrations/0001_init.sql:247-266). Otherwise a later update
      can rewrite it.
    - (c) Requests refused by the capacity middleware (`limit`, http/mod.rs:117-127) are answered before
      any operation is known. "Requests by operation and outcome" needs a label for them, and those
      responses must carry `x-request-id` too.
- Required: add the list of affected contracts, and settle (a)–(c).
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A2-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): FIXED. Contracts listed. `/mcp`: one call per request, with stateless mode verified at http/mod.rs:205-206. `correlation_id` joins the immutability trigger's frozen columns. Capacity refusals carry `x-request-id` and an `unrouted` label, but the label's code does not match the response; see new F6 (optional).

## MEM-P06-A2-F3 — per-process counters sit beside database-wide gauges

- Context: round MEM-P06-A2-R1; decision A2-1.
- Classification: optional suggestion; operational semantics.
- Evidence and suggestion:
  - Counters are kept per process, while the gauges come from the shared database.
  - With more than one service process (06B.3's separate hosts, competing workers), every scrape
    target reports the same database gauges, and `sum()` across instances multiplies them.
  - Document that every instance is scraped, that database gauges are aggregated with `max` and
    counters with `sum`. Alternatively, give the database gauges a distinct prefix.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A2-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): TAKEN.

## MEM-P06-A2-F4 — "lease losses" would count cancel-requested renewals

- Context: round MEM-P06-A2-R1; decision A2-1 ("lease losses (renewal refused, settle `Stale`)").
- Classification: optional suggestion; metric meaning.
- Evidence and suggestion:
  - One statement in `renew_generation` refuses a renewal in every case alike: a lost lease, a changed
    fence, an expired lease and a requested cancel (services/memory/src/adapters/postgres/generation.rs:1460-1466).
  - Counting every refusal as a lease loss would make each purge, delete, revoke or
    `generations.cancel` of a dispatched operation look like a lease loss.
  - Count the refusals by reason. The statement can return the reason, or the row can be re-read when a
    renewal is refused.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A2-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): TAKEN (reasons `renewal_cancel_requested`, `renewal_lost`, `settle_stale`).

## MEM-P06-A2-F5 — "one id" overstates the link to a later recall

- Context: round MEM-P06-A2-R1; decision A2-2, last bullet.
- Classification: optional suggestion; documentation accuracy.
- Evidence and suggestion:
  - A later recall carries its own `ob.r.<run>.<execution>`, not the capture's id.
  - The chain runs: the capture key, then the operation's `correlation_id`, then the observations'
    `generation_operation_id`, then the recall's consumption items (observation ids, E6).
  - Describe it as that join path, and keep the consumption items as the bridge.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A2-R2 (digest 7ce18038…21e6, TASK.md sha256 2c053242…): TAKEN (documented as a join path).

## MEM-P06-A2-F6 — the capacity-refusal label names a code the response does not carry

- Context: round MEM-P06-A2-R2; the fix for F2 (capacity-middleware refusals).
- Classification: optional suggestion; metric consistency.
- Evidence and suggestion:
  - Revision 2 counts capacity refusals as `memory_requests_total{operation="unrouted",code="unavailable"}`
    (TASK.md:461-462).
  - The middleware answers `ErrorCode::CapacityExceeded` (services/memory/src/adapters/http/mod.rs:117-127),
    whose wire name is `capacity_exceeded`, with HTTP 429 (domain/error.rs:6-7, 45).
  - Label it `code="capacity_exceeded"`, so that the metric and the response agree and the 503
    `unavailable` count stays meaningful.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A2-R3 (digest 7ce18038…21e6, TASK.md sha256 b5c3606b…): TAKEN. The label is now `code="capacity_exceeded"`, matching the 429. It is a one-line metric label change and reopens nothing.

## MEM-P06-B1 findings — common context

2026-11-10; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p06/docs/TASK.md (`## p06-A1 implementation`,
`## p06-A3 implementation`, `## p06-A2 implementation`, `## p06-A3 progress`); review record REVIEW.md; batch
p06-B1 (the A1, A3 and A2 implementations, plus the B6 carries and F16); stage batch review; round note
MEM-P06-B1-R1; reviewer claude-opus-5-5, effort xhigh (B abdd866fb428eea05); phase `p06`; HEAD a6bed48a;
artifact digest ab43fef61423e106ebd010a9c2e90ec720dc1c72216931797360d69b8da80e38; frozen tree 5ae80918 (base
a6bed48a; incremental from the accepted p05 tree 7388d1c2: 57 files outside `.sqlx` and the phase docs).

## MEM-P06-B1-F1 — `tenant_absent` still fails definitively on a delete 404 after a create that did not succeed

- Context: round MEM-P06-B1-R1; A3 revision 2, F2's second rule.
- Classification: Minor, actionable; conformance to a gated rule on the "not ours" path.
- Evidence:
  - A3 revision 2 (TASK.md:364-370): every "not ours" answer for a tenant OpenBot recorded postpones
    `provisioner_mismatch`, and "`tenant_unmanageable` stays definite only for the tenant steps' 404 after a
    successful create probe in the same step."
  - `tenant_absent` (src/application/memory/lifecycle.rs:456-466) ignores the create's answer. It accepts
    `Ok`, `Conflict` or `NotFound` alike, and maps any delete 404 to `Fail("tenant_unmanageable")`.
  - A create 409 followed by a delete 404 means a tenant managed by someone else: a delete of our own
    tombstone answers 200. So this is exactly the "not ours" case that the gate says postpones.
  - The kept test `memory_scope_reconciler_foreign_managed_tenant_never_completes_deletion`
    (src/adapters/persistence/memory/lifecycle_tests.rs:306-330) pins the definite failure.
  - The deviation is not in TASK.md's deviation list.
  - Reachable during a rolling restart: an old-token process still claims a tenant row recorded under the
    old id after the operator reassigns. docs/deploy.md:873 accepts that the old process's steps fail.
- Required: one of the following, plus a test either way.
  - Conform: after a create that did not answer 200, a delete 404 goes through `not_ours`, and the test
    asserts the postponement.
  - Or keep the definite failure for tenant steps: record it as a deviation with its reason (a genuinely
    foreign tenant should fail visibly) and state it in A3 revision 2's rule.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-B1-R2 (digest 541b35ad…9351, freeze tree 757148d9): FIXED. `tenant_absent` fails `tenant_unmanageable` only after a create that succeeded in the same step; after a refused create it goes through `not_ours`. The test asserts the postponement, then the definite failure after a successful create and a faulted delete 404. The test fails clippy; see new F3.

## MEM-P06-B1-F2 — small conformance and robustness points

- Context: round MEM-P06-B1-R1.
- Classification: optional suggestion (each item); no item blocks.
- Evidence and suggestions:
  - (a) Postponed rows are gauged for one outcome only.
    - A3 revision 2 gauges `memory_lifecycle_postponed_rows{outcome}`.
    - `MemoryMaintenance::pass` (src/application/memory/maintenance.rs) records only
      `outcome="provisioner_mismatch"`.
    - The waits on a disabled tenant (`tenant_disabled`) and `tenant_recheck` stay invisible, although
      docs/deploy.md documents the disabled wait as open-ended.
    - Count postponed rows grouped by `outcome_code`.
  - (b) Provider error kinds become outcome codes and metric labels.
    - `classify` (services/memory/src/adapters/anthropic/mod.rs:144-153) turns the provider's `error.type`
      into `Rejected(kind)` or `Terminal(kind)`. It reaches `outcome_code` and now the
      `memory_generation_settled_total{outcome}` label (generation.rs:1149).
    - A2-1 promises closed label sets. The DB CHECK `^[a-z_]{1,64}$` bounds the shape but not the set, and a
      kind outside that shape fails the settle until the reaper records `unknown`.
    - Map unknown kinds to a fixed code (for example `provider_error`).
  - (c) The source-removal race at re-record.
    - Acceptance (generation.rs:363-376) and `sources:record` (memory.rs:746-757) follow `INSERT … ON CONFLICT
      DO NOTHING` with an unlocked `fetch_one`.
    - With A1-F5 removing nulled revisions, a delete that commits between those two statements gives
      `RowNotFound`, a transient 503.
    - `sources:record` can also return the id of a removable nulled row that is deleted moments later.
    - Use `fetch_optional` and retry the insert once. Optionally, treat a nulled, removable row as absent.
  - (d) Tenant delete cancels the operations (mod.rs:637) before it locks the namespaces (mod.rs:644-645).
    That is reversed against D13 and `tombstone_namespace`. It is harmless, because the tenant
    `FOR NO KEY UPDATE` excludes every namespace locker, but the documented order should hold: take the
    namespaces first.
  - (e) The B6-F1 carry installs `1.95.0 --profile minimal` (ci.yml:37-38), without the `rustfmt` and `clippy`
    components that services/memory/rust-toolchain.toml lists. Whether rustup then fills them in implicitly
    is the same implicit behaviour F1 set out to avoid. Run `rustup toolchain install` inside
    services/memory, which reads the toml, or add the components.
  - (f) OpenBot's hygiene test covers a capture and a delivery, but not a recall, which A2-3 named. The recall
    path logs only error codes, so the risk is low; add a recall with a marker query, or record the
    omission.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-B1-R2 (digest 541b35ad…9351, freeze tree 757148d9): (a) TAKEN: postponed rows are gauged for `provisioner_mismatch`, `tenant_disabled` and `tenant_recheck`. (b) TAKEN: unknown kinds become `provider_error`, with a test. (c) TAKEN: `retryable` re-runs a transaction on `RowNotFound`. This applies to every `with_retry` closure, but those already re-run whole transactions, and a persistent miss still ends as the same 503 after three tries. (d) TAKEN: namespaces are locked before operations. (e) TAKEN: `--component rustfmt,clippy`. (f) NOT TAKEN, reason accepted: recall uses the same adapter `post`, logs outcomes only, and asserts its header.

## MEM-P06-B1-F3 — the F1 test fails `clippy --all-targets -D warnings`

- Context: round MEM-P06-B1-R2; the rewritten test for F1.
- Classification: Minor, actionable; a CI gate fails (AGENTS.md: CI exercises formatting and compilation;
  the repository's clippy gate is `--all-targets -D warnings`).
- Evidence:
  - `clippy::await_holding_lock` at src/adapters/persistence/memory/lifecycle_tests.rs:331 (the author's
    log: logs/MEM-P06-B1-R2-clippy.txt).
  - A std `MutexGuard` is bound with `let mut state`, released by an explicit `drop(state)`, and followed by
    `due(&f).await`. Clippy still counts the guard as held across the await.
  - The test is test-only and its behaviour is correct.
  - The author acknowledged it and will fix it in the correction round.
- Required: scope the guard in a block, rerun clippy `--all-targets -D warnings`, and record the result.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-B1-R3 (digest 7e5ee98d…9b51, freeze tree 5f080b3b): FIXED. The guard now lives in a block that ends before `due(&f).await` (the only source change since 757148d9). My run of `cargo clippy --locked --all-targets --offline -- -D warnings` exits 0 (logs/MEM-P06-B1-R3-B-clippy.txt), matching the author's log.

## MEM-P06-A4 findings — common context

2026-11-10; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p06/docs/TASK.md (`## p06-A4 architecture gate
(revision 1)`, line 794 to the end; TASK.md sha256 149b20877be108b5… at review); review record REVIEW.md; batch
p06-A4 (provider auth breaker, restore without replay, backup check, operations docs, overload and budgets, smoke
check, stack bounds); stage architecture gate; round note MEM-P06-A4-R1; reviewer claude-opus-5-5, effort xhigh
(B abdd866fb428eea05); phase `p06`; HEAD a6bed48a; artifact digest ab43fef6…0e38 (code unchanged since B1).

## MEM-P06-A4-F1 — the restore procedure leaves credentials and tenant state from after the backup unreconciled

- Context: round MEM-P06-A4-R1; decision A4-4 (restore steps), with A4-2. 06A.4 asks to "restore canonical
  content and operation state consistently".
- Classification: Minor, actionable; data-loss prevention and security in an operator procedure.
- Evidence:
  - Step 4 (TASK.md:855) re-drives only lifecycle rows settled `absent` after the backup time. A restored
    service database also reverses:
    - Credentials issued after the backup. They are unknown to the restored service, yet OpenBot still holds
      them.
      - `namespace_present` settles `present` as soon as a stored credential exists, without checking it
        remotely (src/application/memory/lifecycle.rs:499).
      - After reopening, every client call of such a namespace is a 401. Delivery maps that to the terminal
        `failed("authority")` (src/application/memory/delivery.rs:140-142), which drops the frozen text, so
        promised saves and captures queued during the restore are lost.
      - Recall and tools fail with `memory.forbidden` (src/application/workflow/memory.rs:124).
    - Revocations after the backup, client and admin alike. A provisioner or operator token revoked after
      the backup (for example a compromised one) is valid again. The restored audit cannot list it.
    - Tenant disables and cap changes after the backup. A tenant disabled since the backup is active again.
- Required:
  - Extend the restore decision so OpenBot re-drives every lifecycle row updated after the backup, not only
    absent ones: tenants (disable or delete again), and present namespaces with a check that the stored
    credential still exists remotely (for example, the labelled listing must contain the stored id),
    re-issuing it if not.
  - Run that re-drive, or hold deliveries, before deliveries resume, so no intent is failed `authority`
    meanwhile. Alternatively, treat a 401 on a client credential as a lifecycle signal (re-issue, intent
    retried) rather than a terminal failure.
  - Document that the operator must re-revoke admin credentials revoked after the backup, before reopening.
  - Extend the named OpenBot test to cover a credential issued after the backup.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A4-R2 (digest 541b35ad…9351, freeze tree 757148d9): RESOLVED IN SUBSTANCE. Every lifecycle row updated since the backup is re-driven. A4-8 verifies the stored credential (and re-issues it). A4-9 makes a delivery 401 transient and re-pends the lifecycle with compare-and-set. Admin re-revocation and settings are documented as operator records. Open generation intents are cancelled at restore. The test set is extended. A4-8's clear-then-issue order defeats A4-9; see new F4.

## MEM-P06-A4-F2 — the verification names no test for the breaker or for after-restore in the regular suite, and does not map the 06A focused checks

- Context: round MEM-P06-A4-R1; the verification paragraph (TASK.md:885-887) and the 06A focused checks
  (06-operations-and-verification.md:25-28).
- Classification: Minor, actionable; whether the verification is decisive.
- Evidence:
  - A4-1 is a new concurrency protocol: a breaker shared by up to 4 workers, at most one failed call per
    window, dispatch not counted, reopening that doubles, closing on success. No test is named, and AGENTS.md
    asks for competing claimants.
  - A4-2 is exercised only inside the `#[ignore]` backup test, which runs through the script and the CI step
    only. The regular suite would not catch a regression in idempotency, reservation release, the audit row,
    or a mid-call worker's settle becoming `Stale`.
  - The 06A focused checks say `memory_operations_` covers "retention, in-flight deletion, rotation, restore,
    quota/auth recovery, expired requests and missing model assets". Several of these exist under other
    prefixes: `memory_generation_spend_cap_is_terminal_and_overshoot_is_counted`,
    `memory_startup_missing_model_fails_explicitly`, and the B1 rotation and removal tests. The gate does not
    map them.
- Required:
  - Name DB-backed `memory_operations_` tests for the breaker. With several workers and a provider answering
    `authentication_error`, assert: one call per window; operations back to `accepted` with `attempts`
    unchanged and the reservation released; no claim while open; doubling; closing on success; expiry still
    applying.
  - Name a DB-backed test for after-restore in the regular suite: idempotent; every unfinished state cancelled
    `restored`; reservations released; one audit row; a worker holding a dispatched lease settles `Stale`.
  - Map each 06A focused-check item to its test, existing or new.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A4-R2 (digest 541b35ad…9351, freeze tree 757148d9): FIXED. Named: a breaker test with 3 competing workers (one probe per window, `attempts` unchanged, no claim while open, doubling, closing); after-restore in the regular suite (every unfinished state, zero reservation, idempotent, idle worker, one audit row); and the 06A focused-check map.

## MEM-P06-A4-F3 — three points to state in the breaker and restore decisions

- Context: round MEM-P06-A4-R1; A4-1, A4-2 and A4-4.
- Classification: optional suggestion.
- Evidence and suggestions:
  - (a) Billing.
    - The provider's `billing_error` (account out of credit) is also account-wide and definite, and no
      inference runs.
    - Left as "other terminal errors are unchanged" (TASK.md:819), it fails every queued operation one by
      one, the same pathology the breaker fixes for authentication.
    - The focused checks name quota recovery. Consider tripping the breaker on it too.
  - (b) Half-open state. State that the half-open state admits a single probe call per process: a pool of up
    to 4 workers otherwise sends up to 4 failing calls when the window reopens.
  - (c) Re-sent intents after a restore.
    - OpenBot intents still open at the restore are sent after reopening. Where an earlier attempt had
      landed after the backup, the service has no record, so the request executes again: a second inference
      for a capture.
    - This is a client re-send, not a service replay, but say so among the limits of a restore.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A4-R2 (digest 541b35ad…9351, freeze tree 757148d9): TAKEN: (a) `billing_error` trips the breaker; (b) a single probe permit while half-open; (c) open generation intents are cancelled `service_restored` at restore, while saves are resent (no inference).

## MEM-P06-A4-F4 — A4-8 clears the stored credential before re-issuing, so the delivery claim fails the very intents A4-9 keeps retrying

- Context: round MEM-P06-A4-R2; decisions A4-8 and A4-9 (TASK.md "p06-A4 revision 2").
- Classification: Minor, actionable; data-loss prevention (promised intents) in the restore and 401 paths.
- Evidence:
  - A4-8: when the stored credential is missing from a complete listing, "the stored credential is cleared and
    a new one is issued and stored under the fence".
  - `memory_namespaces` requires a stored credential while `ready` (`CHECK (state <> 'ready' OR
    credential_id IS NOT NULL)`, migrations/20260817000000_init_schema.sql:14953). Clearing the credential
    therefore takes the namespace out of `ready`.
  - The delivery claim reads the envelope only for a `ready` namespace. With none, it settles the intent
    `failed("namespace_unavailable")`, terminal, with its request dropped
    (src/adapters/persistence/memory/deliveries.rs:609-610 and 637-639).
  - So between the clear and the next completion that stores the new credential (an HTTP issue, a store
    under the fence, retries during an outage), every claimable intent of that namespace fails.
  - A4-9's own sequence passes through this window: a delivery 401 keeps the intent pending, re-pends the
    lifecycle, the present step finds the credential missing and clears it, and the retrying intent's next
    claim is a terminal failure.
- Required:
  - Never take a `ready` namespace out of `ready` to re-issue. Issue the new credential first, then swap the
    stored id and envelope in one fenced completion. Meanwhile a delivery with the old credential gets 401,
    which A4-9 keeps transient.
  - Alternatively, make the delivery claim wait (no attempt spent) while the namespace's lifecycle is
    re-issuing, instead of failing `namespace_unavailable`.
  - Add a test: a delivery pending through the re-issue (including one that got the triggering 401) is
    delivered afterwards, not failed.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open.
  - 2026-11-10 recheck MEM-P06-A4-R3 (digest 7e5ee98d…9b51, freeze tree 5f080b3b): FIXED. A4-8 never clears the credential: it issues first, then one fenced UPDATE guarded by `state = 'ready' AND credential_id = <old>` swaps the id and envelope and settles `present`. The namespace stays `ready`, so the claim never fails `namespace_unavailable`. A refused swap or a lost fence leaves a labelled stray that the next pass revokes. A truncated listing keeps the stored credential. A 401 before the swap stays transient, and its compare-and-set does nothing after the swap. The named test covers a delivery pending through the re-issue, and the restore test asserts the row never left `ready`.

## MEM-P06-A4-F5 — a 401 also means a tenant deleted out of band

- Context: round MEM-P06-A4-R2; A4-9.
- Classification: optional suggestion; documentation of an admin edge.
- Evidence and suggestion:
  - A deleted tenant answers every client write with the uniform `credential_revoked`, a 401
    (memory.rs `lock_writer`; error.rs:35).
  - Under A4-9, an operator's manual tenant delete therefore leaves OpenBot's intents retrying until their
    own `expires_at` (up to 7 days) rather than failing.
  - The re-pended present step then fails `tenant_conflict` at create.
  - This is harmless and bounded, but say so in deploy.md beside the manual client-revocation note (A4-9
    replaces a revoked credential; stop OpenBot's access by disabling the tenant).
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional).
  - 2026-11-10 recheck MEM-P06-A4-R3 (digest 7e5ee98d…9b51, freeze tree 5f080b3b): TAKEN. Documented as an unsupported out-of-band operator action: bounded by the intents' deadlines, visible as `tenant_conflict`, and memory for that company is unrecoverable until the service database is restored or reset (the tenant id is the company id).

## MEM-P06-B2 findings — common context

2026-11-10; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p06/docs/TASK.md (`## p06-A4 implementation`,
against "p06-A4 architecture gate (revision 1)", "p06-A4 revision 2" and "p06-A4 revision 3"); review record REVIEW.md;
batch p06-B2 (the A4 implementation); stage batch review; round note MEM-P06-B2-R1; reviewer claude-opus-5-5, effort
xhigh (B abdd866fb428eea05); phase `p06`; HEAD a6bed48a; artifact digest
a83ffe8402206d2cbee9aa6856c4666fc130a5f500bb3dffa42e08bef8e9afd1; freeze tree fb435320 (incremental from 5f080b3b:
33 files outside `.sqlx` and the phase docs).

## MEM-P06-B2-F1 — two robustness notes on the breaker's tests and gauge

- Context: round MEM-P06-B2-R1; A4-1, deviation 7.
- Classification: optional suggestion; no item blocks.
- Evidence and suggestions:
  - (a) The competing-worker breaker test relies on wall-clock timing.
    - `memory_operations_auth_breaker_pauses_competing_workers_and_closes_on_success`
      (services/memory/src/adapters/postgres/operations_tests.rs:1224-1340) asserts call counts at
      `opened + base/2`, `+3/2` and `+5/2`, with a 400 ms base.
    - The probe must therefore have run inside a 200 ms slot, which a loaded CI runner can miss, and the
      P6b reaper tests needed the same fix earlier.
    - It passed 5× for the author and once for me.
    - Consider asserting from recorded call instants: one call per window, and a spacing of at least
      `base` and then `2·base`. Or widen the base.
  - (b) `AUTH_BREAKER_OPEN` is a process-wide static that every `ProviderBreaker` instance writes.
    - Production composes one breaker per process (startup.rs), so the gauge is right there.
    - Breakers in concurrent tests would race on it, but no test asserts the gauge today.
    - Reading the gauge from the shared instance, which startup could hand to the HTTP state, would remove
      the coupling.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional). 2026-11-10 MEM-P06-B3-R1: taken in p06-B3, both parts
  fixed.
  - (a) The test now records each call's start (`CountedProvider::started()`) and asserts only lower bounds:
    gap 1 ≥ `base`, gap 2 ≥ `2·base`, with a 200 ms base. A loaded runner only lengthens the gaps.
  - (b) The gauge reads `App::generation_paused()`. Startup hands the HTTP state the same breaker
    instance it gives every worker (startup.rs:175-186, router built at :235), and the static is gone.
  - Reviewer run: 3× passed (logs/MEM-P06-B3-R1-B-focused-service.txt).

## MEM-P06-B3 findings — common context

2026-11-10; workflow `implement-opus-reviewed-2_2`; task
/Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p06/docs/TASK.md (`## p06-B3 implementation`,
against plan 06B.1–06B.4 in plan/memory-service/06-operations-and-verification.md:35-62, MEM-P06-B2-F1, user
decisions U8 and U9 in COORDINATION I003 r5, and D12's resolution in plan/memory-service/implementation-decisions.md:622-625);
review record REVIEW.md; batch p06-B3; stage batch review; round note MEM-P06-B3-R1; reviewer claude-opus-5-5, effort
xhigh (B abdd866fb428eea05); phase `p06`; HEAD a6bed48a; artifact digest
b86dc1e8992a694729b968f3b9b5511100f6cd4e8ea39f154ed55e33d0f64b0e; freeze tree 540efd56 (incremental from fb435320:
23 files outside `.sqlx` and the phase docs, +1399/−59; diff sha256 0c0a7089…).

## MEM-P06-B3-F1 — the isolation check needs OpenBot's schema, which nothing creates, so CI's live job fails first

- Context: round MEM-P06-B3-R1; 06B.3 ("prove database roles cannot read each other's tables");
  scripts/memory-integration.sh:72-80; .github/workflows/ci.yml:10-52.
- Classification: Minor, actionable. It turns CI's `memory-integration` job red before any live test runs, so the
  06B.2 tests would not run in CI.
- Evidence:
  - The service-role half of the check runs `SELECT count(*) FROM companies` in the `TEST_DATABASE_URL` database
    and requires `permission denied`. Any other answer exits 1.
  - Nothing creates OpenBot's schema in that database:
    - CI's job gets a fresh `open_bots_test` from the service container and runs the script with no migration
      step;
    - OpenBot's `Fixture` creates and migrates a database per test, never the `TEST_DATABASE_URL` one (after
      a lib test, `to_regclass('companies')` there was still null in my run);
    - and the check runs before `cargo test` in any case.
  - My run on a freshly created database (`open_bots_b3r1_test`) failed at the check:
    `isolation: unexpected answer for the service role`, then `ERROR: relation "companies" does not exist`, exit 1,
    with no test run (logs/MEM-P06-B3-R1-B-memory-integration.txt).
  - After I applied migrations/20260817000000_init_schema.sql to that database by hand
    (logs/MEM-P06-B3-R1-B-schema-apply.txt), the same script printed "isolation: neither role can read the other's
    database" and 6 tests passed (logs/MEM-P06-B3-R1-B-memory-integration-migrated.txt).
  - TASK records 06B.3 as "Passed". That run presumably used a database that already had the schema.
- Required correction:
  - Make the service-role probe independent of whether the OpenBot database is migrated. The author chooses how,
    for example:
    - the admin creates a throwaway table with no grants in the OpenBot test database, the service role must get
      `permission denied` on it, and the cleanup drops it;
    - or the script applies OpenBot's migrations to that database before the check;
    - or the check targets a database the script itself creates from the migrations.
  - Rerun the script against a database created just before the run, as CI's is, and record that run.
- Instruction lesson (for root to judge): a check added to a CI script is shown only by a run from the state CI
  starts in.
- Resolution history: 2026-11-10 open. 2026-11-10 MEM-P06-B3-R2: fixed.
  - The admin now creates `memory_isolation_probe_<suffix>` (no grants) in the `TEST_DATABASE_URL` database, and
    the service role must get `permission denied` reading it. The cleanup trap drops it. The script header now
    says the OpenBot database must be on the admin URL's server.
  - The no-grant table matches OpenBot's real tables: the init migration grants nothing to anyone.
  - Reviewer rerun on a database created just before the run (`to_regclass('companies')` null): isolation
    passed both ways and 6 passed, exit 0. No probe table, database or role was left
    (logs/MEM-P06-B3-R2-B-memory-integration-fresh-db.txt).

## MEM-P06-B3-F2 — two 06B items have no real-service evidence and are not recorded as uncovered

- Context: round MEM-P06-B3-R1; 06B.2 ("validate source provenance") and the 06B focused checks ("… a counted test
  provider and the production image. Record topology and unavailable checks explicitly").
- Classification: Minor, actionable. Either add the test, or record the item as uncovered with its reason.
- Evidence:
  - (a) Source provenance.
    - Against the real service, the only provenance assertion is the earlier live test's context validation of an
      unchanged `sources_sha256`, on a save with no sources (src/adapters/persistence/memory/live_tests.rs:286-298).
    - The service records the captured window as source rows, and each generated memory links only revisions
      cited from that window (services/memory/src/adapters/postgres/generation.rs:341-368, 476, 1252-1281).
    - The live capture test checks only the generated memory's body and level (live_tests.rs:377-410). No
      `memory_e2e_` test checks what a memory cites.
    - OpenBot's `MemoryDetail` does not carry `sources`. A check would read the raw API through `call`, as the
      other e2e tests already do.
    - TASK's 06B.2 record does not mention provenance.
  - (b) The production image.
    - Nothing runs a journey against the image.
    - CI's `image` job (memory-service.yml:75-) runs only `version`, `model-check` and a refused startup, all
      with the network denied.
    - The standalone journey and the live script both run the `cargo build` debug binary.
    - TASK says Docker is unavailable locally and that "CI's `image` job covers" the non-root image. That holds
      for startup, but no journey exercises the image.
- Required correction:
  - (a) Add a real-service assertion on the provenance a memory carries. For example, after the live capture,
    fetch the generated memory through the raw API and check that its `sources` are the source rows recorded for
    the delivered window. Alternatively, record in TASK that provenance is covered only by the service suite, name
    those tests, and say why the end-to-end run omits it.
  - (b) Run `standalone_client.py` against the built image in CI's `image` job. The job has Docker; it would need
    a pgvector service and a provisioned database, with the network denied except for that database. Or list,
    among the unavailable checks, that no journey runs against the production image and that 06C carries it.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open. 2026-11-10 MEM-P06-B3-R2: fixed.
  - (a) The live capture test reads the generated memories through the raw fetch with OpenBot's credential. Each
    must cite exactly two distinct `source` refs at revision `1`. The service accepts citations only from inside
    the window, so these are the window's input and output parts. If provenance were dropped, the test would
    fail. Passed in my fresh-database run.
  - (b) The record is corrected (TASK `p06-B3 corrections`): no journey runs against the image, and it is
    carried to 06C as unavailable and not claimed. The earlier lines at TASK:1178 and :1291 stay as written,
    with the correction appended. Accepted.

## MEM-P06-B3-F3 — optional strengthening

- Context: round MEM-P06-B3-R1; 06B.2–06B.4 and U9.
- Classification: optional suggestion; no item blocks.
- Evidence and suggestions:
  - (a) `memory_authority_client_revocation_races_competing_saves`
    (services/memory/src/adapters/postgres/authority_tests.rs:813-) passes even when there is no race.
    - It checks committed = stored = audited and that every refusal is `credential_revoked`. It would also pass if
      every save committed before the revoke, or if every save was refused.
    - The author's first run committed 33 and refused 15, so the race does occur here, but nothing asserts it.
    - Asserting `committed > 0 && refused > 0` would make a lost race fail visibly. If that turns out to be flaky,
      a barrier could start the revoke after the first commit.
  - (b) A level-7 external source is refused at OpenBot's freeze. No e2e case tampers with a frozen intent to add
    one, which the service would refuse as a source above the ceiling. The tampered-consumption and null-ceiling
    cases cover the service-side refusal shape; one more `jsonb_set` case would cover this one.
  - (c) The reverse isolation probe is a generic `NOSUPERUSER` role.
    - OpenBot's test role is `postgres`, a superuser, which bypasses every grant. So the probe proves the service
      database denies ordinary roles, not that OpenBot's actual role is denied.
    - docs/deploy.md does not say that OpenBot's database role must not be a superuser. Saying so would make the
      probe's assumption a documented requirement.
  - (d) The paid smoke exports `MEMORY_ANTHROPIC_API_KEY` into the journey's whole environment. `cargo build`,
    including third-party build scripts, the Python client, `curl` and `psql` all inherit it. Only `serve` needs
    it. The build is locked and offline and nothing prints the key, so the exposure is small. Passing the key only
    to the `serve` command would remove it.
  - (e) Automatic recall is exercised against the real service only through the client calls it makes (the pack
    in the live test; `memory.load` in the 6/8/9 test), not through the agent runtime's recall path, which stays
    fake-backed. TASK could say so beside the "public latest recall" mapping.
- Instruction lesson: none supported.
- Resolution history: 2026-11-10 open (optional). 2026-11-10 MEM-P06-B3-R2: (a)–(e) taken.
  - (a) A barrier releases the revoke only after each of the 6 writers has committed one save, and each writer
    saves until it is refused, up to 500 rounds. The test asserts committed ≥ 6 and refused = 6. A revoke that
    failed to stop authenticated principals would now fail at refused = 0. Reviewer: 3× passed
    (logs/MEM-P06-B3-R2-B-focused-service.txt).
  - (b) A level-7 external source written into the intent after its freeze fails delivery, and nothing is saved.
    Passed in my fresh-database run.
  - (c) The docs/deploy.md "Memory service" section requires a separate service database and role, and a
    non-superuser OpenBot role. Its claim about `provision-db.sh` holds: the script revokes public connect.
  - (d) The key is moved into an unexported variable and both names are unset before the first child process.
    `serve` alone receives `MEMORY_ANTHROPIC_API_KEY`, as an assignment and never in argv, and the paid wrapper
    passes the renamed variable. Reviewer journey rerun: see the MEM-P06-B3-R2 note.
  - (e) Recorded in TASK.
