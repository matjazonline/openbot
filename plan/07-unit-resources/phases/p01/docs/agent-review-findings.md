# 07UR-P01 agent review findings

Shared context for every finding below unless stated otherwise: workflow
implement-opus-reviewed-2_4; task [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md); phase
07UR-P01, worktree /Users/mac02/orca/workspaces/openbot/plan06 (branch plan07); stage
architecture gate G1, decision revision r1 (TASK.md "Architecture gate G1 (decision revision
r1)"); review round [07UR-P01-G1-R1](agent-review-notes.md#07ur-p01-g1-r1-2026-11-20); reviewer
Claude Opus 5.5 (B); reviewed HEAD cc545881, digest
c2d338160c48d4d2eabaa84b5bd5fa5cd389b58d2a93db85478eb2e05fb9ac33 (exclusions
plan/07-unit-resources/phases/p01/docs, plan/07-unit-resources/COORDINATION.md); date
2026-11-20. Line numbers are at cc545881. All findings originate in the gated decision text
(no implementation exists yet).

## 07UR-P01-G1-F1 — D6 puts the captured unit inside the immutable binding identity

- **Classification:** actionable defect; medium; architecture / persisted format.
- **Evidence:** D6 says `AgentBindingSource::{Http,Mcp}` gain `unit_id`. That enum is not
  only revision content; it is the identity of an agent capability binding:
  - `agent_capability::bindings_on` (src/adapters/persistence/agent_capability.rs:10-27)
    deserializes drafts from SQL `agent_binding_source(binding)` (init_schema:12654-12665), which
    is built from `agent_capability_bindings` columns; the row has no unit, and the enum is
    `deny_unknown_fields` (src/domain/entities/agent_capability.rs:140-161).
  - `insert_identity_on` (agent_capability.rs:69-104) requires
    `agent_binding_source(binding) = <serialized source>` or returns "A binding identity cannot
    change its owner, alias or target"; `resolve_bindings_on`
    (agent_binding_selection.rs:11-46) reuses a previous binding id only when
    `binding.source == source`.
  - The deferred constraint trigger `check_agent_revision_members` (init_schema:12781-12812)
    requires every revision's `tools[i].binding` to equal exactly
    `{id, alias, source: agent_binding_source(binding)}`.
  A unit inside `source` therefore either cannot be produced for drafts, or (if derived from the
  connection's current unit) makes a move retarget the binding identity, so a moved connection
  yields a fresh binding id and loses its `agent_binding_grants` row; and every published revision
  with an HTTP/MCP tool fails `check_agent_revision_members` at commit. D11's reader inventory
  names only the preparation-history validators, not this trigger, `native_mcp_tool_source_live`
  (init_schema:19800-19830) or the `native_mcp_brokers.contract` equality
  (workflow/native_mcp/registry.rs:36-38).
- **Required:** capture the unit beside the binding in revision content only (for example
  `ResolvedAgentTool.unit_id`, a sibling of `binding`/`policy`), keep `AgentBindingSource` and
  `agent_binding_source` unchanged, and list `check_agent_revision_members`, the native MCP SQL
  reader and broker contract equality in D11. `SkillSnapshot.unit_id` is unaffected by the
  trigger (it compares only `assignment_id`/`skill_id`).
- **Instruction lesson:** observation: the gate chose a field location without tracing the
  type's other producers (draft identity) and the SQL equality over the same JSON. Candidate
  (persistence AGENTS.md, "Treat persisted JSON..."): before adding a field to a persisted enum,
  list every SQL function that rebuilds or compares that JSON, not only Rust deserializers.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R2 (decision revision r2, same digest): **fixed in decision.** r2 captures `unit_id` on `ResolvedAgentTool` and `SkillSnapshot`/`ResolvedAgentSkill`, leaving `AgentBindingSource`/`agent_binding_source` unchanged. Verified: `check_agent_revision_members` compares only `item->'binding'` and `assignment_id`/`skill_id` (init_schema:12781-12805); no SQL key-set validator over tool or skill JSON exists (searched `'approval_required'`, `'skill_id'`, `'required_tools'`); the native broker contract is per revision, so a captured unit does not change it within a revision. D11's exhaustive reader search still applies at batch review.

## 07UR-P01-G1-F2 — Unslotted HTTP resolution stays company-wide and is re-resolved at runtime without a unit check

- **Classification:** actionable defect; high; security / authorization.
- **Evidence:** D6 keeps `http::registered_on` company-wide ("exactly one enabled connection with
  an equal operation snapshot"; src/adapters/persistence/workflow/http.rs:33-53) and D9 keeps
  "dispatch checks unchanged". But unslotted HTTP tools are re-resolved on every call:
  `http_step_request` (`tool.call`, workflow/http_runtime.rs:166) and `http_tool_request`
  (http_runtime.rs:236) call `registered_on` again, and `authority.rs:174-186` does the same for
  admission/agent-tool closures. Consequences once unit leads can create connections and register
  operations (user decision (a)):
  - an admitted run of unit A whose connection is later disabled re-resolves to a sibling B's
    enabled connection with the same operation snapshot and sends with B's credentials; no unit
    check runs at that point (README decision 1, "Siblings never share");
  - a sibling enabling an identical operation makes resolution ambiguous and fails closed for
    unit A's admitted runs and new admissions (cross-unit interference introduced by the widened
    authority);
  - D9a checks only the connection resolved at admission time, which the runtime does not pin.
- **Required:** make the unslotted candidate set unit-scoped wherever it is resolved (binding
  save, admission closure, and both runtime paths): only connections whose unit is in the run's
  base read set (`workflow_run_scopes`, acting_principal_id NULL, access 'read') participate,
  "exactly one" applying within that set; or pin the admission-resolved connection id for the
  run and require it at runtime. Add a selection test with two units holding identical
  operations (no cross-unit ambiguity, no cross-unit use after disabling one).
- **Instruction lesson:** observation: a company-wide "exactly one owner" resolution was
  preserved while its trust boundary changed from company to unit. Candidate (plan review
  scope): when an authority boundary narrows, list every company-wide resolution by
  name/snapshot as a selection surface, including runtime re-resolution.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R2: **fixed in decision.** r2 makes `http::registered_on` reach-scoped (only enabled in-reach connections; ambiguity judged within reach) at binding save, admission and both runtime paths (`http_runtime.rs:166,236`, `authority.rs:174-186`); runtime reads the run's base scope rows, and child runs copy them (`run_scopes::copy_child_on`, children.rs:376, verified). Planned check `unit_resource_selection_unslotted_*` covers sibling enable and mid-run disable. The reach context for the five `resources::check_readiness` callers is tracked in 07UR-P01-G1-F13.

## 07UR-P01-G1-F3 — D9b refuses resources moved up, though 01B.3 refuses only moves out of reach

- **Classification:** actionable defect; medium; requirements.
- **Evidence:** 01-ownership-and-selection.md 01B.3: admission "refuses with
  `admission.resource_outside_unit` when a resource has since moved out of reach". README
  decision 1: "sharing is moving the resource up". D9b additionally requires current unit =
  captured unit ("even upwards: republish"), and the acceptance mapping row 01B.3 plans to assert
  refusal "after a move up". A move up keeps the resource in every descendant's read set, so this
  refuses runs the plan admits, and makes sharing break every existing agent that uses the
  resource until each is republished. It is also inconsistent with D9a, which checks
  workflow-level connections only for current-unit-in-read-set. No security property depends on
  the equality: refusing when the current unit is outside the agent's read set already rejects
  moves to siblings or below the agent's reach (`run_scopes` read set,
  workflow/run_scopes.rs:42-77, 122-170). The captured unit still makes a move visible as a
  mismatch (01A.2) for diagnostics and for the D4 affected-uses list.
