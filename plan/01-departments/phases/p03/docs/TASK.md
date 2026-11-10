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

## B1 implementation (artifact `2f77ca9f…4279`, base `9291fcd5`)

Snapshot exclusions: `plan/01-departments/phases/p03/docs` `plan/01-departments/COORDINATION.md`.
Full digest `2f77ca9fc17c9b5e30a5481686580f8cb71597058ff9cdaa99d6072007d04279`.

What changed, by decision:
- **D1** `src/adapters/persistence/company_agent.rs`: `create_company_agent_on` (model → agent +
  principal → `publish_initial_on` → `Box::pin(assistant_channel_on(Existing slug, agent:{id},
  Autonomous))` → placement last), `resolve_model_on`, `default_desk_key`, `CompanyAgentService`.
  `AgentPersistence::create` and its two impls are `#[cfg(test)]`; `LibraryAgentCopyPersistence`
  and `LibraryCopyModel` are gone; `prepare_library_agent_on` no longer pins (D1 does).
  `ready_models_on` is `pub(crate)`. Init schema: the commands table comment names `agent:{agent_id}`.
- **D2** `CompanyAgents` port, `AgentPlacement`, `CreatedAgent` (use_cases/agent.rs);
  `AgentUseCases::new` takes three ports, `with_company_agents` adds creation; unconfigured →
  `Internal("agent creation is not configured")`. `WorkflowServices::company_agents()`;
  `infra/setup/accounts.rs` wires it. UI pane/HTMX pass no placement; the JSON create takes
  `department_id`/`role` beside the strict agent fields (`json_placement`).
- **D3** omitted model pinned on `tx`; explicit model checked against `ready_models_on`; no
  connection → the worded `BadRequest`. Schema note: `company_model_connections.api_key` is NOT
  NULL, so "keyed" means "any connection"; the tests cover the no-connection case.
- **D4** onboarding: `Session::insert` → D1 (`Box::pin`), `Session::channel` reads `agent:{id}`,
  `OnboardedAgent.channel: Value`; any refusal → `Rejected`, nothing committed; optional
  `department_name` → `OnboardingDepartment` (id `agent_id(company, key, "department")`) created by
  `create_department_with_id_on` unless present; first selected agent placed as member. Pages:
  `OnboardingChannel::{Refused, NoModel}` removed; route readback key `agent:{id}`.
- **D5** `src/adapters/persistence/workflow/provisioning_builder.rs`: `create_agent_channel_on`
  (savepoint; Conflict→Conflict, BadRequest/NotFound→Invalid, else taken-slug recheck or
  propagate), default-desk rule, `placement_on` (named department / requester's single one /
  role needs department), extra desk via `provision_on` with the invocation key.
  `AssistantAgent` is gone: `Provisioning.agent_slug` names an existing agent; tool input gains
  `department`, `role` (`lead`).
- **D6** command row records the default desk (asserted); table comment updated.

Behaviour changes to report: a library copy (UI or onboarding) is published at creation, so a
library agent whose skill needs a company-local tool is refused instead of installed as an
unpublished draft (plan 03A.2 "unresolved publication prerequisite"); the HTMX/JSON/UI creates,
library copies and onboarding all need a model connection first; the Builder's reviewed desk or
custom steps need an address other than the agent's own.

### Verification mapping B1 (actual)

| Criterion | Test (decisive assertion) |
| --- | --- |
| 03A.1/2 creation commits all | `agent_default_desk_creation_commits_the_agent_with_its_desk` (grew = 1 agent, principal, revision, desk, workflow, active binding, command; desk team/NULL/false at slug; command result = desk id, binding on desk; pinned model; explicit model kept) |
| use case routes through port | `agent_default_desk_creation_goes_through_company_agents` (desk id returned; unconfigured → Internal) |
| JSON placement + refusals | `agent_default_desk_json_create_places_the_agent` (member + lead rows; role w/o dept, bad role, bad id, retired field → 400; 1 agent) |
| no keyed model / bad model | `agent_default_desk_refuses_without_a_keyed_model` (exact message; nothing grew) |
| taken address / slug (F5) | `agent_default_desk_refuses_a_taken_address_or_slug` (Conflict; nothing grew) |
| publication prerequisite | `agent_default_desk_refuses_a_library_copy_it_cannot_publish`, `autonomous_agent_provisioning_library_missing_local_tool_refuses_the_copy` (BadRequest; zero rows incl. skills) |
| library-only / copy | `agent_default_desk_library_definitions_have_none_and_copies_do`; `library_builder_copy_keeps_its_grants_on_an_explicit_model` (+1 desk) |
| lead at creation, C002 grants | `agent_default_desk_places_the_agent_and_its_lead_role_with_grants` (member + lead; participate+view derived on own, parent, child members-only desks; default desk unowned; foreign dept → NotFound, nothing grew) |
| deletion guards | `agent_default_desk_deletion_guards_are_unchanged` |
| no bypass (F4) | `agent_default_desk_company_agents_have_one_creation_path` + `#[cfg(test)]` create (lib build has no caller) |
| competing creators | `agent_default_desk_competing_creators_of_one_slug_make_one_agent` (2 blocked on company lock; 1 Ok + 1 Conflict; one agent/desk) |
| onboarding refusal | `agent_default_desk_onboarding_taken_address_rejects_and_commits_nothing`; `onboarding_new_key_after_rename_does_not_replay_an_old_channel`; `onboarding_new_owner_with_new_key_does_not_replay_the_previous_owners_command` (Rejected; totals unchanged) |
| onboarding replay + department (F3) | `agent_default_desk_onboarding_creates_and_replays_the_first_department` (one department, first name kept on edited-name replay, one placement, desks 2; taken dept slug → Rejected) ; `onboarding_same_key_replays_…` (ids equal; totals 1,1,1) |
| onboarding no model (F3) | `agent_default_desk_onboarding_without_a_keyed_model_is_rejected` |
| onboarding readback | `agent_default_desk_onboarding_completion_reads_back_the_default_desk` (Created{desk id, address}; fixture agent → Missing; outsider → Missing) |
| Builder reuse | `agent_default_desk_builder_reuses_the_default_desk_at_the_agents_slug` (ONE_ASSISTANT; result == `agent:{id}` record) |
| Builder same-slug refusals | `agent_default_desk_builder_refuses_other_settings_at_the_agents_slug` (reviewed/steps/name → invalid; receipt only) |
| Builder extra desk | `agent_default_desk_builder_provisions_another_desk_beside_the_default` (channels 2, workflows 2, bindings 2; default autonomous at agent slug) |
| Builder refusal (F2) | `agent_default_desk_builder_creation_refusals_are_tool_errors` (taken → conflict, no model → invalid; receipt; zero agents/desks) |
| Builder placement | `agent_default_desk_builder_places_the_agent_in_a_department` |
| Builder competing | `agent_default_desk_competing_builder_claimants_make_one_default_desk`; `provisioning_competing_claimants_create_one_assistant` (ONE_ASSISTANT) |
| e2e answered | `builder_provisions_an_assistant_for_a_signed_in_admin` (Builder default desk answers a message, scripted model); `builder_adds_custom_steps_to_the_new_agents_workflow` (extra desk). Stand-in gap: no scripted e2e for a UI/JSON-created desk; it is the same `assistant_channel_on` desk, asserted structurally. |

