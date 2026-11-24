# AWC-P03 — Child execution and agent continuation

## Identity

- Plan `plan/agent-workflow-call`, phase `p03`, doc [03-child-execution-and-agent-continuation.md](../../../03-child-execution-and-agent-continuation.md).
- Workflow implement-opus-reviewed-2_4; records at
  `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/agent-workflow-call/phases/p03/docs`.
- Root/A: Claude Code `1e84a7af`, Opus 5.5/xhigh (user-confirmed). B: Fable/medium `a8d0fa1fbd0b8b88e` (continued).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD `fe7791d3` (start).
  Preserved pre-existing work: p01 and p02 accepted, uncommitted.
- Prerequisites: p01 accepted at `2ebbc0cd…feaa`; p02 accepted at `0639c0f0…2112` (B PASS AWC-P02-B1-R2-N1).
- Coordination `plan/agent-workflow-call/COORDINATION.md` r11. Applicable:
  - AWC-C001 r4 (authority model; p03 publishes a Plan08 notice when it extends cancel-and-archive to call children);
  - AWC-C002 r1 (p03 resolves a replayed call from its invocation record before re-parsing its deadline);
  - AWC-C003 r2 (p03 owns the port, dispatch, launch guard, thread/source, levels, park/resume and the p02 NOT
    EXERCISED items).
- Resources: scratch DBs `openbot_awc_mac02`, `openbot_awc_mac02_test` (localhost:5432).
- Snapshot exclusions: `plan/agent-workflow-call/phases/p03/docs`, `.trail/learnings`.

## Status

- Gate AWC-P03-D1 r1 not passed (AWC-P03-D1-R1-N1: F1, F2 medium; F3–F5 low; F6 optional).
- **Gate AWC-P03-D1 r2 PASSED** (AWC-P03-D1-R2-N1; decision sha256 `516b08b3…1fe3`, a delta on r1).
- 2026-11-23: batch AWC-P03-B1 (D1–D7, merged B1+B2) implemented and verified (see Checks); frozen for review R1.
- **2026-11-23: AWC-P03-B1-R1 PASS** (B note AWC-P03-B1-R1-N1) for digest `b9271b7d…6dbb` (recomputed by root,
  unchanged). F1 (low, non-blocking): disposition recorded in the R4 mapping row (stand-ins declared, production lock
  order by inspection); records only, digest unchanged. F2 optional: gaps accepted as declared. Evidence manifest
  `logs/AWC-P03-B1-R1.evidence.json` validated (9 PASS, 3 FAIL explained, 2 NOT EXERCISED with reasons).
- **p03 accepted in phase 2026-11-23** at `b9271b7d…6dbb`, not integrated, uncommitted. Coordination r13: AWC-C004
  effective; Plan08 notice published (08G coordination r6). Unverified: the full suite (final gate in p06); the
  stack before-measurement; per-desk resources (L6).
- 2026-11-22: gate AWC-P03-D1 r1 (`logs/AWC-P03-D1-r1-decision.md`) was sent to B. Pre-gate digest in
  `logs/AWC-P03-D1-r1-snapshot.txt`. Batches planned: B1 launch (D1–D3, D7), B2 resume and lifecycle (D4–D6).

## Investigation notes

- Runtime routing: `ToolInvocation::ChannelCall` is a placeholder answering `target_unavailable`
  (`src/adapters/workflow_agent/mod.rs:238`). `WorkflowToolDispatch` (`src/application/workflow/tools.rs:88`) is
  the port to mirror.
- Workflow-tool waits are reusable: `children::write_on` writes a `child_run` wait with `child_tool_call_id`;
  `workflow_tool_calls::{park_on, continue_call, replayed_call_on, confirm_suspension}` park, continue and replay.
  `call_result` validates output against the parent's manifest pin, which a call child does not share: the call
  result needs its own reader keyed on `workflow_channel_calls`.
- Terminal facts (`workflow_run_events`) carry only `terminal_state`; deadline expiry is `failed`, so per p01 F6
  no `workflow.child_deadline` code is added.
- `ChannelReplyContract::project(output_schema, result)` (`channel_reply/contract.rs:76`) is pure.
- `knowledge::destination_level_on` (`knowledge.rs:214`) has one caller (`memory_steps::read_ceiling_for`).
- Cancel-and-archive (`department/archive.rs:131`) counts every unfinished run on the subtree's desks as open work
  but cancels only roots; an open call child therefore makes the archive non-cancellable today.
  `controls::cancel_for_archive_on` cancels a tree from a given run; a cancelled child's terminal fact wakes its
  parent through the wakeup subsystem.

