# Next actual parent/private-child/other-root contention test

No implementation of this test or ToolSetup refactor yet. Root read actual helper
spans through Graft; do not redo broad discovery. Approved D1/r2 requires this before
whole P03 code review. Original recovery-work.md remaining duties still apply.

Existing actual fixtures:
- workflow_tool_support.rs ToolSetup164–172 fields; new177–248 builds Fixture,
  reviewer, publishes child (currently plain Fixture.publish_workflow), registers
  refund.review, creates assistantA granted it, stores SCRIPTED_MODEL/model key,
  publishes root agent and parent + binding. persistence250–252, claim255–262,
  admit265–290 (raw AdmitWorkflowRequest, manual root, currentlyhistory_sessionNone),
  cancel323–343, child_of423–440; runtime356–378 supports actual workflow tool.
- workflow_tool_tests.rs409–474 actual parent tool child park/cancel/restart test;
  use workflow_tool_support::*; imports actual tools and scripted_llm. New test
  should be sibling module under this file so pub(super) helper scope works.
- ToolSetup::new publishes child BEFORE creating rootA; existing plain publisher is
  not appropriate for a child AgentRun requiring actual AgentPublicationDirectory.

Minimal shared fixture extraction: keep new(childSource) behavior, but after its
existing Fixture/reviewer/childVersion creation call boxed async
ToolSetup::bind_child(fixture,reviewer,childVersion) for the remaining registration/
A/model/root/binding setup. pub(super), same existing behavior; no flags or changed
production port. Source new is~72lines, extraction improves shape. Graft callers
ToolSetup.new (or exhaustive grep literal ToolSetup::new) before edit; focused old
workflow_tool_parent_cancel/resumes/dispatch cases must remain green. Do not copy
ToolSetup private published fields from outside its module.

New test can create real Fixture, seed SCRIPTED_PROVIDER/MODEL connection, create
and publish tool-free leaf agentB (slugleaf) via existing AgentPersistence/Fixture
publish_agent. Source child from registry::example(agent.run), replace input_schema
and parameter_schema with decision_child() corresponding fields (question + reviewer
needed for ToolSetup registration params). Set agent literalleaf/context ref/input;
output defaultstring okay because no child provider call needed. Publish actual
child id CHILD through workflow_tool_support::publish_through and real
AgentPublicationDirectory (innerPreflight); then bind_child(fixture, owner_user_id,
childVersion). This makes rootA -> child AgentRunB graph acyclic. Same-agent tool
self-reference would be prohibited by existing autonomous cycle publication rules;
retain targeted cycle refusal cases. Accepted01 real same-agent ordinary child
private branch test also remains required. Do not invent a prohibited cyclic graph.

Claim parent, script one LlmTurn::tool_call(DECLARED,{question:q}), execute existing
setup.runtime(); actual Suspended means parentActive history pointer survives its
real workflow-tool child wait. child_of returns real scope; claim child's AgentRunB.
Admit a second genuine MANUAL root on same parent binding with same tenant/creator/
association and history_sessionSome(AgentHistorySessionRef{from_run:parent.run}).
Use exact ToolSetup.admit request fields and real WorkflowService admission, no
copied source scopes or prepared records. AdmissionResultCreated run -> fetch real
execution/job and claim actualscope (HistoryThread.claim_intent shows named SQL row
pattern). Native root input/agent model source comes from frozen published graph.

Order real contenders by observed database blockers, not timing:
1. Independent fixture tx locks this company's existing agent_history_admission row
   FOR UPDATE, obtains pg_backend_pid. It acquires no other owner afterward.
2. Spawn actual child prepare_agent. It holds own childRun/privateConversation and
   blocks at admission leaf. Observe pg_blocking_pids(currentDatabase) to get childPid.
3. Build normal parent CancelCommand using current head revision and real actor.
   Spawn actual p.cancel; observe blocked behind childPid. Native parent terminal
   history hook has now locked/released parent conversation, cancellation retains
   its lock while entering bounded descendant cleanup.
4. Spawn actual reused-root prepare_agent; observe behind cancelPid. Assert its
   active outer SQL is agent_history_admit (conversation-before-leaf); it must not
   be another request for the fixture-held leaf. No global late lock is expected.
5. Release fixture leaf; join ALL tracked futures under bounded hang guard. Child
   finishes its private preparation/source; parent cascade settles child; unrelated
   reused-root acquires parent stream and progresses. Assert parent/child cancelled,
   exact own other-root pointer, no child provider/model usage, scripted parent only1.
   Child Ready may race source cancellation after transaction; any real next CP/provider
   remains fenced. No new worker/queue/notification or per-conversation poller.

Existing test agent_history_retry_deletion_tests.rs now shows minimal actual
row-lock observation loop: within timeout3s query pg_stat_activity/current_database
and $blocker=ANY(pg_blocking_pids(pid)), sleep5ms; native locks still have normal1s
server timeout, so observations must be prompt. Gate helper provides10s hang guard
but never raise real ownership/resource limits to clear a test. Return blocker pid
plus outer SQL when needed; structs instead of tuples with same-typedfields.

This is planned test assembly, not proof or code acceptance. Keep user commentary
<=60sec. Remaining native/worker/independent/provider/no-held-connection and other
recovery/affected tests plus SQLx/lint/offline/Graft/whole review still required.
