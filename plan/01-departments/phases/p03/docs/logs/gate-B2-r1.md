## Architecture gate B2 — decisions (revision r1, 2026-11-10)

Plan 03B; README decisions 4, 8, 9, 12; coordination C001/C002 unaffected. Builds on accepted B1
(`create_company_agent_on`, `place_in_department_on`/`write_placement_on`, `Provisioning.agent_slug`).

### DEP-P03-D7 Templates reach department writes
- `create_department_on`, `create_department_with_id_on`, `assign_department_role_on`,
  `clear_department_role_on`, `place_in_department_on` take `templates: &ProvisioningDeployment` and
  `actor: WorkflowActor` (the desk's binding runs as the actor who made the change; the actor is
  authorized under the company lock on every path: department routes (`begin_department_write`),
  company-agent creation, onboarding session, Builder action).
- `DepartmentPersistence` moves from `PostgresPersistence` to `PostgresDepartments { persistence,
  templates }` (persistence/department.rs), built by `WorkflowServices::departments()` and passed to
  `DepartmentUseCases::new` in setup; reads delegate unchanged. Tests construct it with
  `test_support::provisioning_templates()`.

### DEP-P03-D8 The department desk at creation (03B.1/2)
- After the department row (and before any role write), in the same transaction:
  - lead is an agent principal → its agent slug → `assistant_channel_on(Existing(slug),
    "department:{id}", slug/name = the department's, Autonomous)`; refusal → AppError
    (Conflict/BadRequest/NotFound) → caller rolls back; then `set_desk_ownership_on(desk,
    department_id = id, members_only = false)`;
  - lead is a person or none → team desk: `channel_write` defaults (company `add_3rd_party`,
    enabled) with `ownership = {department, members_only: false}`, `insert_channel_row_on` +
    `insert_channel_slugs` + `write_channel_email_binding`; taken address pre-checked
    (`channel_slugs`) → `Conflict("The address '{slug}' is already used by a desk in this
    company")`; unique violation also → Conflict.
  - then the lead role write (`write_placement_on`, member = false) whose desk effect (D9) finds the
    desk already answered by that agent and changes nothing.
- Lock order: company (held) → parent FOR SHARE → new department row → desk rows (new; the agent
  row FOR UPDATE via publication) → role write: department FOR UPDATE, principals KEY SHARE, one
  sorted gate acquisition (own desk is members_only false, so only neighbours' members-only desks)
  → channel rows. The desk's own gate in `set_desk_ownership_on` is a new channel's (uncommitted, no
  contention).
- Department desk identity (`department_desk_on`): the channel recorded by the `department:{id}`
  command row when it still exists and still has `department_id = id`; else the channel with
  `department_id = id` whose primary slug equals the department's slug; else none (detached or
  deleted — later role changes then touch no desk). Department rename does not rename the desk.
- Onboarding's first department and every route/use-case create get a desk. Department delete:
  unchanged p02 rule (refused while it owns desks), so the desk must be detached or deleted first
  (decision 9).

### DEP-P03-D9 Re-leading (03B.3)
After every lead-role write (`write_placement_on` with Lead, `clear_department_role_on` Lead), with
the department desk D (D8) present, under the same transaction, after the grant sync:
- **desired**: the lead is an agent → D's responder runs that agent, active; otherwise → D's
  responder binding (if any) paused.
- Responder = D's channel reply settings' selected binding (`settings_on`).
- Cases:
  1. agent lead, responder binding exists → **re-point**: if its workflow's `reply`
     (`REPLY_AGENT_STEP`) already names the agent and the binding is active → nothing. Else: read
     the workflow's current draft; `retarget_agent(REPLY_AGENT_STEP, slug)`; save the draft
     (new `save_draft_on`, factored out of `save_draft`) at revision+1; `compiled` (the
     provisioning compile/capture) at a fresh version id; `provisioned_publication` + `publish_on`;
     `publish_initial_on(new agent)` first; reconfigure the binding to the new bundle
     (configuration revision+1, same params/run_as/trigger, same active state; new
     `provisioned_reconfiguration` beside `provisioned_binding`), then activate if paused;
     re-select reply settings to the new binding revision with the same contract and approval
     policy (new `configure_reply_on`, factored out of `configure_channel_reply`). A workflow
     with no `reply` agent step (the user replaced it) → `BadRequest("The department desk's
     workflow has no agent reply step to hand to the new lead; edit its workflow or detach the
     desk first.")`, whole role change refused.
  2. agent lead, no responder (person-lead desk, or settings cleared) → provision the autonomous
     workflow and binding on D through `provision_workflow_on` (made reachable for an existing
     channel) with a fresh key `ProvisioningKey::command(company, "department:{id}:lead:{uuid}")`;
     reply settings bootstrapped when absent, else selected through `configure_reply_on`.
  3. person lead or no lead, responder binding exists → deactivate it (`save_binding_on`
     Deactivate) and clear the selection (`configure_reply_on`, responder `None`): an inactive
     *selected* responder makes every inbound message a visible refusal
     (`channel_reply_binding_inactive_before_ingress_has_visible_refusal_until_selection_cleared`),
     so the paused desk becomes a plain team desk a person reads.
  - A later agent lead on that desk is case 2 (no responder): a fresh workflow and binding; the
    paused one stays (an agent→person→agent cycle leaves one inactive binding per cycle; desk
    deletion already refuses a desk with bindings, unchanged).
- Failure: any error aborts the role write's transaction (route → status; D1 creation → whole
  creation refused).
- `factorings needed` recorded in the plan file afterwards: save_draft_on, configure_reply_on,
  provisioned_reconfiguration, provision_workflow_on for an existing channel.

### Verification plan B2 (`department_desk_`, `department_desk_competing_`)
| Criterion | Assertion |
| --- | --- |
| 03B.1 agent lead | one desk at dept slug/name, department_id = dept, members_only false, published autonomous workflow on the lead, active binding, `department:{id}` row |
| 03B.1 person/no lead | team desk at slug, department_id set, no workflow/binding/command |
| 03B.1 refusal | taken address → Conflict; no department, desk, workflow, binding, role |
| 03B.2 identity | renamed department keeps desk; detached desk → role change touches none |
| 03B.3 agent→agent | same binding id, new configuration revision on a new version whose `reply` step names the new agent; reply settings select it; e2e: a scripted message to the department desk is answered, before and after the re-lead, by the current lead |
| 03B.3 agent→person/cleared | binding inactive, responder cleared; desk still exists; a message is stored with no refusal outcome |
| 03B.3 person→agent | workflow + active binding on the existing desk; reply settings select it |
| 03B.3 agent→person→agent | new active binding selected; the old one stays inactive |
| customized workflow | re-lead refused BadRequest, nothing changed |
| agent created as lead (B1 D1 + D9) | new agent's desk and department desk both answered by it |
| competing | two concurrent re-leads of one department: serialized by company lock; final responder = last writer; one active binding |
| delete | department owning its desk → refused (p02); after desk delete → ok |

