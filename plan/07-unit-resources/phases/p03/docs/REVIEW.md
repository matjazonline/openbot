# 07UR-P03 review record

## B1 (03A) — round 07UR-P03-B1-R1 (2026-11-21, batch review)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD 47c8074c plus the working tree, digest
  d30f98a13277aab7980f5bd3bddc8ac2f666becca082adc8f21182970dd2e62e (exclusions
  plan/07-unit-resources/phases/p03/docs, plan/07-unit-resources/COORDINATION.md). Coordination
  revision 9. Author manifest validated (2 PASS). B re-ran the three carried tests: all passed.
- Outcome: **changes required** (no PASS for B1).
- Notes: [07UR-P03-B1-R1](agent-review-notes.md#07ur-p03-b1-r1-2026-11-21).
- Actionable ([agent-review-findings.md](agent-review-findings.md)):
  - 07UR-P03-B1-F1 (low): `NO_BACKWARD_COMPATIBILITY_NOTES` already listed the plan/07 paths at
    HEAD; B1 lists them a second time.
  - 07UR-P03-B1-F2 (low): the docs say an agent's model is checked at native selection (only
    tools and skills are) and that authoring offers only reachable models (a new agent's form
    lists every model, labelled).
- Optional: 07UR-P03-B1-F3 (move routes list, library fallback wording, root-budget parking,
  delete message, unmentioned decision f member requirement, messaging/JEV, caps deleted with the
  department, the ≥1-connection rule).

## B1 (03A) — round 07UR-P03-B1-R2 (2026-11-21, correction recheck)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD 47c8074c plus the working tree, digest
  1cbaafd74a01ad78dc18d66bfea5864cb033e0833ff01de1673146ac06f9c66a (same exclusions).
  Coordination revision 9. Author manifest validated (1 PASS).
- Outcome: **changes required** (no PASS for B1).
- Notes: [07UR-P03-B1-R2](agent-review-notes.md#07ur-p03-b1-r2-2026-11-21).
- 07UR-P03-B1-F1: fixed.
- 07UR-P03-B1-F2 (low): still open. docs/custom_tools.md:16-19 still says authoring offers no
  out-of-reach model connections, but a new agent's form lists every model; departments.md was
  corrected.
- 07UR-P03-B1-F3 (optional): mostly adopted; a new optional nit is that the delete refusal does
  not name the desks.

## B1 (03A) — round 07UR-P03-B1-R3 (2026-11-21, correction recheck)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD 47c8074c plus the working tree, digest
  43532e1f004ff6d7e87b3e69fe0619a1455eaec4c612d4e876886151a027721d (same exclusions).
  Coordination revision 9. Docs only since R2.
- Outcome: **PASS for p03 B1** at this digest.
- Notes: [07UR-P03-B1-R3](agent-review-notes.md#07ur-p03-b1-r3-2026-11-21).
- 07UR-P03-B1-F2: fixed (the custom_tools.md offers clause matches the code).
  07UR-P03-B1-F3 (optional): closed (desk-refusal wording adopted).
