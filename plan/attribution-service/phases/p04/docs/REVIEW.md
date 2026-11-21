# Attribution p04 independent review

## AS-P04-G1 / AS-P04-G1-N1 — 2026-11-16

**PASS for architecture decision AS-P04-D1/r1**, SHA256
`ab5267ed4b857447ccae0fdf1a6b83dffa0fce2ffd646dd44790cdea01b89d85`.
No actionable architecture findings.

- Decision: [immutable r1](logs/AS-P04-D1-r1.md).
- Phase start/review-base/current HEAD:
  `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
- Source remains accepted p03 artifact
  `fd40f4807955c2e19c00ab9d864c2a0ce84566088e99743b84f9338a7ce7d30c`;
  [start snapshot](logs/AS-P04-G1-reviewer-start.snapshot.txt),
  [end snapshot](logs/AS-P04-G1-reviewer-end.snapshot.txt).
- Exact exclusions: `plan/attribution-service/phases/p01/docs`,
  `plan/attribution-service/phases/p02/docs`,
  `plan/attribution-service/phases/p03/docs`,
  `plan/attribution-service/phases/p04/docs`,
  `plan/attribution-service/COORDINATION.md`.
- Authoritative coordination: plan-root COORDINATION.md, writer `/root`, revision5;
  AS-C001/r1 and AS-C002/r1. Pinned [copy](logs/AS-P04-D1-coordination-r5.md)
  matches the authority byte-for-byte; [identity check](logs/AS-P04-G1-reviewer-identity.txt).
- Reviewer `/root/attribution_reviewer`, Astra/high requested, independent reviewer
  retained across phases; actual model/effort/context telemetry unavailable.
  Author `/root`, user-confirmed Sol6.1/xhigh.
- Requirements, challenged invariants, planned checks and limitations:
  [AS-P04-G1-N1](agent-review-notes.md).

This pass permits implementation of the reviewed decision under the original
phase and wire contracts. It is not implementation, phase, runtime, full-plan,
external-DNS or deployment acceptance. Root owns later acceptance/integration.

## AS-P04-B1 / AS-P04-B1-N1 — 2026-11-16

**Changes required** for frozen implementation
`6837fba3f8f66fb0160bf308ecc0668d25f33be5a70ad3b65a2895fbedea0c0f`.

Two P2/medium verification findings, recorded together in
[agent-review-findings.md](agent-review-findings.md):

- AS-P04-B1-F1: exercise the gated competing store-disable/use case; current store
  disable coverage is sequential.
- AS-P04-B1-F2: replay after the original submitted expiry passes; the current test
  changes only persisted expiry while the submitted expiry remains in the future.

No unsafe production behavior was demonstrated; inspection finds the intended
tenant locking and receipt-before-expiry ordering. The missing discriminating
regressions prevent the promised verification claims from being accepted.

Independently executed16 setup tests and the real browser journey (one test/eight
checks), all PASS. Validated the author's11-entry manifest and inspected its current
identity/foundation/format/offline/migration/metadata/boundary/contract evidence.
Detailed coverage and limitations: [AS-P04-B1-N1](agent-review-notes.md).

Base/HEAD and exact five exclusions remain as in G1. Start/end snapshots and
identity/coordination checks are retained under `logs/AS-P04-B1-reviewer-*`.
Coordination revision6, AS-C001/r1 and AS-C002/r1 unchanged; AS-C003/r1 applies to
future phases after accepted p04. Pinned revision6 matched the authority.

No p04 implementation acceptance. Recheck the two corrections together, reusing
unaffected evidence; no architecture revision is needed unless the protocol changes.
Root owns phase acceptance and later full-plan/container/deployment dispositions.

## AS-P04-B1-R2 / AS-P04-B1-N2 — 2026-11-16

**PASS** for corrected implementation
`a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275`.
AS-P04-B1-F1 and F2 are fixed; no remaining actionable findings.

The appended tests now establish the actual competing store-disable/use outcomes
and exact replay after the original payload expiry. Independently reran all18
setup tests: PASS. Reused unaffected B1 production/browser/identity/foundation
evidence; no production or architecture change. Current13-entry manifest validates.
Details and limitations: [AS-P04-B1-N2](agent-review-notes.md), with dated resolutions
under the original [finding IDs](agent-review-findings.md).

Source start/end identity, original decision hash and pinned coordination revision6
were rechecked; base/HEAD and exact five exclusions remain unchanged. Root owns
phase acceptance/integration. This does not establish full-plan, current container,
external DNS, production deployment or future commerce-route completion.
