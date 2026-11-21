# 01 Document and export

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

The blueprint type exists with strict parsing, bounds and one digest, and a person can export any
unit they manage as YAML or JSON. Nothing is written yet. Continues in
[02](02-plan-and-diff.md).

## Batch 01A The document type

1. A new application module, `src/application/blueprint/`, owns the type: `Blueprint
   { format_version, unit, omitted }`, `UnitSpec`, `AgentSpec`, `SkillSpec`, `DeskSpec`,
   `RoomSpec`. Parsing denies unknown fields at every level, and `format_version` other than `1`
   is refused. One typed error, `blueprint.invalid`, names the path of the first shape or bound
   violation.
2. References are newtypes, per `src/AGENTS.md`: a unit, a desk, an agent and a person are named
   side by side in one document and a swapped argument must not compile. Reuse what
   `src/domain/entities/value_objects.rs` has and add the missing ones with `string_newtype!`.
   Units, desks, rooms, agents and people share the company address namespace of 03
   (`addresses`), so one slug names exactly one of them. Skills have their own slug namespace.
3. `UnitSpec` states: `slug`, `name`, `description`, `kind` and `target_date` (08), `charter`,
   `archived` (08), `escalation` (08), `default_model` and `budgets` (07), `roles`, `members`,
   `skills`, `agents`, `desks`, `rooms`, `units`, and the `procedures` that
   [04](04-procedures-bindings-and-schedules.md) adds. Every field except `slug` is optional,
   because an unstated field means "leave as it is" ([02](02-plan-and-diff.md), batch 02B).
4. `roles` states `lead`, `reviewers`, and `sponsors` with its `decider`, each a principal slug.
   `members` is a list of principal slugs. On a root unit a document may not state `members`,
   `owner` or `admin`: company membership is the invite path and stays there.
5. `AgentSpec` states what the agent editor states through `AgentWrite`
   (`src/application/use_cases/agent.rs:42`): name, slug, description, prompt, provider and model,
   ordered tool grants, ordered skill slugs and `open_to_public`. A tool grant names a built-in,
   or a connection by name with an operation; an MCP grant binds the connection's currently
   reviewed version of that operation, as the editor does. An agent is written under the first
   unit, in tree order, that it belongs to; other units name it in `members`.
6. `SkillSpec` states slug, name, description, instructions and required tools (`skills`,
   `init_schema.sql`). Attached files are not carried.
7. `DeskSpec` and `RoomSpec` state the fields the channel form writes through `ChannelWrite`
   (`src/application/use_cases/channel.rs`) that are configuration: slug, aliases, name, access
   and, for a room, its roster. Bindings arrive in [04](04-procedures-bindings-and-schedules.md).
8. For each write type a document feeds (`AgentWrite`, `ChannelWrite`, the skill and department
   writes), every field is either carried or named in one exclusion list with its reason. The
   round-trip test in [03](03-apply.md) destructures each type without `..`, so a field added
   later fails to compile until someone classifies it.

## Batch 01B Bounds and encodings

1. Bounds are enforced at parse, before any database read: at most 262,144 bytes serialized, the
   bound authored workflow source already has (`MAX_SOURCE_BYTES`,
   `src/application/workflow/compiler/rebase.rs:4`); depth at most `MAX_DEPARTMENT_DEPTH`
   (`src/domain/entities/department.rs:23`); at most 32 units, 64 agents, 64 skills, 64 desks and
   rooms together, and 128 members in one unit. Each text field keeps the bound its column has
   (charter 4,096 bytes, skill instructions 16,384, agent text `MAX_AGENT_TEXT_BYTES`).
2. If an existing ceiling on action arguments or model output is lower than the document bound
   when [05](05-builder-agent-and-approval.md) is implemented, lower the document bound. Do not
   raise an existing ceiling to fit a document.
3. The type has two encodings and one meaning. JSON is what the API, the tools and the digest
   use. YAML is the same type rendered for people. `document_digest` is SHA-256 over canonical
   JSON after normalization: keys ordered, set-valued lists (`members`, `reviewers`, `informed`,
   aliases) sorted, ordered lists (an agent's tools and skills) kept in order. It is never taken
   over YAML bytes.
4. YAML input is parsed with aliases and anchors refused and the byte bound checked first, so an
   imported file cannot expand past the bound.
5. Parsing and normalization are synchronous functions. Nesting is bounded by item 1, so the
   recursion is too. Run `scripts/stack-budget.sh` with a depth-8 document in the focused set;
   do not raise `STACK_BUDGET_KIB` for this plan.

## Batch 01C Reading a unit

1. An application port, `BlueprintReads`, with `unit_state(company, unit) -> UnitState`: one
   tenant-scoped read of the unit and its descendants in a single snapshot. It reads the columns
   the document states and nothing else, by bounded batch reads, not one query per unit.
2. A subtree over a bound of batch 01B is refused with `blueprint.too_large` naming the count.
   Export never truncates: a truncated document applied later would read as a statement of the
   whole.
3. A pure function `export(state) -> Blueprint` builds the document. It is deterministic: the
   same state gives the same bytes.
4. What is inside and what is a reference:
   - the unit and its active descendants are written in full; archived descendants are listed in
     `omitted` and not written, so a recreated setup starts with active units only;
   - an agent that belongs to a unit of the subtree is written in full, whoever else it belongs
     to, because managing any of its units manages it (06 decision 2);
   - a skill owned by a unit of the subtree is written in full; a skill owned by an ancestor is a
     slug in the agent's `skills`;
   - people, agents outside the subtree, connections and models are names only.
5. `omitted` is informational and ignored by plan and apply. It lists what the subtree holds that
   a document does not carry: connections owned by its units, knowledge document counts, skill
   files, archived units, and anything [04](04-procedures-bindings-and-schedules.md) cannot
   express. A reader then knows what a recreated setup will lack.

## Batch 01D Export route

1. `GET /api/companies/{company_id}/departments/{department_id}/blueprint`, beside the department
   routes (`src/adapters/http/routes/department.rs:27`). JSON by default, YAML for
   `Accept: application/yaml`.
2. It requires managing the unit (`ManagementTarget::Unit`,
   `src/application/ports/administration.rs:13`). Anyone else gets the `404` a stranger gets. A
   member who can read the unit's desks but does not manage it gets `404` too: prompts and budgets
   are configuration.
3. The type has no field that can hold a credential. A test still exports a unit whose connections
   have stored secrets and asserts none of their bytes appear.

## Focused checks

- `blueprint_document_`: unknown fields, wrong version and each bound are refused with the path;
  YAML and JSON of one document parse to equal values; the digest is stable under key order and
  set order and changes with any stated value; aliases are refused; root restrictions.
- `blueprint_export_`: with a database, a unit holding every carried kind exports each stated
  field; two exports of unchanged state are byte-equal; an agent in two units is written once; an
  ancestor's skill is a reference; `omitted` names connections and archived units; an over-bound
  subtree is refused and not truncated.
- `blueprint_export_authorization_`: owner, root admin and the unit's lead succeed; a sibling's
  lead, a plain member, a stranger and an agent actor get `404`; no secret bytes in any export.
- The depth-8 stack budget run of batch 01B item 5.