- **Required:** refuse when the current unit is not in the agent's read set (optionally report
  captured vs current); or obtain an explicit user decision for the stricter rule and record it
  in coordination. Adjust the 01B.3 mapping row (move up → admitted, move to sibling → refused).
- **Instruction lesson:** observation: a plan phrase about visibility ("visible as a mismatch")
  was read as a refusal condition that contradicts the plan's explicit refusal condition. Cause
  otherwise unknown; no instruction change supported.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R2: **fixed in decision.** r2 D9b refuses only when the current unit is outside agent reach; moved-up stays usable; captured ≠ current is reported, not refused. Consistent with 01B.3 and with user decision (d). The retained r1 mapping row 01B.3 still says refusal after a move up; tracked in 07UR-P01-G1-F13.

## 07UR-P01-G1-F4 — Native MCP runs use captured agent connections outside the D9 admission check

- **Classification:** actionable defect; medium; requirements / security (scope disposition).
- **Evidence:** D9 places the unit check only in `admission_write::create`. Native MCP calls from
  a local agent harness create workflow runs through `native_mcp::admit_on`
  (src/adapters/persistence/workflow/native_mcp/mod.rs:89-140, called from
  local_agents/protocol/mcp.rs:30-47) using `admission_write::insert_run` and
  `run_scopes::native_on` (run_scopes.rs:286-341). The tool executed is a `ResolvedAgentTool`
  from the agent revision selected by `NativeHarnessSelection::Agent { revision_id }`
  (native_mcp/registry.rs:13-21), i.e. a captured HTTP/MCP connection. Conversation-origin
  native sessions never pass a workflow admission, so a connection moved out of the agent's
  reach stays usable there. The gate does not mention this path; user decision (b) puts
  `local_machines` as unit resources out of scope, not the unit check on company connections
  used from them.
