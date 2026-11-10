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

