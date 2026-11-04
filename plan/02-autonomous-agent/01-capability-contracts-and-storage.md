# 01 — Agent capability contracts and storage

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and implementation order

Define an agent once, including its model, instructions, tools, and skills, and reuse it across
workflows. MCP tools bind directly to company connections. Agents publish their own immutable
revisions. New workflow runs select the active agent revisions; running, retried, and resumed
executions retain their recorded selections while current authorization can still refuse use.

This directory is an implementation plan, not a record of implemented behavior. Implement the
numbered phases in order and integrate all phases before release. Phase 01 owns the shared design
contract and phase 07 owns the final verification gate.

| Phase | Deliverable |
| --- | --- |
| [01 — Contracts and storage](01-capability-contracts-and-storage.md) | Typed agent bindings, revision identities, tenant-safe persistence |
| [02 — Agent publication](02-agent-publication-and-skills.md) | Independent validation, frozen skill contents, atomic publication and activation |
| [03 — Workflow references and run snapshots](03-workflow-references-and-run-snapshots.md) | Workflow agent references and durable selection of revisions at admission |
| [04 — Dispatch and authority](04-tool-dispatch-and-live-authority.md) | Direct MCP targets, existing action dispatch, live revocation checks |
| [05 — Producers and execution variants](05-provisioning-children-and-samples.md) | Provisioning, child workflows, templates, libraries, samples, and all admission paths |
| [06 — Authoring and operations](06-authoring-and-operations.md) | Agent configuration/publication UI and APIs, workflow previews, run provenance |
| [07 — Integration and verification](07-integration-and-verification.md) | End-to-end reuse, concurrency/recovery proof, complete CI gate |

## Shared design decisions

- The agent owns its capability definition. The workflow owns orchestration, task inputs, output
  contracts, and execution context. Tool calls remain inside the existing durable execution and
  action systems. This work does not require an additional agent scheduler or execution engine.
- An agent draft is editable. Publishing validates and atomically activates a new immutable
  revision. Draft edits have no execution effect. There is one active revision per company agent.
- A workflow version records a bounded set of permitted agent identities, not their full mutable
  configuration. New root runs record the active revisions of their complete bounded dependency
  set at admission. Descendant runs inherit those selections. A retry never resolves latest again.
- Publishing agent v2 changes future root runs without republishing their workflows. A workflow
  version alone consequently no longer describes the entire execution configuration: its run
  record also identifies exact agent revisions and the resolved dependency manifest.
- An agent publication freezes tool contracts, concrete target identities, effect/approval policy
  facts, model settings, and skill contents. Credentials are read through existing narrow secret
  interfaces at use time and are never included in revisions, hashes, logs, or task payloads.
- Frozen configuration is not perpetual authorization. Current tenant authority, tool grants,
  reviewed MCP operations, connection status, model availability, and action policy remain live
  checks. Explicit revocation can stop an old run. Adding permission cannot expand an old snapshot.
- Publishing a removal revokes that capability for old runs as well as removing it from new ones.
  Draft removal alone does not revoke. Immediate explicit revocation remains available separately.
  Removing a skill assignment likewise prevents further use of that assignment; editing shared
  skill text affects only agent revisions published afterward.
- Skills express instructions and required tool names; they never grant tools implicitly. Resolve
  those names within the owning agent. Distinct agents may use the same alias for different tools.
- Built-in provisioning tools retain their Agent Builder restriction. Agent publication cannot
  manufacture message, provisioning, or reviewed external-tool authority from a matching name.
- Preserve existing bounds and budgets. Add explicit limits for new revision/dependency work;
  reject oversized input or excessive closure expansion before provider entry. A raised bound
  needs a documented early-failure signal kept in CI.

## Current code and ownership anchors

Re-query graft and trace callers before each implementation batch; these are ownership anchors,
not promises that line numbers or module paths will remain unchanged.

- [Agent writes](../../src/application/use_cases/agent.rs), `AgentWrite`: existing model,
  `workflow_tools`, and `skill_ids` configuration.