- **Required:** either apply the D9b check (current unit in the native run's agent read set) in
  `native_mcp::admit_on`, or record an explicit disposition (with the user if it relies on
  decision (b)) and the follow-up in coordination. Enumerate every `insert_run` producer
  (admission_write.rs:14, children.rs:337, native_mcp/mod.rs:111) with its disposition.
- **Instruction lesson:** observation: the admission check was attached to one function rather
  than to the set of run producers. Candidate (application AGENTS.md or plan template): a new
  admission-time check names every producer of `workflow_runs` and its disposition.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R2: **fixed for native MCP tool calls.** r2 checks the called tool's connection against agent reach in `native_mcp::admit_on` with the admission code; mapping row `unit_resource_selection_native_*`. Run producers: admission_write.rs:14 (D9), children.rs:337 (covered by the root closure and copied scopes), native_mcp/mod.rs:111 (r2) — all dispositioned. Residual: skills and tools delivered in the native bundle itself are not checked; new finding 07UR-P01-G1-F14.

## 07UR-P01-G1-F5 — Soft-deleted MCP connections keep the RESTRICT unit reference forever

- **Classification:** actionable defect; medium; recovery / requirements (01A.3).
- **Evidence:** MCP delete is a soft delete (`UPDATE company_mcp_connections SET deleted_at = ...`,
  src/adapters/persistence/mcp.rs:291); the row is retained because agent capability bindings
  and revisions keep referencing its operations. With D1's `ON DELETE RESTRICT` FK the deleted
  row keeps blocking deletion of its department. D4 does not say whether deleted rows can be
  moved (all current MCP writes filter `deleted_at IS NULL`, e.g. mcp.rs:228, workflow/mcp.rs:200),
  and D5's list would name connections the pane no longer shows. A department that ever owned a
  since-deleted MCP connection becomes undeletable with an instruction ("Move them first") that
  cannot be followed. HTTP connections have no delete, so they rely on D4 only (acceptable).
- **Required:** define deleted-row handling in D1/D4/D5: for example a soft delete reassigns the
  row's unit to the root (or the parent) in the same transaction, or D4 moves deleted rows and D5
  lists them as deleted, with a test deleting a department after deleting its MCP connection.
- **Instruction lesson:** observation: the FK decision did not inventory soft-deleted/retained
  rows of the referencing table. Candidate (persistence AGENTS.md, composite FK checklist):
  "list retained or soft-deleted referencing rows and how they release the parent".
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R2: **fixed in decision.** r2: `company_mcp_connections.unit_id` is NULL exactly when deleted (`CHECK ((deleted_at IS NULL) = (unit_id IS NOT NULL))`), cleared by the only soft-delete write (mcp.rs:291, verified as the only `deleted_at` writer); MATCH SIMPLE then never pins a department. This deviates from 01A.1's literal `NOT NULL` only for deleted rows, explicitly and justifiably. D9b leaves deleted resources to existing revocation (r2 F12), so a NULL current unit is not misreported. Mapping row F5/teardown added.

## 07UR-P01-G1-F6 — The move's revision bump is a live authority event for integration-backed connections

- **Classification:** actionable defect; medium; correctness / recovery.
- **Evidence:** D4 bumps `revision` on MCP/HTTP moves while D9 states "an admitted run is not
  stopped by a later move" and the README keeps credentials unchanged ("a credential follows its
  connection"). Integration-backed connections freeze `connection_revision` and
  `catalog_revision` (= `revision`) into the action's `AuthorizationSnapshot`
  (workflow/integration_authority.rs:25-108); once an action intent exists,
  `frozen_snapshot_on` (:400-421) returns the frozen snapshot and `check_snapshot` (:110-118)
  requires equality with the snapshot recomputed from the current row at preparation and at
  dispatch (workflow/http.rs:376-391, 425-441; workflow/mcp.rs:183-184, 376-377). A move between
  intent freeze and dispatch (for example while awaiting approval) therefore refuses the action,
  and it also denies an in-progress MCP discovery (mcp/discovery.rs:84-93) and pending Vault
  binding proposals (`connection_revision`).
- **Required:** do not change the authorization revision for a move (use a separate unit
  revision, `updated_at`, or an expected-unit guard for optimistic concurrency), or decide
  explicitly that a move is a live authority event, say so in D4/D9, and test it.
- **Instruction lesson:** observation: the revision column's other consumers (integration
  authority snapshots) were not traced before reusing it as an edit counter. Cause otherwise
  unknown.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R2: **fixed in decision.** r2 move changes only `unit_id` (no `revision`, `credential_revision` or `updated_at`), so integration authorization snapshots, discovery and pending Vault proposals are unaffected; concurrency from FOR UPDATE and re-reading the current unit, with management re-checked on the current unit. The retained r1 mapping row 01B.4 still says 'revision bumped'; tracked in 07UR-P01-G1-F13.

## 07UR-P01-G1-F7 — D3's default unit is the creator's placement, not the creator's managed department

