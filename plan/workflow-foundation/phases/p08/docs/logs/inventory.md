# P08 discovery inventory (2026-10-22, HEAD da86871)

Condensed from three read-only Explore reports (rg/read; graft unavailable). Evidence aid only;
line numbers are at `da86871`. Schema = `migrations/20260817000000_init_schema.sql`.

## Workflow engine surfaces

- `WorkflowService::admit` `src/application/workflow/service.rs:45`: `admissible_trigger`
  (:173-183, refuses `Child`, `rerun_of` only with `Manual`) → authorization preflight →
  `WorkflowBindings::admission_binding` → `verify_binding`/`bounded_snapshots` →
  `WorkflowAdmission::admit`. `AdmitWorkflowRequest` `contracts.rs:58-71`; `AdmissionResult`
  `Created|Replayed|Conflict`. `WorkflowActor` = authenticated non-nil user
  (`authorization.rs:14-31`); persistence `authority.rs:45-71` locks the company and requires
  owner/admin.
- `TriggerSource` `src/domain/workflow/causality.rs:158-170`: `Message{message_id}`,
  `Schedule{schedule_id, occurrence_id}`, `Manual`, `Child{parent}`.
- Dedup: `SourceKey::from_trigger` `admission_binding.rs:7-38` (message/schedule keys exclude the
  trigger UUID); `workflow_admissions` PK `(company_id, binding_id, source_key)`;
  `workflow_admission_commands` PK `(company_id, command_key)`. Replay restores the saved binding
  after deactivation/archive. Concurrent duplicates serialize on the company lock.
- Message source check `admission_source.rs:68-95`: Thread association; message in that thread.
  Schedule source :97-123: `channel_schedules`/`schedule_runs` match the association.
- `create` `admission_write.rs:7-58`: run, first execution, `background_tasks` job
  (`queue_kind='workflow'`), history snapshot (Thread association), admission + command rows,
  `admitted` event. Postgres `admit` opens its own transaction (`admission.rs:10-30`).
- Bindings: `BindingConfiguration{id, revision, company_id, params, resources}`,
  `BindingState{configuration, association, revision, active}`; association
  Company|Channel|Thread; `BindingService::{configure, activate, deactivate}`
  (`lifecycle/bindings.rs`), expected-revision CAS. Tables `workflow_bindings` (schema 6765),
  `workflow_binding_revisions` (6746, has `actor_id`), `workflow_binding_events` (6728). No
  trigger/eligibility configuration; no list-by-channel query.
- Production composition: none. `WorkflowService::new`, `WorkflowWorker`, `DefinitionService`,
  `BindingService`, `ChildPublicationDirectory`, `AgentPublicationDirectory`,
  `AgentActionTools::new`, Http/Mcp/Message/WorkflowAgent runtimes, `ActionReconciliationService`
  are only built in test support (e.g. `persistence/workflow/workflow_tool_support.rs`).
  `serve` (`main.rs:187`) spawns task, memory, inbound-event, notification, delivery workers,
  SMTP, mailbox listener.
- Commands without routes: `DefinitionService::{save, validate, publish, archive}`
  (`lifecycle/definitions.rs`), `TemplateCopyService::copy`, `BindingService`, `admit`
  (`rerun_of`), `cancel`/`retry` (`service.rs:78,118`), `WorkflowInspection::head` only (no
  list/detail), decision port `submit_decision`/`add_decision_comment`/`read_decision`
  (`decisions.rs:178-181`, no service wrapper), `ActionReconciliationService::reconcile`,
  `WorkflowTools::{register, revoke, list}` (`tools.rs:55-75`), `AgentWrite.workflow_tools`
  (routes pass `None`).
- Waits: `WorkflowWaits::record_event(WorkflowEvent{company, run, id, name, correlation,
  payload})` (`waits.rs:24-40,78`) → `Recorded|Duplicate|Unmatched`; name must equal a frozen
  `wait.event` step's literal event; payload validated against the step output schema; ≤128
  unconsumed per run. No production caller; no message→run correlation exists.
- Message steps: `message.reply` requires `source_message` in run scope; publication writes
  `workflow_message_destinations` (run, invocation, kind, destination = source message id,
  outbound message id, thread, delivery) and `workflow_message_actions`.

## Legacy ingress and dispatch

