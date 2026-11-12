# Plan coordination: 01-departments

- Authority: /Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md
- Writer: root session 68933786-cbe7-4b6b-9387-b5dcb29689a4, worktree
  /Users/mac02/.codex/worktrees/2050/openbot (branch codex/departments-addressing-plans)
- Revision: 4

## Plan-wide instructions

- 01DEP-I001 / r1: No backward compatibility (plan README, root AGENTS.md). Edit the init schema
  in place; reset databases instead of backfills, dual reads, shims or staged rollouts.
- 01DEP-I002 / r1: Phases run in order 01 -> 02 -> 03 in this one worktree, each committed
  locally once accepted (user authorization, 2026-11-09). 02C (the word "desk") is its own
  commit. Phase 04 (UI and docs) and phase 05 are out of scope for this run.
- 01DEP-I003 / r1: Verification for this run is per-phase focused checks only (user decision,
  2026-11-09). The repository's full gate (`scripts/check.sh` plus CI-only gates) is not run and
  must be reported as not run; the task is not claimed fully verified.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | root 68933786 (A), B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p01/docs/{TASK,HANDOFF}.md — accepted in phase, commit 57adfa7a |
| p02 | root 68933786 (A), B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p02/docs/{TASK,HANDOFF}.md — accepted in phase (B2 0c5df403, B1 next commit) |
| p03 | root 68933786 (A), B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p03/docs/{TASK,HANDOFF}.md — B1 523732b7, B2 01608b1f accepted; B3 pending |

## Cross-phase notices

### 01DEP-C001 / r1
- State: active (2026-11-10)
- Origin: p02 startup (root/A). Code fact: internal relays never consult the receiving desk's
  `participant_access` (`thread/ingest/routing.rs` `authorize_channel` skips the authorized check
  for `IngressOrigin::InternalChannel`); `InternalRelayMail` carries only the source desk. Plan
  02B.5's premise (the relay carries the run's actor into `participant_access`) is false.
- Targets: p02, p03 (and plan 04/05 documentation).
- Instruction/change: **user decision 2026-11-10 — keep internal relays trusted.** A members-only
  desk restricts people and outside senders; desk-to-desk relays inside the company stay admitted
  as today, still bound by the hop budget and trace (`MAX_INGRESS_HOPS`, loop refusal). Derived
  grants are still written for agent members (02B.5), but they do not gate relays. p02's
  `department_desk_relay_` check becomes: a relay from a member agent's desk and from a non-member
  desk into a members-only desk are both admitted, and a reappearing desk in the trace is still
  refused definitely. p03's `department_lead_round_trip_` "a send from a non-member department is
  refused" is replaced by the same trusted-relay assertion; p03 documents this in its dispatch
  section.
- Dependencies/order: effective now; p02 gate decisions build on it.
- Evidence/review: user answer in session 68933786 (AskUserQuestion, 2026-11-10).
- Verification: p02 `department_desk_relay_` tests; p03 round-trip tests.
- Responses: p02 -> satisfied (r1; `department_desk_relay_into_a_members_only_desk_stays_trusted`,
  docs/inter_channel_agent_communication.md paragraph; p02 TASK B1 mapping); p03 -> pending.

### 01DEP-C002 / r1
- State: active (2026-11-10)
- Origin: p02 startup (root/A). Plan conflict: README decision 7 grants a members-only desk to its
  members, role holders and the **parent** department's lead only, while 03C.3 says the child lead
  escalates by messaging the parent's desk and "the parent desk's grants admit the child lead".
- Targets: p02, p03 (and plan 04 docs).
- Instruction/change: **user decision 2026-11-10 — one step both ways.** A members-only desk's
  `department_membership` grants cover the department's members, its role holders, the holder of
  the parent department's `lead`, and the `lead` of each direct child department. Nothing further
  up or down. Consequently a lead change re-derives the department's own, its parent's and its
  children's members-only desks; a re-parent re-derives the moved department's, the old parent's
  and the new parent's members-only desks.
- Dependencies/order: effective now; p02 `sync_department_grants_on`.
- Evidence/review: user answer in session 68933786 (AskUserQuestion, 2026-11-10).
- Verification: p02 `department_desk_members_only_` tests (child lead admitted, grandchild lead
  refused, parent lead admitted, grandparent lead refused); p03 escalation test.
- Responses: p02 -> satisfied (r1; `department_desk_members_only_reaches_one_step_up_and_down`;
  p02 TASK B1 mapping); p03 -> pending.

### 01DEP-C003 / r1
- State: active (2026-11-10)
- Origin: p03 B3 startup (root/A). Code fact: the model-facing directory `list_company_agents` is
  a provisioning tool whose requester must be an owner or admin writing in the app
  (`provisioning.rs` `requester_on`/`authorize_requester`), so a lead run started by a member's
  message or by a relay is refused it; plan 03's outcome ("the lead can find its members and
  their desks") would hold only for admin-started runs.
- Targets: p03 (and plan 04 docs).
- Instruction/change: **user decision 2026-11-10 — roster in the run context.** A run on a desk a
  department owns gets, in its `agent.run` system context after the agent's own prompt, the
  department's name and charter plus a bounded roster: its agent members' desk addresses, its
  child departments' desks and its parent department's desk, captured once in the frozen
  preparation. The directory tool stays admin-only and gains the plan's department fields and
  filter.
- Dependencies/order: effective now; p03 B3 gate.
- Evidence/review: user answer in session 68933786 (AskUserQuestion, 2026-11-10).
- Verification: p03 B3 department-context tests and the round-trip test.
- Responses: p03 -> pending.
