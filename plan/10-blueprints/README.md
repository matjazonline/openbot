# 10 Blueprints: a unit's setup as a document that an agent proposes and a person applies

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-20. Follows the nine organization plans that end with
[09-rooms](../09-rooms/README.md) and is implemented after them. It adds no structure to the
organization model; it adds one way to read and write the whole of it. This is an implementation
plan; blueprints are not implemented by these documents.

File anchors are on `origin/main` at `5bf6a669` (2026-11-20), where delegated administration
([06](../06-delegated-administration/README.md)) and the plans before it are implemented. Symbols
that 07, 08 and 09 introduce are named without a line, because those plans were not implemented on
that day. Re-trace every anchor with Trail before implementation.

## Intended behavior

A blueprint is a document that describes one unit and what is configured under it: sub-units,
their charters and roles, members, agents with their prompts, models, tools and skills, desks,
rooms, procedures, bindings, schedules and budgets. The same document serves four uses:

- **Build.** A builder agent drafts the setup for a goal, a person approves it, and the whole
  setup is created in one commit.
- **Change.** A unit is exported, the document is edited, and the difference is applied.
- **Keep and recreate.** A unit is exported before a database reset and applied after it.
- **Share.** A document is text, so it can be applied in another company.

There are three operations and one implementation of each:

| Operation | What it does | Who may |
| --- | --- | --- |
| Export | Reads a unit and everything under it into a document | A person who manages the unit |
| Plan | Compares a document with the live state and returns the changes, or every reason it cannot be applied | The same person; a builder agent on its requester's authority |
| Apply | Makes the planned changes in one transaction | A person who manages what each change touches |

A builder agent reaches the same three through built-in tools. Its apply tool never runs on the
agent's word: the call parks for a person's approval of that exact document, and the approver is
the actor of every write.

> Person, to the Builder in the goals office: We need a team to launch X by March. A researcher,
> a writer, and weekly reports to me.
>
> Builder: asks two questions, drafts a blueprint, plans it and replies with the nine changes it
> will make. It calls `apply_blueprint` and the run parks.
>
> Head of the goals office, in the decision inbox: reads the nine changes, the tools each new
> agent is granted and the budget, and approves.
>
> Builder: Launch X exists. Write to its desk to start.

A document looks like this. Field names are fixed in [01](01-document-and-export.md).

```yaml
format_version: 1
unit:
  slug: launch-x
  name: Launch X
  kind: goal
  charter: Ship X to fifty pilot customers by March.
  roles:
    lead: launch-pm
    sponsors: {decider: ana, informed: [marko]}
  budgets: [{period: month, cap_micros: 400000000}]
  skills:
    - slug: launch-checklist
      name: Launch checklist
      description: Steps every launch task follows.
      instructions: |
        ...
  agents:
    - slug: launch-pm
      name: Launch PM
      model: {provider: <provider>, model: <model>}
      prompt: |
        ...
      skills: [launch-checklist]
      tools: [{builtin: message.send}]
  desks:
    - slug: launch-x-reports
      name: Launch reports
      bindings:
        - responder: {agent: launch-pm, reply: reviewed}
  units: []
```

## Review of the preceding plans

The READMEs and phase documents of 01 through 09 were read for this plan, and the code on
`origin/main` was read for 01 through 06. Consume the contracts recorded when 07, 08 and 09 are
implemented; this plan approves none of their open proposals.

