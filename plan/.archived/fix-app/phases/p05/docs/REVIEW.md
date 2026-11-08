# fix-app p05 — Review record

Workflow `implement-opus-reviewed-2_2`. Reviewer (B): Opus 5.5 / xhigh. Detailed notes:
`agent-review-notes.md`; findings: `agent-review-findings.md`; evidence: `logs/g1-r1-*`.

## Architecture gate G1 — decisions D1-D5, revision r1

- Round: [FIXAPP-P05-G1-R1](agent-review-notes.md) — 2026-10-31.
- Inputs: decision revision r1 (`TASK.md`, copy `logs/gate-r1-TASK-snapshot.md`); code base
  `e6b0a51`, digest `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66`
  (exclusions `plan/fix-app/phases/p05/docs`); coordination revision 1 with notices I001 r1,
  I002 r1, I003 r1 (copy `logs/coordination-r1.md`).
- Outcome: **changes required** (no pass for r1).

| ID | Severity | Kind | Summary |
| --- | --- | --- | --- |
| [FIXAPP-P05-G1-F1](agent-review-findings.md) | high | actionable | D1.3 composite NO ACTION reviewer→notice FK makes `DELETE FROM companies` fail for any company with a parked decision (reproduced). |
| [FIXAPP-P05-G1-F2](agent-review-findings.md) | high | actionable | D1.6 leaves the email sending account keyed on `DeliveryRecord::company_id()`; notices would go through the tenant's own Resend account instead of the deployment relay. |
| [FIXAPP-P05-G1-F3](agent-review-findings.md) | high | actionable | D4 `destination_channel_id → channels` NO ACTION FK makes company deletion fail for channel destinations (reproduced); redundant with CHECK + thread_messages FK. |
| [FIXAPP-P05-G1-F4](agent-review-findings.md) | medium | actionable (disposition) | Pre-existing: company deletion already fails for any company with workflow message destinations (message/delivery/principal NO ACTION FKs); TASK must record a disposition. |
| [FIXAPP-P05-G1-F5](agent-review-findings.md) | medium | actionable | D1.8 locks member then principal, inverting the notification projection's principal→member `FOR SHARE` order (deadlock); rationale cites the wrong lock. |
| [FIXAPP-P05-G1-F6](agent-review-findings.md) | medium | actionable | Verification plan lacks populated whole-company / channel delete fixtures, so cascade-order failures (F1, F3) would pass. |
| [FIXAPP-P05-G1-F7](agent-review-findings.md) | low | optional | `internal_notes_thread_message_fk` passes company deletion only by RI trigger order; make it deferred like its sibling. |
| [FIXAPP-P05-G1-F8](agent-review-findings.md) | low | optional | `clean_cursors`' deleted-company sweep becomes dead under D5's FK. |
| [FIXAPP-P05-G1-F9](agent-review-findings.md) | low | optional | `schedule_runs` has the same two-path channel gap (outside named scope). |
| [FIXAPP-P05-G1-F10](agent-review-findings.md) | low | optional | Redundant single-column parts FK; name the poll/demand FK failure mode. |
| [FIXAPP-P05-G1-F11](agent-review-findings.md) | low | actionable | Coordination moved to revision 2 during review: notice fix-app-I004 r1 supersedes the `scripts/check.sh` final gate still named in TASK. |

Freshness: digest and `TASK.md` unchanged at return; `COORDINATION.md` advanced to revision 2
(fix-app-I004 r1, copy `logs/coordination-r2-seen-by-B-G1-R1.md`) — recorded as F11; it does not
alter the D1-D5 findings.

## Architecture gate G1 recheck — decision revision r2

- Round: [FIXAPP-P05-G1-R2](agent-review-notes.md) — 2026-10-31, by finding ID.
- Inputs: decision revision r2 (`TASK.md` amendments + r2 verification plan, copy
  `logs/gate-r2-TASK-snapshot.md`); code unchanged at `e6b0a51`, digest
  `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66`; coordination revision 2
  (I001-I004 r1, copy `logs/coordination-r2.md`).
- Outcome: **PASS for r2.** F1, F3, F4, F7, F11 fixed (F1/F3/F4/F7 re-verified on a scratch schema,
  `logs/g1-r2-company-delete-combined.*`); F2, F5, F6, F8 fixed at decision/plan level (their tests
  belong to the batch); F9 deferred out of scope (optional); F10 accepted.

