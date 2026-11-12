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

## Architecture gate B1 — decisions (revision r3, 2026-11-10)

r1 preserved in `logs/gate-B1-r1.md`, r2 in `logs/gate-B1-r2.md`; r2 answers `01DEP-P03-G1-F1..F5`,
r3 closes `01DEP-P03-G1-F4` (`AgentPersistence::create` test-only by construction). Plan/code mismatch
resolved here: there is no shared company-agent creation function today — four paths insert agent
+ principal differently (`AgentPersistence::create` with no lock and no publication; library copy
under `authorize_company`; onboarding's own session; the Builder's `provision_agent_on(New)`).

### DEP-P03-D1 One company-agent creation transaction (r2)

- New `pub(crate) async fn create_company_agent_on(tx, templates: &ProvisioningDeployment,
  company: CompanyId, actor: WorkflowActor, creation: CompanyAgentCreation<'_>) ->
  AppResult<CreatedCompanyAgent>` in `src/adapters/persistence/company_agent.rs`. Precondition:
  the caller holds the actor's account row `FOR SHARE` and the company write lock
  (`lock_company_on` / `authorize_company`) — true on every path (UI/JSON via D2's prefix, library
  copy, onboarding session, Builder `action_authority.rs:75`). **Steps (r2 order, F1):**
  1. resolve the model (D3) — reads `company_model_connections`;
  2. `insert_agent_on` (deterministic id for onboarding and the Builder) and
     `create_agent_principal_on` — new rows (the agents `(company_id, slug)` unique tuple);
  3. `publish_initial_on` — re-entrant account/company locks, `agents FOR UPDATE` on the new row,
     `company_model_connections FOR SHARE`, skills `FOR SHARE`;
  4. `assistant_channel_on` with `AssistantAgent::Existing(slug)` (never `New`: no recursion),
     command key `agent:{agent_id}`, slug and name = the agent's, `ReplyMode::Autonomous` — new
     channel/slug/binding/workflow rows, the existing-slug check, template compile (`FOR KEY SHARE`
     reads), `bootstrap_native_on` on the new desk; the desk is a plain team desk
     (`department_id NULL`, `members_only false`);
  5. **placement last** (03C.0): `add_department_member_on` and, with a role,
     `assign_department_role_on` — department row `FOR UPDATE`, principals `FOR KEY SHARE`, then the
     department's members-only desks' gates and rows (p02 D3). Nothing follows it, so the gates are
     the last locks taken and are held only until commit. This is the plan-wide order (account →
     company → departments → principals → gates → rows) with the creation's own new rows before it.
  **B2 may assume:** when placement assigns `lead`, the agent is already published and its default
  desk exists in the same transaction.
- Errors: a refusal from step 4 becomes `AppError` (Conflict → `Conflict`, Invalid →
  `BadRequest`, Forbidden → `NotFound`, as `routes/assistant_channels.rs`); any error aborts the
  caller's transaction — or the caller's savepoint (D5) — so no agent, principal, placement, desk,
  workflow, binding or command row survives (03A.2). `Box::pin` the provisioning seam.
- **No bypass (r3, F4):** production company-agent rows are inserted only by
  `create_company_agent_on`, and the compiler guards the one other company insert:
  `AgentPersistence::create` (the fixture-only agent insert, no desk) and both of its impls
  (`persistence/agent.rs`, the `agent_tests.rs` mock) become `#[cfg(test)]`, so no production
  code can call it. A source test scans non-test files for `insert_agent_on(` and
  `create_agent_principal_on(` and allows exactly: `company_agent.rs`; `persistence/agent.rs`
  (`create_library`, `company_id = None`; the `#[cfg(test)]` `create`); and the lazy principal
  backfill in `thread/message.rs` (existing agent, no new agent row). `LibraryAgentCopyPersistence`
  is removed: its method becomes `CompanyAgents::copy_from_library` (D2).