| Plan | Contract blueprints build on | Consequence here |
| --- | --- | --- |
| [01 Departments](../01-departments/README.md) | Units nest, hold members and roles, carry a charter; creating a unit creates its desk | The document is the unit tree; a unit's own desk is the desk that shares its slug |
| [02 Workflow roles](../02-workflow-roles/README.md) | Procedures name roles; reviewers are principals or roles | A shipped role-based procedure is bound on a new desk without authoring; approvers may be roles |
| [03 Principal addresses](../03-principal-addresses/README.md) | One address namespace per company | A slug names exactly one unit desk, desk, room, agent or person, so slugs are the references |
| [04 Root unit](../04-root-unit/README.md) | The company is the root unit; company membership is root membership | The whole company is the root's document; root membership and `owner`/`admin` stay outside it |
| [05 Scopes](../05-scopes/README.md) | A run has at most one own unit, frozen at admission | A builder reads and builds only under the unit its run works in |
| [06 Delegated administration](../06-delegated-administration/README.md) | A person manages a unit and what is below it; agents do not administer | Every write is a person's; apply asks each change's existing owner to check that person |
| [07 Unit resources](../07-unit-resources/README.md) | Skills, connections, models and budgets belong to a unit and are usable below it | Documents carry skills, budgets and the default model, and reference connections by name |
| [08 Goals](../08-goals/README.md) | A goal is a unit with a sponsor and an end; any unit can be archived | Goal units are ordinary document units; archive is the only removal a document can state |
| [09 Rooms](../09-rooms/README.md) | A room is a channel kind with an explicit roster | Rooms are listed with their roster |

## Decisions (user, 2026-11-20)

1. **An agent, or a department set up for it, can build the organization.** It creates
   departments, desks and agents and sets skills and the rest of the configuration, to make a
   working setup for a particular goal.
2. **The setup has a serialized representation.** The builder agent produces it, and it is applied
   when approved.
3. **The same representation keeps, shares and recreates a setup.**

## Design

These are planning choices made on 2026-11-20. They are not yet confirmed user decisions.

1. **One format, recursive on units.** A document describes one unit and its descendants. Applied
   at the root it describes the company; applied under a goals office it describes one goal.
   Authority decides how high a person may apply one, not the format.
2. **Export, plan and apply are one implementation each.** The JSON API, the department page and
   the builder's tools call the same three functions. Nothing is written by a second path.
3. **A plan is a diff, and the diff is what is approved.** Plan returns typed operations with the
   values they replace and one digest over them. Apply recomputes the plan under lock and refuses
   with `blueprint.stale` when the digest differs. An edit elsewhere in the company does not
   invalidate a plan; an edit to something the plan touches does.
4. **Stated fields are authoritative, everything else is untouched.** An entity the document does
   not name is not changed. A field it does not state is not changed. A list it states replaces
   the live list. Nothing is removed because it is absent; the only removal is `archived: true`
   on a unit, which is 08's archive.
5. **Apply composes the writes that already exist.** Each operation calls the persistence function
   that owns that write today, with the acting person, inside one transaction. Those functions
   already recheck authority under the company lock (06, batch 01A item 4), so a blueprint cannot
   do what its actor could not do one form at a time.
6. **An agent's apply is a protected action.** `apply_blueprint` always requires approval, through
   the approval path protected tool calls already use. The approving person's identity comes from
   the decision row and is the actor. The agent's requester is consulted for reads only.
7. **A builder's reach is the unit it runs in.** Agent-authored plans may target the run's own unit
   or a descendant and may not change the agents the run itself uses. Proposals 2 and 4 below
   narrow them further.
8. **Configuration only.** No credential, connection definition, knowledge content, memory,
   conversation, run, usage or audit record is in a document. Export lists what it left out.
9. **References are slugs and names; there are no parameters.** A document that names a person,
   connection, model or skill missing in the target gets one refusal listing all of them. The
   importer, or the builder, edits the document.
10. **A document from outside is untrusted.** It carries prompts and tool grants. Import is plan
    and then apply by a person, never a direct write, and it is bounded before it is parsed.

## Proposed here, confirm before implementation

1. **Every agent proposal needs a person's approval.** There is no standing pre-approval of a
   class of changes. A later plan may add one, as a person's bounded and expiring grant.
