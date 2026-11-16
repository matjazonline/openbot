# P09 independent findings

## AS-P09-B1-F1 — current topology denial assertions lack matching evidence

- Date: 2026-11-16. Workflow implement-astra-reviewed-2-3; phase p09,
  batch AS-P09-B1-R1, whole-batch implementation review; note AS-P09-B1-N1.
  Reviewer /root/attribution_reviewer, requested Astra/high; actual telemetry unavailable.
- Artifact: `868a58789f2d34aa43c62256bd6a480c8096613ba7f2dd70b496b6d2561d2339`;
  HEAD/start/base `6e55258cbbdc7633940805f366fb17fe240f02aa`.
  Assigned isolated worktree and records are those in [TASK](TASK.md).
- Classification: actionable verification defect, P2. Initial status OPEN.
- Evidence: `scripts/attribution/topology.py:204`–213 currently exercises HTTP403
  for non-bridge HTTPS routes from both application namespaces and prints the
  corresponding success text. [Retained topology11](logs/AS-P09-topologies-11.txt)
  prints only the earlier cross-application plaintext-port result. Root confirmed
  during review that no retained run proves the newly added gateway assertions.
  The R1 evidence manifest validates file retention but cannot make this stale
  proof establish original09B.3 / AC6's allowlisted boundary.
- Required correction: run the current frozen topology fixture through both layouts,
  retain actual output and an immutable supplemental evidence manifest, and preserve
  failed attempts. No source correction is requested if those assertions pass.
- Instruction lesson: verification-contracts already requires evidence from actual
  current assertions. Re-freeze a fixture only with a matching execution record;
  machine hash validation alone does not detect a stale behavioral result.
- Resolution history: author is rerunning the unchanged source as topology13;
  topology12 failed while task-owned Docker infrastructure was unavailable. No
  correction is considered proven until the current run and evidence are inspected.

- 2026-11-16 follow-up: [topology13](logs/AS-P09-topologies-13.txt) failed in
  the first layout before reaching the separate-network allowlist assertions.
  F1 remains OPEN; topology11 is historical proof of its earlier fixture only.

## AS-P09-B1-F2 — ordered commerce fixture timestamps use different clocks

- Date/workflow/role/worktree/artifact: same AS-P09-B1-R1 identity as F1;
  note AS-P09-B1-N1. Classification: actionable correctness/verification defect,
  P2. Initial status OPEN.
- Evidence: `services/attribution/browser/topology.mjs:51` creates checkout
  `order_created_at` with host `new Date()` immediately after the production redirect
  stores its click using PostgreSQL time. Lines53–58 also create paid/refund times
  on the host. `services/attribution/src/application/website.rs:169` requires
  `clicked_at <= order_created_at`, so even a small positive VM/database clock offset
  makes this supposed attributed fixture return no click. That is correct production
  behavior but a nondeterministic integration fixture.
  [Topology13](logs/AS-P09-topologies-13.txt) reached a successful302 redirect and200
  checkout, then failed the fixture assertion before any conversion. Root reports
  measuring the VM approximately235ms ahead of the host after restart; reviewer
  independently verified the conflicting clocks and production predicate, not that
  numerical clock-offset measurement.
- Impact: the new deployment-boundaries CI gate can fail due to fixture clock offset,
  and AC6/AC7 cannot currently be established for the frozen fixture. Do not loosen
  production eligibility or remove the attributed-ledger assertion to obtain a pass.
- Required correction: derive the ordered fixture commerce timestamps from a
  consistent clock after the relevant preceding effects, preserving payment/refund
  chronology; exercise both current layouts and retain exact-artifact evidence.
- Instruction lesson: integration fixtures spanning a VM and host must establish
  timestamp ordering explicitly when production compares their clocks. Original
  verification-contracts requires a discriminating actual journey, not lucky timing.
- Resolution history: author proposed a fixture-control database clock; no edit or
  passing correction run reviewed in R1. F2 remains OPEN.


## AS-P09-B1-R2 resolution — 2026-11-16

Rechecked artifact `542b32ae822314eb5a16050b153fa4e7a625cfc7380d2b66a9298d79fbdb5d9d`,
note AS-P09-B1-N2; same reviewer/role/worktree/HEAD and coordination26.

- **AS-P09-B1-F1 CLOSED.** [Topology14](logs/AS-P09-topologies-14.txt) exercises both
  layouts using identical base application images and now includes the exact
  current non-bridge HTTPS403 checks from both application network namespaces.
  Actual complete HTTPS SSO/setup/payment/refund/report/outage/recovery assertions
  follow the boundary checks and pass in both layouts. R2 manifest independently
  validated (1 PASS,2 retained FAIL), retaining topology12/13. The earlier R1
  topology11 proof is not relabeled as proof of these later assertions.
- **AS-P09-B1-F2 CLOSED.** Fixture-only `commerce-clock` action reads PostgreSQL UTC
  clock_timestamp with microsecond precision. Browser awaits it after the redirect,
  checkout and paid effect to populate each subsequent creation/payment/refund time.
  Control remains loopback/token protected, and production eligibility is unchanged.
  Existing control calls continue to await the200 JSON response. No assertion was
  weakened: selected click, attributed paid count and exact API/dashboard ledger
  remain required and pass in topology14. Final source audit accounts for only the
  two fixture changes and one new Trail learning; all other R1 coverage is reused.

No open p09 findings remain. See [R2 identity](logs/AS-P09-B1-R2-reviewer-identity.txt)
and [R2 inventory](logs/AS-P09-B1-R2-reviewer-inventory.txt).