### DEP-P03-D2 Wiring (r2)

- New application port `CompanyAgents` (in `use_cases/agent.rs`): `create(actor, company, write,
  placement) -> AppResult<CreatedAgent { agent, desk_id }>` and `copy_from_library(actor, company,
  library_id, placement)`. Adapter `CompanyAgentService { persistence, templates }` (like
  `AssistantChannelService`), built by `WorkflowServices::company_agents()`, injected with
  `AgentUseCases::with_company_agents`. Without it, creation is refused with an explicit
  `Internal("agent creation is not configured")` (no silent default); unit tests get a test double.
- Each path: UI pane / HTMX / JSON → `AgentUseCases::create_agent` → `CompanyAgents::create`
  (`begin_bounded`, account `FOR SHARE`, `authorize_company`, D1); library copy →
  `CompanyAgents::copy_from_library` (same prefix, `prepare_library_agent_on`, D1 with the prepared
  write); onboarding → D1 inside its session (D4); Builder → D1 inside the action transaction
  under a savepoint (D5).
- Consequence (decision 12): a library copy and a UI-created agent are published at creation (the
  desk's workflow needs a published agent); company tools can still be configured on the draft
  and published as a later revision.

### DEP-P03-D3 Model resolution (r2, unchanged)

- An omitted provider+model is pinned to the company default: the default connection **with an
  API key** and its first model, read on `tx`. An explicit model must be among the keyed
  connections' models (`ready_models_on`, the Builder's rule). None usable → `BadRequest("Add a
  model connection with an API key before creating an agent: its desk answers on an explicit
  model.")`, nothing written. `LibraryCopyModel::FollowDefault` goes; the use-case
  `validate_model_selection` stays as the early, worded check. No company agent can be created
  before a keyed model connection exists (the plan's consequence).

### DEP-P03-D4 Onboarding (r2)

- `Session::insert` creates through D1; the default desk commits with the agent (`agent:{id}`).
  `Session::channel` no longer provisions; it reads back the agent's `agent:{id}` desk for agents
  just inserted and for existing agents selected (each already has one). Readback
  (`stored_onboarding_channel`) uses `agent:{id}`. Any refusal or error from D1 rolls back the
  whole onboarding (the existing `complete` path maps `BadRequest`/`Conflict`/`NotFound` to
  `OnboardingOutcome::Rejected`, nothing committed).
- **No keyed model (r2, F3):** every agent creation is refused by D3, so onboarding returns
  `Rejected` with D3's message and commits nothing. The per-agent "no desk / no model connection"
  outcome (`channel: None`) disappears: `OnboardedAgent.channel` becomes the desk readback, always
  present; its rendering drops the no-desk branch.
- 03C.0 department (r2, F3): the onboarding request gains an optional department name. The
  department's id is deterministic like onboarding's agent ids (`agent_id(company, key,
  "department")`, `use_cases/onboarding.rs`): a replay finds that id and reuses the department
  (and placement is idempotent); otherwise it is created in the same transaction through
  `create_department_with_id_on` (p01 `create_department_on` gains an id-taking form). A slug held
  by a different department → `Conflict` → `Rejected`. The first agent is placed as a member.
  UI copy is phase 04; p03 adds the request field and its parsing.

### DEP-P03-D5 Agent Builder `create_agent_channel` (r2)

- **Savepoint (r2, F2):** the Builder runs D1 and any additional desk inside one savepoint, as
  `provision_guarded` does today; an `Err` rolls back to it and maps to `ProvisioningRefusal`
  (Conflict → `Conflict`, BadRequest/NotFound → `Invalid`; a taken slug detected after rollback →
  `Conflict`, as `taken_slug` does), so the tool returns a worded tool error, the action receipt
  commits, and the run continues. Other errors propagate as today.
- Then, by the requested desk: same slug **and** autonomous **and** no custom steps **and** the
  channel name omitted or equal to the agent's name → reuse the default desk; same slug with any
  other setting (reviewed mode, custom steps, a different name) → refuse the whole command
  (`Invalid`: "The agent's own address is its autonomous default desk; choose another address for a
  reviewed desk, custom steps or another name."); different slug → provision the additional desk
  with `AssistantAgent::Existing` and the invocation key.
- Tool input gains optional `department` (slug) and `role` (`lead`); with no department and the
  requester's person principal in exactly one department, the agent is placed there as a member
  (03C.0 default). Frozen builtin tool snapshot changes; DB reset. The UI form's default is phase
  04; the JSON API takes explicit `department_id`/`role` only.

### DEP-P03-D6 Desk identity and later edits (r2, unchanged)

- `workflow_provisioning_commands (company_id, 'agent:{id}')` records the default desk; no new
  column or trigger. Renaming the agent does not rename its desk; desk edits/deletion stay
  ordinary. Directory reads (B3) resolve the recorded desk id to its current primary slug and omit
  a deleted desk. The table comment lists `agent:{agent_id}` and `department:{id}`.

## Verification plan B1

| Criterion | Path | Decisive assertion | Check |
| --- | --- | --- | --- |
| 03A.1/2 each path | use case (UI/JSON), JSON route, library copy, onboarding, Builder | exactly one desk at the agent's slug, team, one published autonomous version, one active binding, one `agent:{id}` command | `agent_default_desk_*` |
| 03A e2e | scripted-model Deployment: message to the new default desk answered | reply delivered | `agent_default_desk_*` |
| 03A.2 refusals | no keyed model; taken address (a desk already has the slug); publication refusal | Err; zero agent/principal/placement/desk/workflow/binding/command rows | `agent_default_desk_*` |
| library-only | `create_library` | no desk | `agent_default_desk_*` |
| onboarding | first run + replay; refusal rolls back | one desk; nothing committed on refusal | `agent_default_desk_*` |
| Builder | same slug reuse; reviewed / custom steps / other name at same slug refused; different slug extra desk | as D5 | `agent_default_desk_*` |
| Builder refusal (F2) | taken slug or no keyed model through the tool | tool error result, action receipt committed, zero agent/desk rows, run continues | `agent_default_desk_*` |
| duplicate slug per path (F5) | UI/JSON, Builder, onboarding, library copy | `Conflict` / tool error / `Rejected` / `Conflict`; nothing written | `agent_default_desk_*` |
| onboarding replay with department (F3) | same submission twice with a department name; again with an edited name | one department (first name kept), one placement, one desk; replays return Completed | `agent_default_desk_*` |
| onboarding without keyed model (F3) | no model connection | `Rejected`, nothing committed | `agent_default_desk_*` |
| lead at creation (F5, C002) | create agent with department + `lead` where the department and its parent own members-only desks | role row; derived grants on own, parent's and children's desks | `agent_default_desk_*` |
| placement | each path with department/role; Builder default | membership/role rows; p02 grants on members-only desks | `agent_default_desk_*` |
| no bypass (F4) | source scan of `insert_agent_on(` / `create_agent_principal_on(`; `AgentPersistence::create` is `#[cfg(test)]` | only the allowed sites; the lib (non-test) build has no `create` to call | `agent_default_desk_*` |
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
- 2026-11-10: G1-R1 changes required (F1–F3 Medium, F4 Low, F5 optional). r2 written: placement
  last with the full lock sequence; Builder savepoint + refusal mapping + name rule; onboarding
  deterministic department id and no-model outcome; source scan of the real insert functions;
  LibraryAgentCopyPersistence removed; F5 rows. Awaiting recheck.
- 2026-11-10: G1-R2: F1–F3, F5 resolved; F4 (Low) still open. r3: `AgentPersistence::create`
  and its impls `#[cfg(test)]`; skills lock wording `FOR SHARE`; onboarding edited-name replay
  asserted. Awaiting recheck G1-R3.