Removed/renamed tests (`logs/B1-test-names.txt`): `creating_an_agent_creates_only_the_agent` →
`agent_default_desk_creation_goes_through_company_agents`;
`the_channel_step_gives_each_modelled_agent_one_assistant_channel` →
`agent_default_desk_onboarding_completion_reads_back_the_default_desk`;
`onboarding_address_refusal_keeps_the_agent_and_other_channels` →
`agent_default_desk_onboarding_taken_address_rejects_and_commits_nothing`;
`autonomous_agent_provisioning_library_missing_local_tool_installs_an_unpublished_draft` →
`…_refuses_the_copy` (its skill-copy assertion moved to `…_install_failure_rolls_back_…`). Each is
an intentionally changed behaviour (D1/D4, 03A.2).

### Checks B1 (TEST_DATABASE_URL `openbot_dept_p03_test`, recreated; SQLX_OFFLINE=true; 4 threads)

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --locked --offline --lib agent_default_desk -- --list` | 21 selected (2 `competing_`) | `logs/B1-test-inventory.txt` |
| `cargo test … agent_default_desk` (first runs) | FAIL 18/2 (keyless fixture), then FAIL (api_key NOT NULL), fixed tests | `logs/B1-agent-default-desk-1.txt`, `-2.txt` |
| `cargo test … -- agent_default_desk routes::agent` (final) | 30 passed | `logs/B1-agent-default-desk-5.txt` |
| affected: `-- onboarding provisioning use_cases::agent routes::agent routes::ui_agents skill_tests department assistant_channel desk_wording authorization routes::workflows agent_retire agent_constraint persistence::agent:: agent_default_desk` | first FAIL 9 (expected consequences, fixed); final 322 passed, 2 ignored | `logs/B1-affected-1.txt`, `logs/B1-affected-3.txt` |
| `cargo clippy --locked --offline --lib --tests -- -D warnings`; `… --lib --bins` | clean | (inline) |
| `cargo fmt --all -- --check` | clean | (inline) |

`B1-affected-3` ran before a formatting-only change to `agent_api_tests.rs`; `-5` ran after it.

## B1 corrections for 01DEP-P03-B1-R1 (artifact `a75c40e7…5798`)

Full digest `a75c40e7b888d42b415fe7408a319812242e96157e642c594a91ab87c7d55798`, same exclusions.

- **F1 (Medium)**: `department.rs` gains `place_in_department_on` (member + optional role) over one
  `write_placement_on`: department row, involved principals sorted, rows, then **one**
  `sync_department_grants_on` over the role's reach (or the department), so the desk gates are one
  sorted acquisition. `assign_department_role_on` and `add_department_member_on` use the same
  writer. D1 calls `place_in_department_on`. Competing test
  `agent_default_desk_competing_lead_creation_takes_its_desk_gates_once`: an admission holds the
  parent desk's gate (lower key); while the lead creation waits, a second admission into the
  department's desk succeeds (lock_timeout 500 ms); creation completes after commit. Mutation
  (old two-call placement restored) fails it with a lock timeout: `logs/B1-R2-gate-mutation.txt`.
- **F2 (Medium)**: Agent Builder instructions (`builtin_agent_library.rs`): every agent gets its
  own autonomous desk at its slug; reviewed replies or custom steps are a second desk at another
  address in the specification; department and role are collected; the final report names the
  second desk. Asserted in `desk_wording_agent_builder_prompt_says_desk` and
  `agent_builder_is_granted_the_provisioning_tools_and_told_their_rules` (clauses updated to the
  new text).
- **F3 (Low)**: the no-bypass scan walks the module tree from `src/lib.rs` with `syn`, follows
  `#[path]`, skips only `#[cfg(test)]`/test items (inline or out-of-line, inherited), and asserts
  the exact production call set by (file, function, primitive). Mutation (a production call added
  at the end of `agent_publication/mod.rs`) fails it: `logs/B1-R2-scan-mutation.txt`.
- **F4 (Low)**: `docs/agent-authoring.md` (create publishes with the default desk; model,
  refusals; `department_id`/`role`) and `docs/rig.md` updated.
- **F5 (optional, done)**: the Builder passes no placement to D1 and places after any extra desk.
- **F6 (optional, done)**: onboarding places the first selected agent after all inserts with
  `place_in_department_on` (idempotent), so a same-key resubmission that first names a department
  places the existing agent: `agent_default_desk_onboarding_names_the_department_on_a_resubmission`.
  D1 placement is now used by `CompanyAgentService` only.

