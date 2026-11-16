# Attribution p03 independent review

## AS-P03-G1 / AS-P03-G1-N1 — 2026-11-16

**Changes required** for architecture decision **AS-P03-D1/r1**.
One actionable finding: [AS-P03-G1-F1](agent-review-findings.md), medium/P2,
company-status freshness can outlive the approved five-minute bound after a
backward database-clock adjustment. No dependent implementation is accepted.

- Decision: [immutable r1](logs/AS-P03-D1-r1.md), SHA256
  `d139936c19eb01c044b924d7a3971b41777f93f87b68c82c99bbba67cf7c8b0f`.
- Starting commit/review base/HEAD:
  `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
- Source snapshot remained
  `f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`;
  [end check](logs/AS-P03-G1-reviewer-end.snapshot.txt).
- Exact exclusions: `plan/attribution-service/phases/p01/docs`,
  `plan/attribution-service/phases/p02/docs`,
  `plan/attribution-service/phases/p03/docs`,
  `plan/attribution-service/COORDINATION.md`.
- Authoritative coordination: plan-root COORDINATION.md, writer `/root`, revision4;
  AS-C001/r1 and AS-C002/r1. Pinned [copy](logs/AS-P03-G1.coordination.md)
  matched the authority byte-for-byte at review end.
- Reviewer `/root/attribution_reviewer`, Astra/high requested, independently
  initialized and retained across phases. Actual model/effort/context telemetry
  unavailable. Author `/root`, user-confirmed Sol6.1/xhigh.
- Scope/evidence and limitations: [AS-P03-G1-N1](agent-review-notes.md).

Recheck needs a new immutable decision revision resolving F1 and its verification
mapping. Reuse the unaffected architecture evidence; retain the original decision
and finding. This is a pre-implementation gate, not runtime or phase acceptance.

## AS-P03-G1 recheck / AS-P03-G1-N2 — 2026-11-16

**PASS for architecture decision AS-P03-D1/r2.** AS-P03-G1-F1 is resolved at the
decision level; no remaining acceptance-blocking architecture findings.

- [Immutable r2](logs/AS-P03-D1-r2.md), SHA256
  `6968a1aa538ec5817939a92198ff2f111da8a581ac7bd17a1d2529645430b722`.
- Source digest, starting/review-base/HEAD and exact exclusions remain as in N1;
  [rechecked source](logs/AS-P03-G1-r2-reviewer.snapshot.txt).
- Coordination revision4 and notices AS-C001/r1, AS-C002/r1 remain unchanged;
  pinned G1 copy matched the authority byte-for-byte.
- Correction scope: process-local monotonic freshness tickets, durable observation
  ordering, cold/restart/other-process behavior, locked deadline checks and the
  deterministic test mapping. Unchanged architecture evidence from N1 reused.
- [AS-P03-G1-N2](agent-review-notes.md) records coverage; F1's dated resolution
  is in [findings](agent-review-findings.md).

This pass permits implementation of the reviewed decision. It does not establish
runtime behavior, pass the p03 implementation batch, or remove the recorded root
baseline compilation limitation.

## AS-P03-B1-R1 / AS-P03-B1-N1 — 2026-11-16

**Changes required** for frozen implementation
`45fc0ad49012ab1f6f58a619c06508430011b0c3a127683196c79ea906c71897`.

All five actionable findings are P2/medium, recorded together in
[agent-review-findings.md](agent-review-findings.md):

| ID | Required correction |
| --- | --- |
| AS-P03-B1-F1 | Idempotent logout retry/cookie clearing and published success response |
| AS-P03-B1-F2 | Usable HTML sign-in recovery after local expiry/upstream revocation |
| AS-P03-B1-F3 | Authority response budget accepts valid maximum company pages |
| AS-P03-B1-F4 | Operational signal reads durable undelivered/failed/expired state |
| AS-P03-B1-F5 | Real bridge membership downgrade/removal, foreign-company and multi-company tests |

Source start and end snapshots retained in logs under AS-P03-B1-R1-reviewer-*;
phase start/review-base/current HEAD and exact exclusions remain those recorded
above. Coordination revision4, AS-C001/r1 and AS-C002/r1 matched the pinned
AS-P03-B1-R1.coordination.md. Unchanged p01/p02 acceptance is reused.

Independent execution: 33 service tests, eight root identity/lifecycle tests and
ten boundary-checker tests PASS; reviewer runtime probes confirmed F1–F3. The
author's evidence manifest validates with 15 PASS and five retained superseded FAIL
entries; it does not cover the required F4/F5 behavior. Details, limitations and
logs: [AS-P03-B1-N1](agent-review-notes.md).

No p03 implementation acceptance. Resolve all findings before one correction
recheck, preserving this round and its immutable evidence. The authorized unrelated
one-line helper correction is included; its previous baseline compile failure is
superseded by retained successful compilation/metadata and focused checks.

## AS-P03-B1-R2 / AS-P03-B1-N2 — 2026-11-16

**PASS** for corrected implementation
`fd40f4807955c2e19c00ab9d864c2a0ce84566088e99743b84f9338a7ce7d30c`.
AS-P03-B1-F1 through F5 are fixed; no remaining actionable findings.

- Rechecked each correction and affected contracts/consumers, reusing unaffected
  R1 and architecture evidence. Resolution details are appended under the original
  finding IDs; [AS-P03-B1-N2](agent-review-notes.md) records scope and limitations.
- Independent execution:21 service identity tests and11 root identity/lifecycle
  tests PASS. Standalone runtime probes now accept the formerly rejected maximum
  company page, complete logout replay, and recover the expired-cookie HTML shell.
- Inspected the current real-browser proof (eight checks), operator signal tests,
  maximum-page80% early guard plus hard rejection, and real producer role/removal/
  foreign-company/pagination cases. Author manifest validates17 PASS plus one
  superseded formatting FAIL; current formatting/offline/metadata gates pass.
- Exact exclusions/base/HEAD unchanged. Source and coordination revision4 with
  AS-C001/r1 and AS-C002/r1 rechecked; end snapshot/identity logs retained.

This is independent batch review approval of the stated artifact. Root still owns
phase acceptance/integration. No full-plan, current-container or production
deployment acceptance is implied; later-phase verification remains outstanding.
