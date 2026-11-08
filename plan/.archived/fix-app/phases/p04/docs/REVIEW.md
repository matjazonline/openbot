# REVIEW — fix-app p04 (Authorization in the application layer)

Review record for phase p04 (workflow `implement-opus-reviewed-2_2`). Detailed rounds:
[agent-review-notes.md](agent-review-notes.md). Findings and dispositions:
[agent-review-findings.md](agent-review-findings.md). Task: [TASK.md](TASK.md).

## G1 — Architecture gate (decision revision r1)

- Round: **FIXAPP-P04-G1-R1** (2026-11-01), reviewer B (Opus 5.5).
- Inputs: decision text `logs/G1-r1-decisions.md`; HEAD `48626a1`; digest
  `c325eea2f3e71d55bbb4f3c5acdfb0df01515171e063db16d2366218d4aa3462` (exclusion
  `plan/fix-app/phases/p04/docs`); coordination revision 4 (`logs/COORDINATION-r4.md`;
  I001–I004 r1, C002 r1, C003 r1).
- Outcome: **changes required** (no pass for r1).

| Finding | Severity | Kind | Summary | Status |
| --- | --- | --- | --- | --- |
| FIXAPP-P04-G1-F1 | high | defect | Company update/delete keep a persistence owner check answering 500 (JSON API, `/ui/companies` delete); HTMX delete swallows errors | fixed in r2 |
| FIXAPP-P04-G1-F2 | medium | defect | 403 policy breaks `list_company_team_members`, which relies on `NotFound` | fixed in r2 |
| FIXAPP-P04-G1-F3 | medium | defect | List-scan company helpers (~30 sites) and `ui_team` stay HTTP authorization over 200-capped lists; no 403 | fixed in r2 |
| FIXAPP-P04-G1-F4 | medium | defect | Owner-only simulation surfaces have no stated level (risk of admin send-as); D3 misses three entry points | fixed in r2 |
| FIXAPP-P04-G1-F5 | medium | defect | Child-entity reads in rewritten handlers still read DB errors as "not found"/empty | fixed in r2 |
| FIXAPP-P04-G1-F6 | medium | defect | Skill-library operator checks stay in HTTP; monitoring/dashboard disposition unrecorded | fixed in r2 |
| FIXAPP-P04-G1-F7 | high | defect | Onboarding idempotency key conflates retry with re-onboarding: stale command replay, permanent Conflict | fixed in r2 |
| FIXAPP-P04-G1-F8 | medium | defect | No competing-claimants onboarding test; channel-error fault injection unspecified | fixed in r2 |
| FIXAPP-P04-G1-F9 | low | defect | In-tx re-check admits admins while the gate is owner-only; in-tx NotFound shown as form error | fixed in r2 |
| FIXAPP-P04-G1-F10 | low | defect | No bound on agents per onboarding transaction (up to ~200 under the company lock) | fixed in r2 |
| FIXAPP-P04-G1-F11 | low | defect | Retried custom agent re-pays prompt generation; factored library helper keeps an error collapse | fixed in r2 |
| FIXAPP-P04-G1-F12 | low | defect | base_layout htmx fragments get 4xx/5xx with no visible failure path | fixed in r2 |
| FIXAPP-P04-G1-F13 | low | optional | Record the workflow-authority boundary of the response policy | adopted r2 |
| FIXAPP-P04-G1-F14 | low | optional | Notifications still call persistence from HTTP | deferred (recorded) r2 |
| FIXAPP-P04-G1-F15 | low | optional | D5 needs a readable-channel list helper | adopted r2 |
| FIXAPP-P04-G1-F16 | low | optional | Reconcile 26 vs 23 sites; widen thread-read filters; name plan tests; hash domain separation | partly adopted r2; remainder adopted r3 (closed) |

## G1 — Correction recheck (decision revision r2)

