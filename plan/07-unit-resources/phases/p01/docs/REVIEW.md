# 07UR-P01 review record

## G1 decision revision r1 — round 07UR-P01-G1-R1 (2026-11-20)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD cc545881, digest
  c2d338160c48d4d2eabaa84b5bd5fa5cd389b58d2a93db85478eb2e05fb9ac33 (exclusions
  plan/07-unit-resources/phases/p01/docs, plan/07-unit-resources/COORDINATION.md); coordination
  revision 1, no notices.
- Outcome: **changes required** (no PASS for G1 r1).
- Notes: [07UR-P01-G1-R1](agent-review-notes.md#07ur-p01-g1-r1-2026-11-20).
- Actionable findings ([agent-review-findings.md](agent-review-findings.md)):
  - 07UR-P01-G1-F2 (high): unslotted HTTP resolution stays company-wide and is re-resolved at
    runtime without a unit check.
  - 07UR-P01-G1-F1 (medium): D6 puts the captured unit inside the immutable binding identity
    (`check_agent_revision_members`, binding reuse).
  - 07UR-P01-G1-F3 (medium): D9b refuses resources moved up; 01B.3 refuses only moves out of reach.
  - 07UR-P01-G1-F4 (medium): native MCP runs use captured agent connections outside D9.
  - 07UR-P01-G1-F5 (medium): soft-deleted MCP connections block department deletion forever.
  - 07UR-P01-G1-F6 (medium): the move's revision bump invalidates integration authority snapshots.
  - 07UR-P01-G1-F9 (medium): agent union authoring unit vs per-run agent read set; outcome not
    stated or tested.
  - 07UR-P01-G1-F10 (medium): planned mapping lacks decisive cases for D8, D9a, refusal
    settlement, D11, company teardown, skill capture.
  - 07UR-P01-G1-F7 (low): D3 default is the creator's placement, not managed department;
    library-agent skill copies.
  - 07UR-P01-G1-F8 (low): D7 omits Agent Builder name-based lookups.
- Optional suggestions: 07UR-P01-G1-F11 (slug conflict disclosure), 07UR-P01-G1-F12 (D9b use-mode
  and retired-resource scope).

## G1 decision revision r2 — round 07UR-P01-G1-R2 (2026-11-20, correction recheck)

- Reviewer B: Claude Opus 5.5. Artifact unchanged: HEAD cc545881, digest
  c2d338160c48d4d2eabaa84b5bd5fa5cd389b58d2a93db85478eb2e05fb9ac33 (same exclusions);
  coordination revision 2 (user decision (d)), no notices.
- Outcome: **changes required** (no PASS for G1 r2), low severity only.
- Notes: [07UR-P01-G1-R2](agent-review-notes.md#07ur-p01-g1-r2-2026-11-20-correction-recheck).
- R1 findings: F1, F2, F3, F5, F6, F7, F8, F10 fixed in decision; F9 resolved by user decision
  (d); F4 fixed for native tool calls (residual F14); F11 dismissed with disposition; F12
  adopted.
- Remaining actionable findings:
  - 07UR-P01-G1-F13 (low): stale r1 mapping rows and D10/risk text contradict r2; no reach context
    named for `check_readiness` callers that run before run scopes exist.
  - 07UR-P01-G1-F14 (low): native bundle delivery of skills/tools is not checked against agent
    reach (decision (d)), or needs an explicit decision (b) disposition.

## G1 decision revision r3 — round 07UR-P01-G1-R3 (2026-11-20, correction recheck)

- Reviewer B: Claude Opus 5.5. Artifact unchanged: HEAD cc545881, digest
  c2d338160c48d4d2eabaa84b5bd5fa5cd389b58d2a93db85478eb2e05fb9ac33 (same exclusions);
  coordination revision 2, no notices.
- Outcome: **PASS for G1 decision revision r3.**
- Notes: [07UR-P01-G1-R3](agent-review-notes.md#07ur-p01-g1-r3-2026-11-20-correction-recheck).
- 07UR-P01-G1-F13 fixed; 07UR-P01-G1-F14 fixed in decision. No actionable finding open.
- Optional: 07UR-P01-G1-F15 (name capture.rs:41 and channel_reply.rs:314 as chain-context reach
  callers; native reach is a live check that also refuses machine Status/Cancel after a move).

## B1 batch review — round 07UR-P01-B1-R1 (2026-11-20)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD cc545881 + working tree, digest
  2c55a5762e07d9e7f2a7d6aaf9a7fffe06571ab1351f8c666a97f9a8a3dad81a (same exclusions);
  coordination revision 2, no notices; G1 r3 PASS relied on.
- Outcome: **changes required** (no PASS for B1).
- Notes: [07UR-P01-B1-R1](agent-review-notes.md#07ur-p01-b1-r1-2026-11-20-batch-review-of-b1).
- Reviewer check: logs/B1-R1-reviewer-unit-resource.txt (5 unit_resource tests passed on a
  disposable database).
- Actionable findings ([agent-review-findings.md](agent-review-findings.md)):
  - 07UR-P01-B1-F1 (medium): the company settings Skills tab is still owner-only, so admins and
    unit leads have no skill UI.
  - 07UR-P01-B1-F2 (medium): agent authoring's skill picker now lists only managed-unit skills;
    leads lose ancestor (root) skills (Plan06 U3 regression).
  - 07UR-P01-B1-F3 (medium): no discriminating tests for Vault propose/confirm/inspect, MCP
    discovery, operation register, level 403 for a lead, root lead, library-agent copy unit,
    binding uses, admin-with-placement default.
- Optional: 07UR-P01-B1-F4 (MCP/skill management checked outside the write transaction),
  07UR-P01-B1-F5 (move response names unmanaged uses).

## B1 correction recheck — round 07UR-P01-B1-R2 (2026-11-20)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD cc545881 + working tree, digest
  c8c0b6d3b7f4634542ffc65305506e2e6d26f1ecaf163029e6156bf354e08cfd (same exclusions);
  coordination revision 2, no notices.
- Outcome: **changes required** (no PASS for B1).
- Notes: [07UR-P01-B1-R2](agent-review-notes.md#07ur-p01-b1-r2-2026-11-20-correction-recheck-of-b1);
  follow-up appended to 07UR-P01-B1-R1 for the missed level defect.
- Reviewer check: logs/B1-R2-reviewer-unit-resource.txt (13 unit_resource tests passed).
- Fixed: 07UR-P01-B1-F2, 07UR-P01-B1-F5 (optional applied). Optional open: 07UR-P01-B1-F4.
- Open actionable:
  - 07UR-P01-B1-F1 (medium): the Skills tab now errors for unit and root leads (`counts` calls the
    owner/admin-only channel list); no route-level test.
  - 07UR-P01-B1-F6 (medium, new): `may_assign_level` refuses every non-admin, so unit leads cannot
    set any connection level; the ceiling test passes on that unrelated refusal; refusal is 400 not
    the planned 403.
  - 07UR-P01-B1-F3 (low remainder): the library-agent copy test cannot distinguish placement from
    the creator's default unit.

## B1 correction recheck — round 07UR-P01-B1-R3 (2026-11-20)

- Reviewer B: Claude Opus 5.5. Artifact: HEAD cc545881 + working tree, digest
  2849d92a62feef47d12e7f73961195e804ef247edbe9801adb55770106493b42 (same exclusions);
  coordination revision 2, no notices.
- Outcome: **PASS for B1 at digest 2849d92a62feef47d12e7f73961195e804ef247edbe9801adb55770106493b42.**
- Notes: [07UR-P01-B1-R3](agent-review-notes.md#07ur-p01-b1-r3-2026-11-20-correction-recheck-of-b1).
- Reviewer check: logs/B1-R3-reviewer-focused.txt (unit_resource, access_level, connection_level,
  ui_companies: 80 passed).
- Fixed: 07UR-P01-B1-F1, 07UR-P01-B1-F2, 07UR-P01-B1-F3, 07UR-P01-B1-F6; optional
  07UR-P01-B1-F5 applied. Optional open (non-blocking): 07UR-P01-B1-F4.
