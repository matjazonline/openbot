# Plan coordination: 01-departments

- Authority: /Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md
- Writer: `/root` (Sol 6.1/xhigh, user confirmed; 2026-11-11 continuation), worktree
  /Users/mac02/.codex/worktrees/2050/openbot (branch codex/departments-addressing-plans)
- Revision: 16
- Writer handoff: the prior root session 68933786-cbe7-4b6b-9387-b5dcb29689a4 stopped at the
  committed checkpoint e7bc780e. The user resumed p05 here on 2026-11-11; this root now serializes
  coordination writes. Instructions and notices from revision 10 remain unchanged.

## Plan-wide instructions

- 01DEP-I001 / r1: No backward compatibility (plan README, root AGENTS.md). Edit the init schema
  in place; reset databases instead of backfills, dual reads, shims or staged rollouts.
- 01DEP-I002 / r2: Phases run in order in this one worktree, each committed locally once
  accepted. p01–p03 accepted and committed (57adfa7a … 30654e3c). User 2026-11-10 "pull main and
  continue": origin/main merged into the branch (b1026003), then phase 04, then 05. No push.
  (r1: phases 01 -> 02 -> 03 only; 04 and 05 out of scope.)
- 01DEP-I003 / r2: Verification is per-batch focused checks; the repository's full gate
  (`scripts/check.sh` plus CI-only gates) runs once in phase 05 (05A), as plan 05 specifies. Until
  then it is reported as not run. (r1: full gate not run in this run, user decision 2026-11-09.)

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | root 68933786 (A), B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p01/docs/{TASK,HANDOFF}.md — accepted in phase, commit 57adfa7a |
| p02 | root 68933786 (A), B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p02/docs/{TASK,HANDOFF}.md — accepted in phase (B2 0c5df403, B1 next commit) |
| p03 | root 68933786 (A), B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p03/docs/{TASK,HANDOFF}.md — accepted in phase (B1 523732b7, B2 01608b1f, B3 30654e3c) |
| p04 | root 68933786 (A), B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p04/docs/{TASK,HANDOFF}.md — accepted in phase (B1 399efe2c, B2 5c64e7d1, B3 next commit) |
| p05 | `/root` (A), fresh B per TASK | /Users/mac02/.codex/worktrees/2050/openbot | plan/01-departments/phases/p05/docs/{TASK,HANDOFF}.md — R3 required image/Doop/live-memory gates PASS; test-fixture teardown correction and evidence pending independent review; not yet accepted or integrated (start c42fc15d, checkpoint e7bc780e) |

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
  docs/inter_channel_agent_communication.md paragraph; p02 TASK B1 mapping); p03 -> disposed by
  C004 (no p03 round-trip test; the trusted-relay rule stands as p02 verified and documented);
  p04 -> satisfied (r1; `docs/departments.md` "Members-only desks": relays from other desks stay
  admitted under the hop budget and loop refusal; B3 artifact 30567a29…c675). p05 -> satisfied for documentation/current contract (r1; current full-run trusted-relay test PASS; shipped guide and unchanged authorization/loop branches inspected; p05 TASK "Coordination dispositions", logs/B1-doc-contract-inspection.txt). Independent p05 review and overall integration acceptance remain pending.

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
  p02 TASK B1 mapping); p03 -> satisfied for grants (B1
  `agent_default_desk_places_the_agent_and_its_lead_role_with_grants`: a lead placed at creation
  gets derived grants on its own, parent's and child's members-only desks); the escalation
  message test is disposed by C004; p04 -> satisfied (r1; `docs/departments.md` "Members-only
  desks" lists members, role holders, parent lead, direct children's leads, "one step of the
  hierarchy each way"; B3 artifact 30567a29…c675).

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
- Responses: p03 -> satisfied (r1; B3 artifact 0db98506…dbf1, B3-R2 PASS:
  `department_context_*` (content, lead flag, public desk without roster, bound, frozen retry,
  e2e request text) and `department_directory_*` (fields, filter, picker entries); the
  round-trip test is disposed by C004); p04 -> satisfied (r1; `docs/departments.md` "Leads" and
  `docs/agent-authoring.md`: the context and its bounds, the directory staying an owner's or
  admin's tool; B3 artifact 30567a29…c675).

### 01DEP-C004 / r1
- State: active (2026-11-10)
- Origin: p03 B3 startup (root/A). Code facts: a workflow `message.send` to a channel destination
  writes an outbound system message into a new thread on that desk (`message_planner.rs`
  `plan_channel`, `message_writer.rs`); only the inbound commit creates admission intents
  (`thread/inbound.rs`), so the target desk's bindings never run. And a reply continues the trace
  its source arrived with (`message_planner.rs` `ReplyEmail`), so a member's reply back to the
  lead's desk finds the lead in the trace and is refused as a cycle (`ingest/routing.rs`). Plan
  03C.2-03C.4 ("nothing new is needed" for dispatch; escalation by `message.send`; the reply chain
  admitted, a new send refused) do not hold in today's messaging.
- Targets: p03 (and plan 04/05 documentation).
- Instruction/change: **user decision 2026-11-10 — defer dispatch to `channel.call`.** p03 does not
  change core messaging or loop safety. It delivers the directory fields and filter, the
  department name, charter and roster in the run context (C003), the `department-lead.yaml`
  fixture and the docs; the dispatch, escalation, round-trip and loop checks
  (`department_lead_round_trip_`, `department_lead_loop_`) are recorded as blocked by current
  messaging and documented as waiting for agent-workflow-call (`channel.call`), which the plan
  already names as their successor.
- Dependencies/order: effective now; p03 B3 gate.
- Evidence/review: user answer in session 68933786 (AskUserQuestion, 2026-11-10).
- Verification: p03 records the two checks as not built, with these code facts; docs say so.
- Responses: p03 -> satisfied (r1; no `department_lead_round_trip_` / `department_lead_loop_`
  tests; docs/inter_channel_agent_communication.md and the plan 03 "Status after p03" note under
  03C item 4; `department-lead.yaml` header); p04 -> satisfied (r1; `docs/departments.md`
  "Dispatch and escalation", `docs/inter_channel_agent_communication.md` "Department dispatch and
  escalation", `docs/custom_tools.md`: no intake schema until `channel.call`; B3 artifact
  30567a29…c675). p05 -> satisfied for documentation/current contract (r1; shipped guide, tool docs and fixture explicitly defer dispatch/escalation to channel.call; planner/writer unchanged since p03, routing changes only directory grouping; p05 TASK "Coordination dispositions", logs/B1-doc-contract-inspection.txt). Independent p05 review and overall integration acceptance remain pending.
