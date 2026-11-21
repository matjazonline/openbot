# 07UR-P02 review record

## G1 decision revision r1 — round 07UR-P02-G1-R1 (2026-11-21)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD 107fd588 (= phase start = review base), digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1 (exclusions
  plan/07-unit-resources/phases/p02/docs, plan/07-unit-resources/COORDINATION.md); coordination
  revision 8, no notices.
- Outcome: **changes required** (no PASS for G1 r1).
- Notes: [07UR-P02-G1-R1](agent-review-notes.md#07ur-p02-g1-r1-2026-11-21).
- Actionable findings ([agent-review-findings.md](agent-review-findings.md)):
  - 07UR-P02-G1-F1 (high): D5's `ON DELETE SET NULL (unit_id)` fires the usage immutability
    guards, so deleting a department with usage fails (reproduced). Dropping a deleted unit's
    spend from every subtree sum lets a deletion lower an ancestor's or the root's spend.
  - 07UR-P02-G1-F2 (medium): D8 re-reads "admit at most what the cap allows" without a user
    decision, and its competing tests do not discriminate.
  - 07UR-P02-G1-F3 (medium): D7's read is linear in a period's usage, under the company lock and
    the claimed-admission timeouts. No bound, metric or plan evidence.
  - 07UR-P02-G1-F4 (medium): the spend definition can overflow `bigint`; rounding and
    current-price semantics are unstated; the page's 10,000-call sample cannot equal admission.
  - 07UR-P02-G1-F5 (medium): the D9 subtree predicate is inverted. The usage report and price
    writes share one authorization that must not widen. The filter must sit inside the bounded
    scan.
  - 07UR-P02-G1-F6 (medium): D1 leaves undefined the model kind's identity in uuid-based p01
    plumbing, the root-default and at-least-one invariants on moves and replaces, and the replace
    lock and unit-conditional upsert (no competing case).
  - 07UR-P02-G1-F7 (low): D2 starts a Deferred placement at the root; library copies lose their
    fallback under reach.
  - 07UR-P02-G1-F8 (low): contract-impact consumers are missing (agent settings selector,
    Builder directory models, prompt generation, inbound-event classification).
  - 07UR-P02-G1-F9 (low): the planned mapping lacks decisive cases (cap authority, nearest
    exhausted unit, history and reserved inputs, period rollover, JEV/child attribution, page
    figures).
- Optional: 07UR-P02-G1-F10 (`now()` period start; drill-down by current desks; stale HANDOFF).

## G1 decision revision r2 — round 07UR-P02-G1-R2 (2026-11-21, gate recheck)

- Reviewer B: Claude Opus 5.5. Artifact unchanged: HEAD 107fd588, digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1 (same exclusions).
  Coordination revision 9 (user decisions j, k), no notices.
- Outcome: **changes required** (no PASS for G1 r2).
- Notes: [07UR-P02-G1-R2](agent-review-notes.md#07ur-p02-g1-r2-2026-11-21).
- R1 findings 07UR-P02-G1-F1 … F10: resolved in decision r2 (dated entries in
  [agent-review-findings.md](agent-review-findings.md)).
- Actionable:
  - 07UR-P02-G1-F11 (medium): the settlement trigger and department delete lock in opposite
    orders through `unit_spend`'s cascading FK. The settlement fails with an FK violation or a
    deadlock (reproduced). Competing test (3) has no outcome, and a dangling unit's charge is
    undefined.
  - 07UR-P02-G1-F12 (low): every settlement locks the root's day/month totals until commit,
    which serializes the company's settlements. There is no metric, and the one-charge-per-
    transaction lock-order assumption is unstated.
- Optional: 07UR-P02-G1-F13: sum saturation; per-row subtree predicate cost; `unit_spend`
  retention; never-settled calls; no warning on a reach fallback; labeling ledger vs totals.

## G1 decision revision r3 — round 07UR-P02-G1-R3 (2026-11-21, gate recheck)

- Reviewer B: Claude Opus 5.5. Artifact unchanged: HEAD 107fd588, digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1 (same exclusions).
  Coordination revision 9, unchanged.
- Outcome: **changes required** (no PASS for G1 r3).
- Notes: [07UR-P02-G1-R3](agent-review-notes.md#07ur-p02-g1-r3-2026-11-21).
- 07UR-P02-G1-F11, F12, F13: resolved. The r3 protocol was executed in a throwaway database:
  deferred exactly-once charging at COMMIT, failure scope limited to that COMMIT, delete
  ordering 3a/3b without FK errors or deadlock, 16-way concurrency.
- Actionable:
  - 07UR-P02-G1-F14 (medium): chain-wide `FOR KEY SHARE` at COMMIT conflicts with the `FOR
    UPDATE` every department write takes. A root-department write, such as an agent placed in
    the root or a member change, stalls every settlement commit in the company past its ≤2 s
    bound (reproduced). Locking only the call's unit, or dropping the totals FK and the locks,
    would avoid it; otherwise accept it with a test.
  - 07UR-P02-G1-F15 (low): `LEAST(spent_micros + cost, …)` overflows in bigint before it
    saturates; compute the sum in numeric.

## G1 decision revision r4 — round 07UR-P02-G1-R4 (2026-11-21, gate recheck)

- Reviewer B: Claude Opus 5.5. Artifact unchanged: HEAD 107fd588, digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1 (same exclusions).
  Coordination revision 9, unchanged.
- Outcome: **changes required** (no PASS for G1 r4).
- Notes: [07UR-P02-G1-R4](agent-review-notes.md#07ur-p02-g1-r4-2026-11-21).
- 07UR-P02-G1-F14, F15: resolved. The r4 protocol was executed in a throwaway database: test 5
  shows no wait on department locks; the enumerated races 3a/3b/in-between behave as stated
  (orphan rows: one per period); 16-way concurrency is exact; the numeric sum saturates.
- Actionable: 07UR-P02-G1-F16 (low): the delete trigger's unordered `DELETE` can deadlock with a
  settlement when a second charge holds the unit's month row (reproduced with a sequential-scan
  plan). Fix: lock the unit's rows ordered by `(period, period_start)` before deleting. Also
  correct "never delay a settlement" (an in-flight delete delays its own unit's charge) and
  "one orphan row" (one per period).

## G1 decision revision r5 — round 07UR-P02-G1-R5 (2026-11-21, gate recheck)

- Reviewer B: Claude Opus 5.5. Artifact unchanged: HEAD 107fd588, digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1 (same exclusions).
  Coordination revision 9, unchanged.
- Outcome: **PASS for G1 decision revision r5.**
- Notes: [07UR-P02-G1-R5](agent-review-notes.md#07ur-p02-g1-r5-2026-11-21).
- 07UR-P02-G1-F16: resolved. The R4 heap-order three-way race was re-run against the r5 delete
  trigger in both arrival orders: no deadlock, all transactions complete.
- All actionable G1 findings (F1–F9, F11, F12, F14, F15, F16) are resolved. Optional F10 and F13
  have dispositions. Recorded limits are listed in the R5 note.

## B1 (02A) — round 07UR-P02-B1-R1 (2026-11-21, batch review)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD 107fd588 plus the working tree, digest
  bbfb5e1c2cbba8700411b000a30a30bdc40506c4cd22830826b721b4558c03b1 (same exclusions).
  Coordination revision 9. Author evidence manifest validated (4 PASS, 2 FAIL; the FAILs are the
  pre-revert suite and the inherited Doop deferral). B re-ran `unit_model_default` (4 passed).
- Outcome: **changes required** (no PASS for B1).
- Notes: [07UR-P02-B1-R1](agent-review-notes.md#07ur-p02-b1-r1-2026-11-21).
- Actionable:
  - 07UR-P02-B1-F1 (medium): planned `unit_model_default_*` cases are missing for changed paths:
    - admission-stage model reach;
    - move authority and move-into-root-default;
    - the root-admin widening and member 404;
    - a child's own default, and none anywhere;
    - the library reach fallback and the Builder reach refusal;
    - reach-filtered offers;
    - the department models route.
  - 07UR-P02-B1-F2 (low): D1 r2's company-wide "≥1 connection" rule is not enforced; the root's
    department page empties the root through the unit path.
  - 07UR-P02-B1-F3 (low): replace maps every lock/authority error to "company not found".
  - 07UR-P02-B1-F4 (low): the Builder directory and create form show no units (r2 contract).
    Prompt generation and the "(default)" marker take the first default by list position, so a
    department's default stands in when the root has none. Two warnings still say "company's
    default".
- Optional:
  - 07UR-P02-B1-F5: move uses read the draft provider, not the revision.
  - 07UR-P02-B1-F6: Builder reach uses the spec's department rather than the actual placement.

## B1 (02A) — round 07UR-P02-B1-R2 (2026-11-21, correction recheck)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD 107fd588 plus the working tree, digest
  5a938fa0571d0863f5c823653901ca94f59b6cb7effd63222ca4ebd317494359 (same exclusions).
  Coordination revision 9.
- Outcome: **PASS for B1** at this digest.
- Notes: [07UR-P02-B1-R2](agent-review-notes.md#07ur-p02-b1-r2-2026-11-21).
- 07UR-P02-B1-F1 … F4: fixed (six new decisive tests; ≥1-connection rule; error mapping; unit
  labels, root-only default, directory `department`, warning texts). F5 and F6: dismissed
  (optional, not adopted). B re-ran 14 affected tests: all passed.
- Optional, non-blocking: 07UR-P02-B1-F7 (no discriminating test for the label, the marker, and
  `prompt_model` with no root default).

## B2 (02B) — round 07UR-P02-B2-R1 (2026-11-21, batch review)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD 55f5f633 (accepted B1) plus the working tree,
  digest 80557b37f0f8cfabff6563c651842dff085ae5685de3c3882580ffba0e83cd8b (same exclusions).
  Coordination revision 9. Author manifest validated (4 PASS, 1 FAIL: the pre-fix full suite plus
  the inherited Doop deferral). B re-ran `unit_budget`: 7 passed.
- Outcome: **changes required** (no PASS for B2).
- Notes: [07UR-P02-B2-R1](agent-review-notes.md#07ur-p02-b2-r1-2026-11-21).
- Actionable: 07UR-P02-B2-F1 (medium), verification only:
  - no test charges through the production settle sites (agent checkpoint settlement inside its
    WriteBound transaction, JEV), as the G1 pass deferred to the batches;
  - the three-way test does not reproduce the planned (6) shape, so it passes even with an
    unordered delete;
  - no re-parent case and no child/JEV attribution case;
  - the desk-less attribution is never asserted.
- Optional: 07UR-P02-B2-F2: message production refusal, the HTTP refusal body and the cap routes
  are untested.
- No code defect found in the delta.

## B2 (02B) — round 07UR-P02-B2-R2 (2026-11-21, correction recheck)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD 55f5f633 plus the working tree, digest
  60e2c15f4935ce5371b485a604069eb4ada4f6add0ec3358466e97af4ea11929 (same exclusions); tests
  only since R1. Coordination revision 9.
- Outcome: **PASS for B2** at this digest.
- Notes: [07UR-P02-B2-R2](agent-review-notes.md#07ur-p02-b2-r2-2026-11-21).
- 07UR-P02-B2-F1: fixed. Covered now: production agent settlement on a desk, JEV settlement,
  lock order in the planned (6) shape (with the author's negative check), re-parent, child
  attribution, company-wide attribution. B re-ran 11 `unit_budget_*` tests: all passed.
- 07UR-P02-B2-F2 (optional): deferred to p03, recorded.
