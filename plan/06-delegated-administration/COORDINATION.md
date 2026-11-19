# Plan coordination: 06-delegated-administration

- Authority: /Users/mac02/orca/workspaces/openbot/plan06/plan/06-delegated-administration/COORDINATION.md
- Writer: Claude Code root (Opus 5.5/xhigh, user-confirmed), session b36d9dc4-495e-4131-9804-05ebcc381915, worktree /Users/mac02/orca/workspaces/openbot/plan06
- Revision: 9

## Plan-wide instructions

- 2026-11-18 handover: the user asked this Claude Code session to continue Plan06
  under implement-opus-reviewed-2_4 from the prior Codex root's stop. The Codex root
  (/root, session 01a1e945-…) had finished and stopped; it no longer writes here.
  Root/A is Opus 5.5/xhigh; independent reviewer B is Opus 5.5/xhigh by user choice
  (in place of the skill's Fable/medium). Default auto-compaction, not 75% (user skipped).
- Plan05 is complete on origin/main (be1b504b). User authorized a WIP commit of the
  accepted p01 B1–B3 work (d7d81ad8) and a merge of origin/main (174c83b3) on branch
  plan06. No push, deployment or shared database reset is authorized.
- No backward compatibility. Use fresh owned databases for schema verification.
- Preserve the existing conversation/read policy, owner-only operations and all
  tenant, identity, classification, transaction and concurrency boundaries.
- Execute original documents 01, 02, 03 in order, with focused checks and independent
  acceptance at stable batch boundaries and one complete final repository gate.
- User decisions 2026-11-18: (a) a unit manager's manual knowledge read AND write
  cover the whole managed subtree; frozen workflow run scopes and agent memory stay
  as Plan05 defined. (b) Root admins may create/update/delete company invites with
  role=member; any invite carrying role=admin (before or after) stays owner-only.
- User decisions 2026-11-19: (c) unit managers may read company model choices and the
  skills list for agent authoring and use the prompt generator; writing them stays
  owner-only. (d) Runs, Deliveries and Dashboard stay root-only (owner/admin).
- User decisions 2026-11-19 (cont.): (e) binding outside managed units stays a 404, no
  `binding.outside_managed_units` code; (f) reviewer stays Opus 5.5 (Fable 5.2 unavailable).
- User 2026-11-19: commit each independently accepted batch locally on plan06; never push.
- Plan05 Doop deferral (05SC-C001/04RU-C005) is inherited: Doop failures and
  assertions are preserved, not relabeled, and not fixed here.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 accepted in phase (B1–B4; 01A.2 removal carried to p02) | Claude root/A; Opus B | /Users/mac02/orca/workspaces/openbot/plan06 | phases/p01/docs/{TASK,HANDOFF}.md |
| p02 B1 (02A) accepted; B2 (02B audit) implementing | Claude root/A; Opus B | same sequential worktree | phases/p02/docs/{TASK,HANDOFF}.md |
| p03 planned | Claude root | same sequential worktree | phases/p03/docs/ when started |

## Cross-phase notices

None. Plan05 is integrated from its published main commit; no shared contract has
been agreed or published to another phase.
