# 03PA-P01 handoff

- Status (2026-11-14): phase started. Records are at `plan/03-principal-addresses/phases/p01/docs/` (TASK has the identity, settings, preserved changes and authorizations). No source edits yet.
- Coordination: `plan/03-principal-addresses/COORDINATION.md` revision 10 (adopted at revision 7). The old authority, `/Users/mac02/.codex/worktrees/2050/openbot`, no longer exists. The plan02 producer is accepted (`cb46c6fd`). C001–C005 are acknowledged.
- Starting commit `cb46c6fd` (plan02 complete, pushed to `origin/codex/departments-addressing-plans`). The p05 HANDOFF and COORDINATION edits made after that push are uncommitted records.
- Open design question for the D1 architecture gate:
  - Plan 01 item 3 disables automatic agent desks in p01. But plan02's agent escalation notice route, `workflow_escalation_agent_route` in `init_schema.sql`, joins the `agent:{id}` provisioning desk and its binding.
  - The replacement (principal direct threads in p02, a principal binding with provisioning in p03, and C001 rerouting) is not in place until p03.
  - Candidate decision: keep automatic agent desks through p02, and move agent principal address registration together with desk removal into p03. Persons get principal addresses in p01. B must gate this.
- D1/r1 is written (TASK "D1"; immutable `logs/D1-r1.md`). The inventory is in `logs/D1-inventory.md`. The gate digest is `eff341d3…d991`, with the exclusions in TASK "Status".
- R1: B (agent ae8aa8649634fe3c9) returned F1–F7 actionable and F8–F9 info (REVIEW.md, agent-review-findings.md). D1/r2 answers them (TASK "D1 — architecture gate (revision r2)"; immutable `logs/D1-r2.md`, sha256 `f51249ee…3a81`). COORDINATION is at revision 8, with C006/r1 proposed.
- R2: R1/F1–F7 fixed. Two Low residuals (R2/F1 outsider SMTP enumeration; R2/F2 lock rationale) and an optional R2/F3 (scoped read). D1/r3 (`logs/D1-r3.md`, sha256 `d5fd5109…d7ee`) fixes all three; it moves the guard to `FOR KEY SHARE`.
- D1/r3 PASS (03PA-P01-D1-R3); C006 is active (COORDINATION revision 9). The user accepted the two schema departures (F8).
- B1 is implemented and checked (TASK "B1 …" sections). It is frozen at digest `888f0a80…416c`, HEAD `830561e2`, with the four exclusions in TASK. Manifest: `logs/03PA-P01-B1-R1.evidence.json`.
- B1-R1 returned F1–F3 (Low) and F4–F7 (Info). They are corrected (TASK "B1-R1 result and corrections"), and the new digest is `dc9329df…ca23`.
- **p01 is accepted in phase** (B PASS 03PA-P01-B1-R2, digest `dc9329df…ca23`, recomputed by root; see TASK "Acceptance"). No review is running.
- Tell the user (F8): the resequencing, and the two schema deviations from decision 1's wording: kind-typed `channel_id`/`principal_id` FK columns instead of `target_id`, and no `principals.slug`.
- Next: p02 (`plan/03-principal-addresses/02-direct-threads-and-bindings.md`), with records at `phases/p02/docs/`.
- Not authorized: plan03 push, merge or deployment.
- The trail learnings were approved and committed as `830561e2` (not pushed). Still to tell the user, carried from plan02: F9 (an explicit rejection escalates) and the plugin `node_modules` backup.