- [Current publication](../../src/adapters/persistence/workflow/agent_publication.rs), `derive_on`,
  `capture_agents_on`, and `granted_tool`: skills already come from `agent_skills`; MCP resolution
  currently depends on captured workflow slots and tool names share a workflow-wide collection.
- [Snapshot types](../../src/application/workflow/publication/types.rs), `AgentSnapshot`,
  `SkillSnapshot`, `ToolSnapshot`, and `ToolKind`: reusable capability facts currently live under
  workflow publication.
- [MCP operations](../../src/application/workflow/mcp.rs), `McpOperations` and `McpOperation`:
  company connection/tool registrations, reviewed catalog state, and approval policy already
  exist. Extend these; do not create a parallel registry or discovery/credential system.
- [Agent preparation](../../src/adapters/persistence/workflow/agent_preparation.rs), `targets` and
  `current_on`: concrete targets and current-authority checks must move together.
- [Init schema](../../migrations/20260817000000_init_schema.sql): change in place and verify on
  freshly migrated disposable databases, following the repository reset policy.

## Batch 01A — Define the capability model

1. Replace name-only agent grants with ordered typed bindings. Each binding has stable identity,
   an agent-local tool name, and a concrete source: trusted built-in, registered HTTP operation,
   registered MCP operation on a company connection, or registered workflow tool with pinned
   workflow version and parameters. Reuse current registrations and policy types where they fit.
2. Keep the agent-visible alias separate from the remote MCP tool name and registration identity.
   Retargeting an alias must create a distinct binding identity; a stale grant must not authorize
   the new target accidentally. Reject duplicate aliases within an agent and reserved-name abuse.
3. Introduce typed agent draft/revision/binding identities and a complete resolved agent
   definition. Move shared capability ownership into the consuming agent application/domain
   layer, retaining dependency direction. Do not add single-purpose wrapper services or an
   interchangeable-provider abstraction merely to rename the current workflow types.
4. Define an immutable run dependency manifest referencing durable revision rows. Queue payloads
   carry identifiers; do not embed mutable entities or credentials. Detailed admission assembly
   belongs to phase 03.

Focused checks: `autonomous_agent_contract_` unit tests for target variants, aliases, identities,
reserved names, bounded decoding, required-tool lookup, and canonical serialization. Run affected
existing agent/publication type tests and offline all-target compilation after shared type edits.

## Batch 01B — Persist drafts, revisions, and bindings

1. Add the smallest schema needed for editable agent capability configuration, immutable agent
   revisions, active-revision selection, current grant authority, and run-to-revision references.
   Store ordered skill assignments using the existing skill ownership model.
2. Enforce tenant-qualified foreign keys on agents, connections, registrations, revisions, and run
   references. A typed target must reference a real resource of the correct kind; an unchecked
   polymorphic UUID or application-only tenant check is insufficient.
3. Keep revisions immutable and retain rows referenced by runs. Enforce unique agent-local names,
   revision identity, and one active selection. Model editable configuration separately from live
   authorization so saving a draft cannot silently revoke or expand a running agent.
4. Update persisted entity projections, conversions, reads, inserts, updates, and fixtures as one
   change. Remove the superseded name-only storage after dependent code is switched within this
   implementation; do not maintain compatibility reads/writes or data backfills.

Focused checks: `autonomous_agent_storage_` database round trips for create/update/read, ordering,
immutability, and tenant/target constraints. Demonstrate invalid cross-tenant rows fail in the
database. Regenerate SQLx metadata after SQL changes and execute runtime-query tests as well.

## Verification rules for every phase

Test prefixes in this plan are future test families, not claims of existing coverage. For each
batch, list its tests with `cargo test --locked --offline --lib PREFIX -- --list`, verify a nonzero
selection, then run `cargo test --locked --offline --lib PREFIX` against a disposable database.
Record exact commands, selected/executed counts, results, and unavailable checks. Use the smallest
relevant existing regressions; expand checks early for shared compiler, admission, or action code.
Actual competing claimants are required for every new concurrency protocol.

Follow [07](07-integration-and-verification.md) for database setup, SQLx regeneration, and the one
final full-suite gate. No phase may claim completion from skipped database tests or a zero-match
filter. No schema reset or application implementation is part of saving these plan documents.