- Round: **FIXAPP-P04-G1-R2** (2026-11-01), reviewer B (Opus 5.5).
- Inputs: decision text `logs/G1-r2-decisions.md`; HEAD `48626a1`; digest at round start
  `c325eea2f3e71d55bbb4f3c5acdfb0df01515171e063db16d2366218d4aa3462`, **at return
  `1da491129a47537d970a6f06d189f6d4b163dca6ae119e59af569828a7eafdee`** (author's WB1 edits to
  `src/adapters/persistence/company.rs`, `src/application/use_cases/company.rs` began during the
  recheck; not reviewed); coordination revision 4 (unchanged). Final observation before return: the edits were still in progress — 13 files (`app_error_impl.rs`, `persistence/company.rs`, `app_error.rs`, `inbound_event_worker.rs`, `agent_tests.rs`, new `use_cases/caller.rs`, `use_cases/company.rs`, new `use_cases/company_test_support.rs`, `use_cases/mod.rs`, `thread/mod.rs`, `thread/test_support.rs`, `use_cases/user.rs`, `domain/entities/user.rs`), digest `cef53723e33366a6aca07ba97c005a88651272aa0e8adb18cef0ca1d01cc822b`.
- Outcome: **changes required (minor)** — no pass for r2.

| Finding | Severity | Kind | Summary | Status |
| --- | --- | --- | --- | --- |
| FIXAPP-P04-G1-F17 | low | process | WB1 edits started before the G1 pass; artifact changed mid-recheck | fixed in r3 (records) |
| FIXAPP-P04-G1-F19 | low | defect | F3's explicit-id rule makes the operator's company dashboard 404/403 (resolve calls the managed read first) | fixed in r3 |
| FIXAPP-P04-G1-F18 | low | optional | State how `PUT /companies/{id}` answers authorization failures (not a 200 alert) | adopted r3 |
| FIXAPP-P04-G1-F20 | low | optional | `custom` role reuse edge; base_layout toast dismiss wiring; `onboarding_key` name clash | adopted r3 |

## G1 — Correction recheck (decision revision r3)

- Round: **FIXAPP-P04-G1-R3** (2026-11-01), reviewer B (Opus 5.5).
- Inputs: decision text `logs/G1-r3-decisions.md`; HEAD `48626a1`; freeze digest
  `4f458bd25090bae457d310e858875fba1211f2f7eafa94510b16728d51e1e7ed` (exclusion
  `plan/fix-app/phases/p04/docs`; 14 in-progress WB1 files listed in TASK "G1 r3 freeze", unchanged
  at start and return); coordination revision 4 (unchanged).
- Outcome: **PASS for G1 decision revision r3.** No open acceptance-blocking findings; F13–F16,
  F18, F20 adopted or dispositioned. The pass covers the decisions only; the in-progress WB1 files
  are unreviewed and belong to the WB1 batch review.


## G1 — Fresh resume validation (decision revision r4)

- Round **FIXAPP-P04-G1-R4** (2026-11-04), workflow `implement-astra-reviewed-2-2`, independent
  reviewer `/root/p04_reviewer` (requested Astra/high; actual runtime settings not exposed).
- Decision [logs/G1-r4-decisions.md](logs/G1-r4-decisions.md), SHA256
  `90714b8edd91eb82dbff5f9c401fb02d8562b51e7d535b0c01fdb19fc8f9a245`; start/base/HEAD
  `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; source digest
  `c5b583876682c317238f7a32d04fd2ba70ff220b40a87cc6b8c394daeea893bc`, sole exclusion
  `plan/fix-app/phases/p04/docs`, rechecked unchanged at return.
- Coordination revision 6, pin `logs/COORDINATION-r6.md`, hash
  `d3c5b83ecf08b42314e1194bb371ebee030216d8c875cc7f76cf5b0f20322371`; I001–I004/r1,
  C002/C003/r1, C007/r1 applicable; C008/r1 proposed output. Freshness rechecked.
- **Changes required — no G1 r4 pass.** F11 reopened (**medium**): same-key concurrent custom
  submissions can both pay for prompt generation before serialization; the loser's output is
  discarded. See [round note](agent-review-notes.md#fixapp-p04-g1-r4) and
  [finding follow-up](agent-review-findings.md#fixapp-p04-g1-f11--reopened-follow-up-competing-retries-still-discard-a-paid-prompt).
  Original historical passes/dispositions remain preserved. No source acceptance or test pass
  is implied; all source remains WIP. No other architecture blocker identified in D1–D5/D7.


## G1 — Correction recheck (decision revision r5)

- **FIXAPP-P04-G1-R5**, 2026-11-04, `implement-astra-reviewed-2-2`; independent B
  `/root/p04_reviewer` (requested Astra/high, actual runtime settings not exposed).
- Decision [r5](logs/G1-r5-decisions.md), SHA256
  `176ddf0a29a0cf9ca5b41358f4ccdf7783af6ae21c031c3475737de52e5c06c2`; source digest
  `510dba5e4bd0cdf496b59c6fcacbc424a02e04721fe9fbd826e30cb3d8aaaef4`; start/base/HEAD
  `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; sole exclusion `plan/fix-app/phases/p04/docs`.
  Coordination r6 pin unchanged; applicable I001–I004/r1, C002/C003/C007/r1, proposed C008/r1.