- Entry points: Resend webhook → inbox (`inbound_events`) → `InboundEventWorker` →
  `ResendApiInboundDecoder` → `commit_inbound`; SendGrid and SMTP call `preflight_inbound` +
  `commit_prepared_inbound` synchronously; internal relay → `ingest_relayed_message`; UI
  (`ui.rs` create_thread/send_reply, `ui_schedules.rs:621`) via `ingest_canonical`; channel
  simulation `compose_and_ingest` (`channel.rs:1314`).
- `preflight_inbound` (`thread/ingest/mod.rs:246-305`): `guard_ingress`, system addresses,
  `resolve_addresses`, `prepare_channels`, `CommitPlan::build` — read-only. Protections there: DMARC
  for external transport, internal-source mismatch, hop limit 5, loop trace, auto-reply, spam
  threshold, sender authorization (`authorize_channel`/`participant_access`), thread injection,
  turn limit, disabled channel bounce, thread resolution (preflight).
- `commit_inbound` → `commit_on` (`persistence/thread/inbound.rs:69-140`): channel gates,
  provider key locks, redelivery recognition (content hash), thread re-resolution under lock,
  canonical message + mappings, outreach transitions, `create_tasks` (one
  `email_agent_dispatch` task per pipeline, `ON CONFLICT (company, source_message, channel)`),
  handoffs, deliveries, inbox event completion. **Atomic acceptance + enqueue today.**
- TaskWorker `run_task` (`task_worker.rs:954`): `scheduled_agent_run` → scheduled dispatch;
  anything else → inbound dispatch → `run_agents` → `AgentRunner` → `RigHarness`.
- Channel execution settings: `channel_agents` (schema 4618), `channels.owner_agent_id`, six memory
  booleans, `response_trigger` (`always|mentioned|mentioned_or_reply`), review/reply-handling
  overrides; company `external_response_review`, `external_reply_handling`, `memory_provider`;
  triggers `enforce_owned_channel_position_zero`, `enforce_enabled_channel_has_active_agent`,
  `initialize_task_ownership`.

## Other legacy owners (still present)

- Scheduled dispatch: `ScheduleUseCases` (`use_cases/schedule.rs`, 2072 lines),
  `SCHEDULED_AGENT_RUN_TASK`, `ScheduledRunPayload`, `execute_schedule_run` enqueue,
  `process_due_schedules` (TaskWorker poll), routes `schedule.rs`, `ui_schedules.rs`, pages
  `schedules.rs`; tables `channel_schedules` (4666, `run_as_user_id`), `schedule_runs` (5664).
- Outreach/quorum: creation dead, machinery live (ingest reply correlation, worker quorum
  timeouts, delegation controls routes, pages, projections); tables `task_outreaches`,
  `task_outreach_targets`, `task_outreach_replies`, `delegation_control_commands`.
- Legacy approvals: `ApprovalUseCases`, `human_approvals` (`approver_email`),
  `task_approval_waits`, routes `approval.rs`, page, attention; only creator is quorum timeout.
- Response reviews (`response_*` tables, `ResponseReviewUseCases`, `ui_response_reviews.rs`,
  `human_completion.rs`) and thread handoffs (`thread_handoff*`, `manual_handoffs`, routes).
- Agent/channel creation: `create_addressable_agent` (+ personal channel),
  `create_agent_from_library`, `create_channel_with_agent`, onboarding; Agent Builder prompt
  installed at boot.
- Harness: `HarnessConfig{Rig}`, `HarnessKind{Rig}`, `HarnessRegistry`, `AgentRunner`,
  `RigHarness`/`AgentHarness`, `LegacyBinding` (`rig/execution.rs:308-540`), `HarnessRunStore`/
  `task_harness_runs`; shared with workflow: `Execution::drive`, Rig protocol/providers/usage
  modules, `RunIdentity.harness`.
- Skills: prompt-only already.
- Memory: providers, coordinator, worker, jobs, tables, routes, settings (live when env set).
- Projections on legacy tables: `ui_tasks.rs`, `task.rs` JSON, mailbox `ui.rs`, `attention.rs`,
  `notifications.rs` + notification worker, `ui_task_counts.rs`, `ui_dashboard.rs`, task board.
- Already removed: native tool catalogue/outreach/agent-channel tools, ai-agents runtime,
  `agents.harness_kind`, `DEFAULT_AGENT_HARNESS`, `SkillInstruction::Tool`.
