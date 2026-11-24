# AWC-P02 handoff

- Phase p02 of `plan/agent-workflow-call` (doc 02, target snapshots and dependency scopes); workflow
  implement-opus-reviewed-2_4; records here; TASK.md is authoritative.
- Root/A: Claude Code `1e84a7af`, Opus 5.5/xhigh (user-confirmed). B: Fable/medium `a8d0fa1fbd0b8b88e` (continue
  with SendMessage).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD = start = `fe7791d3`.
  Everything (p01 and p02) is uncommitted; commit only with the user's authorization.
- Prerequisite: p01 accepted in phase at `2ebbc0cd…feaa`.
- Coordination `plan/agent-workflow-call/COORDINATION.md` r10 (writer: this session): AWC-C001 r4, AWC-C002 r1,
  AWC-C003 r2 (p02 → p03 ownership of the `call_channel` port, dispatch, launch, and the NOT EXERCISED items).
- Gate AWC-P02-D1 r3 PASSED (`logs/AWC-P02-D1-r3-decision.md`, a delta on r2).
- Batch AWC-P02-B1 implemented; frozen at `543ef9e9…1777` (exclusions `plan/agent-workflow-call/phases/p02/docs`,
  `.trail/learnings`); evidence `logs/AWC-P02-B1-R1.evidence.json`. Review R1 with B. Contract changes found in
  implementation (seal-guard exemption for recorded call children; record before run scopes) are in TASK.
- Resources: scratch DBs `openbot_awc_mac02`, `openbot_awc_mac02_test` (localhost:5432). Drop at plan completion.
- Untracked learnings under `.trail/learnings/` (l-a5048a8f, l-11ae6c52, and l-c0bc0871 superseded): commit only
  with the user's authorization.
- R1 (`AWC-P02-B1-R1-N1`): changes required, F1–F4; F5 optional. Corrections frozen as R2 at `0639c0f0…2112`
  (evidence `logs/AWC-P02-B1-R2.evidence.json`), with B for recheck.
- **p02 accepted in phase** at `0639c0f0…2112` (B PASS `AWC-P02-B1-R2-N1`); see TASK "Acceptance" for carried gaps.
- Next: p03 (records `phases/p03/docs/`). After the plan: Plan08 p03, once Plan08
  README proposals 4 and 5 are settled with the user.
