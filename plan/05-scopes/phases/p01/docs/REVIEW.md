# Plan05 p01 independent review

## 05SC-P01-D1-R1 — 2026-11-17

Architecture gate D1 revision 1: **CHANGES REQUIRED**. This is a decision review,
not implementation acceptance. See [notes](agent-review-notes.md#05sc-p01-d1-r1-n1)
and findings **05SC-P01-D1-R1-F1** (high, lock ordering) and
**05SC-P01-D1-R1-F2** (medium, unsupported service contract assumption).

Reviewer B `/root/p05_reviewer`, requested Astra/high, independent context;
host does not expose additional actual-settings/context telemetry. Workflow
`implement-astra-reviewed-2-3`; worktree
`/Users/mac02/orca/workspaces/openbot/plan03-04`, branch `feature/plan05-scopes`.
Start/review base/HEAD `d748c2ff64309903449a3d860377a5e0385377a4`.
Source digest `3774130c5113a407f3af1c3c37f8c288742f2365b02c1d5a741a34901719f33d`;
exact exclusions `plan/05-scopes/phases/p01/docs` and
`plan/05-scopes/COORDINATION.md`. Independently recomputed at review end; no source diff.

Decision retained in [D1/r1 TASK](logs/05SC-P01-D1-R1-TASK.md).
Authoritative coordination `/Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md`
revision 1, notices 05SC-C001/r1 and 05SC-C002/r1, inherited contracts acknowledged.
Pinned [coordination](logs/D1-R1-coordination.md) compares byte-for-byte with current
authority at review end. No tests or Doop work executed by B.

Required next step: A revises D1 and its proof/impact mapping to resolve both findings;
B rechecks the decision delta before dependent implementation.

## 05SC-P01-D1-R2 — 2026-11-17

Architecture gate **PASS for D1 revision 2** at unchanged source digest
`3774130c5113a407f3af1c3c37f8c288742f2365b02c1d5a741a34901719f33d`, unchanged
start/base/HEAD and exclusions above. Findings F1/F2 from R1 are resolved at the
decision level; implementation and promised behavioral checks remain unverified.
See [05SC-P01-D1-R2-N1](agent-review-notes.md#05sc-p01-d1-r2-n1).

Reviewed [retained D1/r2 TASK](logs/05SC-P01-D1-R2-TASK.md): locks precede actual
principal upserts, production upsert-versus-delete races enter AC2, and p01 only
adds local provenance columns. Actual remote subject propagation/query/purge is a
p02 dependency requiring a separate architecture gate and affected service tests.

Coordination revision 3 and C001/r1, C002/r1, C003/r1, C004/r1 checked/ACK;
retained [r3 copy](logs/D1-R2-coordination-r3.md). C003 changes the phase03
publication checkpoint and source-verification timing; C004 carries F2's explicit
p02 contract dependency. No Doop work or tests run by B. Snapshot independently
rechecked unchanged before returning.

## 05SC-P01-B1-R1 — 2026-11-17

**PASS for the frozen p01 B1 implementation**, digest
`0713361f3809b86cfbde192b30267041ea50f862c3183afcb4fe38896890dabf`.
No actionable or optional findings. Note [05SC-P01-B1-R1-N1](agent-review-notes.md#05sc-p01-b1-r1-n1)
records coverage and acceptance limits. D1/r2's F1/F2 dispositions remain satisfied:
owner locks precede principal writes; remote subject support is not claimed here.

Same reviewer B, independent context, worktree/branch and start/review base/HEAD
`d748c2ff64309903449a3d860377a5e0385377a4`. Entire staged/unstaged/new-source artifact
reviewed against that base; exact snapshot exclusions remain
`plan/05-scopes/phases/p01/docs` and `plan/05-scopes/COORDINATION.md`.
Coordination revision 4, C001–C004/r1 and inherited contracts checked/ACK; immutable
input [B1 coordination](logs/B1-R1-coordination.md) and
[TASK/proof mapping](logs/05SC-P01-B1-R1-TASK.md).

Independent execution: nine focused database shape/lifecycle/rejoin/deferred-FK
tests PASS. Test discovery: 4,762 → 4,768, six additions, no removals. See
[reviewer evidence](logs/B1-R1-reviewer.evidence.json). Author's
[evidence manifest](logs/B1-R1.evidence.json) and all five initial-failure
dispositions inspected; the five exact successful rechecks and final 18-test step
run support the affected scope. Both evidence manifests validate.

Acceptance is p01 only. P02 owns replacement wire/tool grammar and shared run scope
snapshots, remote subject propagation/query/purge under C004, and their production
proofs. The full repository and affected real-service integration gate remains due
before phase03 publication under C003; full CI is not claimed. B ran no Doop checks.
Source snapshot and coordination freshness independently rechecked before return.
