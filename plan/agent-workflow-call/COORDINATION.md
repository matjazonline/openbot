# Plan coordination: agent-workflow-call

- Authority: /Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/agent-workflow-call/COORDINATION.md
- Writer: Claude Code session `1e84a7af` (root/A) in `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`
- Revision: 26

## Plan-wide instructions

- r1 (2026-11-22): the user authorized implementing this plan in phase order (p01–p06) on
  branch `plan08_ph1`, stacked on Plan08 p02 (`fe7791d3`), before Plan08 p03.
- r1: user decision U1 (2026-11-22) — the call's child runs the destination binding as
  configured, with its own `run_as`; the caller is recorded as requester. r2: U2 — the gate is the
  calling agent's write right on the desk; U3/U4 — child ceiling follows plan 11 03C's formula, allowance level 0
  until plan 11. See
  notice AWC-C001.
- r15 (2026-11-23): user decision U5 — a call's destination reply on the desk thread is published by the
  existing final-reply publisher (its own transaction, exactly once), not written in the call-settling
  transaction; doc 04 C amended (gate AWC-P04-D1 r2 D6).
- r21 (2026-11-23): user decision U6. A calling agent finds callable desks in the `channel.call` tool's own
  help, prepared per turn. It lists the desks this agent passes the write check on (check 3): id, name, intake,
  capped. `list_company_agents` stays builder-only. Doc 05 A.4 and gate AWC-P01-D1 r3 D3 "Discovery" are
  superseded on where discovery lives; their filtering rule (check 3, real desks only, no inboxes) stands.
