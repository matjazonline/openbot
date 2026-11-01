# fix-app p05 — Reviewer (B) findings

Shared findings log for phase p05 (workflow `implement-opus-reviewed-2_2`). Review-round notes:
`agent-review-notes.md`. Each finding: context, classification, evidence, instruction lesson,
resolution history. Evidence logs live in `logs/` next to this file.

Common context for every G1 finding below unless stated otherwise: 2026-10-31,
`implement-opus-reviewed-2_2`, task `TASK.md`, review `REVIEW.md`, batch "plan batches 1-5",
stage **architecture gate**, review round `FIXAPP-P05-G1-R1`, reviewer Opus 5.5 / xhigh (B),
phase p05, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p05`, reviewed decision
revision **r1** (`TASK.md` "Architecture gate — decisions (revision r1)", immutable copy
`logs/gate-r1-TASK-snapshot.md`), code base `e6b0a51`, digest
`9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66`.

Background fact used by several findings (verified, not assumed): a non-deferrable `NO ACTION`
foreign key is **not** checked after all cascades of the statement finish. Postgres runs RI
cascade actions with `fire_triggers = false`, so the cascaded row events are appended to the
outer statement's after-trigger queue and fired in breadth-first rounds. The `NO ACTION` check
for a referenced row deleted in round *n* fires in round *n+1*; if the referencing row is only
deleted by a deeper cascade (round > *n+1*), or in the same round but later in queue order, the
check sees it and the whole `DELETE` fails. Toy reproduction:
`logs/g1-r1-toy-noaction-depth.log`. Minimum cascade depths from `companies` on the real schema:
`message_deliveries` 1, `messages` 1, `channels` 1, `principals` 1, `workflow_runs` 1,
`thread_messages` 2, `workflow_message_destinations` 2, `internal_notes` 3,
`workflow_decisions` 4, `workflow_decision_reviewers` 5. RI trigger firing order on `companies`
(name order = OID order): `logs/g1-r1-ri-trigger-order.log` (channels 19377 < deliveries 19512 <
messages 19582 < principals 19647 < workflow_runs 20367). The schema already uses
`DEFERRABLE INITIALLY DEFERRED` for exactly this situation (`internal_notes_message_fk`,
`workflow_decisions_company_id_execution_id_job_id_fkey`, `workflow_run_events_…_decisio_fkey`,
…). Fixture method for the real-schema reproductions: scratch database `p05_b_gate_scratch` on
the phase cluster with the unmodified init schema; fixture rows inserted with
`session_replication_role = replica` (CHECK/NOT NULL still enforced), the `DELETE` run with
triggers enabled, everything rolled back; scratch databases dropped afterwards.

## FIXAPP-P05-G1-F1 — D1.3 composite NO ACTION reviewer→notice FK makes company deletion fail

- Classification: **actionable defect, high**, architecture/correctness. Originates in gated
  decision D1.3 combined with D1.1 (not an implementation error).
- Evidence:
  - D1.1 gives notice deliveries a `NOT NULL company_id` so `message_deliveries_company_id_fkey ON
    DELETE CASCADE` deletes them at cascade depth 1. D1.3 makes
    `workflow_decision_reviewers_notice_fkey` a composite `NO ACTION` FK. Reviewers are deleted only
    via `companies → workflow_runs → workflow_executions → workflow_waits → workflow_decisions →
    workflow_decision_reviewers` (depth 5). The `NO ACTION` check for the deleted notice fires in
    round 2 and still sees the reviewer.
  - Reproduced on the real schema (`logs/g1-r1-company-delete-reviewer-notice.sql/.log`): case A
    (today) `DELETE FROM companies` succeeds and leaves the orphan notice (the bug the plan fixes);
    case B (D1.1 + D1.3 as written) fails with `update or delete on table "message_deliveries"
    violates foreign key constraint "workflow_decision_reviewers_notice_fkey"`; case C (same FK
    `DEFERRABLE INITIALLY DEFERRED`) and case D (`ON DELETE CASCADE`) both succeed and leave no
    reviewer.
  - Impact: after D1, any company that ever parked a decision with a non-opted-out reviewer can no
    longer be deleted (`delete_company_with_cleanup`, `company.rs:72-103`, is one
    `DELETE FROM companies`). Violates the plan's "company deletion behaves as today" and the
    plan's own batch-1 test ("Deleting a company removes its pending notification deliveries").
  - Required: change D1.3 for this FK — `DEFERRABLE INITIALLY DEFERRED` keeps the "notice cannot
    be deleted from under its reviewer" backstop and matches the schema's precedent; `ON DELETE
    CASCADE` also works but would let any future delivery pruning delete immutable decision facts.
    The `workflow_decision_reviewer_guard` keeps the insert-time check either way. Add a company
    deletion test (`own_database`) with a parked decision whose reviewers hold notices.
- Instruction lesson: `src/adapters/persistence/AGENTS.md` "Preserve invariants at the database
  boundary" asks for an invalid-row test but says nothing about how a new `NO ACTION` FK interacts
  with multi-path cascades; plan 05 (batch 5) and plan 01 both state "`NO ACTION` is checked at the
  end of the statement", which is only true relative to the cascade round. Candidate rule (scope:
  persistence AGENTS.md): "A non-deferrable `NO ACTION` FK fails a parent delete when the
  referencing row is removed by a deeper cascade path than the referenced row. For every new
  `NO ACTION` FK, compare cascade depths from `companies` (and `channels`) or make it
  `DEFERRABLE INITIALLY DEFERRED`, and prove it with a whole-company delete test that populates
  both rows." Evidence: this finding, F3, F4.
- Resolution history: 2026-10-31 open (G1-R1).
  2026-10-31 **fixed** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): FK made `DEFERRABLE INITIALLY DEFERRED`; combined company delete succeeds and the deferred FK still rejects a foreign-company notice at commit (`logs/g1-r2-company-delete-combined.log` cases A, C).

## FIXAPP-P05-G1-F2 — D1.6 silently moves platform notices onto the tenant's own Resend account

- Classification: **actionable defect, high**, architecture/correctness/security (tenant sending
  identity). Gap in gated decision D1.6.
- Evidence:
  - `DeliveryRecord::company_id()` (`src/application/transport/queue.rs:224-233`) returns `None`
    for an unattributed row and documents the policy: "`None` is a platform notice … which belongs
    to the deployment rather than to a tenant, and so goes out over the deployment's own relay."
  - Its only production caller is the email egress: `self.transports.transport_for(
    delivery.company_id())` (`src/adapters/protocols/email/egress.rs:518`).
    `CompanyResendApiTransports::transport_for` (`src/adapters/resend_api/accounts.rs:112-122`)
    sends through the company's own connected Resend account whenever `company_id` is `Some`.
  - D1.6 gives `DeliveryRecord` a top-level non-optional `company_id` and drops it from
    `DeliveryAttribution`, but does not say what `company_id()` / the transport selection uses
    afterwards. The natural implementation (`Some(record.company_id)`) routes every notification
    email, decision notice, rejection bounce (`mailer-daemon@<app domain>`) and help reply through
    the tenant's Resend account. The apex app-domain senders are almost certainly not verified on a
    tenant account, so those notices dead-letter; where they do send, platform mail goes out under
    and is logged in a tenant's provider account.
  - Required: D1.6 must state that the sending account is chosen from attribution (channel-attributed
    rows → company account; standalone rows → deployment relay), e.g. a renamed
    `sending_company()` that stays `Option`, and the verification plan must include a test that a
    standalone notice for a company with a connected Resend account resolves the deployment
    transport (`transport_for(None)` or equivalent).
- Instruction lesson: a decision that changes a field's meaning must trace every reader of the
  field, not only the struct and the SQL (`src/adapters/persistence/AGENTS.md` "Row struct and
  queries move together" covers SQL readers only). Candidate (scope: TASK architecture-gate
  template): "For each field whose nullability or meaning changes, list every production reader
  (graft callers) and the behaviour each derives from it." Evidence: this finding.
- Resolution history: 2026-10-31 open (G1-R1).
  2026-10-31 **fixed at decision level** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): `sending_company() -> Option<Uuid>` derived from attribution drives `transport_for`; accessor and egress tests named. Batch review must see the egress test.

## FIXAPP-P05-G1-F3 — D4 `destination_channel_id → channels` NO ACTION FK makes company deletion fail

- Classification: **actionable defect, high**, architecture/correctness. Originates in gated
  decision D4.
- Evidence:
  - `channels` are deleted in round 1 by `companies`' RI trigger 19377, `workflow_runs` later in
    round 1 by trigger 20367, so `workflow_runs → workflow_message_destinations` deletes the
    destination in round 2 *after* the channel's `NO ACTION` check for
    `destination_channel_id` has already fired in round 2.
  - Reproduced (`logs/g1-r1-company-delete-destinations.sql/.log`): with D4's FKs case B and B2
    (channel FK present) fail with `violates foreign key constraint "d4_channel"`; case B3 (only
    D4's `thread_messages` composite FK) succeeds; case C (both FKs `DEFERRABLE INITIALLY
    DEFERRED`) succeeds and leaves no destination.
  - The FK is also redundant: D4's CHECK requires `destination_channel_id = channel_id` for a channel
    destination, and D4's `(company_id, channel_id, thread_id, message_id) → thread_messages` FK
    already proves `channel_id` is a channel of the company.
  - Impact: D4's replacement of `workflow_message_destination_message` would by itself make a
    company with channel destinations deletable again (case B3); the new channel FK re-blocks it
    for every company that ever sent to a channel.
  - Required: drop the separate `destination_channel_id → channels` FK (CHECK + thread_messages FK
    prove it) or make it `DEFERRABLE INITIALLY DEFERRED`; cover with a whole-company delete test.
    `replied_message_id → thread_messages` behaves like case B3 (safe); `destination_user_id →
    users` is not touched by company deletion.
- Instruction lesson: same as F1.
- Resolution history: 2026-10-31 open (G1-R1).
  2026-10-31 **fixed** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): `destination_channel_id` FK deferred; combined company delete succeeds (case A).

## FIXAPP-P05-G1-F4 — Pre-existing: company deletion already fails for any company with workflow message destinations

- Classification: **actionable (disposition required), medium**, requirements/verification.
  Pre-existing defect in the table D4 rewrites; the gate's assumption "company deletion behaves as
  today" (and that today it works) is wrong for this table.
- Evidence:
  - Today (unmodified schema) a company with one channel destination cannot be deleted:
    `logs/g1-r1-company-delete-destinations.log` case A fails on
    `workflow_message_destination_message` (messages deleted round 1 by trigger 19582, destination
    only in round 2 after the check).
  - With D4's thread_messages FK in place, a user destination with an email delivery still blocks
    company deletion on the untouched `workflow_message_destination_delivery` (deliveries round 1)
    and, with that removed, on `workflow_message_destination_principal` (principals round 1):
    `logs/g1-r1-company-delete-user-destination.log`.
  - No existing test deletes a company that has destinations (grep over workflow tests), so the
    suite is green while `delete_company_with_cleanup` fails for any company that ever ran
    `message.send`/`message.reply`.
  - Plan 05's outcome is "Nothing outlives its company" and each new constraint must show company
    deletion "still behaves as today"; a D4 company-deletion test written with destinations will
    hit this, one written without them proves nothing.
  - Required: record an explicit disposition in `TASK.md` before implementing D4: either (a) make
    `workflow_message_destination_delivery` and `workflow_message_destination_principal`
    `DEFERRABLE INITIALLY DEFERRED` in this phase (same table, but statements the plan does not name
    — needs the manager's agreement under README "edit only the statements it names"), or (b) record
    it as a known pre-existing failure, request a cross-phase notice, and keep D4's own
    company-deletion test honest (e.g. channel-kind destination only, with the gap named).
- Instruction lesson: "behaves as today" acceptance criteria should be established against a
  baseline run, not assumed (AGENTS.md "Establish the baseline before calling a failure
  pre-existing" covers flaky tests, not schema behaviour). Candidate (scope: plan template):
  "Before gating a constraint change, record the current behaviour of the deletes it must preserve
  with a populated fixture." Evidence: this finding.
- Resolution history: 2026-10-31 open (G1-R1).
  2026-10-31 **fixed** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): manager-approved scope extension (as reported in TASK/author message) defers `workflow_message_destination_delivery` and `_principal`; company delete with channel, user+email and reply+email destinations succeeds (case A); reply FK still blocks message deletion (case B). Residual other blockers only observable in the F6 tests.

## FIXAPP-P05-G1-F5 — D1.8 member-removal protocol inverts the notification projection's lock order

- Classification: **actionable defect, medium**, concurrency. Originates in gated decision D1.8.
- Evidence:
  - The projection's commit transaction re-resolves the recipient with
    `… FOR SHARE OF principal, account, member, company, channel`
    (`recipient_for_source`, `src/adapters/persistence/notification.rs:649-690`, lock clause `:684`, called from
    `commit_notification_projection`, `:211-258`). Row marks are applied in that order, so it
    locks the principal before the member.
  - D1.8 locks the member `FOR UPDATE` (a) and then updates the principal (b). Interleaving
    "projection locks principal → removal locks member → projection waits on member → removal waits
    on principal" is a deadlock; Postgres aborts one side after `deadlock_timeout`, so either the
    member removal returns an internal error or the projection burns an attempt. Today's
    `remove_member` updates the principal first and deletes the member second — the same order as
    the projection — so this inversion is new.
  - D1.8(a)'s justification cites the `FOR KEY SHARE` from D5's new `notifications →
    company_members` FK, but the projection already holds the stronger `FOR SHARE` on member and
    principal from `recipient_for_source` before it inserts; the decision-reviewer path holds the
    principal `FOR SHARE` (`workflow_decision_person_snapshot`). So taking the principal first
    already serializes against both producers.
  - Required: order D1.8 consistently with the producers — principal first (keep today's demotion
    as step one, or lock principal then member), then the notice lock/cancel, then the member
    delete — and correct the rationale. The competing-claimant test should cover both orders
    (producer first: notice ends cancelled; removal first: producer refuses/goes stale) plus an
    in-flight notice that receives only the intent.
- Instruction lesson: AGENTS.md asks for competing-claimant tests for concurrency protocols but not
  for an inventory of every existing locker of the rows the protocol locks. Candidate (scope:
  persistence AGENTS.md "Fence leases…" or a new "Lock order" note): "A protocol that adds row
  locks lists every existing statement that locks the same rows and takes them in the same order."
  Evidence: this finding.
- Resolution history: 2026-10-31 open (G1-R1).
  2026-10-31 **fixed at decision level** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): order is principal → member → notices → member delete, matching every producer (inspection); competing-claimant tests for both orders and an in-flight notice named. Live race test belongs to the batch.

## FIXAPP-P05-G1-F6 — Verification plan cannot detect cascade-order failures of the new FKs

- Classification: **actionable defect, medium**, verification.
- Evidence: TASK "Verification plan" lists filters only. Because `NO ACTION` checks depend on
  cascade rounds and trigger OID order (see background), the plan's required "channel deletion and
  company deletion still behave as today" tests only mean something if the deleted company/channel
  actually holds rows referencing every new FK: reviewers with notices (F1), destinations of each
  kind (F3/F4), internal notes, external threads, channel schedules of both binding kinds, intents
  with a settled reply, notifications, demand/poll rows. F1 and F3 would pass an unpopulated
  company-deletion test.
- Required: add to `TASK.md` a whole-company delete test (`own_database`, asserting table state)
  and a raw-SQL channel delete test whose fixture populates each of those rows, plus the D1.8 and
  F2 tests named above, each with nonzero counts.
- Instruction lesson: covered by the F1 candidate.
- Resolution history: 2026-10-31 open (G1-R1).
  2026-10-31 **fixed at plan level** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): populated whole-company and raw-SQL channel delete tests added to the r2 verification plan.

## FIXAPP-P05-G1-F7 — Optional: `internal_notes_thread_message_fk` passes company deletion only by trigger order

- Classification: **optional suggestion, low**, robustness.
- Evidence: notes are deleted in round 3 (`companies → channels → threads → internal_notes`) and
  the new FK's check for `thread_messages` (deleted round 2 via `messages`) also fires in round 3.
  It passes today only because `channels`' RI trigger (19377) precedes `messages`' (19582), so the
  thread cascade is queued first (`logs/g1-r1-delete-internal-note.log`: company, channel and
  thread deletes succeed; message delete stays blocked). Any constraint re-creation that reorders
  those OIDs flips it. The sibling `internal_notes_message_fk` is already `DEFERRABLE INITIALLY
  DEFERRED` for this reason; making the new FK deferred too costs nothing (both writers insert the
  association first).
- Resolution history: 2026-10-31 open (G1-R1), optional.
  2026-10-31 **fixed** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): FK deferred; covered by combined case A.

## FIXAPP-P05-G1-F8 — Optional: D5 leaves a now-dead deleted-company sweep in `clean_cursors`

- Classification: **optional suggestion, low**, requirements completeness.
- Evidence: `clean_cursors` (`src/adapters/persistence/workflow/fair_polling.rs:91-96`) deletes
  `workflow_poll_companies` rows whose company no longer exists ("Retire deleted-company hints…").
  With D5's `ON DELETE CASCADE` FK that predicate can never match. The plan asks to drop a sweep
  that exists only for deleted companies; the author correctly found `VALID_HINT` is not one, but
  this one is.
- Resolution history: 2026-10-31 open (G1-R1), optional.
  2026-10-31 **fixed at decision level** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): `clean_cursors` deleted-company statement dropped.

## FIXAPP-P05-G1-F9 — Optional: `schedule_runs` has the same two-path channel gap (out of named scope)

- Classification: **optional suggestion, low**, requirements (scope note).
- Evidence: `schedule_runs` carries `(company_id, channel_id) → channels` and `(company_id,
  binding_id) → workflow_bindings` separately (`\d schedule_runs` on the scratch schema), the same
  shape plan batch 3 fixes for `channel_schedules`. Not in the plan's table, so per README it is not
  this phase's statement to edit; D3's new `workflow_bindings` keys would make the same fix cheap.
  Suggest recording it as a cross-phase notice / follow-up candidate.
- Resolution history: 2026-10-31 open (G1-R1), optional.
  2026-10-31 **deferred (out of named scope)** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): recorded as a follow-up candidate in TASK "Cross-phase notices requested". Optional; not blocking.

## FIXAPP-P05-G1-F10 — Optional: redundant single-column parts FK and unnamed D5 failure mode

- Classification: **optional suggestion, low**, schema hygiene / failure modes.
- Evidence: (a) once `message_delivery_parts.company_id` is `NOT NULL`,
  `message_delivery_parts_delivery_id_fk (delivery_id) → message_deliveries(id)` duplicates the
  composite FK ("do not model two facts"); dropping it is optional and outside the named
  statements. (b) D5's new `companies` FKs make `discover` (`fair_polling.rs:73`, under a 1 s
  `lock_timeout`) and `fairness::available` (`fairness.rs:23`) take `FOR KEY SHARE` on the company
  row, so a concurrent company delete can make a poll time out or fail with 23503; name it as a
  failure mode (retry next poll) in D5.
- Resolution history: 2026-10-31 open (G1-R1), optional.
  2026-10-31 **accepted** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): (a) left as is by choice (optional); (b) failure mode named in the amendments.

## FIXAPP-P05-G1-F11 — TASK verification plan predates coordination notice fix-app-I004 r1

- Classification: **actionable, low**, requirements freshness (not an author error: the notice
  arrived during this review).
- Evidence: the authoritative `COORDINATION.md` moved to **revision 2** while this gate was under
  review (copy `logs/coordination-r2-seen-by-B-G1-R1.md`; the pinned input was revision 1). New
  plan-wide notice **fix-app-I004 r1** (user instruction) supersedes the README per-plan final gate:
  phases do not run the full suite or `scripts/check.sh`; they run targeted tests for affected
  behaviour, callers and shared contracts with nonzero counts, after `./scripts/reset-db.sh --all`
  when the schema changed, plus `cargo fmt --check`, compiling affected targets and `.sqlx/`
  regeneration. `TASK.md` "Requirements (original)" (acceptance: "final gate
  `./scripts/reset-db.sh --all` then `scripts/check.sh` passes") and "Verification plan" ("Final
  gate: … `scripts/check.sh`") still name the superseded gate, and "Coordination" acknowledges
  only I001-I003 at revision 1.
- Required: acknowledge I004 r1 / revision 2 in `TASK.md`, replace the final gate with I004's
  targeted scope (the populated company/channel deletion tests of F6 are affected behaviour and stay
  in scope), and pin revision 2 for the recheck. The gated D1-D5 architecture is not affected by
  I004.
- Instruction lesson: none beyond the workflow's existing freshness rule, which worked here.
- Resolution history: 2026-10-31 open (G1-R1).
  2026-10-31 **fixed** (G1-R2, decision r2, digest `9d5bde92…4c66`, coordination rev 2): I004 r1 acknowledged; acceptance and verification plan revised to targeted tests, fmt, build, `.sqlx`.

## FIXAPP-P05-G1-F12 — Optional: deferred FKs report violations at COMMIT

- Context: as above, but review round `FIXAPP-P05-G1-R2`, decision revision **r2**.
- Classification: **optional suggestion, low**, implementation watch item.
- Evidence: r2 makes `workflow_decision_reviewers_notice_fkey`, `destination_channel_id`,
  `workflow_message_destination_delivery`, `workflow_message_destination_principal` and
  `internal_notes_thread_message_fk` deferred. Their violations now surface from `COMMIT`, not the
  statement (`logs/g1-r2-company-delete-combined.log` case C: the insert is accepted, the commit
  fails). Existing tests that assert a statement-time 23503 on these FKs, and code that maps FK
  violations on the statement (e.g. `ChannelPersistence::delete` maps `is_foreign_key_violation()`
  on its `DELETE`, `channel.rs:418-434`), will not see them. Today the app pre-checks make those
  paths unreachable (a channel destination or note always leaves a thread, which `delete` refuses
  first), so this is a watch item for the batch: adapt such tests to assert at commit and keep the
  insert-time guards (reviewer guard trigger, writer ordering) as the first line.
- Resolution history: 2026-10-31 open (G1-R2), optional.
  2026-11-01 **closed, no change needed** (BR1, digest `2a3cb838…c09c4`): no application path deletes threads, messages, bindings or users, and the only statement-level FK mapper (`ChannelPersistence::delete`) is preceded by its own refusal; the tests that touch deferred keys assert at commit (`message_thread_fk_internal_note_message_is_filed_in_its_own_thread`, the reply-to test via autocommit).

---

Common context for every BR1 finding below: 2026-11-01, `implement-opus-reviewed-2_2`, task
`TASK.md` (copy `logs/br1-TASK-seen-by-B.md`), review `REVIEW.md`, batch "implementation batch 1 =
plan batches 1-5", stage **batch review**, review round `FIXAPP-P05-BR1`, reviewer Opus 5.5 / xhigh
(B), phase p05, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p05`, base = HEAD
`e6b0a51` (uncommitted changes), digest
`2a3cb838df2f8983bb82f2adf75cb7c044d0d121fa6d1fb02309702a8c4b09c4`, decisions r2 + r3
amendments, coordination revision 2 (copy `logs/coordination-r2-seen-by-B-BR1.md`).