- **Changes required.** F11 fixed in decision; **F21 medium** newly identifies a pool wait
  cycle caused by invoking pool-backed preparation helpers while retaining the transaction.
  [Round note](agent-review-notes.md#fixapp-p04-g1-r5),
  [finding](agent-review-findings.md#fixapp-p04-g1-f21--retained-onboarding-transaction-waits-for-a-second-pool-connection).
  No code acceptance or test pass; unchanged D1–D5/D7 evidence reused. Source and coordination
  rechecked unchanged before return.


## G1 — Correction recheck (decision revision r6)

- **FIXAPP-P04-G1-R6**, 2026-11-04, `implement-astra-reviewed-2-2`; independent B
  `/root/p04_reviewer`, requested Astra/high (actual runtime settings not exposed).
- **PASS for G1 r6**, [decision](logs/G1-r6-decisions.md) SHA256
  `c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b`.
  F21 fixed in decision; F11 remains fixed; no open architecture blockers. See
  [round note](agent-review-notes.md#fixapp-p04-g1-r6) and
  [F21 disposition](agent-review-findings.md#fixapp-p04-g1-f21--r6-correction-disposition).
- Start/base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; source digest
  `510dba5e4bd0cdf496b59c6fcacbc424a02e04721fe9fbd826e30cb3d8aaaef4`; sole exclusion
  `plan/fix-app/phases/p04/docs`. Snapshot unchanged before/after. Coordination r6 pin unchanged,
  SHA256 `d3c5b83ecf08b42314e1194bb371ebee030216d8c875cc7f76cf5b0f20322371`; applicable
  I001–I004/r1, C002/C003/C007/r1, C008/r1 proposed.
- Pass covers decisions D1–D7 with unaffected prior evidence reused. All source remains WIP;
  WB1/WB2 tests and independent code reviews are outstanding. No cargo/DB checks run by B.


## G1 — Local continuity validation (unchanged decision r6)

- **FIXAPP-P04-G1-R7**, 2026-11-04, `implement-astra-reviewed-2-2`; independent B `/root/p04_reviewer`, requested Astra/high (actual runtime settings unavailable).
- **PASS for G1 r6/current applicability**, [decision](logs/G1-r6-decisions.md) SHA256 `c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b`, D1–D7. [Round note](agent-review-notes.md#fixapp-p04-g1-r7); no new findings; F11/F21 remain fixed in decision.
- Local worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p04`, branch `fix-app/p04`; start/base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; source `a366d1d6eaba053c6a66ad52a798a073ec14e8a9cecd5903c3a22840d504e092`, sole exclusion `plan/fix-app/phases/p04/docs`, unchanged before/after.
- Authority `/Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md` r15; [immutable pin](logs/COORDINATION-r15-resumed.md) byte-identical before/after. I001/I003/I004 r1, I002 r2, C002/C003/C007/C009 r1 apply; I005 not applicable; C008 proposed output. Current explicit resume and sequential batching verified.
- [Identity checks](logs/G1-R7-reviewer-final-checks.log); original phase requirements and current notices reviewed. No cargo/DB/full-suite checks; no code acceptance. WB1 remains unaccepted; WB2 starts only after WB1 whole-batch acceptance.


## WB1 — Whole batch review, plan batches 1–4

- **FIXAPP-P04-WB1-R1-MAC02**, 2026-11-04, `implement-astra-reviewed-2-2`; independent B `/root/p04_reviewer`, requested Astra/high, actual runtime settings unavailable.
- **CHANGES REQUIRED**, digest `c4983d97a745723104fcdf8a4a3d1804739e1863d72721fe14e03e901b55b3e3`; start/base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`, sole exclusion `plan/fix-app/phases/p04/docs`. Entire dirty WB1 source reviewed. G1 r6 retained; current r15 pin/authority unchanged.
- [Round note](agent-review-notes.md#fixapp-p04-wb1-r1-mac02); **F1-MAC02 medium** team mutations swallow authorization errors into HTTP 200; **F2-MAC02 low** new layout test fails required Clippy. Both open in [findings](agent-review-findings.md).
- Final author affected union 468 passed / 0 failed / 4 ignored, all 35 filters nonzero; SQLx/all-target compilation passes; Clippy failed. No full suite or fixture checks by B. Source identity confirmed before/after. No WB1 acceptance; WB2 stays unstarted until corrected WB1 acceptance.


## WB1 — Correction recheck and pass

- **FIXAPP-P04-WB1-R2-MAC02**, 2026-11-04; independent B `/root/p04_reviewer`, requested Astra/high (actual runtime unavailable), `implement-astra-reviewed-2-2`.
- **PASS WB1 plan batches1–4** for `c5debdeab77f80515c4a88ae2384af6503ae320e53cb48b4fc3dc3c87a2719da`; start/base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`, sole exclusion `plan/fix-app/phases/p04/docs`. R1 unchanged evidence reused; correction and affected callers reviewed.
- [Round note](agent-review-notes.md#fixapp-p04-wb1-r2-mac02); **F1/F2-MAC02 fixed**, no new findings. Team mutation errors retain typed responses; business refusals remain explicit; layout lint fixed.
- Final affected472passed/0failed/4ignored,35nonzero filters; SQLx/all-target offline compilation, Clippy-Dwarnings, fmt/whitespace PASS. [Final checks](logs/mac02-WB1-correction-final-check-summary.txt). Current coordinationr15 pin and source digest unchanged before/after; G1r6 retained.
- Pass is limited to WB1. WB2 and combined integration remain outstanding; no phase-complete claim. Root acceptance is the next step before WB2 implementation.


## WB2 — Whole batch and current phase shared-effects review

- **FIXAPP-P04-WB2-R1-MAC02**, 2026-11-04; independent B `/root/p04_reviewer`, requested Astra/high (actual runtime unavailable), `implement-astra-reviewed-2-2`.
- **PASS WB2 plan batches5–6 and current whole-phase shared effects** for `9b1bfc76eb32f15cf6131b1ab55bebe375d49b2beb90b03d48a7dc71dcc29e91`; start/base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`, exact sole exclusion `plan/fix-app/phases/p04/docs`. G1 r6 and coordination r16 pin unchanged; accepted WB1 evidence reused only where unaffected.
- [Round note](agent-review-notes.md#fixapp-p04-wb2-r1-mac02), [disposition](agent-review-findings.md#fixapp-p04-wb2-r1-mac02--batch-disposition), [final identity/filter recount](logs/WB2-R1-reviewer-final-checks.json). No new findings. F11/F21 satisfied in source and concurrency/one-connection tests; WB1 F1/F2 remain closed.
- Independent inspection covers onboarding authorization, single-transaction copies/provisioning, retry identity/provider effects, cancellation/timeout/rollback, production connection inventory and shared model helpers, HTTP wiring and SMTP trust/error fallback. Author final affected union **523 passed / 0 failed / 4 ignored**, all40 filters nonzero; final offline all-target Clippy -Dwarnings, fmt and whitespace PASS. B inspected evidence and recounted filters; no fixture checks or full suite by B.
- Source and coordination verified before/after. This is acceptance of the frozen p04 phase artifact, not later rebase/merge resolutions or combined-main integration. No full-suite, browser or external-provider verification claimed.
