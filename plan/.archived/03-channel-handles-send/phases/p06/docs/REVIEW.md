# p06 independent review

## CHS-P06-AG1-R1 — CHANGES REQUIRED

2026-11-07. Architecture review of exact CHS-P06-D1/r1 SHA `88b417d440d9ff30154e45fd06d90d3fe4b2d6b65fc6344ab57bb046605ccbed`, source `c50170bc439a8c58e37fb20ee861230b0bdc26fe199bf7bc116a62f7885a7976`, HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`.

- **CHS-P06-AG1-F1 — high/correctness:** required-body root schema conflicts with retained reviewed reject→End and custom early-End paths. Valid review rejection would fail submission validation and leave the decision open. The terminal behavior needs an explicit revision. [Evidence and required verification](agent-review-findings.md#chs-p06-ag1-f1--typed-native-response-schema-does-not-cover-retained-rejection-and-early-end-routes).

[Coverage and limitations](agent-review-notes.md#chs-p06-ag1-r1--architecture-gate-changes-required), [entry](logs/CHS-P06-AG1-R1-reviewer-entry.json), [exit](logs/CHS-P06-AG1-R1-reviewer-exit.json). Architecture-only result: no draft code or runtime acceptance.

Exit freshness limitation: the two externally owned verification scripts `scripts/check-workflow-evidence.py` and `scripts/tests/workflow-evidence.py` changed during review. Entry source matched; exit source does not. HEAD, D1/r1 and coordination r19 remain exact. The architecture finding stands against immutable D1/r1; no source acceptance is issued. Retained exit receipt records the mismatch.


## CHS-P06-AG1-R2 — PASS (architecture decision only)

2026-11-07. **PASS** for exact [CHS-P06-D1/r2](logs/CHS-P06-D1-r2.md), SHA `bbef5246c9403f3279a7398d6d9d7cdea8d9dbf516e1c598a211c6404509811b`. F1 is [FIXED at architecture level](agent-review-findings.md#chs-p06-ag1-r2-resolution--fixed-at-architecture-level): genuine suppressed rejection and explicit native custom-terminal authoring constraints cover the previously omitted terminal paths. No further actionable findings.

Isolated worktree `/private/tmp/openbot-chs-p06-3f43t2k7/worktree`, branch `codex/chs-p06-3f43t2k7`, HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; exact source `12004bd37b416ffe9000c47d04281525fbd8882877ab3352a2cd346290f53953`. Coordination21 authority, isolated copy and immutable pin are identical. [Notes/coverage](agent-review-notes.md#chs-p06-ag1-r2--architecture-correction-recheck-pass), [entry](logs/CHS-P06-AG1-R2-reviewer-entry.json), [exit](logs/CHS-P06-AG1-R2-reviewer-exit.json).

Unchanged R1 architecture coverage is explicitly reused. No p06 draft code, runtime behavior or test acceptance is implied; targeted implementation verification and whole-phase review remain required.


## CHS-P06-B1-R1 — CHANGES REQUIRED (whole phase)

2026-11-07. Reviewed the entire71-path p06 delta against base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`, exact digest `ccc172b148c3ae623f2c33af4aaddaf20e0b419a52a22d1565b4a2facb2f9269`, isolated branch `codex/chs-p06-3f43t2k7`. D1/r2 and coordination22 unchanged.

- **CHS-P06-B1-F1, medium:** later questions addressed to a previous human answer are excluded by the original-source filter, losing active wait/delivery status.
- **CHS-P06-B1-F2, medium:** a child formal-review wait leaves the owning response labeled Processing.
- **CHS-P06-B1-F3, medium:** new native selection makes an existing onboarding teardown/replay regression fail; adapt its prerequisite through the authorized clear operation, preserving retention constraints.

[All evidence and correction requirements](agent-review-findings.md), [whole-phase coverage/check limitations](agent-review-notes.md#chs-p06-b1-r1--whole-phase-code-review-changes-required), [71-path coverage](logs/CHS-P06-B1-R1-reviewer-coverage.json), [entry](logs/CHS-P06-B1-R1-reviewer-entry.json), [exit](logs/CHS-P06-B1-R1-reviewer-exit.json).171distinct recorded passes independently matched to log lines; additional onboarding16PASS/1FAIL. No full-suite, live-provider or phase acceptance claimed. Architecture PASS remains architecture-only.


## CHS-P06-B1-R2 — CHANGES REQUIRED (combined correction recheck)

2026-11-07. Exact digest `4d02d443e2c23026be2e6ccfbc59625bdb16ef9452d207f4b86c153f8106730a`, same base/HEAD97e5d60d and isolated branch. All six correction paths reviewed; unchanged R1 whole-phase coverage reused.

- **F1 FIXED:** genuine root/child later-source questions now retain correct prompt and original response identity.
- **F3 FIXED:** onboarding uses authorized revisioned clear before fixture teardown; all17 cases pass.
- **F2 partially fixed, medium OPEN:** fresh reader and direct-child behavior are correct, but an open root pane receives no matching refresh when a grandchild parks for review while both ancestors remain `child_run`. It can remain Processing until reload/reconnect.

[Combined evidence/dispositions and required nested live regression](agent-review-findings.md#chs-p06-b1-r2-dispositions--f1f3-fixed-f2-partially-fixed-open), [recheck notes](agent-review-notes.md#chs-p06-b1-r2--correction-recheck-changes-required), [entry](logs/CHS-P06-B1-R2-reviewer-entry.json), [exit](logs/CHS-P06-B1-R2-reviewer-exit.json).193distinct passing test names verified; no whole-phase acceptance while F2 remains open.


## CHS-P06-B1-R3 — PASS (exact whole-phase artifact)

2026-11-07. **PASS** for exact source digest `90f382b9e60fff25ef5f6f304a413eb395d95b8e447d5b944ebe7b9d0895073e`, base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`, isolated branch `codex/chs-p06-3f43t2k7`.

F2 is FIXED: actual nested formal-review opening/settlement now wakes the genuine root through the existing bounded helper. F1/F3 remain FIXED. No unresolved actionable findings. Three correction paths inspected; unchanged R1 whole-phase and R2 coverage explicitly reused for all74 source paths.

[Final dispositions](agent-review-findings.md#chs-p06-b1-r3-resolution--f2-fixed-all-b1-findings-closed), [coverage/evidence/limitations](agent-review-notes.md#chs-p06-b1-r3--nested-reconciliation-recheck-pass), [232-name evidence audit](logs/CHS-P06-B1-R3-reviewer-evidence-audit.json), [entry](logs/CHS-P06-B1-R3-reviewer-entry.json), [exit](logs/CHS-P06-B1-R3-reviewer-exit.json). Current13views and38formal-decision cases, fresh migrations, actual SQLx and offline all-target lint/format pass. No full-suite/live-provider claim; combined integration remains pending.
