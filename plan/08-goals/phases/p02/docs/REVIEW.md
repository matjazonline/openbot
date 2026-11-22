# 08G-P02 review record

Reviewer B entries. Details live in [agent-review-notes.md](agent-review-notes.md) and [agent-review-findings.md](agent-review-findings.md); this file links them.

## B1 architecture gate — decision revision D1-r1 — CHANGES REQUIRED (2026-11-21)

- Round note: `08G-P02-D1-R1-N1`.
- Reviewed: D1.1–D1.10, Failure modes, Verification plan, and the `channel.call` startup disposition, as retained in `logs/08G-P02-D1-r1-decision.md`.
- Identity: HEAD `42bc327e61674dcd5114f3c948454036dfd51506`, digest `dfb7097b05f83153178c265aad32a87fb36534839a74fa3c624daf5f930bed34` (exclusion `plan/08-goals/phases/p02/docs`), coordination revision 3 (`logs/08G-P02-coordination-r3.md`), no notices. Rechecked before returning.
- Evidence basis: code inspection only; nothing executed.

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| `08G-P02-D1-R1-F1` | defect | medium | Admission returns `unavailable`, not `unit.archived`, when the binding's run-as is the archived unit's own lead (D1.5 after D1.10) | fixed in D1-r2 |
| `08G-P02-D1-R1-F2` | defect | high | Retry control, native MCP admission and schedule run-now open work on an archived unit without passing `admit` | fixed in D1-r2 |
| `08G-P02-D1-R1-F3` | defect | medium | D1.8 misses transport bindings and other writers, assumes a lock some do not hold, and leaves desk delete / move-out undefined | fixed in D1-r2 |
| `08G-P02-D1-R1-F4` | defect | medium | Cancel-and-archive widens cancel authority, writes no actor event for root runs, and omits lock, recount and bound | fixed in D1-r2 |
| `08G-P02-D1-R1-F5` | defect | low | Skipped schedules are advanced "as today": one-offs are disabled and `last_run_at` is stamped | fixed in D1-r2 |
| `08G-P02-D1-R1-F6` | defect | low | Verification plan lacks mail/relay, archive-vs-mail, 02D.4, restore cases and the mapping table | fixed in D1-r2 |
| `08G-P02-D1-R1-S1` | optional | — | Deferred constraint trigger for "no active child under an archived unit" | adopted in D1-r2 |
| `08G-P02-D1-R1-S2` | optional | — | State the lock the backstop trigger takes | adopted in D1-r2 |
| `08G-P02-D1-R1-S3` | optional | — | Enum instead of the `cancel_open` flag | adopted in D1-r2 |
| `08G-P02-D1-R1-S4` | optional | — | Constraints B1 places on routing reads, gate order, default-unit functions and B2 | adopted in D1-r2 |

Accepted without change: D1.1, D1.2, D1.7, D1.9, D1.10, the D1.4 race argument for `admit`, and the `channel.call` NOT APPLICABLE disposition (to be kept as a NOT EXERCISED criterion).

## B1 architecture gate — correction recheck, decision revision D1-r2 — CHANGES REQUIRED (2026-11-21)

- Round note: `08G-P02-D1-R2-N1`. Reviewed text: `logs/08G-P02-D1-r2-decision.md`. Same HEAD, digest and coordination revision as above, rechecked before returning. Inspection only.
- All R1 findings are resolved in the decision; two low defects were introduced by the correction.

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| `08G-P02-D1-R2-F1` | defect | low | Backstop trigger `UPDATE OF finished_at` never fires on a reopen; key it on `state` | fixed in D1-r3 |
| `08G-P02-D1-R2-F2` | defect | low | Archived check "in the management helper" would refuse allowed writes (desk delete, schedule disable/delete, binding deactivate) | fixed in D1-r3 |
| `08G-P02-D1-R2-S1` | optional | — | `FOR UPDATE OF` in `claim_due`; deadlock outcome in the running-job case; record U3/U4 under User decisions | adopted in D1-r3 |

## B1 architecture gate — correction recheck, decision revision D1-r3 — PASS (2026-11-21)

- Round note: `08G-P02-D1-R3-N1`. Reviewed text: `logs/08G-P02-D1-r3-decision.md`. HEAD `42bc327e61674dcd5114f3c948454036dfd51506`, digest `dfb7097b05f83153178c265aad32a87fb36534839a74fa3c624daf5f930bed34`, coordination revision 3, rechecked before returning. Inspection only.
- `08G-P02-D1-R2-F1`, `08G-P02-D1-R2-F2` fixed; `08G-P02-D1-R2-S1` adopted. No open findings for the B1 gate.
- The pass covers the design and verification plan (D1-r1 unchanged parts + D1-r2 + D1-r3). Implementation evidence is still to come; `channel.call` remains NOT EXERCISED.

## B1 batch review — 08G-P02-B1-R1 — CHANGES REQUIRED (2026-11-22)

