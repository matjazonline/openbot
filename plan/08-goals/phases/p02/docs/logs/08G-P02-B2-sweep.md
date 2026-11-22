# 08G-P02 Batch 2 sweep (Explore agent, 2026-11-22, read-only, worktree at B1 digest 98cbdbc7…)

## Reads that list units (production)

| # | Read | Archived today | Feeds |
| --- | --- | --- | --- |
| 1 | port `list_by_company` (ports/department.rs:148) → `departments_of_company` (persistence/department.rs:557) | not filtered; `Department.archived` set | rows 3–5 |
| 1b | channel port `list_departments` (ports/channel.rs:84, persistence/channel.rs:396) | not filtered | mail bounce desk directory headings (use_cases/thread/support.rs:121 → ingest/routing.rs:824); desks already dropped by `takes_traffic` |
| 2 | `list_for_user` (department.rs:809) | not filtered | `visible_set` only |
| 3 | `DepartmentUseCases::list_departments` (use_cases/department.rs:411) | not filtered; `managed_units` includes archived | routes/channel.rs:352,451,509,539,612 (desk grouping, desk owner picker channel_settings.rs:479); ui_channels.rs:145; ui_companies.rs:206 (count) |
| 4 | `visible_departments` (use_cases/department.rs:435) | not filtered | ui_departments.rs:133 (tree, parent picker `parent_options` company_departments.rs:315, labels :454/:698/:996); routes/department.rs:114 JSON list |
| 5 | `desk_departments` (use_cases/department.rs:510) → `DeskDepartments` | not filtered | attention.rs:104,220,267 (`department_filter` pages/attention.rs:36, `known_department` routes/mod.rs:67, `group` entities/department.rs:388); ui_dashboard.rs:247; workflows/ui/runs/mod.rs:324,338,378,609 (run board options/filter/grouping); reviewer_picker.rs:216 |
| 6 | `agent_departments` → `department_roster::agent_departments_on` (department_roster.rs:143) | not filtered/exposed | routes/agent.rs:254,560 (agent page/list badges) |
| 6b | `DepartmentRef` (application/agent/authoring_reads.rs:158) via agent_authoring/directory.rs:36 | no field | catalog.rs:131, agent_picker.rs:137/172/188, ui_agents.rs:695 |
| 6c | provisioning_directory.rs:68 | not filtered | Agent Builder ListAgents (provisioning.rs:225) |
| 7 | `managed_units` (administration.rs:119) | not filtered | many use cases (department, channel, company model connections, skill, mcp), ui_departments.rs:131 |
| 8 | `managed_unit_choices` (administration.rs:131) → `ManagedUnit` | not filtered | skill/mcp/http-connection unit pickers |
| 8b | SQL `resource_default_unit` | excludes archived already | |
| 9 | knowledge `Permissions::choices` (knowledge/permissions.rs:268) | not filtered | upload and move-target pickers |
| 10 | `department_roster::departments_on` children (department_roster.rs:207) | not filtered | run prompt roster (agent_history/preparation.rs:250) |
| 11 | `unit_budgets_on`, `report_scope`, `load_calls` (model_usage/report.rs:248,55,193) | not filtered; `UnitBudgetStatus` no field | usage page, dashboard spend panel |

## Writers (02C)

| Write | Path | Archived check |
| --- | --- | --- |
| Knowledge upload | routes/knowledge/upload.rs:28 → KnowledgeLibrary::upload → `authorize_unit` (knowledge/permissions.rs:325) → writes.rs:38 | none; no DB trigger on `knowledge_documents` |
| Knowledge move | knowledge/mod.rs:247 → moves.rs:3 | none (move into archived allowed) |
| Resource move (skill, MCP, HTTP, model connection) | `unit_resources::move_resource_on` (unit_resources.rs:84, loop :108 checks both units alike) | none; target must refuse archived, source archived allowed |
| Resource create in named unit | `resource_creation_unit` (ports/administration.rs:96) | none |
| Budgets set/clear/replace | unit_budgets.rs:48 `authorize_on` → `require_active_unit_on` | yes (B1) |

## Watch-outs

- A: `unit_management_role` keeps archived targets for root/ancestor managers → pickers list archived units; `move_resource_on` lets an owner move into an archived unit.
- C/D: `DeskDepartments` and `list_departments` serve both option lists (hide archived) and labels of past work (keep, mark archived). Filter options only, or carry the flag.
- E: department page needs `visible_departments(include_archived)` while keeping archived parents for name lookups.
- F: `DepartmentRef`, `HeldDepartment`, `AgentDepartment`, `ManagedUnit`, `KnowledgeUnit`, `UnitBudgetStatus` carry no archived flag.
- G: knowledge, unit_budgets and resource `unit_id` columns have no DB backstop.
