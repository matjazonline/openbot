# 02 — Independent agent publication and skills

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-capability-contracts-and-storage.md). Validate and publish an agent without
supplying a workflow definition, resource slot, channel, or run. Publishing creates and activates
one immutable revision containing everything needed to describe its capabilities.

## Ownership anchors

- [Current derivation and verification](../../src/adapters/persistence/workflow/agent_publication.rs),
  `derive_on` and `verify_on`: extract agent-owned validation and capture; keep workflow integration
  as a consumer of published agent definitions.
- [MCP operation policy](../../src/application/workflow/mcp.rs), `McpOperation::validate`: reuse
  reviewed catalog/schema matching, connection grant checks, and approved operation versions.
- [Workflow tool registration](../../src/adapters/persistence/workflow/workflow_tools.rs): reuse
  company-scoped registration identity, version pins, parameters, and revocation semantics.
- [Existing capability validation](../../src/application/workflow/publication/capabilities_tests.rs):
  required skills currently fail publication if their tools were not granted.

## Batch 02A — Resolve a complete agent revision

1. Build one bounded resolver/validator owned by the agent application boundary. It reads the
   agent draft, explicit model selection, ordered tool bindings, and assigned skills. UI previews,
   publication, and fixtures use that same validation logic.
2. Resolve HTTP and MCP tools through existing registrations and explicit connection identities.
   An MCP binding must identify a granted, reviewed tool and valid operation policy; mere discovery
   of a remote name is insufficient. No workflow slot participates in this resolution.
3. Freeze target identity and contract/policy facts, including the reviewed MCP operation identity
   needed to reject stale approval or schema changes. Preserve external remote names separately
   from agent aliases. Freeze workflow-tool pins and parameters; closure validation is phase 03.
4. Capture skill instructions and required-tool lists in the agent revision. Validate requirements
   against that agent's bindings, including alias resolution and ordering. A shared skill edit
   cannot mutate a published revision, and no skill requirement automatically creates a grant.
5. Include instructions, model settings, ordered bindings, policies, and skills in deterministic
   revision content/hash validation. Apply existing schema and content bounds, propagate invalid
   stored-data errors, and keep credentials out of ordinary projections and serialized content.
6. Preserve trusted built-in identities and Agent Builder restrictions. Validate the selected
   provider/model against company availability independently of workflow publication.

Focused checks: `autonomous_agent_resolution_` and `autonomous_agent_skills_` tests covering an
MCP-enabled agent with no workflow, alias collisions, same alias on two different agents, missing
registrations, unreviewed catalog state, schema drift, wrong tenant, missing skill requirements,
reserved names, and skill edits leaving old revision content unchanged.

## Batch 02B — Publish and activate atomically

1. Expose a company-authorized publication operation with expected draft/active revisions and a
   stable idempotency key. Use one durable transaction to validate captured facts against current
   state, insert the immutable revision, update live grants, and replace the active revision.
2. Protect source reads and publication commit against concurrent draft, skill, registration,
   catalog, and authority edits. Use an explicit lock order or checked revision protocol; never
   publish a mixture of facts from before and after an edit.
3. A lost publication race returns a conflict with no partial activation. Replaying an already
   committed request returns its recorded result, even if a newer publication has since won;
   it must not reactivate the old result or mint a second revision.
4. Activation adds grants for future runs and removes/revokes bindings no longer authorized.
   Authority checks use binding identities, not aliases alone. Replacing a target under the same
   alias must not resurrect an old grant or make an old run call the replacement target.
5. Preserve an explicit immediate revocation path. Disabling a connection, revoking a reviewed
   operation, or removing model availability remains independently effective. Ordinary draft edits
   and changes to shared skill text do not mutate live authority.
6. Retain immutable historical revisions used by runs. Deactivation prevents new admissions;
   deletion must not break audit/replay references or bypass the defined revocation behavior.

Focused checks: `autonomous_agent_publication_` database tests with competing publishers,
publication versus draft/skill/operation edits, conflict rollback, repeated idempotency keys,
revocation/recreation without ABA, failed validation preserving the active revision, and activation
removing live authority. Assert exact stored revision and grant identities.

## Acceptance and evidence

An agent can be validated, published, and inspected without a workflow. Its complete definition
is immutable and self-contained except for deliberately live authority and secret access. The
publication boundary is race-safe and drafts cannot affect executing agents. Record focused test
commands/results and SQLx regeneration under the rules in [01](01-capability-contracts-and-storage.md)
and [07](07-integration-and-verification.md).
