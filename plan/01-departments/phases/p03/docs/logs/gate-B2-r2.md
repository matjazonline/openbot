## Architecture gate B2 — decisions (revision r2, 2026-11-10)

Plan 03B; README decisions 4, 8, 9, 12; coordination C001/C002 unaffected. Builds on accepted B1
(`523732b7`: `create_company_agent_on`, `place_in_department_on`/`write_placement_on`,
`Provisioning.agent_slug`). r1 preserved in `logs/gate-B2-r1.md`; r2 answers
`01DEP-P03-G2-F1..F7`.

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
- **Where (F1):** inside `write_placement_on` (role `lead`) and `clear_department_role_on`
  (`lead`), **right after the department row lock and before the principals, rows and grant
  sync**, so the desk gates stay the last locks. Sequence: account(actor) `FOR SHARE` → company →
  department row `FOR UPDATE` → [desk effect: the lead agent row `FOR UPDATE` (publication),
  workflow draft/version rows, binding head/revision rows, the desk's reply settings row; the only
  account it authorizes is the actor's, already held] → principals `FOR KEY SHARE` sorted → member
  and role rows → one sorted gate acquisition → channel rows.
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
  - new lead is a person, or the role is cleared: S = B and B active → deactivate B and clear the
    selection (`configure_reply_on`, responder `None`; a selected inactive responder makes every
    inbound message a visible refusal). Anything else → no desk change (a user's own responder is
    never paused; person→person and person→none change nothing).
- **Re-point (F3):** the source re-targeted is **B's bound version's** source
  (`retarget_agent(REPLY_AGENT_STEP = "draft", slug)`), never the current draft blindly: if the
  workflow's current draft revision ≠ the bound version's `draft_revision` (unpublished edits) →
  refuse `BadRequest("The department desk's workflow has unpublished edits; publish or discard
  them before changing the lead.")`. Otherwise save the retargeted source as the next draft
  (`save_draft_on`, factored out of `save_draft`), compile/capture with `compiled`, publish a new
  version (`provisioned_publication` + `publish_on`), `publish_initial_on(A)` first; reconfigure
  B (configuration revision + 1, same params and trigger, run-as = actor, same active flag; new
  `provisioned_reconfiguration` beside `provisioned_binding`), then activate if paused, then select
  with the settings' current contract (`configure_reply_on`, factored out of
  `configure_channel_reply`).
- **Run-as and refusals (F2):** B's run-as becomes the actor: attribution of the department desk's
  answers moves to whoever made the lead change (as for any binding reconfigured by an admin).
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

