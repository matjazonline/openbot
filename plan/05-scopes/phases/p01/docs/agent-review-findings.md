# Plan05 p01 review findings

All entries below concern architecture D1 revision 1 under
implement-astra-reviewed-2-3, reviewed 2026-11-17 by independent B
`/root/p05_reviewer` (requested Astra/high; additional actual-setting telemetry
unavailable), phase p01/B1, worktree `/Users/mac02/orca/workspaces/openbot/plan03-04`.
Review base/HEAD d748c2ff64309903449a3d860377a5e0385377a4; source digest
3774130c5113a407f3af1c3c37f8c288742f2365b02c1d5a741a34901719f33d.
Decision: [retained D1/r1](logs/05SC-P01-D1-R1-TASK.md).
Round: [05SC-P01-D1-R1-N1](agent-review-notes.md#05sc-p01-d1-r1-n1).

## 05SC-P01-D1-R1-F1

- Classification: actionable, **high**, architecture/concurrency; originated in the
  proposed decision, no implementation has been written.
- Evidence: D1 item 4 says creation paths already hold the owner-lock prefix and
  call the same ensure helper. `src/adapters/persistence/participant.rs:449–472`
  first INSERTs/UPSERTs the principal, taking its row lock; it does not first lock
  company and agent. `src/adapters/persistence/thread/message.rs:311–352` calls that
  function when producing a message for an existing agent, not only at creation.
  `src/adapters/persistence/agent.rs:523–595` deletes in company → agent → principal
  order. Current `memory/namespaces.rs:45–105` obtains company/owner locks before
  provisioning. Appending that ensure operation after the principal upsert reverses
  these orders: writer holds principal and waits for company/agent; delete holds
  company/agent and waits for principal. Person restoration similarly already takes
  principal locks (`participant/person.rs:84–161`) and requires its documented
  account/company prefix to be held before it reaches that point.
- Required correction: explicitly require the agreed prefix before the first owner
  INSERT/UPSERT or existing-principal lock, and disposition all production callers
  (including message-author upsert and person restoration). Preserve the account
  before company ordering for persons. Map a competing actual principal-upsert versus
  retirement test to this change, not merely competing lazy namespace ensures; cover
  both commit orders with no deadlock/stale live namespace. A owns the implementation
  choice and the complete lock protocol.
- Instruction lesson: shared helper call edges are a lock-order contract. The existing
  skill's actual-caller tracing rule applies; no new instruction is required.
- Resolution history: **OPEN** at D1/r1. Sent to A before dependent edits.
- 2026-11-17, D1/r2 recheck **RESOLVED AT DECISION LEVEL**, source digest unchanged.
  [R2-N1](agent-review-notes.md#05sc-p01-d1-r2-n1) verified explicit prefix acquisition
  before principal writes for agent and person helpers, exact owner revalidation and
  production existing-principal upsert-versus-delete competition in both orders.
  Implementation/test fulfillment remains for B1 review.

## 05SC-P01-D1-R1-F2

- Classification: actionable, **medium**, architecture/verification; unsupported
  original-plan assumption carried into D1 item 6.
- Evidence: D1 says subject propagates through the service's existing opaque
  provenance metadata. `src/application/memory/ports.rs:199–215` and
  `services/memory/src/application/contract.rs:67–97` have no provenance/subject
  field; both deserialize with `deny_unknown_fields`. Save freezing
  `src/adapters/persistence/memory/deliveries.rs:222–240` cannot carry it either.
  Therefore local delivery columns alone cannot fulfill original 01A2/02B3 subject
  persistence and purge requirements, and an unchanged service contract is not proof.
- Required correction: revise D1 to limit p01 to retained nullable local provenance
  columns, explicitly mark actual remote subject propagation/purge as an unresolved
  p02 contract dependency, and require an independently reviewed contract decision and
  real cross-service proof before its implementation/acceptance. The service may
  remain department-agnostic while its opaque metadata contract is extended. Correct
  AC4/final verification scope if that extension changes the service or fake.
- Proposed disposition from A (message, not yet accepted decision): p01 columns only;
  narrow opaque-subject extension and purge integration will be designed/reviewed in
  p02 before service edits. This is reasonable once retained in the authoritative
  decision and cross-phase records; it is not evidence that the feature exists.
- Instruction lesson: inspect wire request types before assuming a plan's claimed
  metadata field exists. Existing verification-contract instructions already cover it.
- Resolution history: **OPEN** at D1/r1 pending corrected decision/obligation mapping.
- 2026-11-17, D1/r2 recheck **RESOLVED AT DECISION LEVEL**, source digest unchanged.
  [R2-N1](agent-review-notes.md#05sc-p01-d1-r2-n1) verified p01 columns-only boundary
  and explicit p02 service-contract gate/proof obligation, durably propagated by
  coordination revision 3 / 05SC-C004/r1. This resolves the false architecture
  assumption; remote subject/purge behavior remains an unimplemented p02 criterion.

## 05SC-P01-B1-R1 disposition — 2026-11-17

Whole-batch implementation review at digest
0713361f3809b86cfbde192b30267041ea50f862c3183afcb4fe38896890dabf:
**no actionable defects or optional suggestions recorded**. See
[05SC-P01-B1-R1-N1](agent-review-notes.md#05sc-p01-b1-r1-n1) and REVIEW.

- D1-R1-F1: implementation and independent nine-test focused execution confirm the
  accepted prefix-before-upsert correction and retained deferred-FK proof. Closed.
- D1-R1-F2: p01 does not claim remote subject support; coordination4/C004r1 preserves
  the p02 contract gate and proof obligation. Decision defect remains closed; future
  feature criteria remain pending in their owning phase.