- r21: user decision U7. The Opus substitute reviewer (B') reviews all of p05, since the Fable B is rate-limited.
- r22 (2026-11-23): user decision U8, which narrows U6. The desk list holds only public desks, never members-only
  ones even with a grant. It is omitted when the calling run answers an external correspondent; a call subtree
  counts as internal. Gate AWC-P05-D1 r2.
- r23 (2026-11-23): p05 accepted in phase (B1 `22fdcacd…ba13`; B2 `905b73f8…8d5a`, B' PASS AWC-P05-B2-R2-N1).
  AWC-C002 and AWC-C005 are satisfied for p05.
- r24 (2026-11-23): p06 started (root `1e84a7af`). B' continues as reviewer.
- r25 (2026-11-23): user decision U9 ("continue and use fable for review"). A Fable/medium reviewer reviews p06,
  superseding r24's B'. p06 Batch A (tests G1–G8) is frozen for review AWC-P06-A-R1 (digest in p06 TASK.md).
- r26 (2026-11-24): p06 accepted in phase.
  - Batch A accepted at `c7fba340…7431`.
  - The Batch B final gate was reviewed: Fable PASS AWC-P06-B-R2-N1 at `bc8b9841…764f`.
  - A desk-wording regression from p05 was fixed: the run page's "Desk calls" heading, and the authoring label
    "Desk id for the call tool".
  - AWC-C005 is satisfied for p06.
  - The plan is not declared complete: several gates are unavailable on this machine, and one intermittent
    gap remains (doc 06 completion record).

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | Claude Code `1e84a7af` (accepted in phase 2026-11-22, digest `2ebbc0cd…feaa`, uncommitted) | /Users/mac02/orca/workspaces/openbot/plan08_ph1 | phases/p01/docs/TASK.md; phases/p01/docs/HANDOFF.md |
| p02 | Claude Code `1e84a7af` (accepted in phase 2026-11-22, digest `0639c0f0…2112`, B PASS AWC-P02-B1-R2-N1, uncommitted) | /Users/mac02/orca/workspaces/openbot/plan08_ph1 | phases/p02/docs/TASK.md; phases/p02/docs/HANDOFF.md |
| p03 | Claude Code `1e84a7af` (accepted in phase 2026-11-23, digest `b9271b7d…6dbb`, B PASS AWC-P03-B1-R1-N1, uncommitted) | /Users/mac02/orca/workspaces/openbot/plan08_ph1 | phases/p03/docs/TASK.md; phases/p03/docs/HANDOFF.md |
| p04 | Claude Code `1e84a7af` (gate AWC-P04-D1 r2 PASSED 2026-11-23; accepted in phase 2026-11-23 at digest `46e25d95…995c` (B1 `7b3f300c…8a13`, B' PASS AWC-P04-B1-R3-N1; B2 B' PASS AWC-P04-B2-R1-N1), uncommitted) | /Users/mac02/orca/workspaces/openbot/plan08_ph1 | phases/p04/docs/TASK.md; phases/p04/docs/HANDOFF.md |
| p05 | Claude Code `1e84a7af` (accepted in phase 2026-11-23 at digest `905b73f8…8d5a` (B1 `22fdcacd…ba13`, B' PASS AWC-P05-B1-R2-N1; B2 B' PASS AWC-P05-B2-R2-N1), uncommitted) | /Users/mac02/orca/workspaces/openbot/plan08_ph1 | phases/p05/docs/TASK.md; phases/p05/docs/HANDOFF.md |
| p06 | Claude Code `1e84a7af` (accepted in phase 2026-11-24 at digest `bc8b9841…764f` (A `c7fba340…7431`, Fable PASS AWC-P06-A-R2-N1; B final gate, Fable PASS AWC-P06-B-R2-N1); not declared complete: unavailable gates plus one intermittent gap, see doc 06 completion record; uncommitted) | /Users/mac02/orca/workspaces/openbot/plan08_ph1 | phases/p06/docs/TASK.md; phases/p06/docs/HANDOFF.md |

## Cross-phase notices

### AWC-C001 / r4
- State: active (r4 2026-11-22: gate r3 passed; names the replaced plan items. r3: gate revision r3, user decisions U2–U4; r1/r2 superseded)
- Origin: p01, user decision U1; `phases/p01/docs/logs/AWC-P01-D1-r3-decision.md`
- Targets: p01–p06
- Instruction/change: the authority model is gate decision AWC-P01-D1 (D1.1–D1.6). Where plan
  text says the child runs as the caller's actor or requires company operations authority, D1
  replaces it. D2 replaces 01 A.3's envelope (closure is the result kind) and D3 replaces 01 C.5's
  `reply` step (final output `/fields`; `channel.deny`, `channel.hand_off`). p04 refuses a
  `channel.hand_off` whose nested call did not close `done`. p03 publishes a Plan08 notice when it
  extends Plan08 p02's cancel-and-archive to call children.
- Dependencies/order: effective once AWC-P01-D1 passes review; blocks authority, launch and
  scope work in every phase.
- Evidence/review: AWC-P01-D1 r1 not passed (note AWC-P01-D1-R1-N1); r2 superseded by r3 during review; r3 PASSED (AWC-P01-D1-R3-N1)
- Verification: the D1 verification plan table, per phase
- Responses: p01 -> satisfied r4 (accepted in phase); p02 -> satisfied r4 (accepted in phase, r11); p03 -> satisfied r13 (Plan08 notice published, 08G coordination r6)

### AWC-C002 / r1
- State: active
- Origin: p01 batch AWC-P01-B1, review AWC-P01-B1-R1-N1 (F2, F4, F9); `phases/p01/docs/logs/AWC-P01-D1-r3-addendum-A1.md`
- Targets: p03, p04, p05
- Instruction/change: phase ownership moved by p01's implementation.
  - p04 owns the `channel.deny` and `channel.hand_off` steps (compile and runtime), result-field
    validation and projection, and `hand_off` refusal of a nested call not closed `done`.
  - p05 A owns the authoring catalogue entry and directory discovery: real channel desks only,
    never principal inboxes (F9).
  - p03 resolves a replayed call from its invocation record before re-parsing its deadline.
  - `MessageTriggerInput::schema()` declares no `fields`; a responder declares its intake in its
    own input schema.
- Dependencies/order: effective now; each target acknowledges at its startup.
- Evidence/review: B accepted the placements in AWC-P01-B1-R1-N1.
- Verification: each target's focused checks named in the addendum.
- Responses: p03 -> satisfied r13 (replay resolves before the full parse; R2 replays after the call's own deadline); p04 -> acknowledged r14 (deny/hand_off in gate AWC-P04-D1 D2); p04 -> satisfied r19 (p04 accepted); p05 -> acknowledged r20 (phase startup); p05 -> satisfied r23 (catalogue entry and public-desk discovery, B1)

### AWC-C003 / r2
- State: active (r2 2026-11-22: gate AWC-P02-D1 r3 passed; r1 superseded)
- Origin: p02, gate `phases/p02/docs/logs/AWC-P02-D1-r3-decision.md` (delta on r2: D3, D4, owner table), reviews AWC-P02-D1-R1-N1 (F3), R2-N1 (F6), R3-N1 (pass)
- Targets: p03
- Instruction/change:
  - p02 takes 03 A.1, the typed child identity in `children::create_on`. It covers the call
    child's actor, association, knowledge levels, dependency scope, unit scopes and invocation
    record, plus the admission checks of AWC-P01-D1 D1.1 steps 3–8 except tree limits: intake,
    levels, destination admission, and the role and unit-cap refusals under a savepoint.
  - p03 owns:
    - the port `call_channel(fence, call)` and its dispatch transaction (outer timeout 10 s);
    - the agent runtime's handling of `Conflict` and timeouts;
    - the tree-limit mapping to `workflow.tool_limit`;
    - the destination thread and source message;
    - the launch guard tying `workflow_channel_calls` to the `child_run` wait and
      `ChildCause::ToolCall`. It cannot be a plain FK, because workflow-tool waits share those
      columns; use a trigger, or a discriminator column with a unique key;
    - the call case of `knowledge::destination_level_on`;
    - the park, deadline, cycle re-check at launch, history session, replay, resume,
      cancellation and liveness.
  - p03 proves these, which p02 leaves NOT EXERCISED:
    - fence convergence of two claimants;
    - replay through dispatch without re-reading settings;
    - changed arguments on a real saved call.
- Dependencies/order: p03 builds on p02's accepted functions.
- Evidence/review: gate AWC-P02-D1 r3 PASSED (AWC-P02-D1-R3-N1).
- Verification: p03 `channel_call_launch_` and `channel_call_resume_`.
- Responses: p02 -> satisfied in gate; p03 -> satisfied r13 (L8, R2; accepted AWC-P03-B1-R1-N1)

### AWC-C004 / r2
- State: active, effective (r16 2026-11-23: r2 narrows `response_invalid` per gate AWC-P04-D1 r2 D3; r13: p03 accepted; r12: published)
- Origin: p03, gate `phases/p03/docs/logs/AWC-P03-D1-r2-decision.md` (review AWC-P03-D1-R2-N1)
- Targets: p04
- Instruction/change:
  - `channel_call.response_invalid` is reserved. r2: it is a defect guard only (stored content that does not
    decode); an invalid projection fails inside the child at its terminal, and the caller reads `child_failed`
    (gate AWC-P04-D1 r2 D3). p03's interim success arm is replaced by p04.
  - p03 declines a call to a desk whose reply settings carry an approval policy (denied closure
    `destination_unavailable`, at the end of D1.1 step 8). p04 removes that rule when it adds review.
  - The record's thread and source-message keys are `NO ACTION`; p04 B.6 owns the remaining retention races.
- Dependencies/order: effective when p03 B2 is accepted.
- Evidence/review: AWC-P03-D1-R2-N1; implemented and accepted under AWC-P03-B1-R1-N1 (L10, R3).
- Verification: p04 `channel_call_response_` and `channel_call_review_`.
- Responses: p04 -> acknowledged r14 (gate AWC-P04-D1 r1: interim rule removed in D4; response_invalid narrowed in D3, see gate; NO ACTION keys in D5)

### AWC-C005 / r1
- State: active, effective (r18 2026-11-23: p04 B1 accepted; r17: published)
- Origin: p04 batch B1 (`phases/p04/docs/TASK.md`, "Fixes found by the new tests")
- Targets: p02 (owner of the call child's scope capture), p05, p06
- Instruction/change:
  - A call child is a fresh personal scope. `memory_tool_checkpoint::user_scope_owner_on` no longer climbs past
    a run that is a `workflow_channel_calls.child_run_id`. Before, a destination run as the caller's own user
    inherited the caller's private-session user scope, and the scope seal refused it. This tightens p02's
    AWC-P02-D1 D1.2 ("fresh scope") rather than changing it; p02's accepted evidence is unaffected, because
    no p02 test ran a destination as the caller's user.
  - The final-reply publisher accepts call owners: `channel_reply_publish_authority::lock_run_on` requires
    `(run.parent_run_id IS NULL) = (owner_kind = 'ingress')` instead of a root run (U5).
- Dependencies/order: effective with p04 B1 acceptance.
- Evidence/review: p04 A9 `channel_call_response_agent_destination_prepares`, A2/D6
  `channel_call_response_reply_note_published`; B review of p04 B1 (pending).
- Verification: p06's final gate covers the agent-history/memory suites (`memory_scope`, `agent_history`, `run_scope`).
- Responses: p05 -> acknowledged r20 (views must not label a child scope as the caller's); p05 -> satisfied r23 (call child provenance names its own scope, V5); p06 -> satisfied r26 (full suite: `memory_scope` 80/80, `run_scope` 4/4, `agent_history` 158/159; the failure is the pre-existing version-7 test. Gap: `agent_history_e2e_process_crash_before_successor…` failed once under 2 MiB load, passed in every other run, kept unresolved)