2. **Agent-authored documents carry no procedure source.** They bind shipped templates, procedures
   already published in the company, and the custom steps `create_desk_for_agent` accepts today.
   Documents a person exports or imports may carry procedure source
   ([04](04-procedures-bindings-and-schedules.md), batch 04C).
3. **Agent Builder becomes the Builder.** Its `create_agent` and `create_desk_for_agent` tools are
   removed, so an agent has one way to write and it is approved structurally. Today the
   confirmation is a sentence in the prompt (`AGENT_BUILDER_PROMPT`,
   `src/application/use_cases/builtin_agent_library.rs:21`) and the trusted snapshot is "never an
   approval" (`src/application/workflow/provisioning.rs:116`).
4. **Agent-authored documents cannot open public access.** Setting `open_to_public` on an agent,
   `public` access on a desk, or an outside address grant stays a person's action in the product.
5. **Moves and deletions are not in the format.** Re-parenting a unit, moving a desk or a skill
   to another unit, and deleting anything stay in the product. A document that implies a move is
   refused.

## What does not change

- The tenant boundary, access levels, and desk and conversation read policy. Export shows
  configuration and never a conversation, as 06 decision 5 already separates the two.
- 06 decision 6: agents do not administer. `manages_unit` gains no agent arm.
- Publication immutability, frozen run selections and live authority checks. An applied change to
  an agent or a procedure affects later admissions, as an edit in the product does.
- Every management use case and its persistence recheck. Blueprints call them and replace none.
- The department JSON API (`src/adapters/http/routes/department.rs:27`). Three routes are added
  beside it and no shape changes.
- The protected-action approval mechanism: its digests, reviewers, notices and deadline
  (`src/adapters/persistence/workflow/decision_approvals.rs:55`).

## Out of scope

- An agent principal that holds administrative authority, and approval by agents.
- Knowledge document content, memory, conversations, runs, usage and audit history. A database
  backup remains the backup; an export is the setup only.
- Credentials and connection definitions. A document references a connection by name and the
  connection must exist where it is applied.
- Company creation, invites, root membership and the root's `owner` and `admin` roles.
- A marketplace, ratings, or upgrades of an applied blueprint. [todo](../todo.md) keeps the
  package installer unplanned.
- Editing a draft by patches, and applying by digest without resending the document. The builder
  sends the whole document to plan and to apply; measure the cost before adding either.

## Documents and verification

1. [Document and export](01-document-and-export.md).
2. [Plan and diff](02-plan-and-diff.md).
3. [Apply](03-apply.md).
4. [Procedures, bindings and schedules](04-procedures-bindings-and-schedules.md).
5. [Builder agent and approval](05-builder-agent-and-approval.md).
6. [UI, samples and documentation](06-ui-samples-and-docs.md).
7. [Integration and final verification](07-integration-and-final-verification.md).

Execute in order after 09. Phases 01 through 03 depend on 06 for authority and could start before
07, 08 and 09 land. Skill ownership and budgets (07), kind and archive (08) and rooms (09) would
then be added to the document type and to the round-trip test of [03](03-apply.md) as each
lands. The default is to wait; record the choice here if it is made.

Proposed identifiers describe ownership; they are not claims that the symbols exist. Use the
existing runtime and purpose-specific persistence ports.

For each batch's new test family, first run
`cargo test --locked --offline --lib <filter> -- --list` and assert a nonzero count, then run
`cargo test --locked --offline --lib <filter>` with an isolated `TEST_DATABASE_URL`. Record exact
commands, selected and executed counts and results. Tests that assert whole-table state use
`own_database`. Do not use `ALLOW_MISSING_DATABASE_URL=1` as evidence. Run the focused checks each
phase names while implementing it and the complete repository and CI gate once after integration,
as [07](07-integration-and-final-verification.md) specifies.

Every file here carries the reset note and is registered in `NO_BACKWARD_COMPATIBILITY_NOTES` in
`src/main.rs`. Implementation evidence remains pending until the phases execute.