- **Classification:** actionable defect; low; requirements (01A.1).
- **Evidence:** 01A.1: "Creation defaults it to the creator's single managed department, else the
  root." D3 uses `workflow_creating_department` (init_schema:20189-20207), which picks the single
  non-root *placement* (department_members ∪ department_roles), whether or not the creator
  manages it. Consequences: an owner or root admin who is a plain member of unit X gets new
  company resources in X instead of the root (contradicting the planned mapping row "as owner →
  root" unless the fixture has no placement); a lead of A who is also a member of B gets the root
  and then the D3 management check returns 404 for a create without an explicit unit. The
  library-agent copy (`prepare_library_agent_on`, persistence/channel.rs:611-646) copies skills
  for a new agent that Plan06 places in a managed department; with an actor-based default the
  copies can land outside the agent's chain, and the agent's first publication is then refused.
- **Required:** compute the default from managed placements (the single unit where the actor
  holds lead; root for owner/admin or when ambiguous) and make library-agent skill copies use the
  new agent's placement unit; add the owner-with-placement and lead-with-extra-membership cases
  to `unit_resource_schema_default_*`.
- **Instruction lesson:** observation: an existing helper with a similar name was reused without
  comparing its predicate to the plan wording. No instruction change supported.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R2: **fixed in decision.** r2 `resource_default_unit`: root for a root manager; the single led unit; else 400 'Choose a unit'. Library-agent skill copies take the new agent's placement (single) or the root. Matches 01A.1's intent; the 400 instead of 'else the root' is justified because a non-root manager could not create in the root anyway. The planned default row lacks the 400 case; tracked in 07UR-P01-G1-F13.

## 07UR-P01-G1-F8 — D7 omits name-based authoring lookups (Agent Builder)

- **Classification:** actionable defect; low; requirements (01B.1).
- **Evidence:** Besides the catalog reads D7 names, the Agent Builder provisioning path selects
  resources by name company-wide: skills by slug (`SELECT id FROM skills WHERE company_id = $1
  AND slug = $2`, workflow/provisioning.rs:576) and tools through `resolve_bindings_on` →
  `registered_source_on` (agent_binding_selection.rs:50-80, "exactly one" operation name across
  all HTTP/MCP connections and workflow registrations; called at provisioning.rs:600). With unit
  ownership these lookups can attach out-of-reach resources (then refused by the initial
  publication, provisioning_agent.rs:46) and a sibling's same-named operation or skill makes the
  name ambiguous or resolves to the sibling's resource.
- **Required:** scope both lookups to resources in reach of the new agent's placement unit(s)
  (the D7 rule), and list them in the contract-impact table with a selection test.
- **Instruction lesson:** observation: authoring "reads" were inventoried by trait name only.
  Candidate: inventory selection by `trail grep` on the resource tables' SELECTs, not only by
  authoring traits.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R2: **fixed in decision.** r2 applies agent reach to the provisioning skill slug lookup and `registered_source_on`, treating out-of-reach as not found (ambiguity therefore judged within reach).

## 07UR-P01-G1-F9 — The agent "union" authoring unit and the per-run agent read set disagree; outcome not stated or tested

- **Classification:** actionable defect; medium; requirements / verification.
- **Evidence:** D7 (per 01B.1) offers an agent the union of its placement units. D9b checks the
  captured unit against the agent's *run* read set, which `run_scopes` builds from the desk's
  chain when the run has a desk, from the agent's placement only when it has exactly one, and
  otherwise the root alone (`chain_on` returns empty unless `workflow_target_departments` yields
  exactly one unit, run_scopes.rs:89-120; `capture_agent_on` :122-170; `run_scopes` root-only for
  an empty chain, application/workflow/scopes.rs:101-111). An agent in units A and B that
  publishes with a B resource (allowed by the union) is therefore refused on every A desk and on
  every desk-less run, refusing the whole admission. The plan's explicit "agent in two
  departments sees the union" case thus produces agents whose runs are refused except on B desks;
  the gate does not state this outcome and the acceptance mapping has no admission case for it.
- **Required:** state the intended admission outcome for a two-unit agent (desk in A, desk in
  B, no desk) in D9 and add those admission cases to `unit_resource_selection_`; if the outcome
  is not acceptable, raise it with the user rather than changing the plan's read-set rule.
- **Instruction lesson:** observation: two plan rules defined over different unit sets were
  gated separately. Candidate: when authoring and enforcement use different unit sets, the gate
  includes a case table of their disagreement.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R2: **resolved by user decision (d)** (coordination revision 2): an agent's own tools and skills are checked against the agent's current placements wherever it runs; workflow-level connections against the run's scope. r2 D9b implements this and the mapping adds the two-department agent on both desks and desk-less (admitted), agent moved away (refused).

## 07UR-P01-G1-F10 — Planned acceptance mapping lacks decisive cases for several gated decisions

- **Classification:** actionable defect; medium; verification.
- **Evidence:** TASK.md "Acceptance mapping (planned)" covers schema, default, authority, delete,
  move, offers, agent publication, admission and the move/publication race, but has no planned
  decisive assertion for:
  - D8 `binding.resource_outside_unit` at binding save (slot and unslotted);
  - D9a workflow-level refusal (slot connection moved; connection reached through a child or an
    agent-tool workflow, which the Plan05 learning
    `.trail/learnings/2026-11-17-check-workflow-requirements-along-agent-tool-mac02.md` shows is
    easy to miss);
  - schedule occurrence and message intent settlement with the new code (`classify` arm and
    `schedule_runs_refusal_check`, init_schema:16456-16460) proving a settled refusal rather than
    a transient retry or a CHECK violation;
  - D11: a real agent publication and prepared history on a fresh migration through
    `check_agent_revision_members` and `agent_history_*` (Plan05 learning
    2026-11-17-preparation-format-versions-include-the-sql);
  - company teardown with a non-root-owned skill/connection (RESTRICT FKs vs
    `companies_delete_tree`, init_schema:20015-20027);
  - skill-only capture and refusal (D6 `SkillSnapshot.unit_id`).
- **Required:** add rows with production path, case and decisive assertion for each, keeping
  "no run/job rows" style assertions for refusals.
- **Instruction lesson:** observation: the mapping followed the plan's three focused-check names
  rather than the gate's added decisions (D8 third code, D9a, D11). Candidate (skill
  verification-contracts): every gated decision that adds a code, a format or a check gets at
  least one mapping row.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R2: **fixed.** 'Acceptance mapping additions (r2)' add binding save, closure (root, child, agent-tool workflow), agent reach cases, unslotted runtime, native, refusal settling, fresh-migration formats, F5/teardown and skill-only rows with decisive assertions. Teardown FK mode is decided by that test with bounded alternatives (RESTRICT or NO ACTION). Remaining stale r1 rows are tracked in 07UR-P01-G1-F13.

## 07UR-P01-G1-F11 — Company-wide slugs disclose sibling resources on create (optional)

- **Classification:** optional suggestion; low; security (disclosure).
- **Evidence:** `skills_company_slug_key UNIQUE (company_id, slug)` (init_schema:6864-6866) and
  `company_mcp_connections_company_id_slug_key` (init_schema:6352-6353) stay company-wide, so a
  unit lead's create conflicts with, and so reveals, a sibling unit's slug. Plan06's D3 review
  treated conflict-based disclosure of another unit's definition as a gap
  (`.trail/learnings/2026-11-17-bound-procedures-follow-any-active-binding-mac02.md`).
- **Suggested:** record an explicit disposition in D3 (slug-only disclosure accepted because
  agents address skills by slug, or a non-identifying conflict message).
- **Instruction lesson:** none supported.
- **Resolution history:** 2026-11-20 open (optional).
  2026-11-20 recheck 07UR-P01-G1-R2: **dismissed with disposition** (optional): slugs stay company-unique; a conflict reveals only that the slug is taken, as before Plan07. Accepted.

## 07UR-P01-G1-F12 — D9b scope details (optional)