Checks (TEST_DATABASE_URL `openbot_dept_p03_test`, SQLX_OFFLINE, 4 threads):

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --locked --offline --lib agent_default_desk -- --list` | 23 (3 `competing_`) | `logs/B1-R2-test-inventory.txt` |
| `cargo test … agent_default_desk` | 23 passed | `logs/B1-R2-agent-default-desk.txt` |
| affected filter set (B1 set + `builtin_agent_library response_schema`) | 329 passed, 2 ignored | `logs/B1-R2-affected.txt` |
| mutations (scan, gate order) | both fail as intended, then restored | `logs/B1-R2-*-mutation.txt` |
| clippy `--lib --tests` and `--lib --bins` `-D warnings`; `fmt --check` | clean | (inline) |
| test names HEAD vs tree | same 4 removed (accounted), 24 added | `logs/B1-R2-test-names.txt` |

## Architecture gate B2 — decisions (revision r4, 2026-11-10)

Plan 03B; README decisions 4, 8, 9, 12; coordination C001/C002 unaffected. Builds on accepted B1
(`523732b7`: `create_company_agent_on`, `place_in_department_on`/`write_placement_on`,
`Provisioning.agent_slug`). r1 preserved in `logs/gate-B2-r1.md`, r2 in `logs/gate-B2-r2.md`;
r2 answers `01DEP-P03-G2-F1..F7`; r3 (root, before the G2-R2 verdict) moves the desk effect back
after the grant sync -- see D9 "Where"; r3 is in `logs/gate-B2-r3.md`; r4 answers G2-F2 (residual),
G2-F8 and G2-F9.

Plan/code mismatches recorded here (F5): the plan's "Builder edit path (`apply`,
`provisioning.rs:192`)" does not exist -- `apply` only lists and creates -- so re-pointing is new
code over existing publication/binding primitives; the agent step is `REPLY_AGENT_STEP = "draft"`
(not `reply`); `AssistantAgent::Existing` no longer exists (B1: `Provisioning.agent_slug`); plan
03B.2 marks a person-lead desk by `department_id` + slug equality, which a slug edit breaks, so
both desks are marked by the `department:{id}` command row instead (D8; no new column or trigger,
as 03B.2 requires).

### DEP-P03-D7 Templates and actor reach department writes (r2, unchanged in substance)
- `create_department_on`, `create_department_with_id_on`, `assign_department_role_on`,
  `clear_department_role_on`, `place_in_department_on` (and `write_placement_on`) take
  `templates: &ProvisioningDeployment` and `actor: WorkflowActor`. The actor is the account whose
  row the caller holds `FOR SHARE` and who is authorized under the company lock on every path
  (department routes via `begin_department_write`, company-agent creation, onboarding session,
  Builder action transaction).
- `DepartmentPersistence` moves from `PostgresPersistence` to `PostgresDepartments { persistence,
  templates }` (persistence/department.rs), built by `WorkflowServices::departments()` and passed
  to `DepartmentUseCases::new` in setup; reads delegate unchanged. Tests construct it with
  `test_support::provisioning_templates()`.

### DEP-P03-D8 The department desk at creation, and its record (r2)
- After the department row, in the same transaction, before any role write:
  - lead is an agent principal → `assistant_channel_on(agent slug, "department:{id}",
    slug/name = the department's, Autonomous)` (it writes the `department:{id}` command row with
    the desk, workflow and binding ids); refusal → AppError (Conflict/BadRequest/NotFound), caller
    rolls back; then `set_desk_ownership_on(desk, department_id = id, members_only = false)`.
  - lead is a person or none → a plain team desk through the owned-desk create sequence
    `channel.rs` `create` uses (F7): `prelock_ownership_on` → `insert_channel_row_on` (company
    `add_3rd_party` default, enabled, ownership `{department, members_only: false}`) →
    `insert_channel_slugs` → `write_channel_email_binding` → `sync_desk_grants_on`; a taken
    address is pre-checked on `channel_slugs` → `Conflict("The address '{slug}' is already used by
    a desk in this company")` (a unique violation also maps to Conflict). Then D8 writes the
    `department:{id}` command row itself: digest over `{department, desk}`, actor = the actor,
    result `{"channel": {"id", "slug", "address"}, "binding_id": null}`.
- **The record (F4):** `workflow_provisioning_commands (company, 'department:{id}')` is the one
  marker of the department desk and of the binding D8/D9 own: `result.channel.id` = the desk,
  `result.binding_id` = the owned binding or null. D9 updates `result.binding_id` when it
  provisions one (case 2). Desk identity = that channel while it exists and still has
  `department_id = id`; otherwise none (detached or deleted -- later role changes touch no desk).
  A department slug or name edit never renames or loses the desk. The table comment lists
  `agent:{agent_id}` and `department:{id}`.
- Lock order: company (held) → parent `FOR SHARE` → new department row → desk rows (new; the
  agent lead's row `FOR UPDATE` via publication; the new desk's gate, uncontended) → the role
  write (D9 then finds the owned binding already answering that agent: no change) → principals →
  one sorted gate acquisition → rows.
- Every create path gets a desk: routes/use case, onboarding's first department (its address can
  collide with an agent's slug or desk → `Conflict` → `Rejected`, nothing committed; a replay
  finds the department and its desk), and tests/fixtures that create departments.

### DEP-P03-D9 Re-leading (r2)
- **Where (F1, r3):** inside `write_placement_on` (role `lead`) and `clear_department_role_on`
  (`lead`), **after the grant sync**. Sequence: account(actor) `FOR SHARE` → company →
  department row `FOR UPDATE` → principals `FOR KEY SHARE` sorted → member and role rows → one
  sorted gate acquisition → the reached members-only desks' channel rows `FOR UPDATE` → [desk
  effect, in procedure order (F9): the lead agent row `FOR UPDATE` (publication) → workflow
  draft/version rows → binding head/revision rows → the department desk's channel row `FOR UPDATE`
  (re-entrant when it is members-only and reached) → its reply settings row; **the only account
  it authorizes or locks is the actor's, already held** (r4: see "Run-as")].
  - Why not before the sync (r2): the reply-settings write locks the department desk's channel
    row `FOR UPDATE`. When that desk is members-only it is in the sync's reach, so the row would be
    held before its gate: an inbound admission holding the gate shared and waiting on the row (its
    message insert's FK lock), or a demotion holding a principal and updating the row, would close
    a cycle with the sync waiting on that gate or principal.
  - Why after is safe: G2-F1's cycle needed D9 to lock another account (a stored run-as or a
    reviewer) after the principals; r2/r3 make the run-as the actor and refuse desks with an
    approval policy, so D9 takes no account lock it does not already hold. Its channel row and
    reply-settings locks follow the sync's row locks (same direction as every channel edit); the
    lead agent row and workflow/binding rows are taken only by writers under the company lock,
    which this transaction holds. Cost: the reached desks' gates are held through the re-point's
    compile and publication (milliseconds; admissions into those desks wait, then re-check their
    revision) -- the B1-F5 exception, stated here.
- **What (F4):** the owned binding B = `department:{id}` `result.binding_id`; the selected
  responder S = the desk's reply settings binding. D9 acts only when the desired state differs:
  - new lead is an agent A:
    - B exists, S ∈ {B, none}: if B is active, S = B and B's bound version already runs A → no
      change. Otherwise re-point B to A if its version runs another agent (below), activate B if
      paused, select B (F6: a paused B is reused, no new binding);
    - B null, S none: provision the autonomous workflow and binding on the desk
      (`provision_workflow_on` for an existing channel, key
      `ProvisioningKey::command(company, "department:{id}")`, run-as = actor), select it
      (bootstrap when no settings row, else `configure_reply_on`), record `binding_id`;
    - S is a binding other than B (a user chose the responder) → no desk change.
  - new lead is a person, or the role is cleared (F9): B active → deactivate B (a Deactivate
    change re-authorizes no one), and when S = B also clear the selection (`configure_reply_on`,
    responder `None`; a selected inactive responder makes every inbound message a visible
    refusal). B already inactive, or no B → no desk change. A user's own responder S ≠ B is never
    paused or cleared; person→person and person→none change nothing.
