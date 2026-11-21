# Plan coordination: 07-unit-resources

- Authority: /Users/mac02/orca/workspaces/openbot/plan06/plan/07-unit-resources/COORDINATION.md
- Writer: Claude Code root (Opus 5.5/xhigh, user-confirmed), session b36d9dc4-495e-4131-9804-05ebcc381915, then 73d846ee-63bd-49f3-bbdf-7f3d92fcce54 (from B2), worktree /Users/mac02/orca/workspaces/openbot/plan06 (branch plan07)
- Revision: 6

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
- Inherited: Plan05 Doop deferral (05SC-C001/04RU-C005) stays visible and unfixed; Plan06
  authorization conventions (404 for what the caller does not manage, 403 inside their authority).
- Plan08 p01 exists unmerged on branch plan08_ph1 (6c63f292, base 8fdb892b; Codex root session
  01a1e095, stopped after its handoff). It is not a Plan07 prerequisite and is not touched here;
  overlaps are reconciled when it is integrated.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 (doc 01 ownership and selection): B1 accepted in phase (B PASS 07UR-P01-B1-R3 at 2849d92a…), committed locally (aa22b1b0); B2 accepted in phase (B PASS 07UR-P01-B2-R2 at 9179c26d…), committed locally; p01 complete in phase pending the user's answer on optional F9 | Claude root/A; Opus B (G1, B1), Fable B (B2 on) | /Users/mac02/orca/workspaces/openbot/plan06 | phases/p01/docs/{TASK,HANDOFF}.md |

## Cross-phase notices

None.