- **Classification:** optional suggestion; low; correctness.
- **Evidence:** the dependency walk distinguishes `AgentUseMode::ModelOnly`, which follows no
  tools (application/agent/dependency.rs:133-140); D9b checks "each captured connection and
  skill" for every manifest agent. A deleted skill (assignment `live_skill_id` set NULL) is
  already handled by the existing revocation path.
- **Suggested:** check only what the use mode executes (skills for model-only uses), and let a
  missing/retired resource keep its existing refusal instead of `admission.resource_outside_unit`.
- **Instruction lesson:** none supported.
- **Resolution history:** 2026-11-20 open (optional).
  2026-11-20 recheck 07UR-P01-G1-R2: **adopted.** r2 checks only Tools-mode uses; verified that ModelOnly uses expose neither tools nor skills (application/workflow/publication/capabilities.rs:52-63). Deleted or retired resources keep existing revocation refusals.

## 07UR-P01-G1-F13 — r2 leaves contradicting r1 rows and no reach context for the readiness preflight callers

- **Context:** found in round 07UR-P01-G1-R2 against decision revision r2 (same digest
  c2d338160c48d4d2eabaa84b5bd5fa5cd389b58d2a93db85478eb2e05fb9ac33, coordination revision 2).
- **Classification:** actionable defect; low; verification / contract impact.
- **Evidence:**
  - TASK.md says "r1 stands except where changed here", but the retained "Acceptance mapping
    (planned)" still asserts what r2 reversed: row 01B.4 "moved, revision bumped" (r2 F6: no
    revision change) and row 01B.3 "after a move up → `admission.resource_outside_unit`" (r2 D9b:
    moved up is admitted). Row 01A.1 default has no case for r2's 400 "Choose a unit". The r1
    D10 text ("old unit captured, refused at admission by D9b") and the r1 risk "D9b refuses
    moved-up resources until republish" also no longer hold as written (refusal now only when the
    move leaves agent reach). These rows define what the batch tests assert.
  - r2 changes `http::registered_on` to take a reach context but the contract-impact table is
    unchanged. Its unslotted call in `resources::check_readiness` (resources.rs:61-97) has five
    callers: bindings.rs:75 (binding save), admission.rs:74 (admission preflight, which runs
    before `admission_write::create` writes `workflow_run_scopes`), admission_replay.rs:102
    (replay), channel_reply.rs:314 (desk reply configuration), agent_manifest/capture.rs:41
    (closure). r2 defines run reach as the scope rows, "at binding save, the association's target
    chain", but not which context the pre-capture admission preflight, replay and channel-reply
    callers use; reading scope rows there would find none and fail closed for every unslotted
    tool.
- **Required:** mark the three r1 rows (and the D10/risk sentences) superseded with their r2
  replacements; add the 400 default case; add a contract-impact row for `registered_on`'s reach
  context naming each caller above, with the association chain (equal to the base read set, both
  from `workflow_target_departments`) for callers that run before scopes exist.
- **Instruction lesson:** observation: a revision that says "earlier text stands except where
  changed" left contradicting test rows in place. Candidate (skill verification-contracts): a
  decision revision rewrites or strikes each mapping row it changes rather than appending.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R3 (decision revision r3, same digest, coordination revision 2): **fixed.** The r1 rows are corrected in place (01A.1 default incl. the 400 case and owner/admin/root-lead/member-who-manages-root cases; 01B.4 with revision, credential_revision and updated_at unchanged; 01B.3 moved up admitted), the r1 D10 and risk sentences carry superseded markers (TASK.md:167, :184), and the contract-impact table has a `registered_on` reach row with the rule 'pre-run callers build reach from the association target chain (= base read set); callers once the run exists read its base scope rows'. By that rule every `check_readiness` caller is classifiable; two are not named explicitly (see optional 07UR-P01-G1-F15).

## 07UR-P01-G1-F14 — Native bundle delivery of an agent's skills and tools is not checked against agent reach

- **Context:** found in round 07UR-P01-G1-R2 against decision revision r2 (same digest,
  coordination revision 2); residual of 07UR-P01-G1-F4 under user decision (d).
- **Classification:** actionable defect; low; requirements (user decision (d)) / scope
  disposition.
- **Evidence:** user decision (d): "An agent's own tools and skills must be in reach of one of
  the agent's current departments ... wherever it runs." r2 checks native runs only at
  `native_mcp::admit_on`, i.e. per MCP tool call. The native harness receives the agent
  revision's instructions, skills and tool list as a `NativeBundle`
  (application/local_agents/harness.rs:79-85) through `native::selected_with_locks_on`
  (persistence/local_agents/native.rs:56-) from `bundles::authorize_with_locks_on`
  (local_agents/protocol/bundles.rs:45-104). A conversation-origin native session therefore
  still receives a skill whose unit moved out of the agent's reach (and lists tools that the
  per-call check would refuse); no workflow admission runs for it.
- **Required:** check agent reach for the bundle's skills (and tools) where the bundle is
  selected for a session, refusing with the admission code, or record an explicit disposition
  that native bundle delivery falls under user decision (b) and its recorded follow-up.
- **Instruction lesson:** none beyond 07UR-P01-G1-F4's candidate (enumerate every place a
  captured resource is delivered or used, not only run producers).
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-G1-R3: **fixed in decision.** r3 puts the agent-reach check for the selected agent's skills and tool connections in `native::selected_with_locks_on` (local_agents/native.rs:56), refusing with `admission.resource_outside_unit`. Verified its callers: session creation (`management/writes.rs:175` via `selected_on`), commands (`protocol/commands.rs:79`), bundle read and native MCP authority (`protocol/bundles.rs:100`), frozen-authority validation (`shared.rs:135`, used by workflow local-agent preparation and native live checks) — so conversation-started and workflow native sessions are both covered before anything is sent; mapping row F4 extended. Machines stay out of scope per decision (b). Side effect noted in 07UR-P01-G1-F15.

