# Review record — 06DA-P02

## 06DA-P02-D1-R1 — architecture gate D1/r1 (02A + 01A.2, 02B)

- **Outcome: CHANGES REQUIRED** for decision revision D1/r1 (TASK sha256 `7a6b8015…070d3`; reviewer copy `logs/06DA-P02-D1-R1-reviewer-task-r1.md`). Note: [06DA-P02-D1-R1-N1](agent-review-notes.md).
- **Code identity:** HEAD `5874dfb1908aeb8d7663664ff2e6b2e4631c2c40`, clean source. Snapshot `f720ea29…9bbb`, exclusions `plan/06-delegated-administration/phases/p02/docs` and `plan/06-delegated-administration/COORDINATION.md`.
- **Coordination:** r7 (sha256 `2458f32e…a1f6`; adds (c)/(d) = U3/U4), no notices. TASK still cites r6.
- **Actionable findings** ([details](agent-review-findings.md)):
  - **F1 (medium):** the existing `department_administration_events` table is unreconciled. A parallel table is proposed, with a different immutability rule and label semantics, and the pane would omit system role removals.
  - **F2 (medium):** a root lead is possible, so `manages_root_unit()` ≠ `unit_management_role(root)`. Root-lead semantics are undefined.
  - **F3 (medium):** `owned_company` keeps 403, which contradicts item 9's 404 promise for (b) routes and 02A.4.
  - **F4 (medium):** `list_departments` at AnyUnit entry exposes every department to leads in desk pickers.
  - **F5 (medium):** "plus what they may read" (02A.2) is narrowed without a user decision.
  - **F6 (low):** the audit writer and acting-role inventory is incomplete: root removal in the pane, manager agent placements, create-with-lead and reparent cardinality, and the existing `workflow_binding_events` and `workflow_definition_events`.
  - **F7 (low):** the p01-carried admin member-invite UI has no disposition.
- **Optional:** F8 (low) — pane rule wording and the coordination revision reference.
- **Accepted as designed:** the `administered_company` entry gate, `managed_company` refusals becoming 404, the single AnyUnit SQL function, the class split and U4, the scoped desk and definition lists, the per-department pane flags, the rail, the 01A.2 removal list, and the atomic audit with its competing test and cascade.
- **Next:** A publishes an immutable D1/r2 that resolves F1–F7, with user decisions where it chooses them for F2, F3 or F5, then requests a recheck. No dependent source edits before a PASS.

## 06DA-P02-D1-R2 — architecture gate recheck (D1/r2)

- **Outcome: PASS for decision revision D1/r2.** Note: [06DA-P02-D1-R2-N1](agent-review-notes.md).
- **Decision identity:** TASK sha256 `173a25c6d6c2fd6e7dd5313838c7c6a961af7e04e690deb71e6bb101c878b4c7` (pin `logs/D1-r2-TASK.md`).
- **Coordination:** r7, `2458f32e…` (pin `logs/D1-r2-coordination-r7.md`), no notices.
- **Code:** HEAD `5874dfb1908aeb8d7663664ff2e6b2e4631c2c40`, unchanged; no source.
- **Findings:** [F1–F8](agent-review-findings.md) are FIXED at decision level. F2 is closed under U5 and F5 under U6.
- **Optional, carried into the batches** ([details](agent-review-findings.md)):
  - F9 (low): root-membership removal controls for a root lead.
  - F10 (low): scope of the 404 rule against the accepted 403 payload and level refusals.
  - Binding implementation notes:
    - F1: scope the deferred role-cleared check to system rows; write the root person removal row before the principal is retired.
    - F6: a role-bearing agent placement also records `role_assigned`.
    - F7: the member-invite edit control, and the admin-address refusal shown inline.
- **Scope:** architecture and verification plan only. No implementation acceptance; the promised tests have not run.
- **Next:** root accepts the gate. A implements B1 (02A), then B2 (02B), and each gets a frozen batch review.

## 06DA-P02-B1-R1 — batch review of B1 (02A)

- **Outcome: CHANGES REQUIRED** at `6248bc8cd74f200a4e11f9ba2d00fd37eee979cfc341cb09f363d7e05814897d`. HEAD `5874dfb1` plus the working-tree delta (84 paths); exclusions `phases/p02/docs`, `phases/p01/docs`, `COORDINATION.md`. The digest and file list were reproduced by me. Note [06DA-P02-B1-R1-N1](agent-review-notes.md).
- **Coordination:** r8 `3ea7a412…` (pinned; U7 and the reviewer choice), no notices.
- **Actionable:**
  - [B1-F1](agent-review-findings.md), medium: the attention team-work and operational-summary refusals stay 403 although they are company-administration refusals (r2-2/F10), and the attention and runs authorization tests gained no lead cases (02A.1).
  - [B1-F2](agent-review-findings.md), medium: lead workspaces embed root-only fragments. The agent pane's "used by" card fails, and the desk and agent workspaces open a run-count SSE stream that returns 404 and reconnects.
  - [B1-F3](agent-review-findings.md), low: the JSON workflow-bindings list is still root-only, while the definitions list and binding reads are unit-scoped.
- **Optional:** [B1-F4](agent-review-findings.md) — legacy desk list without readable rows; legacy invites page owner-only; the authoring test tolerates a non-404 refusal; a root lead is not offered add-member at root.
- **Accepted as reviewed:** unit-entry gates and 404s; `Ui::detail` and every by-id check; list scoping including U6 desk rows and picker scopes; department-pane rules including F9; rail; team UI for admins (decision b, F10); `administers_company` (U5); the 01A.2 removal; the test-change inventory (+12 new, 2 renames, none dropped).
- **Evidence:** my run 215 PASS / 0 FAIL; author full lib 4869/3 known unrelated/30 ignored. Both manifests validate.
- **Next:** A corrects F1–F3, adds lead page tests for the agent pane and desk workspace, re-freezes, and requests a recheck.

## 06DA-P02-B1-R2 — correction recheck of B1-F1..F4

- **PASS for B1 (02A)** at `2c8e4add9aa78a22a356146f998028cbe4b0c0c8e9a35b668c29f4b692eb2f3d`. HEAD `5874dfb1` plus the working tree; same three exclusions; digest and file list reproduced by me. Note [06DA-P02-B1-R2-N1](agent-review-notes.md).
- [B1-F1](agent-review-findings.md) FIXED: attention refusals are 404, with lead tests in the attention and runs authorization tests.
- [B1-F2](agent-review-findings.md) FIXED: the agent usage card is unit-entry plus an agent management check; the run-count bar and SSE render for company administrators only, and no lead-facing stream remains.
- [B1-F3](agent-review-findings.md) FIXED: the bindings list and `visible_binding` enter via `ListManaged`. The other `visible_binding` callers are root-gated or `Admit`-checked per association, so no authority is added.
- [B1-F4](agent-review-findings.md) dispositioned; the U7 assertion is now exact.
- **Evidence:** my run 396 PASS / 0 FAIL / 4 ignored; author manifest 8 PASS; both manifests validate. Coordination r8 matches its pin.
- **Scope:** batch only. 02B, p03 and the final full gate remain.