## FIXAPP-P05-BR1-F1 — The removal-first member race has no competing-claimant test, and its sequential stand-in passes on any panic

- Classification: **required, low.** Gated verification item (G1-R2 F5 amendment) not delivered as
  planned; the protocol itself is correct.
- Evidence: the r2 amendment F5 lists three competing-claimant tests: producer first, "removal
  first (a producer arriving while removal holds the locks sees no current member)", and an
  in-flight notice. `src/adapters/persistence/delivery/tenant_tests.rs`
  `notification_delivery_tenant_late_producer_finds_no_member` runs `remove_member` to completion
  and only then starts the producer, so no lock is ever contended. Its only assertion is
  `assert!(refused.is_panic(), ...)`: any panic in `notice_on` (a parse, fixture or unrelated SQL
  error) passes, although the comment names the member key. TASK records no deviation. B's psql
  reproduction (`logs/br1-removal-first-race.log`) shows the real behaviour is right: the projection's
  `recipient_for_source` lock shape blocks behind removal's principal/member locks and then returns
  no recipient. So this is a missing regression net for a concurrency protocol (AGENTS.md: "include a
  test with competing claimants"), not a defect in the code.
- Required change: (1) Make the removal-first case concurrent. One way that needs no hooks: give the
  leaving member one pending notice, hold `SELECT ... FROM message_deliveries WHERE id = $notice FOR
  UPDATE` open in a third transaction, and spawn `remove_member`. It demotes the principal and locks
  the member, then blocks in `cancel_member_notices_on`. Start the producer: the
  principal/member `FOR SHARE` select the producer-first test uses, or the recipient lock shape.
  Wait until two backends are blocked, release the notice lock, and assert that removal succeeds,
  the producer's select returns no row (or its notification insert fails on
  `notifications_recipient_member_fk`), and the held notice ends `RecipientRevoked`. (2) Wherever a
  refusal is asserted, check the constraint name (`notifications_recipient_member_fk`), not
  `is_panic()`. Use `own_database` for the race (it counts blocked backends).
