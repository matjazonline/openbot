# 01.6 replacement inventory

Original01 item6 and phase acceptance govern. Astra `/root/astra_01_6`,session `01a0de2c-5e72-71b1-a9d3-0859bcf6af93`. Documentation only: create REPLACEMENT-MAP.md beside plans, link README. Preserve source/prior work. User stops after01, no02 implementation.

Map current code to future ownership, not shipped integration. No removals now; phase08 gates. Exhaustively discover all8 families via graft grep/callers, narrow capped outputs; skeleton/exact spans for source. Include production entry/callers/persistence, UI projections, setup/seed/config and invariant tests. Each retained behavior gets ONE owner from01 table; split mixed behaviors. Row: exact symbols/spans, caller/projection, invariant, owner/port/responsibility, implementing phase, removal gate. Separate production/tests. Add removal checklist and reproducible discovery ledger; acknowledge graph gaps/caps, no ranked-output completeness claims.

## Discovery anchors (verify precise spans)

- Dispatch: execute_claimed_agent_task_and_dispatch thread/dispatch.rs346–362, TaskWorker::run_task task_worker.rs954–1056. Trace ingress/admission/reload/run_agents/commit_dispatch/message/delivery. Independent binding runs replace combined answers; ingress protections stay infrastructure.
- Scheduled: execute_claimed_scheduled_agent_task_and_dispatch dispatch.rs366–372, same worker; ScheduleUseCases trigger_schedule_now/execute_schedule_trigger/execute_schedule_run/process_due_schedules schedule.rs454–766. Trace routes/projections/SCHEDULED_AGENT_RUN_TASK/ScheduledRunPayload, preserve slot dedup.
- Response review: ResponseReviewUseCases response_review.rs424, persistence367–421; ui_response_reviews routes; hidden thread_handoffs send_draft331–377/send_edited_draft392–493. Include settings/edit/publication/mailbox/pages. Split decision/artifact/action/delivery owners.
- Approval: ApprovalUseCases approval.rs107, AgentApprovalHandler services/harness/approvals.rs107, open_assigned_approval/approval_link_handler/list_channel_approvals_handler. Checkpoints/links/notifications/pending projections; approval before effect and current access.
- Outreach: literal outreach_and_await_quorum / OUTREACH_TOOL_ID; tool call outreach_tool.rs162–236, native_tools.rs121–159. Reply matching/timeout/quorum/persistence/control; ui_tasks::control_delegation581–626,ui::control_visible_task_delegation1971–2063,ui_team::redirect_delegated_ask632–669. Child workflow for local delegation, send+single response wait for conversation; quorum removed.
- Agent-channel: create_addressable_agent_with agent.rs631–668, create_addressable_agent,UI create_agent_with_channel621–666; ChannelUseCases::create_channel_with_agent,ui_channels::create_channel276–341; CREATE_AGENT_CHANNEL_TOOL_ID/CreateAgentChannelTool::call250–271/AgentChannelProvisioning. Onboarding/API/ownership/native catalogue/builder seed. Agent save only agent; explicit setup creates binding. Resource persistence stays infrastructure.
- Executable skills: SkillInstruction; ai_agents/compile.rs compile_step196–217; Rig skills.rs validate_templates199–219. Trace loader/compiler/execution/tool host/authoring/library/capability/storage/examples. Keep instruction skills/frozen selections; procedures become workflows/actions. execute_skill has no indexed hit, don't invent symbol.
- Alternate harness: HarnessKind/HarnessConfig/deployment_registry adapters/harness/mod.rs20–43, runner/default_agent_harness/UI/API/onboarding/persistence. Narrow broad capped HarnessKind search by subtree. Remove ai-agents config/dependency when Rig gates pass, no selector compatibility.

Cross-cutting: THREAD_TASK_LOOKUP_SQL persistence/thread/views.rs177–197; schedule projections list_schedule_runs577–614/schedule_run_contains_thread616–641; simulation routes/channel.rs simulate_channel_handler1051–1223,simulation_stream1233–1301,load_simulation_thread_fragment967–1030. Include task detail/status, mailbox/attention, notifications and bootstrap/config. Projections not competing state owners.

Owners: Workflow/version definitions/frozen bundle; Binding/revision admission/config; Run independent input/lifecycle/budgets/parent; Step committed resolved input/output/route; Job/attempt leases/retry; Agent checkpoint model continuation; Action authorized operation/idempotency/receipt; Human decision assignment/artifact/submission/audit; Delivery provider attempts/receipts. Auth/credentials/canonical message/resource storage remain infrastructure, not invented orchestration owner.

Removal gates:02–07 replacement contracts,08–09 retargeted interfaces/projections, retained auth/recovery/delivery tests,10 matrix. No dual executor/shims/reset/source edits. Verify references/coverage/whitespace only; behavioral tests unnecessary for docs. No blockers/resources/cleanup. Parent owns progress/brief.

## Review1 required corrections

REPLACEMENT-MAP:
1. Lines68/85: Step owns durable wait/result, Run lifecycle/deadlines. Frozen skill selection version/step-input, checkpoint conversation/continuation copies. Qualify67: run-local delegation child, conversation-level internal communication explicit messages.
2. Add hidden preparation/memory rows: context/recall/classification results Step, explicit memory writes Action; phase06 replaces automatic save drafts/feedback. Evidence dispatch.rs480–499,991–1010,1143–1163.
3. Line85 wrong skills span agent_runner/mod.rs443–512 is tool_host. Actual resolve_agent_capabilities params.rs238–318, snapshot111–195. Line53 trait response_review.rs417–421 not concrete publication; use adapters/persistence/response_review/commands.rs approve_on267–359 atomic canonical reply/delivery/review/task/handoff. Separate authorized publication Action vs transport Delivery.
4. Simulation currently can send: routes/channel.rs delivery_for1327–1334 Run->Send,RunTest->InAppOnly. Lines36–37 must reflect both current modes/future explicit policies, no blanket isolated/no-send claim.
5. Opening/ledger106–109: dependent UI/API retarget BEFORE phase08 deletion,09 completes journey. No phase08/09 ordering contradiction.
Docs only, exact evidence/reference/ownership/whitespace checks; no behavior tests. Broad8family coverage otherwise present.
