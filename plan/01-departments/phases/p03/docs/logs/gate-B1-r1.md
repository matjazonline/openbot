# p03 Leads and coordination — TASK

## Identity

- Plan / phase: `plan/01-departments/`, phase `p03` (document `03-leads-and-coordination.md`)
- Workflow: `implement-opus-reviewed-2_2` (skill dir `/Users/mac02/.claude/skills/implement-opus-reviewed-2_2`)
- Record root: `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/phases/p03/docs/`
- Root = Author (A): session 68933786, Opus 5.5 / xhigh (user confirmed). Reviewer (B): agent
  `ad409b2d3d149c2c6`, Opus 5.5 / xhigh (independent; authored nothing).
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`,
  phase start commit `9291fcd5` (p01 57adfa7a, p02 0c5df403 + 9291fcd5 accepted). No pre-existing changes.
- Snapshot exclusions: `plan/01-departments/phases/p03/docs` `plan/01-departments/COORDINATION.md`
- Coordination rev 3 (pinned `logs/coordination-r3.md`, sha256 880cac54…04b8). Applicable:
  I001–I003/r1; **C001/r1** (relays trusted → p03 round-trip test asserts admission, dispatch
  section says so — acknowledged); **C002/r1** (one step both ways → escalation to the parent desk
  is admitted by the child lead's derived grant — acknowledged).
- Authorizations: local commit per accepted batch. No push. Full repo gate not run (I003).
- Resources: test DB `openbot_dept_p03_test`; worktree `target/`.

## Requirements

- `plan/01-departments/03-leads-and-coordination.md` (03A, 03B, 03C, focused checks), README
  decisions 4, 5, 8, 10, 12; plan 05 item on the provisioning-commands table comment; as amended
  by C001/C002.
- Instructions: `AGENTS.md`, `src/AGENTS.md` (async stack rule, Box::pin seams),
  `src/application/AGENTS.md`, `src/adapters/persistence/AGENTS.md`, `src/adapters/http/AGENTS.md`.
- Research: explorer report condensed in root scratchpad (`p03-research.md`), facts re-cited below.

## Batches

- **B1** = 03A automatic agent desks + 03C.0 placement on the shared creation transaction.
- **B2** = 03B department desk (agent lead / person lead) and re-leading.
- **B3** = 03C.1–6: directory tool fields and department filter, department context in
  `agent.run`, `department-lead.yaml` fixture, dispatch/escalation/loop/round-trip tests, docs
  (`agent-builder-context.md`, `custom_tools.md`, provisioning-commands table comment).

## Architecture gate B1 — decisions (revision r1, 2026-11-10)

Plan/code mismatches resolved here (from the code, not the plan text): there is no shared
company-agent creation function today — four paths insert agent + principal differently
(`AgentPersistence::create` with no lock and no publication; library copy under
`authorize_company`; onboarding's own session; the Builder's `provision_agent_on(New)` inside the
action transaction).

### DEP-P03-D1 One company-agent creation transaction (r1)

- New `pub(crate) async fn create_company_agent_on(tx, templates: &ProvisioningDeployment,
  company: CompanyId, actor: WorkflowActor, creation: CompanyAgentCreation<'_>) ->
  AppResult<CreatedCompanyAgent>` in `src/adapters/persistence/company_agent.rs`. The caller holds
  the account row `FOR SHARE` and the company write lock (`lock_company_on` /
  `authorize_company`), the order every provisioning path already uses. Steps, all on `tx`:
  1. resolve the model (D3);
  2. `insert_agent_on` (optionally with a deterministic id: onboarding, Builder) and
     `create_agent_principal_on`;
  3. placement (03C.0): `add_department_member_on` and, when a role is given,
     `assign_department_role_on` (p01 `_on` contract; p02 re-derives the department's desks);
  4. `publish_initial_on` (the desk's workflow needs a published agent);
  5. `assistant_channel_on` with `AssistantAgent::Existing(slug)` (targets the row just
     inserted; never `New`, so no recursion), command key `agent:{agent_id}`, channel slug and
     name = the agent's, `ReplyMode::Autonomous`. The default desk is a plain team desk
     (`department_id NULL`, `members_only false`, 03A.3).
  A refusal (taken address, invalid input) or error at any step returns `Err` and the caller's
  transaction rolls back: no agent, principal, placement, desk, workflow, binding or command row
  (03A.2). Refusals map as `routes/assistant_channels.rs` does (Conflict → `Conflict`, Invalid →
  `BadRequest`, Forbidden → `NotFound`).
- `Box::pin` the provisioning seam (src/AGENTS.md async-stack rule).
- `AgentPersistence::create` stays the agent-row-only insert used by ~40 test fixtures; after B1
  **no production path calls it** (the UI/HTMX/JSON use case moves to D2). A source test asserts
  no non-test file outside `persistence/agent.rs` calls `AgentPersistence::create` / `.create(`
  on the agent port. Library-only creation (`create_library`, builtin install) gets no desk.

### DEP-P03-D2 Wiring (r1)

- New application port `CompanyAgents` (in `use_cases/agent.rs`): `create(actor, company,
  write, placement) -> AppResult<CreatedAgent { agent, desk_id }>` and `copy_from_library(actor,
  company, library_id, placement) -> AppResult<CreatedAgent>`. Adapter `CompanyAgentService
  { persistence, templates }` (like `AssistantChannelService`), built by
  `WorkflowServices::company_agents()` and injected with `AgentUseCases::with_company_agents`.
  An `AgentUseCases` without it refuses creation with an explicit `Internal("agent creation is not
  configured")` — no silent default (src/AGENTS.md port rule); unit tests get a test double.
- Each path:
  - UI pane / HTMX / JSON → `AgentUseCases::create_agent` → `CompanyAgents::create` (begins
    `begin_bounded`, account `FOR SHARE`, `authorize_company`, then D1).
  - Library copy → `CompanyAgents::copy_from_library` (same prefix; `prepare_library_agent_on`
    then D1 with the prepared write).
  - Onboarding → `Session::insert` calls D1 inside its session transaction (D4).
  - Builder `create_agent_channel` → D1 inside the action transaction (D5).

### DEP-P03-D3 Model resolution (r1)

- An omitted provider+model is pinned to the company default: the default connection **with an
  API key** and its first model, read on `tx` (`company_model_connections`, as `ready_models_on`).
  An explicit model must be among the keyed connections' models (`ready_models_on`), the rule the
  Builder already applies. None usable → `BadRequest("Add a model connection with an API key
  before creating an agent: its desk answers on an explicit model.")` and nothing is written
  (03A.2). `LibraryCopyModel::FollowDefault` goes (every company agent now answers a desk, so it is
  pinned); the use-case-level `validate_model_selection` stays as the early, worded check.
- Consequence (plan's explicit decision): no company agent can be created before a keyed model
  connection exists.

### DEP-P03-D4 Onboarding (r1)

- `Session::insert` creates through D1: the default desk is committed with the agent (key
  `agent:{agent_id}`). `Session::channel` no longer provisions for agents it inserted; for an
  existing agent selected in onboarding it reads back that agent's `agent:{id}` desk. Readback
  (`stored_onboarding_channel`) uses the `agent:{id}` key. A refusal now **rolls back the whole
  onboarding transaction** (today it commits with `Some(Err)`), matching 03A.2.
- 03C.0: the onboarding request gains an optional department name; when present, the first
  department is created in the same transaction (p01 `create_department_on`, no lead) and the
  first agent is placed in it as a member. UI copy is phase 04; p03 adds the request field and its
  route/form parsing only.

### DEP-P03-D5 Agent Builder `create_agent_channel` (r1)

- Creates the agent through D1 (default desk at the agent's slug, autonomous) instead of
  `provision_agent_on(New)`. Then, by the requested desk:
  - same slug, autonomous, no custom steps → reuse the default desk; the tool result describes it;
  - same slug, reviewed mode or custom steps → refuse the whole command (`Invalid`: "The agent's
    own address is its autonomous default desk; choose another address for a reviewed desk or
    custom steps.") (03A.4);
  - different slug → provision the additional desk with `AssistantAgent::Existing` and the
    invocation key, in the same action transaction.
- Retries replay the action receipt (unchanged); the `agent:{id}` command row's digest includes
  the caller, and only the creating transaction writes it.
- Tool input gains optional `department` (slug) and `role` (`lead`); when no department is given
  and the requester's person principal belongs to exactly one department, the agent is placed
  there as a member (03C.0 default). Tool schema change → frozen builtin snapshot changes; DB reset.
  The UI form's default (creator's single department) is phase 04's form; the JSON API takes
  explicit `department_id`/`role` only.

### DEP-P03-D6 Desk identity and later edits (r1)

- `workflow_provisioning_commands (company_id, 'agent:{id}')` records the default desk; no new
  column or trigger. Renaming the agent does not rename its desk; desk edits/deletion stay
  ordinary (deleting is refused while its binding exists, as today). Directory reads (B3) resolve
  the recorded desk id to its current primary slug and omit a deleted desk. The
  `workflow_provisioning_commands` table comment lists `agent:{agent_id}` and `department:{id}`
  (plan 05 item).

## Verification plan B1

| Criterion | Path | Decisive assertion | Check |
| --- | --- | --- | --- |
| 03A.1/2 each path | use case (UI/JSON), JSON route, library copy, onboarding, Builder | exactly one desk at the agent's slug, team, one published autonomous version, one active binding, one `agent:{id}` command | `agent_default_desk_*` |
| 03A e2e | scripted-model Deployment: message to the new default desk answered | reply delivered | `agent_default_desk_*` |
| 03A.2 refusals | no keyed model; taken address (a desk already has the slug); publication refusal | Err; zero agent/principal/placement/desk/workflow/binding/command rows | `agent_default_desk_*` |
| library-only | `create_library` | no desk | `agent_default_desk_*` |
| onboarding | first run + replay; refusal rolls back | one desk; nothing committed on refusal | `agent_default_desk_*` |
| Builder | same slug reuse; reviewed at same slug refused; different slug extra desk | as D5 | `agent_default_desk_*` |
| placement | each path with department/role; Builder default | membership/role rows; p02 grants on members-only desks | `agent_default_desk_*` |
| no bypass | source scan | no production caller of `AgentPersistence::create` | `agent_default_desk_*` |
| competing | two creates of one slug (company lock held); two retries of one Builder command | one agent + desk; one default desk + binding | `agent_default_desk_competing_*` |
| deletion guards | delete an agent with a default desk | refused as today, desk/binding intact | `agent_default_desk_*` |

## Contract impact B1 (planned)

| Changed contract | Producers / consumers | Disposition |
| --- | --- | --- |
| company-agent creation | UI/HTMX/JSON routes, `AgentUseCases`, library copy, onboarding session, Builder `provision_agent_on` | all through D1 |
| `LibraryCopyModel` | `ui_agents.rs` library copy (FollowDefault), onboarding (PinDefault) | FollowDefault removed |
| `create_agent_channel` tool schema | builtin tool snapshot, agent revisions freeze it, `docs/custom_tools.md`, `agent-builder-context.md` | input gains `department`, `role` |
| onboarding readback key | `routes/onboarding.rs` `onboarding_key` | `agent:{id}` |
| `AgentUseCases` construction | infra setup, tests | `with_company_agents`; test double |

## Status

- 2026-11-10: p03 started from 9291fcd5; B1 gate r1 written, awaiting B.