- Instruction lesson: when a gated verification item ships differently, record the deviation and
  its reason in TASK before requesting review.
- Resolution history: 2026-11-01 open (BR1). 2026-11-01 A: fixed — concurrent removal-first test `notification_delivery_tenant_removal_first_producer_finds_no_member` (own_database, held notice, 2 blocked backends, constraint-name assertion); see TASK "BR1 corrections"; awaiting recheck.
  2026-11-01 B: **fixed** (BR1-R2, digest `202392ee…7da34`, coordination rev 3): `notification_delivery_tenant_removal_first_producer_finds_no_member` (own_database) holds the notice, blocks removal inside the cancellation with principal and member locked, queues the producer behind it (2 blocked backends), and asserts no recipient, the `notifications_recipient_member_fk` refusal by name, `dead_letter` + `RecipientRevoked` and an external principal; the sequential test is gone. B: 5/5 repeats pass (`logs/br1r2-b-checks.log`).

## FIXAPP-P05-BR1-F2 — Optional: destination deletion tests do not show the email deliveries exist before deleting

- Classification: **optional, low.**
- Evidence: `message_thread_tests.rs` `message_thread_fk_company_deletion_removes_channel_and_user_destinations`
  and `..._removes_a_reply_destination` assert the destination kinds and then that every scoped
  count is 0 after the delete. They never assert that `message_deliveries`/`message_delivery_parts`
  were non-zero before it. The F4 requirement ("user with email delivery") therefore rests on the
  fixture's default delivery policy rather than on the test. The behaviour itself is covered at the
  SQL level (`logs/g1-r2-company-delete-combined.log`, `logs/br1-merged-company-delete.log`).