## 07UR-P01-G1-F15 — Name the remaining pre-run reach callers; native reach becomes a live check (optional)

- **Context:** found in round 07UR-P01-G1-R3 against decision revision r3 (same digest,
  coordination revision 2).
- **Classification:** optional suggestion; low; contract impact / recovery.
- **Evidence:**
  - The r3 `registered_on` reach row lists "admission closure in `admission_write::create`" with
    the scope-row callers, but the existing closure call `resources::check_readiness` at
    agent_manifest/capture.rs:41 runs inside `agent_manifest::capture_on`, the first statement of
    `admission_write::create` (admission_write.rs:12), before `insert_run` (:14) and
    `run_scopes::capture_on` (:66); `channel_reply.rs:314` (desk reply responder configuration) is
    not named. Both are pre-run by the r3 rule and must use the association chain; reading scope
    rows there finds none and fails closed. Replay (admission_replay.rs:102) uses the current
    chain although the run's frozen rows exist; equivalent unless the desk moved since admission.
  - Because `native::selected_with_locks_on` also runs on per-call native MCP authority
    (`mcp_authority_on`, protocol/mcp.rs:149-157), the reach check is a live check for native
    sessions: after a move out of reach, the machine's Status/Cancel actions for in-flight calls
    are refused as well (as for other authority losses there today), leaving expiry to the
    native maintenance sweep.
- **Suggested:** name capture.rs:41 and channel_reply.rs:314 as chain-context callers in the
  contract-impact row; in the batch, keep (or test) the sweep retiring an in-flight native call
  whose agent resource moved out of reach.
- **Instruction lesson:** none supported.
- **Resolution history:** 2026-11-20 open (optional).

## 07UR-P01-B1-F1 — The company settings Skills tab still lets only the owner manage skills

- **Context:** batch review B1, round 07UR-P01-B1-R1 (2026-11-20); reviewer Claude Opus 5.5 (B);
  HEAD cc545881 + working tree, digest
  2c55a5762e07d9e7f2a7d6aaf9a7fffe06571ab1351f8c666a97f9a8a3dad81a; coordination revision 2.
  Implementation defect against G1 D2 (decision unchanged).
- **Classification:** actionable defect; medium; requirements (user decision (a), D2 "UI+JSON
  routes").
- **Evidence:** B1 widens skill writes to the skill unit's managers in the use case
  (`verify_skill_manager`, `managed_skill`), but the Skills tab of company settings still renders
  with `editable = selected.membership.is_owner()` (src/adapters/http/routes/ui_companies.rs:505),
  and `company_skills_section` then shows "Only the company owner can edit these
  settings." with no list or form (src/adapters/http/pages/company_skills.rs:47-55). That tab
  (and the agent page's "Browse library" link to it, pages/agent_settings.rs:867) is the UI entry
  to skills; the editable section route `/ui/companies/{id}/skills` returns only an htmx fragment
  (routes/skill_library.rs:223-288). A root admin, root lead or unit lead therefore cannot create,
  edit, copy or move skills in the UI, while the MCP/HTTP pages do follow the new rule. No test
  covers the tab for a non-owner manager.
- **Required:** make the Skills tab editable for anyone who manages some unit (AnyUnit, as
  `verify_skill_manager`), listing their managed units' skills, and add a UI test that a unit
  lead sees the editable section with their unit's skills and a sibling's are absent.
- **Instruction lesson:** observation: an authority widening changed the use case but not the
  page-level gate that mirrored the old rule. Candidate (src/adapters/http/AGENTS.md or plan
  template): when a configuration authority changes, grep the pages for the old predicate
  (`is_owner`, `administers_company`) on the same resource.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-B1-R2 (digest c8c0b6d3b7f4634542ffc65305506e2e6d26f1ecaf163029e6156bf354e08cfd): **not fixed for unit or root leads.** The tab now passes `editable = manages_skills(..)` (ui_companies.rs:505-510), but `skills_pane` first calls `self.counts(company.id, editable)` (ui_companies.rs:303), and with `editable = true` `counts` calls `ChannelUseCases::list_company_channels` (ui_companies.rs:192-195), whose `verify_company_manager` is `managed_company` — owner/admin only (use_cases/channel.rs:199-201, use_cases/company.rs:234-244). For a unit lead or root lead (membership `member`) the Skills tab request therefore fails with the company not-found instead of rendering, which is worse than the previous read-only text; only owner and admin now work. The new test `unit_resource_skills_tab_and_agent_authoring_reads` checks `manages_skills` in the use case, not the tab route, so it cannot see this. Required: do not gate the Skills tab's counts on owner-only lists (e.g. skip counts or compute them with the manager's own reads), and add a route-level test rendering `/ui/companies?company_id=…&tab=skills` as a unit lead (editable section with their unit's skills, sibling's absent). Still open.
  2026-11-20 recheck 07UR-P01-B1-R3 (digest 2849d92a62feef47d12e7f73961195e804ef247edbe9801adb55770106493b42): **fixed.** `skills_pane(access, editable)` computes the owner-only counts and settings controls from `access.membership.is_owner()` and the skills section from `manages_skills` (ui_companies.rs:304-352), so a unit manager no longer reaches the owner/admin-only channel list. Route test `unit_resource_skills_tab_renders_editable_for_unit_managers_and_read_only_for_members` (ui_company_skills_tests.rs) drives `companies_page` as owner, root admin and two unit leads (editable section with the unit picker, no error) and as a plain member (read-only text, no section); re-run by B, passed. Listing scope per unit stays covered by `unit_resource_skills_tab_and_agent_authoring_reads`.

## 07UR-P01-B1-F2 — Agent authoring's skill picker now shows only managed-unit skills (Plan06 regression)

- **Context:** as 07UR-P01-B1-F1.
- **Classification:** actionable defect; medium; correctness / contract impact.
- **Evidence:** before B1, `SkillUseCases::list_company_page`/`get_company` let any unit manager
  read every company skill "to choose them for an agent (Plan06 U3)" (cc545881
  application/use_cases/skill.rs:179-190). B1 changes both to the caller's *managed* units
  (`unit_id = ANY(managed units)`, persistence/skill.rs list_company_page; use case
  `managed_skill`), which is right for the configuration list (D2) but these readers also feed
  agent authoring: the agent settings skill picker and the create pane's draft re-resolution
  (src/adapters/http/routes/ui_agents.rs:549-561, 591-600). A lead of Sales manages Sales and its
  descendants, so root-owned (ancestor) skills disappear from their agent picker, although their
  agents may use them today and will under B2's rule (in reach = the agent's unit or an
  ancestor). MCP/HTTP agent offers still come from the unchanged company-wide catalog
  (persistence/agent_authoring/catalog.rs), so B1 leaves the two offer sets inconsistent. No test
  covers a lead's agent skill options.
