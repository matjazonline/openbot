# B2 (D1.9) schedule inventory — read-only Explore pass, 2026-10-22

Source: Explore subagent report during WF-P08-R004 (B1 frozen). Line numbers as of the B1
artifact (digest 90d4e0ab…7390). M = migrations/20260817000000_init_schema.sql.

## Schema
- channel_schedules M:4692-4716. Drop: subject_template, prompt_template, delivery_mode (+ CHECK
  4711), recipient_emails, run_as_user_id (+ FK 12296). Add binding_id + composite FK
  (company_id, binding_id) → workflow_bindings PK (M:9386). Keys UNIQUE (company_id,id) 7795;
  channel FK 12280; due index 10104; trigger schedule_channel_immutable 11268 / fn 2410-2420.
- schedule_runs M:5690-5713: task_id → run_id (composite FK to workflow_runs PK M:9820 /
  UNIQUE (company_id,id,binding_id) M:9804); add refusal, command_key UNIQUE
  (company_id,schedule_id,command_key); rewrite _materialization_state_check 5710 (refusal-failed
  with attempts<5); _task_requires_thread_check 5712 and thread_* FKs (13128/13136) and
  task_scope FKs (13112/13120) are legacy; slot key (schedule_id,scheduled_for) 8610 (run-now
  scheduled_for=now must not collide). Trigger require_legacy_schedule_task 11205 / fn 2598-2648
  (needs a run_id rewrite; keeps identity immutability and snapshot scope).
- background_tasks legacy tie-ins: source_schedule_run_id 4551, single_source CHECK 4594,
  workflow_shape 4609, UNIQUE 7715, schedule idx 9992 (scheduled_agent_run), 9999, scope key 7731.

## Application / domain
- domain/entities/schedule.rs: ScheduleDeliveryMode 112, ChannelSchedule 153-182 (snapshot;
  render_* 194-248), ScheduleWrite 263-278, ScheduleRunAs/Choices/RunAsSelection 287-348,
  ScheduledRunPayload 363-397, ClaimedScheduleRun 399-406, ScheduleRun 435-452.
- use_cases/schedule.rs: port 32-90; SCHEDULED_AGENT_RUN_TASK 94; authority managed_* 142-168 (no
  channel visibility); validate_write 186-227; run-as rule 229-281 (mirrored in
  persistence/workflow/bindings.rs:165-200); ops create 283 … trigger_schedule_now 460-483 (no
  occurrence, no dedup, Unattributed); materializer execute_schedule_run 558-712 (thread +
  prompt message + legacy task + record_run_task fence); process_due_schedules 715-772; tests
  775-2072.
- Legacy consumers: services/task_worker.rs 35, 241-268, 959-972 (+ tests 2666…3345);
  use_cases/thread/dispatch.rs 35, 191-197, 348-728, 1570-1582; TaskSource::ScheduleRun
  domain/entities/task.rs:586-601; persistence/task/queue.rs:699-707;
  ensure_schedule_run_thread (use_cases/thread/mod.rs:189, persistence/thread/mod.rs:310-362);
  persistence/thread/views.rs:193.

## Persistence (adapters/persistence/schedule.rs)
create 177-226, get 228, list 240/259, update 271-324, delete 326, set_enabled 336,
claim_and_advance_due_schedules 351-474 (claim CTE + generation), record_run_task 476-511
(completion fence), record_run_error 513-543 (backoff 5/15/60/300/900 s; no lease-expiry check),
record_manual_run 545, release_failed_claim 560 (dead), clear_last_error 584,
list_schedule_runs 594-631 and schedule_run_contains_thread 633-659 (legacy task joins).

## HTTP / pages reading removed fields
routes/schedule.rs (ScheduleRequest 65-120, UiScheduleForm 145-232, handlers 234-425, tests);
routes/ui_schedules.rs (workspace 121-262, page 276/366, runs stream 442-477, thread pane 479,
reply_in_thread 546-658 — run-thread concept removed by D1.9, forms 660-860);
pages/schedules.rs (run_as 19/48/154/495-625, delivery label 234-237, thread pane 382-445, form
671-905); pages/channel_settings.rs 188-190, 431, 1095-1310; pages/agent_settings.rs 203-205,
557; routes/ui_channels.rs 81-87, 511-523, 602; routes/ui_agents.rs 255, 951-958, 1204-1210;
routes/live_updates.rs 44-52.

## Lifecycle
main.rs:230-245 TaskWorker.with_schedules + spawn (joined 330); task_worker.rs start_worker_loop
391-420, schedule poll 2 s, lease 60 s, batch 10; workflow side already has the token bridge
(main.rs:208-217) and infra/workflow.rs spawn() reserves a connection for the materializer.
ScheduleUseCases built at infra/setup.rs:329-335; AppState.schedule_use_cases app_state.rs:61.

## Tests touching schedules
Unit: domain schedule (459, 496); use_cases/schedule.rs mocks + run-as tests (796…2038);
routes/schedule.rs (432-488); routes/ui_schedules.rs (959-1462); task_worker scheduled tests;
pages/tests.rs fixtures (1380, 1410, 1433, 1904, 1958, 4346). DB: persistence/schedule.rs (679,
891); routes/schedule_channel_tests.rs (28, 35); workflow job_tenant_association_tests.rs (52-387)
and job_tenant_fixtures.rs:15-58; admission_source_tests.rs Occurrence fixture 113-138 (141,
180, 359); admission_binding_tests.rs:245; thread task_lookup_tests / view_tests (scheduled task
type ordering).

## Workflow hooks present
TriggerSource::Schedule (causality.rs:162-165); source key (admission_binding.rs:13-18);
ScheduleTriggerInput (triggers.rs:81-98, representative not yet checked — model:
binding/mod.rs:105-116); admission_source.rs schedule() 98-125 requires the occurrence
thread_id to match the association thread (NULL for Channel); B1 classify()/Stale loop in
message_admission.rs to reuse; ManageBinding authority.

## Gaps / risks
1. run-now: occurrence row + command_key; slot-key collision. 2. refusal-failed CHECK branch.
3. thread_id legacy; admission needs NULL for Channel association. 4. snapshot contains removed
fields; trigger checks snapshot scope. 5. release_failed_claim dead. 6. record_run_error lacks
lease-expiry check. 7. UI thread pane/reply and list_schedule_runs built on legacy tasks/threads.