- Suggested change: assert `delivery_id IS NOT NULL` on the user and reply rows, or non-zero
  delivery counts, before deleting.
- Resolution history: 2026-11-01 open (BR1), optional. 2026-11-01 A: done — delivery-with-parts asserted before deletion in both tests; awaiting recheck.
  2026-11-01 B: **fixed** (BR1-R2, digest `202392ee…7da34`, coordination rev 3): both tests assert the user/reply destination has a delivery with parts before deleting.

## FIXAPP-P05-BR1-F3 — Optional: test comments and shared-database hygiene

- Classification: **optional, low.**
- Evidence: (a) `message_thread_fk_destination_names_a_message_of_its_own_thread` says "A message
  filed in another thread, of another channel", but `create_thread(setup.channel, ...)` uses the
  same channel. (b) `message_sql_tests.rs` "A second member user" inserts only a user (the user FK
  is to `users`). (c) `message_thread_fk_internal_note_message_is_filed_in_its_own_thread` runs
  `ALTER TABLE internal_notes DISABLE TRIGGER` on the shared `test_pool` database. That holds an
  ACCESS EXCLUSIVE lock on `internal_notes` until the failed commit, briefly blocking parallel tests;
  `own_database` would isolate it. (d) `fairness_tests.rs` leaves 33 "Stale demand" companies in the
  shared test database.
