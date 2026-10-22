# P07 handoff — USER-REQUESTED STOP 2026-10-21

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

The user asked to write a handoff and stop after the reviewer's response. The stop is
intentional; resume only on a new user instruction (`implement-opus-reviewed-2_2`, root = A).

## Identity and records

- Phase `p07`, plan `plan/workflow-foundation/07-child-workflows-and-review-revisions.md`.
- Records: `/Users/mac03/dev/openbot/plan/workflow-foundation/phases/p07/docs/` —
  TASK.md (authoritative requirements, batches, decisions, status), REVIEW.md /
  agent-review-notes.md / agent-review-findings.md (B-owned), `logs/decision-D1.md` (rev 1,
  immutable gate text).
- Root/A: Opus 5.5/xhigh (user-confirmed). B: agent `a46b56e8565db0c84` (opus/xhigh, fresh
  context); finished R001, idle; continue it with `SendMessage` (a fresh root that cannot reach
  it spawns a new B from `references/reviewer-assignment.md`).
- Worktree `/Users/mac03/dev/openbot`, branch `workflows`, start/HEAD `921079d`.
- Snapshot exclusions (exact): `plan/workflow-foundation/phases/p07/docs
  plan/workflow-foundation/COORDINATION.md marketing website` (`marketing/`, `website/` = user's
  concurrent unrelated edits). Current digest `e2714983…e3fc` = baseline, no p07 source edits.
- Preserved pre-existing work: 440 deleted `plan/workflow-foundation/**` record files (user
  cleanup) — do not restore or stage.
- Coordination: `/Users/mac03/dev/openbot/plan/workflow-foundation/COORDINATION.md` r1, no notices.

## State

- B1 (child calls, `flow.repeat`, inheritance, review template) — architecture gate P07-D1
  rev 1 reviewed: **WF-P07-R001 changes required** (F001 High, F002–F004 Medium, F005–F006 Low,
  F007 optional). Plus A self-finding: production publication never captures pinned child
  bundles. Details and fix directions in TASK.md "Decisions".
- B2 (agent workflow tools) not started. Investigation note: agent tools already suspend via
  `AgentToolObservation::Parked(wait)` and resume by replaying the saved call
  (`continue_delivery`); B2 can reuse that with a child wait. The product question of how
  admins expose a workflow as an agent tool (named registration pinning a version vs direct
  workflow reference) should go to the user before P07-D2, explained practically.
- No source/schema edits, tests, databases created, commits or deployments. No workers running.

## Resources

Task PostgreSQL 18.6 `/private/tmp/workflow0410-20261017`, `127.0.0.1:55450`, role
`workflow0410`, `PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH`, always 127.0.0.1. p07 may
create/drop only `workflow07_*` databases (none exist now); leave `workflow0410_*` and
`mail_agents_own_*` untouched. Builds with `CARGO_INCREMENTAL=0`.

## Next action

1. Write `logs/decision-D1.md` revision 2 as a new file (`decision-D1-r2.md`), addressing
   F001–F006, selected F007 items and the publication self-finding; update TASK.
2. `SendMessage` B for the correction recheck by finding ID; obtain explicit PASS of rev 2.
3. Implement B1 per the passed decision, focused checks, freeze, batch review.