| ID | Severity | Kind | Summary |
| --- | --- | --- | --- |
| [FIXAPP-P05-G1-F12](agent-review-findings.md) | low | optional | Deferred FKs report violations at COMMIT; adapt statement-time FK assertions/mappings during implementation. |

Carry to the batch review: F2 egress test, F5 live competing-claimant tests, F6 populated deletion
tests, F12.

Freshness at return (G1-R2): `TASK.md` = r2 snapshot (PASS scope). The code digest drifted to
`d318e4ba…9d10b` because implementation edits started during the round (not reviewed here; batch
review needs a fresh digest). `COORDINATION.md` I002 gained an operational disk note without a
revision bump (copy `logs/coordination-r2-amended-seen-by-B-G1-R2.md`); no effect on the gate.

## Batch review BR1 — implementation batch 1 (plan batches 1-5)

- Round: [FIXAPP-P05-BR1](agent-review-notes.md) — 2026-11-01.
- Inputs: digest `2a3cb838df2f8983bb82f2adf75cb7c044d0d121fa6d1fb02309702a8c4b09c4` (base = HEAD
  `e6b0a51`, uncommitted), TASK gate r2 + r3 amendments + Check log (copy
  `logs/br1-TASK-seen-by-B.md`), coordination revision 2 edited in place (copy
  `logs/coordination-r2-seen-by-B-BR1.md`), p01 `4d940da` for merge awareness.
- B's checks: plan filters 23/0 (`logs/br1-b-plan-filters.log`); F5 tests x5 and `company_invite`
  10/0 (`logs/br1-b-repeat-and-callers.log`); `cargo fmt --check` clean; removal-first race
  reproduced with real lock shapes (`logs/br1-removal-first-race.log`); merged p05+p01 schema:
  company delete and member removal pass (`logs/br1-merged-company-delete.*`), fairness fixture
  shape fails (`logs/br1-merged-fairness-fixture.*`).
- Accepted: schema D1-D5 r2 and r3 (reply-to deferral; binding hard-delete refusal is the right
  assertion), `remove_member`/`cancel_member_notices_on` (D1.8 r2), BounceInfo, F8, F2 egress test,
  F6 deletion tests, F12 closed.
- Outcome: **CHANGES REQUIRED** — FIXAPP-P05-BR1-F1.

| ID | Severity | Kind | Summary |
| --- | --- | --- | --- |
| [FIXAPP-P05-BR1-F1](agent-review-findings.md) | low | required | Removal-first member race tested only sequentially, with an assertion that passes on any panic. |
| [FIXAPP-P05-BR1-F2](agent-review-findings.md) | low | optional | Destination deletion tests do not show the email deliveries exist before deleting. |
| [FIXAPP-P05-BR1-F3](agent-review-findings.md) | low | optional | Test comment inaccuracies; DDL on the shared test DB; leftover fairness companies. |
| [FIXAPP-P05-BR1-F4](agent-review-findings.md) | merge | merge watch | After merging p01, the fairness fixture's two-statement company insert fails `companies_owner_member_fk`. |

## Batch review BR1 recheck

- Round: [FIXAPP-P05-BR1-R2](agent-review-notes.md) — 2026-11-01, by finding ID.
- Inputs: digest `202392ee3dd968a2c9c80aeccf1b293b13ee79c904b6dd02cd67f18690d7da34` (base = HEAD
  `e6b0a51`), TASK "BR1 corrections" (copy `logs/br1r2-TASK-seen-by-B.md`), coordination revision 3
  (`logs/coordination-r3.md`). Delta confirmed: five test files only.
- B's checks: plan filters + touched suites 56/0; both race tests 5x clean; `cargo fmt --check`
  clean (`logs/br1r2-b-checks.log`, `logs/br1r2-b-fmt-check.log`).
- Outcome: **PASS.** BR1-F1 fixed (live removal-first race, constraint asserted by name); F2, F3
  fixed; F4 fixed ahead of merge (trial merge with p01 166/0). No new findings. Merge note: use
  `insert_company_with_owner` once p01 is merged.