- **Re-point (F3, F8):** the source re-targeted is **B's bound version's** source
  (`retarget_agent(REPLY_AGENT_STEP = "draft", slug)`), never the current draft blindly: if the
  workflow's current draft **source** differs from the bound version's source (unpublished edits;
  revision numbers are not compared, so re-saving the bound content clears it) → refuse
  `BadRequest("The department desk's workflow has edits its desk does not run; publish them and
  bind the desk to the new version, or restore the bound version's content, before changing the
  lead.")`. Otherwise save the retargeted source as the next draft
  (`save_draft_on`, factored out of `save_draft`), compile/capture with `compiled`, publish a new
  version (`provisioned_publication` + `publish_on`), `publish_initial_on(A)` first; reconfigure
  B (configuration revision + 1, same params and trigger, run-as = actor, same active flag; new
  `provisioned_reconfiguration` beside `provisioned_binding`), then activate if paused, then select
  with the settings' current contract (`configure_reply_on`, factored out of
  `configure_channel_reply`).
- **Run-as and refusals (F2):** B's run-as becomes the actor: attribution of the department desk's
  answers moves to whoever made the lead change (as for any binding reconfigured by an admin).
  **r4 (residual):** an Activate change keeps the stored configuration (run-as included) and
  `check_run_as`, `selected_on` and `lock_settings_accounts` all authorize or lock the binding's
  stored run-as. So before **any** activation or selection of B, if B's stored run-as ≠ the actor,
  B is first reconfigured (configuration revision + 1, the same bound version -- or the re-pointed
  one -- same params and trigger, run-as = actor, same active flag). Then every authorization D9
  causes names the actor only. (Deactivation and clearing the selection authorize no stored
  run-as.)
  Worded outcomes, each refusing the whole role change:
  - the bound workflow has no `draft` agent step → `BadRequest("The department desk's workflow
    has no agent reply step to hand to the new lead; edit its workflow or detach the desk
    first.")`;
  - its bound version declares resource slots → `BadRequest("The department desk's workflow
    uses resources; change its lead in the workflow editor or detach the desk first.")`;
  - the desk's reply settings carry an approval policy → `BadRequest("The department desk asks
    reviewers to approve replies; remove that policy or detach the desk before changing the
    lead.")` (this keeps D9 from authorizing other accounts under these locks, F1);
  - the actor cannot see the desk (a members-only department desk without a grant, non-owner) →
    `NotFound` from association is mapped to `BadRequest("You cannot see the department desk
    '{slug}', so its replies cannot follow the new lead.")`;
  - unpublished edits (above).
- Failure: any error aborts the transaction (route → status; company-agent creation as lead →
  whole creation refused; Builder → tool error per D5).

### Verification plan B2 (`department_desk_`, `department_desk_competing_`)
| Criterion | Assertion |
| --- | --- |
| 03B.1 agent lead | one desk at dept slug/name, `department_id` = dept, members_only false, published autonomous workflow on the lead, active binding, `department:{id}` row naming desk and binding |
| 03B.1 person/no lead | team desk at slug, `department_id` set, `department:{id}` row with `binding_id` null, no workflow/binding |
| 03B.1 refusal | taken address → Conflict; no department, desk, workflow, binding, role, command |
| onboarding (F7) | onboarding's department gets its desk; slug colliding with the agent → Rejected, nothing; replay → same department and desk |
| 03B.2 identity | department slug/name edit keeps the desk; detached/deleted desk → role change touches no desk |
| agent→agent | same binding id, configuration revision + 1, new version whose `draft` step names the new agent, run-as = actor; settings select it; e2e: a scripted message to the department desk answered by the current lead before and after |
| agent→person / cleared | B inactive, selection cleared; a message is stored with no refusal outcome |
| person→agent | (plain desk) workflow + active binding provisioned, selected, `binding_id` recorded; e2e answered (F7) |
| agent→person→agent | B re-pointed if needed, reactivated and reselected; no second binding (F6) |
| run-as moves (G2-F2 r4) | A (set by admin X) → person → A (set by admin Y) after X lost admin: succeeds; B run-as = Y, active, selected |
| lead not an agent, B active but unselected (F9) | B deactivated; selection untouched |
| draft edits (F8) | draft source ≠ bound source → refused; after re-saving the bound content (revision moved, source equal) → re-lead succeeds |
| user's own responder | S ≠ B: lead changes leave S and B untouched |
| unpublished edits / no draft step / resources / approval policy / invisible desk | each refused with its message; nothing changed (role, binding, settings, versions) |
| agent created as lead (B1 D1 + D9) | its own desk and the department desk both answered by it |
| competing | two concurrent lead changes of one department (company lock): final responder = last writer, one active binding; a lead change racing an admission into a reached members-only desk takes its gates once (as B1-F1) |
| delete | department with its desk → refused; after the desk is deleted or detached → ok |

### Contract impact B2 (F5)
| Changed contract | Producers / consumers | Disposition |
| --- | --- | --- |
| department create also creates its desk | department routes/use case, UI (phase 04), onboarding, p01/p02 tests and fixtures creating departments | all create a desk; tests asserting desk counts updated; slug collisions with existing desks/agents now Conflict |
| department delete | every department now owns its desk → delete refused until the desk is detached/deleted (p02 rule) | p01/p02 delete tests detach or delete the desk first |
| `_on` signatures (templates, actor) | `company_agent.rs`, Builder `provisioning_builder.rs`, onboarding session, department persistence | updated |
| `DepartmentPersistence` construction | `infra/setup/accounts.rs`, route tests, persistence tests | `PostgresDepartments` |
| `workflow_provisioning_commands` comment | init schema | lists `department:{id}` (DB reset) |
| binding run-as on re-lead | department desk binding audit/attribution | moves to the actor (documented) |

## B2 implementation (artifact `8fd91d04…5c9b`, base `523732b7`)

Full digest `8fd91d0452986625ad448afcdbe382b456c75bcaeaa9cddef117523947535c9b`; exclusions as B1.

- **D7** `PostgresDepartments { persistence, templates }` (department.rs) implements
  `DepartmentPersistence`; `WorkflowServices::departments()`; setup wires it. `create_department_on`,
  `create_department_with_id_on`, `assign_department_role_on`, `clear_department_role_on`,
  `place_in_department_on` take `templates` + `actor`; `write_placement_on` is unchanged and the
  lead effect runs after it (`follow_lead_on`). `delete_department_on` also deletes the
  `department:{id}` record. p02's `expect(dead_code)` on `set_desk_ownership_on` removed.
- **D8/D9** `src/adapters/persistence/workflow/provisioning_department.rs` (submodule of
  provisioning): `department_desk_key`, `provision_department_desk_on` (agent lead →
  `assistant_channel_on("department:{id}")` + `set_desk_ownership_on`; else plain desk in a
  savepoint: prelock → insert row/slugs/email → `sync_desk_grants_on`, taken → Conflict, record
  with `binding_id: null`), `follow_department_lead_on` (record → owned B, selected S; person/none
  → deactivate active B, unselect when S = B; agent → leave a user's S, provision on a plain desk
  (`provision_workflow_on(select = settings absent)` + select + record), or re-point
  (`repoint_on`: bound source, draft-source check, `retarget_agent("draft")`, `save_draft_on`,
  `compiled`, `publish_on`, `provisioned_reconfiguration`), reconfigure to run as the actor,
  activate, select (`select_on`: bootstrap or `configure_reply_on`); `guard_on` refuses approval
  policies and maps only the explicit association `NotFound`).