- **Required:** keep agent authoring on a separate read (company-wide with AnyUnit entry until
  B2, then B2's in-reach rule) and leave the managed-unit filter to the configuration list;
  record the consumer split in the contract-impact table and test a unit lead's picker.
- **Instruction lesson:** observation: a shared reader was narrowed for one consumer without
  checking its other callers. Candidate: when a list read changes scope, list its callers
  (`trail callers` / grep) and state each one's intended set.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-B1-R2: **fixed.** Agent authoring reads `SkillUseCases::list_authoring_page` (AnyUnit entry, every company skill; ui_agents.rs:548-560) while the configuration list keeps managed units; test `unit_resource_skills_tab_and_agent_authoring_reads` asserts a Sales lead's authoring list holds root, sibling and own skills and the configuration list only its own; stranger 404. The create pane's re-resolution of drafted ids beyond the page still uses the managed `get_company` (only matters past 100 skills; acceptable until B2 replaces authoring reads).

## 07UR-P01-B1-F3 — Several changed write paths and planned cases have no discriminating test

- **Context:** as 07UR-P01-B1-F1.
- **Classification:** actionable defect; medium; verification.
- **Evidence:** the D2 mapping row promises "each write path as owner, root admin, root lead,
  unit lead (own, child, sibling), member … level above own → 403", r2 F7 promises library-agent
  skill copies in the agent's placement, and 01B.4 promises affected bindings in the move
  response. The new tests (unit_resources_tests.rs, delegated_agent_management_tests.rs:476) cover
  sibling refusals for skill/MCP/HTTP edits, delete, token/secret, enable, level, HTTP and MCP
  operation *revoke* and move, and the agent use list. Not exercised by any test with a non-root
  unit or a non-owner actor (existing integration_binding/vault suites run as the owner on
  root-owned rows, so they cannot discriminate the change from `authorize_company` to the unit
  rule):
  - Vault binding propose, confirm (`verify_consent_on`) and source inspect
    (persistence/integration_bindings.rs:160-168, 302-309; integration_bindings/source.rs:63-70);
  - MCP discovery refresh for integration connections (persistence/mcp/discovery.rs:67-73,
    145-149);
  - HTTP and MCP operation *register* (only revoke is tested);
  - a unit lead's level above their own → 403 on a connection of their unit;
  - a root lead (not owner/admin) configuring a root-owned resource;
  - library-agent copy placing skills in the new agent's placement (company_agent.rs:291-302)
    and onboarding copies in the root;
  - the move response listing an active binding whose slot maps to the connection
    (unit_resources.rs `uses_on` binding query);
  - the default unit for an owner/admin who is also placed in a department.
- **Required:** add these cases (positive for the resource unit's lead, negative for a sibling
  lead with no row change), and update the B1 mapping with the actual test names.
- **Instruction lesson:** observation: existing suites were counted as coverage although their
  fixtures (owner actor, root unit) cannot distinguish the old and new authority. Candidate
  (skill verification-contracts): a changed authorization path needs one positive and one
  negative case whose actor/unit differ from the old rule's.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-B1-R2: **mostly fixed.** New tests (all re-run by B, 13 passed, logs/B1-R2-reviewer-unit-resource.txt): Vault propose/source inspect by a sibling 404 and by the unit lead OK, confirm refused after the proposer's unit is left; HTTP and MCP operation register by unit; root lead configuring root-owned resources; sibling discovery refresh 404 before any connect; placed owner/admin default to the root; library-agent copy; current-configuration binding uses; unmanaged uses counted. Remaining: (1) `unit_resource_library_agent_copies_skills_into_its_placement_or_the_root` uses the Sales lead placing into Sales and the owner unplaced (root) — both equal the actor's default unit, so it does not distinguish 'placement' from 'creator default' (use e.g. the owner or a root admin placing into Sales); (2) the level-ceiling case is not discriminating — see new 07UR-P01-B1-F6. Still open (low) for (1).
  2026-11-20 recheck 07UR-P01-B1-R3: **fixed.** The library-copy test now has the owner (default unit = root) place a library agent into Sales and asserts the copied skill is in Sales, so placement, not the creator's default, is shown to decide; the level case is covered under B1-F6. Re-run by B, passed.

## 07UR-P01-B1-F4 — MCP and skill writes check management outside the write transaction (optional)

- **Context:** as 07UR-P01-B1-F1.
- **Classification:** optional suggestion; low; concurrency.
- **Evidence:** HTTP writes authorize the resource unit inside the transaction after the company
  and row locks (`lock_edit_on`, workflow/http.rs:124-160), and level/Vault/discovery/operations
  use `authorize_resource_on` in the transaction. MCP update/delete/token and skill update/delete
  check management in the use case on the pool (`managed_unit`, `managed_skill`) and the
  persistence then only requires `unit_id = <authorized unit>` (mcp.rs update_on/delete/
  replace_mcp_token; skill.rs update_skill/delete_company). A concurrent move is excluded by that
  predicate, but a concurrent removal of the actor's lead role is not (the same shape as the old
  owner/admin checks, so not a regression).
