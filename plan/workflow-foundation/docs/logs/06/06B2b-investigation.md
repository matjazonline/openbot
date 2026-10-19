# 06B2b investigation (not an architecture gate)

No backward compatibility. Research only while06B2a checks run; no dependent implementation.

Existing Rig owns persisted model reservations, saved model turns, pending calls, bounded
repair and final output. Reuse these contracts rather than inventing a parallel turn ledger.
Relevant sources: harness/runs/state.rs RunCheckpoint and Mutation; rig/execution.rs
Execution::drive/model_turn/tool; persistence/task/harness_runs.rs.

Current obstacles requiring a coherent explicit boundary:
- OpenRun and RunWrite carry legacy TaskLeaseRef. lock_task_execution_on expressly
  requires queue_kind=legacy and owner principal/version. Workflow writes require
  WorkflowFence with root/run/execution/attempt identity; never synthesize a legacy lease.
- Rig execute_claim implicitly prepares legacy MCP and compiles legacy tools. Workflow
  tools must exclusively enter phase04 ActionService, sequentially, using saved model calls.
- Agent runtime receives a frozen spec/context/capability selection; current authorization
  reloads legacy agent grants and compares fingerprints. Preserve current revocation checks
  while using published capabilities and immutable inputs, not newly edited agent defaults.
- Model reservation precedes provider I/O and settlement follows response capture. This is
  the correct transaction seam for root model-call debit, token allowance admission and
  usage projection. Provider errors retain unknown reservations; no reset on approval.
- Existing checkpoint pending-call priority before another model turn provides replay
  semantics. Workflow action-approval parks the queue attempt; checkpoint tool result must
  be recorded only after a durable action receipt. Reclaim uses the same saved call.

Potential design must address typed fence ownership, shared Rig driver/model protocol,
workflow tool bridge and wait identity, token allowance subjects/reviewer authorization,
root-budget lock order, unknown usage and partial provider counters, cancellation/deadline,
and repeated non-decision effect waits. Do not treat06-D3 acceptance as approval of these.

Additional traced facts:
- RunCheckpoint policy currently hard-bounds16 model calls,64 tools,131072 input/request,
  16384 output/request and conservative aggregate input/output reservations. These safety
  bounds must remain distinct from optional workflow token approval; approval must not
  reset them or the existing root model-call debit.
- RootBudgetLimits currently covers activation/model-call/repetition only. Token allowance
  needs its own explicit durable authorization facts rather than repurposing monetary caps.
- CompiledRun embeds the legacy ToolBridge and private preamble. Driver reuse must separate
  prepared prompt/tool definitions from legacy dispatch without allowing workflow actions
  to enter that legacy bridge.
- AgentSnapshot currently has AgentKey (string), instructions, ModelSettings, tools and skills;
  it lacks a database agent UUID. Production usage attribution must bind trusted agent
  identity when loading/freeze occurs, and preserve it across retries instead of accepting
  caller-supplied attribution or re-resolving a potentially changed key blindly.

Continuation research after06-R008 (still not an accepted design):
- Rig Execution::drive/model_turn/tool (execution.rs350–586) is the actual durable loop;
  CompiledRun has a legacy ToolBridge and dynamic preamble. Extract shared model request /
  capture / repair mechanics, but do not make workflow construct legacy task ownership or
  enter that bridge. Workflow-native store can reuse RunCheckpoint::apply(Mutation),
  revision validation and usage projection with an explicit WorkflowFence. Persistent
  checkpoint rows must bind company/run/execution/job and immutable frozen preparation.
- Model usage reserve_on currently inserts task_id/agent_id/provider/model only; schema
  attribution derives workflow identity from workflow queue tasks. Reuse that projection
  transactionally with checkpoint reserve/settle, not a second token ledger of model calls.
- Existing root budget reserve_on locks local run, checks live fence, then shared usage;
  it explicitly avoids locking the root run from descendants. New token checks must
  preserve that order and recheck lease after waiting for shared usage.
- Action approval park/submit (decision_approvals.rs46–247) carries exact wait/job/attempt
  and admin/owner reviewers. Token approval needs a distinct subject and proposed allowance,
  not a fabricated action intent. Extend the established decision authorization/notice and
  replay machinery; maintain distinct event-family discriminator and immutable audit IDs.
- MessageActions::call_tool currently maps Suspended to Pending. message_publish.rs252–358
  parks deliveries only for MessageOrigin::Step. Workflow saved model calls must retain
  real wait identity and resume the checkpoint after delivery, rather than finish the
  whole agent step or spend a failure retry. Repeated delivery waits need explicit per-
  invocation history and exact parked attempt, analogous to approval continuations.
- Frozen model-tool target is currently resolved through registered_on at every HTTP/MCP
  preparation. Save/check target identity with prepared run so registry rebinding cannot
  redirect a saved pending call. Current revocation checks still apply on receipt replay.
- Worker execute_io supervises external handlers and confirms durable suspended wait.
  New token/effect suspension must be recognized by confirmation and timeout/cancel paths;
  dropping a provider future must leave its durable reservation unresolved and charged.

New accounting constraint found before architecture submission: RunCheckpoint::commit_turn
(state.rs513–559) and validate_ledger253–332 reject reported tokens above the reservation.
protocol::captured_usage105–130 preserves reported numbers, so such a received response
currently fails before usage projection and remains merely unresolved. A draft promising
settlement-triggered token approval cannot assume that path already works. D4 must own
this explicitly: retain bounded received usage, charge effective max(reserved,actual) to
hard safety accounting, and persist a non-executable budget-failure state when an actual
hard limit is exceeded. Only optional root allowance overflow may be manually continued.
Never clamp known reported usage to the estimate or let token approval reset hard bounds.

Agent registry schema currently omits optional with.approval (families.rs35–70), while
registry/actions.rs owns its static reviewer/dynamic deadline schema and checks. Native
agent tools need that existing policy shape on agent.run, factored into one helper, so
protected actions can actually open the repeated approvals tested by06B2a. Token policy
reviewers remain root-scoped, separate from permission to perform a protected action.

Native table-extension audit: task/harness_runs.rs::persist_checkpoint_on74–91 also
projects legacy task_harness_invocations through persist_changed_invocations_on. Native
workflow writes must reuse checkpoint validation/serialization and model usage only;
never call that legacy invocation projection. Preserve every auxiliary legacy guard.
Table reader inventory saved06B2b-harness-table-uses.txt; most reads are explicitly legacy,
while final publication uses a prior legacy lease lock and principal/version predicates.
Channel-assignment dependent counts receive scoped task IDs; verify its producer remains
legacy-only when proving isolation. Native fields must not satisfy legacy owner predicates.