- Factorings: `save_draft_on` (definitions.rs), `configure_reply_on` + `native_contract`
  (channel_reply.rs; `settings_on` pub(super)), `provisioned_reconfiguration` +
  `provisioned_activation` (lifecycle/provisioned.rs), `provision_workflow_on`/`provision_binding_on`
  take `select`. Recorded in plan 03B item 4.
- Callers: D1 (`company_agent.rs`), Builder, onboarding (department create + first placement)
  pass templates/actor. Init schema: the commands table comment names `department:{id}` and that
  the row is updated (DB reset).
- Test fixture: `Departments.worker` is now a real company agent (`company_agent`, published, own
  desk); `agent_principal` (bare) kept for the agent-delete test; `desk_of`/`drop_desk` helpers.

### Verification mapping B2 (actual)

| Criterion | Test (decisive assertion) |
| --- | --- |
| 03B.1 agent lead | `department_desk_with_an_agent_lead_answers_on_it` (desk slug/name/department/members_only false; binding active rev 1 run-as owner, `draft` = worker; selected) |
| 03B.1 person/none | `department_desk_with_a_person_lead_or_none_is_a_plain_team_desk` (record binding null; no bindings; no settings) |
| 03B.1 refusal | `department_desk_taken_address_refuses_the_department` (Conflict for agent/person/no lead; department, role, channel, command, binding counts unchanged) |
| onboarding (F7) | `department_desk_onboarding_address_collision_with_an_agent_is_rejected` (Rejected, totals 0); `agent_default_desk_onboarding_creates_and_replays_the_first_department` (desk counted, replay stable) |
| 03B.2 identity | `department_desk_survives_a_rename_and_is_left_once_detached` (desk keeps address; follows after rename; detached → untouched) |
| agent→agent | `department_desk_agent_to_agent_repoints_the_same_binding` (same binding, config rev 2, run-as admin, `draft` = closer, selected at rev 2, 1 binding; repeat → no change) |
| person/agent cycles (F6, F9) | `department_desk_follows_person_and_agent_leads_with_one_binding` (pause + unselect; reactivate + reselect, 1 binding; plain desk provision + record; unselected active B paused on clear) |
| run-as moves (G2-F2) | `department_desk_run_as_moves_to_the_admin_who_changes_the_lead` (admin X removed; admin Y re-leads; active, run-as Y) |
| refusals (F3, F8) | `department_desk_refuses_lead_changes_it_cannot_follow` (draft edits → refused, nothing changed; restored content → re-lead ok; approval policy → refused; members-only desk, admin without grant → refused) |
| agent created as lead | `department_desk_follows_an_agent_created_as_its_lead` |
| delete | `department_desk_blocks_deleting_its_department_until_detached` (refused, detach → deleted, record gone) |
| competing | `department_desk_competing_lead_changes_leave_the_holders_agent_answering` (2 blocked on company lock; both apply; 1 binding; active on the holder's agent) |
| e2e | `department_desk_answers_through_its_current_lead` (person → agent A → agent B; scripted messages answered; `agent_history_exchanges` = [A, B]) |

Stand-in gaps: no test drives a workflow without a `draft` step or with resource slots (both
need a hand-built bound version); those refusals are covered by code review only. No test makes a
user's own responder S ≠ B (needs a second binding on the desk); the "S ≠ B → no change" branch is
covered by code review only.

### Checks B2 (`openbot_dept_p03_test` recreated after the comment edit; SQLX_OFFLINE; 4 threads)

| Command | Result | Log |
| --- | --- | --- |
| `--list` `department_desk_ agent_default_desk` | 53 (30 `department_desk_`, 4 `competing_` incl. p02's) | `logs/B2-test-inventory.txt` |
| `department onboarding agent_default_desk` (first) | FAIL 7 (expected: raw fixture agent cannot lead; every department owns its desk; counts), fixed | `logs/B2-first-run.txt` |
| `department_desk_ agent_default_desk` (final) | 53 passed | `logs/B2-focused-final.txt` |
| affected: `department onboarding provisioning channel_reply definition binding assistant_channel desk_wording authorization routes::workflows agent_default_desk use_cases::agent routes::agent skill_tests member_removal user_deletion removed_membership template builtin_agent_library response_schema` | 686 passed, 1 ignored | `logs/B2-affected-1.txt` |
| e2e (after extending it to person → agent → agent) | passed | `logs/B2-e2e-2.txt` |
| clippy `--lib --tests` / `--lib --bins` `-D warnings`; `fmt --check` | clean | (inline) |
| test names since 523732b7 | none removed | `logs/B2-test-names.txt` |

`B2-affected-1` ran before the e2e test was extended (test-only change, re-run alone).

## B2 corrections for 01DEP-P03-B2-R1 (artifact `31d16940…39ea`)

Full digest `31d169402e1d795115670b31b6399674904c1f0d41463afb0bdbe0c1378139ea`, same exclusions.

- **F1**: B2 tests renamed to the plan's `department_lead_desk_` / `department_lead_desk_competing_`
  (14 / 1; the p02 `department_desk_*` tests are no longer selected by them). The mapping above
  reads with the new prefix.
- **F2**: `own_desk` (agent:{id} desk, binding state, selection) asserted unchanged across creation
  as lead (`…_with_an_agent_lead_answers_on_it`), agent→agent for both leads
  (`…_agent_to_agent_repoints_the_same_binding`), agent→person→agent
  (`…_follows_person_and_agent_leads_with_one_binding`); an agent created as lead keeps its own
  desk and binding, active and selected (`…_follows_an_agent_created_as_its_lead`).
- **F3**: `department_lead_desk_leaves_a_users_own_responder_alone`: a user's own binding (same
  version, configured and activated through `BindingService`, selected with
  `configure_channel_reply`) stays selected and unchanged through agent→agent and agent→person;
  the owned binding is unchanged by agent→agent and paused by agent→person.
- **F4 (optional, done)**: the pause path deactivates B before clearing the selection (binding
  rows, then desk row and settings, as the gate's lock list).
- Remaining stand-in gaps: no-`draft`-step and resource-slot refusals (code review, accepted by B).

| Command | Result | Log |
| --- | --- | --- |
| `cargo test … department_lead_desk` | 14 passed | `logs/B2-R2-lead-desk-1.txt` |
| `--list department_lead_desk agent_default_desk` | 14 + 23 | `logs/B2-R2-test-inventory.txt` |
| `-- department onboarding provisioning channel_reply agent_default_desk department_lead_desk binding` | 502 passed | `logs/B2-R2-affected.txt` |
| clippy `--lib --tests -D warnings`, `fmt --check` | clean | (inline) |
| test names since 523732b7 | none removed | `logs/B2-R2-test-names.txt` |

## Architecture gate B3 — decisions (revision r2, 2026-11-10)

Plan 03C.0 (directory filter), 03C.1-6, README decisions 4/5/10; coordination r5 (pinned
`logs/coordination-r5.md`): **C003** (roster in the run context; directory tool stays admin-only)
and **C004** (dispatch, escalation, round-trip and loop checks deferred to `channel.call`; no core
messaging change) are user decisions taken at B3 startup. Builds on B1 `523732b7`, B2 `01608b1f`.
r1 preserved in `logs/gate-B3-r1.md`; r2 answers `01DEP-P03-G3-F1..F4`.

Plan/code mismatches: `AgentDirectoryEntry` is the authoring picker's list, not model-facing; the
model-facing directory is the provisioning tool `list_company_agents` (admin requester only, C003);
the intake schema of agent-workflow-call does not exist; `message.send` to a desk does not start
that desk's workflow and a reply back to the lead is a loop refusal (C004).

### DEP-P03-D10 Directory fields and the department filter
- `list_company_agents` (`application/workflow/provisioning.rs` schemas, `persistence/workflow/
  provisioning.rs` `list_on`): input gains optional `department` (slug); with it, only agents that
  are members or role holders of that department are listed (unknown slug → `Invalid` tool
  error). Each listed agent gains `departments: [{slug, name, roles}]` (its memberships and held
  roles, ≤ 16, by slug) and `desk: {id, address} | null` -- the channel its `agent:{id}` record
  names, at its current primary slug, `null` when that desk was deleted or no longer exists (never
  recreated). Bounds: the existing 48 KiB budget and truncation flag, names cut as today. The
  model-facing description of the tool (`PreparedTool::declaration`, provisioning target) states
  that a department's members are the agents listing that department, and that an agent's `desk`
  is where its own replies come from. Frozen builtin snapshot changes → DB reset.
- `AgentDirectoryEntry` (authoring picker) gains `departments: Vec<DepartmentRef { id, name, slug,
  charter, roles }>` and `desk: Option<String>` (the address as above); page key (slug) unchanged.
  Rendering is phase 04; the JSON shape grows.

### DEP-P03-D11 Department context in `agent.run` (C003)
- For an `agent.run` whose run's channel (`workflow_runs.channel_id`) is owned by a department
  (`channels.department_id`), preparation reads a `DepartmentContext { name, slug, charter, desk,
  parent, children, members }` on its transaction (plain reads): the department's own desk
  (`department:{id}` record, while it still belongs to it), its parent's name and desk, its child
  departments' names and desks (≤ 20, by slug), and its agent members and role holders with their
  own desks from `agent:{id}` (≤ 50, by slug); desks as `{id, address}`; truncation noted in the
  text. People are not listed (no desk).
- `PreparedAgent` gains `department: Option<DepartmentContext>` (serialized); the preamble is a
  pure function of the selection and the department (`preamble(selection, department)`), and
  renders the department **after the agent's own instructions and before skills**: name and
  charter; when the run's agent holds the department's `lead` role, one sentence that it answers
  as the department's lead (G3-F2); then the roster. The charter and names are company-managed,
  so they are instructions, not untrusted data. The text lists addresses only; it does not say a
  send to a desk starts that desk's work (C004).
- **Public desks (G3-F3):** on a department-owned desk whose access mode is public, the context
  carries the name, charter and lead sentence but no roster, so outsiders' conversations never
  get internal desk addresses.
- **Frozen once (G3-F1):** the preamble is a stored field and its length feeds the history budget
  (`agent_context.rs` budget, `with_frozen_history`'s budget check). `restore_on` therefore
  restores in this order: build the candidate with the live department → **replace it with the
  saved `department` and rebuild the preamble** (`PreparedAgent::with_department`) →
  `with_frozen_history(saved history)` → compare the whole document. A first preparation builds
  with the live department and freezes the result. So a charter or roster edit between attempts
  neither fails the retry nor changes what it sends. Runs without frozen history read it each
  preparation, as they do the agent.

### DEP-P03-D12 `department-lead.yaml` fixture
- `src/application/workflow/fixtures/department-lead.yaml`: `context.history` → `agent.run` (agent
  literal `lead`, the base's untrusted context only) → `data.map` reply. Its header says the lead
  framing and roster come from the run's department context (D11, trusted system text), not from
  the workflow, and that dispatch waits for `channel.call` (C004). Added to `fixtures::ALL` with
  snapshots for the agent key `lead`, so the fixture tests and the sample tests compile it.
- Plan/code mismatch (G3-F2): there is no authoring sample list apart from the shipped template
  catalogue (`TemplateCatalogue::shipped`, two templates); the fixture is not offered there (it
  would offer a workflow whose dispatch is deferred). Not a base template; `autonomous-response.yaml`
  stays the base.

### DEP-P03-D13 Docs and records
- `docs/agent-builder-context.md`, `docs/custom_tools.md`: Builder `department`/`role` inputs and
  the default/second desk rule; `list_company_agents` fields and filter; admin-only requester.
- `docs/inter_channel_agent_communication.md`: a channel destination of `message.send` posts into
  a new thread on that desk and starts none of its bindings; a reply back along a relay chain to
  the desk that started it is a loop refusal; desk-to-desk dispatch waits for `channel.call`
  (C004).
- `docs/agent-authoring.md`: the department context a run on a department-owned desk gets; it
  follows the desk's ownership, not the agent's membership -- a member agent's own default desk is
  unowned, so its runs there get none (G3-F4); public desks get no roster.
- Plan file 03: a "Status after p03" note under 03C items 2-4 citing C004.

### Verification plan B3 (`department_directory_`, `department_context_`)
| Criterion | Assertion |
| --- | --- |
| directory fields | `list_company_agents`: member and lead list their departments and roles; an agent in none lists `[]`; `desk` = its default desk address; after that desk is deleted, `desk` = null and no desk is recreated |
| directory filter | `department` slug lists exactly its members and role holders; unknown slug → invalid tool error |
| directory description | the provisioning tool declaration carries the department sentence |
| picker entries | `AgentDirectoryEntry` lists departments with charter and roles and the desk address; page key (slug) and paging unchanged |
| context content | a run on a department-owned desk: preamble = agent instructions, then department name/charter, the lead sentence for the lead's run only, the roster (own desk, parent desk, child desks, member desks), then skills; a run on an unowned desk has none; a public owned desk has no roster |
| context bound | roster capped with a truncation note |
| context frozen | the charter (and so the preamble length) edited between first preparation and a retry: the retry restores -- neither "Frozen history differs" nor "Frozen agent preparation changed" -- and sends the original charter |
| fixture | `department-lead.yaml` compiles in the fixture and sample tests |
| C004 | records and docs state the deferred checks; no `department_lead_round_trip_` / `department_lead_loop_` tests |

### Contract impact B3
| Changed contract | Producers / consumers | Disposition |
| --- | --- | --- |
| `list_company_agents` input/output schema | builtin snapshot, frozen agent revisions, Builder prompt/docs, existing listing tests | updated; DB reset |
| `AgentDirectoryEntry` JSON | authoring picker routes/tests | fields added |
| `PreparedAgent` serialization / frozen preparation | agent history freeze/restore, harness | `department` added; restore takes the saved one |
| docs | as D13 | updated |

## B3 implementation (artifact `241d6d8d…24e9`, base `01608b1f`)

Full digest `241d6d8d955a1f236e384677c12f66cb37918ff980e2b3e1fef596b69c1524e9`; exclusions as B1 (phase records dir and COORDINATION.md).

- **D10** `list_company_agents`: input `department` (slug; unknown → `Invalid` "No department has
  the slug '…'"); each agent lists `departments: [{slug, name, roles}]` (≤ 16, `MAX_LISTED_DEPARTMENTS`;
  more sets `truncated`) and `desk: {id, address} | null`; the model-facing description is
  `LIST_COMPANY_AGENTS_DESCRIPTION` (`PreparedTool::declaration`). `AgentDirectoryEntry` gains
  `departments: Vec<DepartmentRef { id, name, slug, charter, roles }>` and `desk: Option<String>`;
  the picker route reads the selected agent's entry through `agent_directory_entry` instead of
  building a partial one. Shared reads: `src/adapters/persistence/department_roster.rs`
  (`agent_departments_on`, `agent_desks_on`, `DeskAddresses`): an agent's desk is the channel its
  `agent:{id}` record names, at its current primary slug (`channel_slugs.is_primary`), while it
  exists; a department's desk is the channel its `department:{id}` record names while the
  department owns it.
- **D11** `DepartmentContext { name, slug, charter, lead, roster: Option<DepartmentRoster> }`
  (`application/workflow/agent_department.rs`; roster: own desk, parent, ≤ 20 children, ≤ 50
  agents with lead flag and desk, `more_*` flags). `PreparedAgent.department` (serialized);
  `PreparedAgent::with_department` rebuilds the preamble (`preamble(selection, department)`),
  refused for non-`agent.run` steps and after frozen history. Preparation (`candidate_on`) reads
  it with `department_context_on` for an `agent.run` whose run has a channel. Public desk → no
  roster. `restore_on` applies the saved `department` before `with_frozen_history` (G3-F1).
  The lead flag is stored (B's G3-R2 note), so the lead sentence needs no roster.
- Desk addresses need the app domain in preparation and authoring reads:
  `PostgresPersistence::with_app_domain` (set by `WorkflowEngine::new` beside the reply notices);
  `DeskAddresses` asks for it only when a desk is listed, so readers that list no desk need none.
- Prepared-document `VERSION` stays 5: the `department` key is additive (a document without it
  restores as `None`), and the init schema pins version '5' in `agent_history_preparation_valid`
  and the checkpoint check, so a bump would be a schema change with no reader that needs it.
- **D12** `fixtures/department-lead.yaml` (`Fixture::DepartmentLead`, version 108, agent key
  `lead`, step `lead`), in `fixtures::ALL`; not in `TemplateCatalogue::shipped`.
- **D13** `docs/agent-builder-context.md` (Builder tools, default/second desk, department/role,
  listing fields/filter, owner/admin only), `docs/custom_tools.md`,
  `docs/inter_channel_agent_communication.md` (C004 note), `docs/agent-authoring.md` (context,
  ownership not membership, public desks, retry), plan 03 "Status after p03" under 03C item 4.

### Verification mapping B3 (actual)

| Criterion | Test (decisive assertion) |
| --- | --- |
| directory fields | `department_directory_lists_departments_roles_and_desks` (lead: `[{sales, Sales, [lead]}]`, member `roles: []`, none → `[]`; desk = `{id of agent:{id} record, slug@company.domain}`; recorded desk deleted → `null`, channel count unchanged) |
| directory filter | `department_directory_filters_by_one_department` (sales → [closer, scout]; support → [helper]; none → all four; unknown → `workflow.invalid_input` with message) |
| directory description | `department_directory_tool_description_names_departments_and_desks` (declaration = `LIST_COMPANY_AGENTS_DESCRIPTION`; `department` parameter) |
| picker entries | `department_directory_picker_entries_name_departments_and_the_desk` (page and `agent_directory_entry` give `DepartmentRef` with charter and `[Lead]`, desk address; `next` None) |
| context content | `department_context_follows_the_instructions_and_precedes_skills` (exact preamble prefix: instructions, department, charter, lead sentence, roster lines, then skill, then untrusted notice; document carries `department`); `department_context_roster_lists_the_desks_around_the_department` (DB: own/parent/child/agent desks by address, lead flag per agent, people not listed; member's own desk → None; public → roster None); `department_context_reaches_the_lead_on_its_department_desk` (e2e: the scripted request's system text carries the exact block; the member's own-desk run has none) |
| lead sentence / public | `department_context_names_the_lead_only_for_its_run_and_no_roster_on_a_public_desk` |
| agent runs only | `department_context_belongs_to_agent_runs_only` |
| context bound | `department_context_roster_is_bounded` (21 children / 51 agents → 20 / 50 by slug, `more_*` true) |
| context frozen | `department_context_frozen_preparation_replays_the_department_it_froze` (longer charter between prepare and retry: retry Ready, same preamble and prepared document, frozen row unchanged); mutation check: without the saved department the retry fails "Frozen history differs from the prepared request" |
| fixture | `department_lead_fixture_runs_as_a_sample` (Completed, output `{body}`, 1 model call); `workflow_fixtures_all_compile_and_freeze_with_real_child_content`, `workflow_fixtures_taking_a_message_accept_the_standard_message_input`, `workflow_storage_restores_all_frozen_fixtures_and_child_identity` iterate `ALL` |
| C004 | no `department_lead_round_trip_` / `department_lead_loop_` tests; docs and plan note |

Stand-in gaps: the deleted-desk case uses a recorded plain desk, because a default desk with its
binding cannot be deleted through the product path (`ChannelPersistence::delete` refuses a desk with
bindings); the read under test is the same. Picker JSON rendering is phase 04.

### Checks B3 (`openbot_dept_p03_test`, unchanged schema; SQLX_OFFLINE=true; 4 threads)

| Command | Result | Log |
| --- | --- | --- |
| affected: `agent_history_ provisioning department_ agent_default_desk_ workflow_agent_ workflow_model workflow_classify workflow_decide picker authoring sample workflow_fixtures_ workflow_storage_ ingress autonomous_agent channel_reply knowledge_e2e agent_tests onboarding builder agent_preparation` | 849 passed, 7 failed, 6 ignored: `refusals_and_results_are_both_valid_receipts` (its sample listing lacked the now-required `departments`/`desk`: contract change, test updated and extended); 6 `cloudroom_*` local-agent e2e tests (matched by `agent_tests`) panic "build the counted Node e2e peer before Rust tests" -- `integrations/cloudroom-plugin/build-tests/` absent here; local agents use their own preparation (no `PreparedAgent`) and have no diff since 01608b1f: **unavailable, unrelated** | `logs/B3-affected-1.txt` |
| consumers of the listing: `agent_api_tests catalog_tests workflow_tool_registration agent::contract_tests registry::message_tests application::workflow::provisioning::tests ui_agents routes::agent routes::onboarding ui_companies` | 59 passed, 1 ignored | `logs/B3-affected-2.txt` |
| focused final: `department_directory_ department_context_ department_lead_fixture workflow_fixtures_ workflow_storage_ provisioning_lists refusals_and_results` (`--locked --offline`) | 38 passed | `logs/B3-focused-final.txt`, `logs/B3-test-inventory.txt` |
| mutation: `restore_on` without the saved department | FAIL as expected ("Frozen history differs from the prepared request"); restored | `logs/B3-frozen-mutation.txt` |
| `clippy --all-targets -D warnings`; `fmt --check` | clean (after two row type aliases) | (inline) |
| test names since 01608b1f | none removed; 12 added | `logs/B3-test-names.txt` |

Not run: the full gate (`scripts/check.sh`, CI-only gates) per the user's "focused checks only"
(I003); migrations unchanged in B3.

## Status

- 2026-11-10: p03 started from 9291fcd5; B1 gate r1 written, awaiting B.
- 2026-11-10: G1-R1 changes required (F1–F3 Medium, F4 Low, F5 optional). r2 written: placement
  last with the full lock sequence; Builder savepoint + refusal mapping + name rule; onboarding
  deterministic department id and no-model outcome; source scan of the real insert functions;
  LibraryAgentCopyPersistence removed; F5 rows. Awaiting recheck.
- 2026-11-10: G1-R2: F1–F3, F5 resolved; F4 (Low) still open. r3: `AgentPersistence::create`
  and its impls `#[cfg(test)]`; skills lock wording `FOR SHARE`; onboarding edited-name replay
  asserted. Awaiting recheck G1-R3.
- 2026-11-10: G1-R3 **PASS** for r3 (`logs/gate-B1-r3.md` 14f1ffe0…5aef); no open findings.
  B1 implementation started.
- 2026-11-10: B1 implemented; focused + affected checks pass; artifact 2f77ca9f…4279 frozen; batch
  review 01DEP-P03-B1-R1 requested.
- 2026-11-10: B1-R1 changes required (F1, F2 Medium; F3, F4 Low; F5, F6 optional). All six
  addressed; artifact a75c40e7…5798; recheck 01DEP-P03-B1-R2 requested.
- 2026-11-10: B1-R2 **PASS** at a75c40e7…5798; root recomputed digest, coordination r3 unchanged,
  manifest valid. **B1 accepted in phase**; committed locally.
- 2026-11-10: B2 gate r1 (DEP-P03-D7..D9) written; G2-R1 requested.
- 2026-11-10: G2-R1 changes required (F1–F4 Medium, F5 Low, F6/F7 optional). r2 written: desk effect
  before principals/gates with full sequence; run-as = actor and worded refusals; re-point from the
  bound version, refuse unpublished edits; owned binding recorded in `department:{id}` for both
  desks; contract impact; F6 reuse paused binding; F7 rows. Awaiting G2-R2.
- 2026-11-10: G2-R2 (against r3): r3 ordering confirmed; G2-F2 residual + G2-F8 open, F9 optional.
  r4: reconfigure B to run as the actor before any activation/selection; compare draft source with
  the bound version's; worded remedy; F9 deactivate active B when the lead is not an agent; lock
  list in procedure order. Awaiting G2-R3.
- 2026-11-10: G2-R3 PASS (r4). B2 implemented; artifact 8fd91d04…5c9b; batch review 01DEP-P03-B2-R1
  requested.
- 2026-11-10: B2-R1 changes required (F1–F3 Low, F4 optional); all four addressed; artifact
  31d16940…39ea; recheck 01DEP-P03-B2-R2 requested.
- 2026-11-10: B2-R2 **PASS** at 31d16940…39ea; root recomputed digest, coordination r3 unchanged,
  manifest valid. **B2 accepted in phase**; committed locally.
- 2026-11-10: user decisions C003 (roster in context) and C004 (dispatch deferred to channel.call);
  coordination r5. B3 gate r1 (DEP-P03-D10..D13) written; G3-R1 requested.
- 2026-11-10: G3-R1 changes required (F1 Medium, F2 Low, F3/F4 optional). r2: restore order
  (saved department → rebuilt preamble → frozen history → compare); lead framing in the trusted
  context; fixture test-only, no catalogue entry; public desks without roster; docs note.
- 2026-11-10: G3-R2 **PASS** for gate r2 (`logs/gate-B3-r2.md` 4aa241d5…357c); G3-F1..F4 resolved.
  B note for the batch: the lead sentence derives from a stored lead flag in the saved
  `DepartmentContext`, not from roster membership. B3 implementation started.
- 2026-11-10: B3 implemented (D10–D13); artifact 241d6d8d…24e9 (base 01608b1f); evidence
  `logs/01DEP-P03-B3-R1.evidence.json` valid; batch review 01DEP-P03-B3-R1 requested.
- 2026-11-10: B3-R1 changes required (B3-F1 Low, docs): library-copy sentence in
  agent-builder-context.md replaced (every copy gets its own desk, as rig.md says); "Later edits…"
  moved back to the create paragraph in agent-authoring.md; optional: custom_tools.md button label
  "Create assistant desk". Docs-only; artifact 0db98506…dbf1; recheck 01DEP-P03-B3-R2 requested.
- 2026-11-10: B3-R2 **PASS** at 0db98506…dbf1; root recomputed the digest, evidence manifest
  valid, coordination r5 unchanged through review. **B3 accepted in phase**; committed locally.
  Coordination r6 (`logs/coordination-r6.md`, dff5898e…efe0): C003 and C004 p03 responses
  satisfied. **p03 accepted in phase** (B1, B2, B3). Full repository gate not run (I003).
