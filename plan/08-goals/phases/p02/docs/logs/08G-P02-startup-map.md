# 08G-P02 startup enforcement map (2026-11-21, merge HEAD candidate on 6c63f292 + origin/main a1cdb2d5)

Produced by a read-only Explore subagent for A; spot-checked by A where noted. Line numbers are at map time.

## Plan deviations found
- `channel.call` NOT implemented (`plan/agent-workflow-call/README.md` "These documents do not implement the feature"; only `src/application/workflow/fixtures/department-lead.yaml:5` mentions it). A confirmed `grep -rln 'channel\.call' src` → only that fixture. 02B.3 call clause and `unit_archive_runs_` channel.call case have no producer.
- `reports:<slug>` labelling exists for memory hits only (`workflow/memory_tool_checkpoint.rs:237–245`), not knowledge search.
- Workflow admission does not take the channel gate; it serializes on the company row (`workflow/authority.rs:81` `FOR NO KEY UPDATE`). Only inbound mail takes `ChannelGateAccess::Admit` (`thread/inbound.rs:205`), and inbound takes the company row `FOR KEY SHARE` (:148).
- Disabled desk (`channels.enabled=false`) is refused only by mail routing and internal relay; neither admission nor the schedule sweeper checks it.
- Stale plan anchors: `departments` table now `init_schema.sql:15709`; `guard_department_shape` :16101.

## 1 Admission
`workflow/admission.rs:11` admit; `management::authorize_association_on` :33; `verify_current` :67/:273 (inactive binding → Stale :310); `admission_binding.rs:193` current_binding. Unit read: `workflow/management.rs:67` target_on (`SELECT department_id FROM channels … FOR SHARE` :78); `workflow/association.rs:115` authorize_channel. Unit-level refusal pattern: `unit_budgets.rs:158` check_admission_on → `AppError::BudgetExhausted` code `budget.unit_exhausted` (`app_error.rs:21,226`); `ResourceOutsideUnit` (:17,:180). Classification: `message_admission.rs:450` classify → `BindingOutcome::Refused(code)`.
Mail disabled desk: `thread/ingest/routing.rs:1048` addressed_slug → DisabledDesk; `record_undeliverable` :1062; bounce text `ingest/policy.rs:89`, `thread/notices.rs:314`; directory skip `routing.rs:833`. Gate invalidation pattern: `department_desks.rs:231` rederive_on (ChangeSettings :249, settings_revision+1 :275).

## 2 Schedules
`workflow/schedules.rs:662` claim_due (`WHERE enabled AND next_run_at <= now`, no channel join) → claim_occurrence :751; `application/workflow/schedule_admission.rs:188` materialize; refuse → `record_refusal` `schedules.rs:836` (`materialization_status='failed'`, refusal metric). No skip-without-failure path today. Admission rechecks schedule `admission_source.rs:140`.

## 4 Relay
Rust `ingest/relay.rs:192` (`desk.enabled` else Forbidden); `persistence/thread/relay.rs:99` validate_targets_on → SQL `relay_desk_can_participate` (:19137), `internal_relay_source` (:19151/:19188), trigger `check_message_internal_relay_target` (:19227).

## 5 Configuration writes
Company lock `department.rs:196` begin_department_write; `lock_department_on` :248 FOR UPDATE; `lock_parent_on` :259 FOR SHARE.
- update (name/charter/escalation/parent) `department.rs:580` → `lifecycle.rs:136` update_department_on
- members `add_department_member_on` :461 → `write_placement_on` :407; remove :471; root members `department/root.rs:53`
- roles `department/roles.rs` assign/clear/sponsor/write_role_on
- placement `department/placement.rs:9`, :73
- create `department.rs:558` → `lifecycle.rs` insert_department_on :88
- desk create/assign chokepoint `department_desks.rs:48` prelock_ownership_on (target `departments … FOR SHARE` :62) from `channel.rs:290` insert_channel_row_on and `channel_edit.rs:105` update_channel_on; `set_desk_ownership_on` `department_desks.rs:307`
- desk responder `workflow/channel_reply.rs:134` configure_reply_on
- workflow binding `workflow/bindings.rs:46` save_binding_on (via management::begin → authorize_association_on)
- transport binding `integration/binding.rs:36`, :170
- schedules `schedules.rs:336` create, :369 update, :426 set_enabled
SQL backstops today: `guard_department_member_kind`/`guard_department_role_holder_kind` (:16158/:16160); no trigger on `channels.department_id` (FK only, :15839).

## 6 Authority
SQL `unit_management_role` (:15788), `any_unit_management_role` (:15820), `agent_management_role` (:20503; no-department → root arm :20514–20518), `workflow_definition_management_role` (:20524), `workflow_creating_department` (:20482), `resource_default_unit` (:16047). Rust `persistence/administration.rs:43` management_role_on, `managed_units` :115, `managed_unit_choices` :127; `ports/administration.rs:79` manages_unit, :93, :111. UI mirror `pages/company_departments.rs:102–140`.

## 7 Listing reads
`department.rs:524` departments_of_company (list_by_company :746, `channel.rs:392`); port `ports/department.rs:124/126/163/171`; use cases `use_cases/department.rs:385/409/465/484`; DeskDepartments → `pages/attention.rs:36` department_filter (`routes/attention.rs:104,220,267`), dashboard `ui_dashboard.rs:247`, run board `workflows/ui/runs/mod.rs:333`, reviewer picker. Agent directory `authoring_reads.rs:158` built at `agent_authoring/directory.rs:46` from `department_roster.rs:143`. Pickers: `managed_unit_choices` (`pages/unit_resources.rs:52,95`), `provisioning_directory.rs:17`, `provisioning_builder.rs:136`, `authoring_reads.rs:365`, `skill.rs:316`, channel forms (`routes/channel.rs:352,451,509,539,612`, `ui_channels.rs:145`, `ui_companies.rs:206`). Usage `model_usage/report.rs:55,193,248`.

## 8–10
`memory/namespaces.rs:276` retire_namespaces_on (callers `lifecycle.rs:317`, `agent.rs:691`, `member_retirement.rs:56`); run scopes `workflow/run_scopes.rs:122`. Knowledge `knowledge/permissions.rs:119`. Resource move `unit_resources.rs:84` move_resource_on (requires managing both units). Delete `lifecycle.rs:296` → guard :214 (no open-run check). Run states `queued|running|waiting|succeeded|failed|cancelled`; open = `finished_at IS NULL`; index `workflow_runs_open_by_channel` (:7860); count helper `run_counts.rs:10`. Cancel `application/workflow/service.rs:94` → `workflow/controls.rs:29`, cancel_tree_on :190.

## 11 Fixtures
`workflow/message_ingress_tests.rs:60` Deployment; `department_tests.rs:19` Departments; `test_support.rs:807` department_use_cases, `test_unit` :942, `test_unit_lead` :957, `test_root_unit` :933; UI `pages/department_ui_tests.rs` tree/managed_when/section; routes `ui_departments_tests.rs`.