## Batches (decision recorded 2026-11-23)

- **B1 and B2 are reviewed as one batch, `AWC-P03-B1`.** The D4 consumption is part of the dispatch
  port: `dispatch_on` either starts, answers or consumes in one transaction. A B1 review without it
  would cover a port with an unreviewed third arm, and B2's D5/D6 code is small. One review of D1–D7
  with L1–L12 and R1–R9 is cheaper and keeps the stable contract (the port) whole. The r1
  `## Batches` split is replaced by this record; the gated decisions are unchanged.

## Implementation (AWC-P03-B1)

| Decision | Code |
| --- | --- |
| D1 port and order | `application/workflow/channel_call.rs` (`ChannelCallDispatch`, `ChannelCallToolCall`, `ChannelCallRequest::desk`, `RESPONSE_INVALID`); `adapters/workflow_agent/mod.rs` (`ToolInvocation::ChannelCall`, `with_channel_calls`); `infra/workflow.rs`; `persistence/workflow/channel_call_dispatch.rs` (`call_channel`, `dispatch_on`); `channel_call_target.rs::start_on` (steps 0–8, `CallStart::Stale`); `agent_origin.rs::channel_call_on` |
| D2 thread and source | `channel_call_source.rs::store_on`; `children.rs::write_call_on` (source first, `Thread` association, `trigger_input_on`, `capture_for`); `admission_history.rs::capture_for`; schema: `workflow_channel_calls.thread_id/source_message_id`, `UNIQUE(company_id,source_message_id)`, `NO ACTION` FKs, snapshot trigger `thread_id`, producer-seal and author-valid arms |
| D3 launch guard | schema `check_workflow_channel_call_launch` / `workflow_channel_call_launch` |
| D4 consumption | `channel_call_dispatch.rs::{consume_on, still_authorized_on, project_on, committed_output_on}`; `workflow_tool_calls.rs::continue_call` (a recorded call child is continued with `Deliver(Null)`, never read through the workflow-tool `call_result`) |
| D5 audience | `knowledge.rs::destination_level_on` call case, `call_ceiling_on`, `thread_readers_on`/`human_readers` |
| D6 archive | `department/archive.rs::archive_department_on`: roots, then recorded call children on the subtree's desks whose caller's desk is outside it; one bound |
| D7 stack | `call_channel` boxes `dispatch_on`; `start_on`/child writer stay boxed |

Corrections made during implementation (no gated decision changed):

- **Retry scope.** `call_channel` first retried every `AppError::Conflict`. A refused park
  (`persistence_bound::refused`) is also a `Conflict`, so a lost lease would have been re-dispatched three
  times and reported as "configuration kept changing". The settings race now returns `CallStart::Stale`
  and only that is retried (D1 "Conflict" / failure-mode table "Settings race"); L9 asserts the park
  refusal is returned at once.
- **Shared fixtures.** `CallSetup` and its helpers moved from `channel_call_snapshot_tests.rs` to
  `channel_call_support.rs`; p02's tests are unchanged apart from imports, V11 split (below) and the
  `Stale` arm in V5.
- **V11 split.** `channel_call_target_facts_decide_before_any_row` claimed four jobs at once; a company runs
  two jobs by default (`WorkflowCapacityPolicy::default`, company 2) and an answered call keeps its job running,
  so the fourth claim was `None`. The desk-state cases moved to
  `channel_call_target_facts_desk_states_decide_before_any_row`; every assertion is kept.
- **Test runtime.** `ToolSetup::runtime` composes `with_channel_calls` as `infra::workflow` does.
- **Error list.** `channel_call.response_invalid` added to `01-contracts-and-authority.md` item 4. The
  README carries no error list; p01's decision log is p01's accepted record and is not edited (the r2
  decision and AWC-C004 carry the addition).

## Verification mapping (AWC-P03-B1)

Tests are in `src/adapters/persistence/workflow/channel_call_{launch,resume}_tests.rs`. Every dispatch
goes through the production port `PostgresPersistence::call_channel`, using a call the real
`WorkflowAgentRuntime` saved, except where a row says otherwise.