- Resolution history: 2026-11-01 open (BR1), optional. 2026-11-01 A: done — (a)(b) comments, (c) `Fixture::isolated`, (d) stale companies deleted; awaiting recheck.
  2026-11-01 B: **fixed** (BR1-R2, digest `202392ee…7da34`, coordination rev 3): comments corrected (a, b); note test on `Fixture::isolated` (c); stale companies deleted (d).

## FIXAPP-P05-BR1-F4 — Merge watch (not blocking this digest): p05's fairness fixture breaks on p01's owner key

- Classification: **optional for this digest; required at merge with p01** (`fix-app/p01`
  `4d940da`). Not observable at this digest.
- Evidence: p01 adds `companies_owner_member_fk (id, user_id) -> company_members(company_id,
  user_id) DEFERRABLE INITIALLY DEFERRED` plus owner-agreement constraint triggers, and asks
  fixtures to use `test_support::insert_company_with_owner`. p05's
  `workflow_fairness_invalid_offer_is_cleaned_before_older_invalid_history` (`fairness_tests.rs`)
  inserts each stale-demand company and its owner member as two autocommit statements. On the merged
  schema the first statement's commit fails `companies_owner_member_fk`; one transaction passes
  (`logs/br1-merged-fairness-fixture.log`). No other p05 fixture inserts a company. `tenant_tests.rs`
  `member()` inserts only non-owner members, and the owner `remove_member` call is a no-op.
- Also verified on the merged schema (no action needed): populated company deletion and p05's
  member removal both succeed (`logs/br1-merged-company-delete.log`, cases A and B). p01's
  "checked at the end of the statement" comment is inaccurate (per-round), but harmless for its three
  keys.
- Suggested action: the merge (or whichever phase merges second) rewrites that fixture with
  `insert_company_with_owner` and reruns `workflow_fairness_`. Pass this to the manager as a merge
  note.
- Resolution history: 2026-11-01 open (BR1), merge watch. 2026-11-01 A: fixture now writes company and owner row in one statement (passes on the p01 trial merge); switch to `insert_company_with_owner` at merge; awaiting recheck.
  2026-11-01 B: **fixed ahead of merge** (BR1-R2, digest `202392ee…7da34`, coordination rev 3): company + owner row in one CTE statement; author's trial merge with p01 `4d940da`, 166/0 (`logs/a-mergecheck-p01-4d940da.log`). At merge, switch to `insert_company_with_owner`.
