# Plan coordination: 07-unit-resources

- Authority: /Users/mac02/orca/workspaces/openbot/plan06/plan/07-unit-resources/COORDINATION.md
- Writer: Claude Code root (Opus 5.5/xhigh, user-confirmed), session b36d9dc4-495e-4131-9804-05ebcc381915, then 73d846ee-63bd-49f3-bbdf-7f3d92fcce54 (from B2), worktree /Users/mac02/orca/workspaces/openbot/plan06 (branch plan07)
- Revision: 9

## Plan-wide instructions

- 2026-11-20: the user asked this session to continue with the next plan under
  implement-opus-reviewed-2_4 after Plan06 (integrated on origin/main 5bf6a669). Root/A is
  Opus 5.5/xhigh; reviewer B is Opus 5.5 (the user twice chose an Opus reviewer over Fable 5.1 in
  this session). Default auto-compaction, not 75% (user skipped it).
- Base: branch plan07 from cc545881 (origin/main 5bf6a669 plus the local Plan06 p03 record
  correction). Execute documents 01, 02, 03 in order as phases p01, p02, p03 in this worktree.
- Authorizations: commit locally after each independently accepted batch; never push (a push
  needs a new user request). No deployment, no shared or production database reset.
- 2026-11-20 (session 73d846ee, same root role, Opus 5.5/xhigh re-confirmed): the user authorized
  a push to main once all of Plan07 (p01–p03) is complete and accepted; not before. Reviewer for
  the remaining work: Fable/medium (user choice this session). Default auto-compaction.
- No backward compatibility. Use fresh owned databases for schema verification.
- User decisions 2026-11-20:
  (a) Unit managers everywhere: creating, editing, deleting and moving a unit's skills, MCP and
  HTTP connections and model connections belongs to whoever manages that unit under Plan06 (the
  owner, a root admin, or the lead of that unit or an ancestor). Root-owned resources therefore
  belong to the owner, root admins and root leads. This widens today's owner-only skill writes and
  owner-plus-root-admin connection writes. Moving requires managing both units.
  (b) Cloudroom company machines (`local_machines`) are out of scope for Plan07; a follow-up is
  recorded for the plan that owns them (README decision 6).
  (c) A unit's spend cap is set by that unit's own manager (its lead or anyone above); ancestor
  caps still bound the subtree.
  (d) An agent's own tools and skills must be in reach of one of the agent's current departments
  (its unit or an ancestor) wherever it runs; workflow-level connections follow the run's (desk's)
  scope. Asked after review finding 07UR-P01-G1-F9.
- User decision (e), 2026-11-21 (asked on review finding 07UR-P01-B2-F5): using a unit's HTTP or
  MCP connection — a binding's slot inspection and every run action — belongs to whoever manages
  the connection's unit (its lead or anyone above), in addition to the desk-reach check; the
  company's messaging and JEV resources stay with root management. Replaces root management as
  the run actor's requirement for connections.
- User decision (f), 2026-11-21 (asked on optional review finding 07UR-P01-B2-F9), **replaces
  (e)**: any run whose desk reaches an HTTP or MCP connection may use it, whoever its run-as
  actor is (the actor must still be a current company member); binding save and every run action
  no longer require managing the connection's unit or the root. Configuring, deleting and moving
  a connection still belong to its unit's managers (decision a); messaging and JEV keep root
  management. Moving a connection up therefore shares it with every desk below.
- 2026-11-21: the user switched the reviewer back to Opus 5.5/xhigh "from now" (p02 on).
- User decisions 2026-11-21 for p02 (asked on the p02 investigation):
  (g) model connections stay one per provider per company (key `(company_id, provider)`); each is
  owned by a unit (root by default) and edited by that unit's managers; a unit marks one of its
  own connections as its default.
  (h) unit spend caps count priced usage only; unpriced tokens count as zero and are flagged per
  unit on the usage page.
  (i) cap periods are UTC calendar days and months (day from 00:00 UTC, month from the 1st 00:00
  UTC).
- User decisions 2026-11-21 on p02 gate findings 07UR-P02-G1-F1–F5:
  (j) spend for caps is kept in per-unit running totals per period: each settled model call adds
  its priced cost to its unit and every ancestor at that moment (deviates from doc 02's "no new
  ledger"); deleting or moving a department does not remove spend already counted above it.
  (k) a cap stops new runs once counted spend reaches it; admitted runs finish and may overrun
  (doc 02's "not stopped mid-flight"); "admit at most what the cap allows" means an admission
  that orders after spend reached the cap is refused.
- Inherited: Plan05 Doop deferral (05SC-C001/04RU-C005) stays visible and unfixed; Plan06
  authorization conventions (404 for what the caller does not manage, 403 inside their authority).
- Plan08 p01 exists unmerged on branch plan08_ph1 (6c63f292, base 8fdb892b; Codex root session
  01a1e095, stopped after its handoff). It is not a Plan07 prerequisite and is not touched here;
  overlaps are reconciled when it is integrated.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 (doc 01 ownership and selection): B1 accepted in phase (B PASS 07UR-P01-B1-R3 at 2849d92a…), committed locally (aa22b1b0); B2 accepted in phase (B PASS 07UR-P01-B2-R2 at 9179c26d…), committed locally; B3 (decision f) accepted (B PASS 07UR-P01-B3-R1); p01 complete in phase, committed locally; optional 07UR-P01-B3-F1 carried to p03 | Claude root/A; Opus B (G1, B1), Fable B (B2 on) | /Users/mac02/orca/workspaces/openbot/plan06 | phases/p01/docs/{TASK,HANDOFF}.md |
| p02 (doc 02 models, budgets, usage): G1 r5 PASS (07UR-P02-G1-R5); B1, B2, B3 accepted (B PASS 07UR-P02-B1-R2, -B2-R2, -B3-R2), committed locally; p02 complete in phase | Claude root/A; Opus B | /Users/mac02/orca/workspaces/openbot/plan06 | phases/p02/docs/{TASK,HANDOFF}.md |
| p03 (doc 03 integration and final verification): started 2026-11-21 | Claude root/A; Opus B | /Users/mac02/orca/workspaces/openbot/plan06 | phases/p03/docs/{TASK,HANDOFF}.md |

## Cross-phase notices

None.