| Row | Test | Decisive assertion | Stand-ins / not proved |
| --- | --- | --- | --- |
| L1 | `channel_call_launch_parks_and_commits_atomically` | `Parked`; caller waiting / job completed / `child_parked`; the wait names the record's call and child; one desk thread and one message (agent author = calling agent, role agent, inbound, delegation, caller's level and exactness, sealed to the caller); child on that thread with the caller's deadline; zero intents, deliveries, reply contexts, external messages | — |
| L2 | `channel_call_launch_refusal_leaves_no_thread_or_message` | role refusal and desk-unit cap: typed answer, zero desk threads and messages, zero records and children, fence still live | — |
| L3 | `channel_call_launch_origin_is_proven` | another agent, an unsaved call ID, other arguments than saved, the workflow-tool binding: each `Err` (not a tool result) with zero rows; the genuine call then parks | — |
| L4 | `channel_call_launch_continuation_limit` | unwritable desk answers `target_unavailable` first; then continuation 63 answers `workflow.tool_limit`; zero rows | continuation 63 set by a raw update (triggers bypassed), as `workflow_tool_child_failure_limit_and_result_bound_are_typed_errors` does |
| L5 | `channel_call_launch_guard_names_the_wait` | a wait naming another call ID: `workflow_channel_call_launch`; untampered passes; `store_on` without a record: producer-seal refusal | the seal raises without a constraint name, so the message text is asserted |
| L6 | `channel_call_launch_two_targets_differ` | two desks, two versions, params `{}` / `{"tier":"gold"}`: each child carries its own desk, binding, version and params; callers on no channel | resources NOT EXERCISED: neither destination declares one; resource capture is p02's binding capture (V0, V7–V10) |
| L7 | `channel_call_launch_history_is_the_fresh_thread` | `context.history` destination; with an earlier call's thread on the desk, the child's snapshot is exactly its own source message, eligible 1 | — |
| L8 | `channel_call_launch_competing_claimants_converge` | two `call_channel` of one saved call: one `Parked`, one `OwnershipLost`; one record, child, thread (AWC-C003 item 1) | — |
| L9 | `channel_call_launch_lease_expired_at_commit` | lease expires while the child insert waits: `Conflict` "park refused" returned once (no retry); zero rows; caller running/processing; retired job pending and retryable | — |
| L10 | `channel_call_launch_review_desk_declines` | approval policy on the desk: denied closure `destination_unavailable`, zero rows | — |
| L11 | `channel_call_launch_malformed_desk_answers_first` | company row held `FOR UPDATE`: a non-UUID `channel_id` answers `invalid_input` within 5 s | — |
| L12 | `channel_call_launch_shorter_deadline_fails_child` | child deadline = the call's, before the caller's; expiry fails the child; the replay answers `workflow.child_failed` in the same execution | deviation: the gate named `workflow_child_timeout_fails_child_and_reaches_parent_error_route`; its `AdmissionFixture` has no agent or desk, so the case is a channel-call test |
| R1 | `channel_call_resume_scripted_agent_continues` | scripted agent: park, child succeeds, wakeup, the resumed attempt saves `done` with the projected content, second model turn, same execution, continuation 1, one child | — |
| R2 | `channel_call_resume_replay_never_reselects` | another responder selected while waiting, and the call's own deadline passed: two replays both give the original child's `done`; one record and child; the saved call ID with other arguments gives `invalid_input` (AWC-C002, AWC-C003 items 2–3) | — |
| R3 | `channel_call_resume_outcomes` | succeeded: `done`, caller absorbs level 2 and inexactness; failed (output breaks its schema): `child_failed`; cancelled: `child_cancelled`; unprojectable: `response_invalid`; 140 KB fields: `tool_result_too_large`; no absorption in any error case | child facts raised by a raw update (ceiling PUBLIC until request rules, U4); unprojectable output by a raw corruption (`channel.no_reply` arrives in p04); deadline expiry is L12 |
| R4 | `channel_call_resume_revoked`, `channel_call_resume_revocation_races_consumption` | grant removed, desk disabled, binding deactivated, agent republished without `channel.call`: `revoked`, no content, no absorption, caller running; each writer raced against a consumption within 20 s gives `done` or `revoked` | grant removal and desk disable are raw single statements (F1): they prove the consumption's share locks serialize with a row writer, not the production writers' lock order. That order is by inspection: channel edit `write_channel_settings_on` takes users, company ownership, the gate, then the channel row (`channel_edit.rs:112-153,199`); grant sync takes `companies FOR NO KEY UPDATE` (`department_desks.rs:38`), principals (`:150`), `channels … FOR UPDATE` (`:264`), then deletes grants (`:179`). The consumption takes the account union and the company row first (D1 step 1), so every production writer serializes with it on the company row before any desk or grant row; no cycle. Binding deactivation and the agent republish race through their production services. |
| R5 | `channel_call_resume_competing_resumers` | two `resume_wait`: one `Continued`; no waiting wait; continuation 1; the runtime then saves one invocation | — |
| R6 | `channel_call_resume_cancel_races` | cancel racing the launch: caller cancelled, no live child; cancel racing the child's completion: caller stays cancelled, not claimable, no waiting wait | — |
| R7 | `channel_call_resume_archive_cancels_call_child` | cancel-and-archive of the desk's unit (caller outside it): the call child is cancelled and the replay gives `workflow.child_cancelled` | without D6 the archive refuses with open work |
| R8 | `channel_call_resume_destination_audience` | agent-only thread: PUBLIC at ceiling 0, 2 at ceiling 2 (without the call case it would be PUBLIC); a level-1 human reader lowers it to 1 | raised ceiling by a raw update (U4) |
| R9 | `channel_call_resume_restart_at_each_boundary` | fresh `PostgresPersistence` after the dispatch commit (no work), after the child terminal (`Wait`), after the wakeup (`Job`); the replay's result lost with the worker (lease expired, retired); the retried attempt replays and finishes; one child, one record, one saved result, same execution, continuation 1, no waiting wait | process death is simulated by a new persistence on the same pool and an expired lease; the retry backoff is cleared by a raw update |

