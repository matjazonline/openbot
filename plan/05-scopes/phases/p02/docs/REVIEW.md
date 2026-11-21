# Plan05 p02 independent review

## D2 revision 1 — CHANGES REQUIRED (2026-11-17)

Independent reviewer B `/root/p05_reviewer`, requested Astra/high (additional actual
setting telemetry unavailable), workflow implement-astra-reviewed-2-3.

Reviewed [immutable D2/r1](logs/D2-R1-TASK.md) and
[coordination revision 7](logs/D2-R1-coordination.md), original Plan05 README,
02-scope-resolution-and-tools and downstream 03-knowledge-scopes, against current
producer/consumer and memory-service contracts. Review base/HEAD
`e6eb5bc8fff93492f23935b9c84114905801824c`; source digest
`cc06127eac2c2713ece05d3c97376c9c33a62e128a5e390c6c7feb573f9eb484`.

Two architecture corrections are required before dependent edits:

- [05SC-P02-D2-R1-F1](agent-review-findings.md#05sc-p02-d2-r1-f1): distinguish fresh
  owner validation from exact inherited snapshot copies after owner changes/deletion.
- [05SC-P02-D2-R1-F2](agent-review-findings.md#05sc-p02-d2-r1-f2): define native
  workflow-origin snapshot inheritance separately from fresh conversation admission.

No p02 implementation or tests have been exercised. This is a design gate, not an
implementation/security-test pass. Other D2 contracts have no actionable finding at
this gate, subject to their declared real production/service proof obligations.
See [round note](agent-review-notes.md#05sc-p02-d2-r1-n1) for coverage and limits.
Source snapshot reproduced exactly and current coordination matched retained input.
C001–C004/r1 acknowledged; C005/r1 remains proposed pending a corrected gate PASS.

## D2 revision 2 — PASS AT DECISION LEVEL (2026-11-17)

[Retained D2/r2](logs/D2-R2-TASK.md) supersedes the specified r1 paragraphs;
remaining r1 decisions remain in force. Independent B rechecked the correction
delta and seal protocol at the same base/HEAD and source digest above, against
[coordination revision8](logs/D2-R2-coordination.md). Current coordination matched
this retained input and the snapshot digest reproduced exactly.

Both F1 and F2 are **resolved at decision level**; no further actionable findings.
Fresh live-owner validation and canonical exact historical inheritance are
separate seal rules. Workflow-native scopes inherit only the correct source
projection; unavailable source context produces an immutable zero-authority state.
This may preserve independently authorized MCP-only execution and does not allow
memory/knowledge/provisioning fallback. The common run mutex plus NOWAIT prefix
is coherent at design level and has explicit competing-claimant proof obligations.

C001–C004/r1 remain acknowledged; C005/r2 is suitable for activation by its owner.
See [R2-N1](agent-review-notes.md#05sc-p02-d2-r2-n1) and findings dispositions.
This PASS authorizes implementation of the revised design; it does not establish
AC1–AC5 implementation acceptance, C004 real service proof, or full verification.
No p02 tests, migrations, service calls, source edits or Doop work were performed.

## B1 revision 1 — CHANGES REQUIRED (2026-11-17)

Independent whole-p02 implementation review `05SC-P02-B1-R1`, note
[05SC-P02-B1-R1-N1](agent-review-notes.md#05sc-p02-b1-r1-n1).
Base/HEAD `e6eb5bc8fff93492f23935b9c84114905801824c`; all 92 changed paths at digest
`4371836f22d139758bc0e6c2de4e4cf124a12f34c3016e251cf989b1acc7321f`.
Exact exclusions remain p02/docs and the exact plan COORDINATION.md file.
Inputs: [TASK](logs/B1-R1-TASK.md), [coordination revision9](logs/B1-R1-coordination.md),
[evidence](logs/B1-R1.evidence.json), [test accounting](logs/B1-R1-test-accounting.json).
Original README/02 requirements and D2/r2 govern; 03 knowledge integration is pending.

Actionable findings (all **medium**, no optional suggestions):

- [05SC-P02-B1-R1-F1](agent-review-findings.md#05sc-p02-b1-r1-f1): admission misses
  `own` requirements in workflows reached through agent workflow-tool bindings.
- [05SC-P02-B1-R1-F2](agent-review-findings.md#05sc-p02-b1-r1-f2): promised fresh-seal
  rejection and competing admission/topology assertions are not exercised.
- [05SC-P02-B1-R1-F3](agent-review-findings.md#05sc-p02-b1-r1-f3): new unit/reports
  tool selectors and local tool-save targeting lack production consumer proof.

Independent execution: [17 root focused tests passed](logs/B1-R1-reviewer-focused.txt)
and [3 real-service subject tests passed](logs/B1-R1-reviewer-service-subject.txt).
Manifest retention validates (15 PASS, 4 retained FAIL, 1 NOT EXERCISED); its success
is not behavioral acceptance. Runner accounting independently matches 4768→4799,
with one explicitly replaced thread-capture name and 32 additions. The six unchanged
names from the seven-test failed memory attempt pass in current evidence; the seventh
passes under its accounted unit-capture replacement. Source and coordination were
reproduced at review start and return; [final snapshot](logs/B1-R1-reviewer-final-snapshot.txt).
No implementation changed; no Doop, full repository gate, or phase03 work executed.


## B1 revision 2 — PASS (2026-11-17)

Independent correction review `05SC-P02-B1-R2`, note
[05SC-P02-B1-R2-N1](agent-review-notes.md#05sc-p02-b1-r2-n1), passes source digest
`7a555e95283fa22b7face97a1463d8efdfb6cf8c2ab92a3b77e5eae734ab51a5`.
Start/review-base/HEAD remains `e6eb5bc8fff93492f23935b9c84114905801824c`;
102 changed paths; exact exclusions `plan/05-scopes/phases/p02/docs` and
`plan/05-scopes/COORDINATION.md`. The original whole-batch R1 review remains the
basis for unchanged source; this round reviews the correction delta and affected
production paths for all three findings together. D2/r2 remains unchanged.

**F1, F2 and F3 are resolved.** The bounded dependency closure now checks local-unit
requirements along agent workflow-tool edges before root admission. New fresh-seal
and deferred-commit negatives reach their intended SQL guards; real admission and
reparenting compete in both orders with exact scope assertions. Actual desk/lead
memory tools prove unit/reports filtering, overlapping namespace deduplication,
labels/references and strict save targets. Missing native context explicitly refuses
memory eligibility without namespace provisioning while MCP authorization succeeds.

Independent execution: [11 focused tests PASS](logs/B1-R2-reviewer-focused.txt),
including all ten correction cases and the unchanged Company own-session case.
Retained current author format, SQLx/all-target compilation, clippy and source-module
checks pass; manifest validation yields 15 PASS, 8 retained FAIL, 1 NOT EXERCISED.
The three earlier new-test failures all have exact current passing replacements.
[Runner accounting](logs/B1-R2-reviewer-test-accounting.txt) independently confirms
4768 baseline / 4799 R1 / 4808 R2 names, nine R2 additions and no R2 removals.
Unchanged service, client, CLI and schema evidence is reused from R1; the new tool
fixtures prove host policy through ContractFake and do not add a real-service claim.

Current coordination exactly matches [pinned revision9](logs/B1-R2-coordination.md),
C001–C004/r1 and C005/r2. [Final snapshot](logs/B1-R2-reviewer-final-snapshot.txt)
exactly matches the start snapshot. No new actionable or optional findings remain.
This is p02 source-review PASS, subject to root acceptance; the required full source
verification and knowledge integration remain p03 before publication. Doop remains
user-deferred. No full-repository or whole-CI-green claim is made.