- Round note: `08G-P02-B1-R1-N1`. HEAD/review base `42bc327e61674dcd5114f3c948454036dfd51506`, digest `98cbdbc7f5ecd0ba310fbed0770954e7fe66a04f4ed48594942a92b8403ae887`, coordination revision 3; rechecked before returning. Manifest `logs/08G-P02-B1-R1.evidence.json` valid.
- Reviewer check: `cargo test --lib -- unit_archive message_admission_classifies` → 30 passed (`logs/08G-P02-B1-R1-reviewer-focused.txt`). Everything else by inspection.

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| `08G-P02-B1-R1-F1` | defect | low | A local-agent session started before the archive still accepts `Send` on an archived unit's conversation | fixed (B1-R2) |
| `08G-P02-B1-R1-F2` | defect | low | Plan's Deployment-harness message check replaced by a stand-in that skips the message path | fixed (B1-R2) |
| `08G-P02-B1-R1-F3` | defect | low | Archive/restore routes and the 409 bodies (`code`, `count`) have no test | fixed (B1-R2) |
| `08G-P02-B1-R1-F4` | defect | low | Six cloudroom e2e failures are an unverified affected path, recorded as unrelated (and counted as seven) | fixed (B1-R2) |
| `08G-P02-B1-R1-S1` | optional | — | Function length in `archive.rs`; deadlock arm of the running-job test not produced; `sponsor:` acting role in the history pane | open |

## B1 correction recheck — 08G-P02-B1-R2 — PASS (2026-11-22)

- Round note: `08G-P02-B1-R2-N1`. HEAD/review base `42bc327e61674dcd5114f3c948454036dfd51506`, digest `a10ec4c20a9011c4af28694f27467765e02ed34dd8cf54d33e193229771012ad`, coordination revision 3; rechecked before returning. Manifest `logs/08G-P02-B1-R2.evidence.json` valid.
- `08G-P02-B1-R1-F1`…`F4` fixed; `08G-P02-B1-R1-S1` optional, open. Reviewer check: 32 `unit_archive`/classify tests passed (`logs/08G-P02-B1-R2-reviewer-focused.txt`).
- Not covered by this pass: `channel.call` (NOT EXERCISED), a full-suite run after the corrections, Batch 2 scope.

## Batch 2 batch review — 08G-P02-B2-R1 — CHANGES REQUIRED (2026-11-22)

- Round note: `08G-P02-B2-R1-N1`. HEAD/review base `42bc327e61674dcd5114f3c948454036dfd51506`, digest `51dd88e18771de73170477a1a633f13bf5adeecb34f9c59835ed1fcc60849afa`, coordination revision 3; rechecked before returning. Manifest `logs/08G-P02-B2-R1.evidence.json` valid.
- Reviewer check: 42 `unit_archive` tests passed (`logs/08G-P02-B2-R1-reviewer-focused.txt`). Everything else by inspection.

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| `08G-P02-B2-R1-F1` | defect | medium | Hits from an archived child are not marked archived on the `reports:<slug>` label (02C.1) | fixed (B2-R2) |
| `08G-P02-B2-R1-F2` | defect | low | No memory case in `unit_archive_scopes_`; the "namespace not retired" assertion is vacuous | fixed (B2-R2) |
| `08G-P02-B2-R1-F3` | defect | low | 02C.4 (desk history readable; roles on archived units grant nothing else) has no disposition or check | fixed (B2-R2) |
| `08G-P02-B2-R1-F4` | defect | low | "Include archived departments" filters only the caps table; the department grouping always lists archived units, unlabelled | fixed (B2-R2) |
| `08G-P02-B2-R1-S1` | optional | — | Run-board scope option; `managed_units` omission unrecorded; resource edits in an archived unit undocumented | closed as recorded (B2-R2) |

## Batch 2 correction recheck — 08G-P02-B2-R2 — CHANGES REQUIRED (2026-11-22)

- Round note: `08G-P02-B2-R2-N1`. HEAD `42bc327e61674dcd5114f3c948454036dfd51506`, digest `6de44ac6cdd14b58bf950e56b1332842be5b4d24585f50c3e196eb4d3d33e2b5`, coordination revision 3; rechecked before returning. Manifest `logs/08G-P02-B2-R2.evidence.json` valid.
- Reviewer check: 55 tests (`unit_archive`, `role_binding`, `role_context`) passed (`logs/08G-P02-B2-R2-reviewer-focused.txt`).

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| `08G-P02-B2-R2-F1` | defect | medium | Archive/restore change agents' role-binding context (new `workflow_target_departments` filter) without `role_binding::check_principals_on` | fixed (B2-R3) |

## Batch 2 correction recheck — 08G-P02-B2-R3 — PASS (2026-11-22)

- Round note: `08G-P02-B2-R3-N1`. HEAD `42bc327e61674dcd5114f3c948454036dfd51506`, digest `b066cce60ef4ee7a3928db2f5e2f6054aaa1cf922d129b44a2b61b7654123559`, coordination revision 3; rechecked before returning. Manifest `logs/08G-P02-B2-R3.evidence.json` valid.
- `08G-P02-B2-R2-F1` fixed. Reviewer check: 56 tests (`unit_archive`, `role_binding`) passed (`logs/08G-P02-B2-R3-reviewer-focused.txt`).
- Not covered by this pass: full suite and `sqlx prepare --check` after the Batch 2 changes (final gate); the in-flight direct-address run disposition is untested.