## Checks (AWC-P03-B1)

| Check | Result | Log |
| --- | --- | --- |
| `cargo test --offline --lib channel_call_launch -- --test-threads 4` | 12 passed (try1: L9 asserted an immediate reclaim; corrected to the retry state) | `AWC-P03-B1-launch-try1.txt` |
| `cargo test --offline --lib channel_call_resume -- --test-threads 4` | 10 passed | `AWC-P03-B2-resume-try1.txt` |
| `cargo test --offline --lib channel_call_ -- --test-threads 4` (p02 + unit, after the V11 split) | 39 passed | `AWC-P03-B1-try4.txt` |
| affected: `cargo test --offline --lib -- channel_call_ workflow_tool_ workflow_child message_thread_ channel_reply_ unit_archive_ knowledge_ producer --test-threads 4` | 710 passed, 1 ignored | `AWC-P03-B1-affected.txt` |
| callers of `capture_for` and `destination_level_on`: `cargo test --offline --lib -- admission_ workflow_context_history agent_history memory_ department_ --test-threads 4` | 1604 passed, 1 failed, 9 ignored. The failure is `doop_agent_current_preparation_reaches_ready_before_model_io` (expects preparation format 7, got 8): unrelated, recorded by p02 (`PREPARED_AGENT_VERSION` is 8 since `2edd0a73`) | `AWC-P03-B1-callers.txt` |
| `scripts/stack-budget.sh --offline -- channel_call_ workflow_tool_ --test-threads 4` (2048 KiB) | 93 passed | `AWC-P03-B1-stack-budget.txt` |
| `scripts/stack-frames.sh target/debug/open_bots 32` | `channel_call_target::start_on` 38 KiB is the chain's only frame ≥ 32 KiB; `write_call_on` 21, `dispatch_on` 17, `consume_on` 14 | `AWC-P03-B1-stack-frames.txt` |
| stack "before" measurement | NOT EXERCISED: the pre-p03 state is uncommitted, and a second build needs a ~100 GB target with 76 GB free | — |
| `cargo sqlx prepare --check -- --all-targets` | exit 0 | `AWC-P03-B1-sqlx-check.txt` |
| `cargo clippy --offline --locked --all-targets -- -D warnings` | exit 101: the same 9 pre-existing `redundant_field_names`, none in p03 files | `AWC-P03-B1-clippy.txt` |
| `cargo fmt --all -- --check && git diff --check` | exit 0 | `AWC-P03-B1-fmt.txt` |
| `cargo test --locked --offline --test source_modules` | 8 passed | `AWC-P03-B1-source-modules.txt` |

Environment: scratch DB `openbot_awc_mac02` (tests use `openbot_awc_mac02_test`), migrated from the edited init
schema. Earlier tries `AWC-P03-B1-try1..3.txt` ran without `DATABASE_URL` or before the V11 split; they are not
evidence.