- **Suggested:** use `authorize_resource_on` inside these transactions too, for one rule.
- **Instruction lesson:** none supported.
- **Resolution history:** 2026-11-20 open (optional).
  2026-11-20 recheck 07UR-P01-B1-R2: not applied by the author (optional); the unit predicate still excludes concurrent moves. Remains an open optional suggestion.
  2026-11-20 recheck 07UR-P01-B1-R3: still not applied (optional); accepted as a non-blocking suggestion — the unit predicate excludes concurrent moves and the remaining role-revocation window has the pre-B1 shape.

## 07UR-P01-B1-F5 — The move response names uses in units the mover does not manage (optional)

- **Context:** as 07UR-P01-B1-F1.
- **Classification:** optional suggestion; low; security (disclosure).
- **Evidence:** `uses_on` (persistence/unit_resources.rs) returns names of every agent and
  binding company-wide that uses the resource; until B2 restricts selection, a unit's connection
  can be used by agents of sibling units, whose names its lead then sees in the move response
  and notice (Plan06 convention: what the caller does not manage is not disclosed).
- **Suggested:** name only uses in units the mover manages and count the rest in `more_uses`.
- **Instruction lesson:** none supported.
- **Resolution history:** 2026-11-20 open (optional).
  2026-11-20 recheck 07UR-P01-B1-R2: **fixed (optional applied).** `uses_on`/`listed` (persistence/unit_resources.rs:145-240) name only uses with `agent_management_role`/`workflow_definition_management_role` for the mover, sorted managed-first, and count all in the total; test `unit_resource_move_counts_but_does_not_name_unmanaged_uses`.

## 07UR-P01-B1-F6 — Unit leads cannot set any connection access level; the ceiling test does not discriminate

- **Context:** found in round 07UR-P01-B1-R2 (digest
  c8c0b6d3b7f4634542ffc65305506e2e6d26f1ecaf163029e6156bf354e08cfd, coordination revision 2);
  also present at the R1 digest and missed there (see the follow-up on 07UR-P01-B1-R1).
  Implementation defect against G1 D2 (decision text "access level … requires management of the
  resource's unit; access-level ceilings stay `may_assign_level` (403)").
- **Classification:** actionable defect; medium; requirements (user decision (a)) / verification.
- **Evidence:** `set_mcp_level` and `set_http_level` now authorize the connection's unit but still
  call `may_assign_level(actor, …)` with the person's company `LevelActor`
  (persistence/mcp.rs:241-254; workflow/http.rs set_http_level). `may_assign_level` refuses first
  with `NotAdministrator` unless `actor.manages_levels()`, which is
  `membership.administers_company()` — owner/admin only (domain/entities/access_level.rs:88-90,
  116-123). A lead who manages the connection's unit (membership `member`) is therefore refused
  every level, including levels at or below their own, and the level form shows "A teammate
  with a sufficient level can change it." (pages/connection_levels.rs:22-27). New connections
  default to level 0, so a lead-created connection cannot be raised for runs above public
  without an admin. The refusal is `AppError::BadRequest` (mcp.rs `invalid(&e.to_string())`),
  not the 403 the gate and mapping promised. The R2 test
  `unit_resource_authority_root_lead_level_ceiling_refresh_and_placed_defaults`
  (unit_resources_tests.rs:831-852) asserts only `BadRequest` for a lead requesting L3, which
  `NotAdministrator` satisfies, so it does not show that the ceiling (rather than the admin gate)
  refused it.
- **Required:** for connection levels, let the resource unit's manager assign levels within their
  own (keep `TargetAboveActor`/`RequestedAboveActor` for non-owners), e.g. a connection-level
  actor whose management comes from `authorize_resource_on`; decide and record the status code
  (403 per D2, or document that 400 stays); test a lead setting a level at or below their own
  (accepted, revision bumped) and above it (refused with `RequestedAboveActor`), for MCP and HTTP.
- **Instruction lesson:** observation: the gate named the helper (`may_assign_level`) but not its
  hidden admin gate, and the test asserted an error class two different checks produce.
  Candidate (skill verification-contracts): a refusal test asserts the specific reason when
  several checks share an error type.
- **Resolution history:** 2026-11-20 open.
  2026-11-20 recheck 07UR-P01-B1-R3: **fixed.** New `may_assign_connection_level` (domain/entities/access_level.rs:139-156: owner unbounded; otherwise current and requested at most the actor's own level, no admin gate) is used by `set_mcp_level` (after `authorize_connection_on`) and `set_http_level` (after `lock_edit_on`), refusing with `AppError::Forbidden` (403); the level form uses the same rule; team levels keep `may_assign_level`. Tests discriminate: the Sales lead at level 2 sets MCP and HTTP levels to 2 (accepted) and is refused `Forbidden` at 3 (unit_resources_tests.rs:841-881), plus the domain test `access_level_connection_assignment_is_bounded_by_the_unit_managers_own_level`; the existing demotion race now expects 403. Re-run by B, passed.
