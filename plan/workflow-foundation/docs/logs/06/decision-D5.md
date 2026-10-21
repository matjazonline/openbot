# 06-D5 — agent-owned workflow capabilities, revision2 (PROPOSED)

Architecture gate for 06B2b. Revision2 answers 06-R014 (findings 06-F008…F014, optional
F015); revision1 is retained as `decision-D5-revision1.md`. No backward compatibility: edit
`migrations/20260817000000_init_schema.sql` in place, reset identified test databases; no
backfill, legacy decoder or staged rollout.

## Evidence that forced this gate

- `authority::supported_publication` refuses every bundle with agent/skill/profile
  snapshots ("no authoritative durable owner yet ... (phase 06)"); it runs at publish
  (`definitions.rs:192`), binding save (`bindings.rs:117`) and admission replay/admission
  (`admission_replay.rs:88`, admission paths). No `agent.run` workflow can be published.
- `agents.granted_tool_ids` accepts only static legacy `CatalogueTool` ids, so it cannot
  hold workflow tools; nothing supplies current grants for revocation checks.

## User decisions (chat, 2026-10-19; quoted in TASK.md)

1. Tools and skills are defined on the agent; `agent.run` uses exactly them. No step-level
   narrowing: remove `capability_profile`, workflow profiles, classifier profile selection
   and the workflow-level `capability_ceiling`. An agent never receives a capability beyond
   its own definition.
2. Skills are company library items attached to agents: prompt text plus required tools.
3. Agent configuration is frozen at workflow version publication; republish picks up
   edits; current revocation still applies at execution.
4. Legacy catalogue and skill tool steps go with the legacy runtime (06C/08); our own native
   tools are repurposed as built-in workflow tools there. "We don't need existing legacy
   behaviour after we implement workflows": the closed `Binding::Legacy` of the shared Rig
   driver is temporary and is deleted with the last legacy caller; no new legacy features.
5. Agent tools are named registered HTTP operations, approved MCP tools and built-in message
   tools; generic `http.request` remains a workflow step only.

## Decision

**Schema.** `agents.workflow_tools text[] NOT NULL DEFAULT '{}'` and
`skills.required_tools text[] NOT NULL DEFAULT '{}'`: ordered, unique, at most 32 entries,
each matching the Rust `TypeName` grammar (one SQL shape function mirrored by a parity test).
Separate from legacy `granted_tool_ids`. Writes validate names syntactically only.

**Writers preserve (F011).** `AgentWrite.workflow_tools` and `SkillWrite.required_tools` are
`Option`: `None` keeps the stored value (`COALESCE` in the existing full-row UPDATEs), `Some`
replaces it. Agent skill links (`agent_skills`) are replaced only while the agent row is
locked `FOR UPDATE` by its writer (verify/keep the existing writer order). Editing UI is
phase 09; until then these fields are written by the agent/skill use cases and tests.

**Snapshot shapes.** `AgentSnapshot` keeps its trusted `agent_id`; `SkillSnapshot` gains a
trusted `skill_id` UUID (never model input). The skill `name` is its slug and must parse as
`TypeName`, else capture refuses with an actionable error. Skill order is canonical:
`agent_skills.position`, then `skill_id`. Skill `instructions` = its prompt texts joined in
order; a skill containing any legacy tool step is refused for workflow use (actionable).

**Agent universe (F009).** For each `agent.run`/`decision.agent` step: a literal `agent`
names one key; a non-literal selector must have a compiled input type that is a finite
string enum of agent keys, and every key in it is captured; otherwise publication refuses
("dynamic agent selection requires a finite set of agent keys"). The existing
`prepare_step_inputs` catalogue check then keeps runtime selection inside that universe.
Each captured agent must have explicit provider and model, and the company model connection
for that provider must offer the model at publication (F015); otherwise refusal.

**Capture and tool composition (F010).** One persistence-owned publication capture runs the
existing step-direct HTTP/MCP/message directories and then the agent capture, and merges
their tool snapshots into the bundle's single tool universe keyed by
`(name, connection slot)`: byte-identical snapshots deduplicate; the same key with different
snapshots, or the same name resolving differently, refuses publication. Granted tools thus
also join the shared bundle universe (they are verified registrations). Agent-granted
names resolve with exactly the SQL owner's rules: HTTP — a single enabled, credential-ready
registration of that name in the company (zero or several → refuse; `http::registered_on`
then re-verifies the exact snapshot); MCP — exactly one MCP slot declared by the draft whose
selected approved tool has that name (`mcp::registered_on` re-verifies); `message.send`/
`message.reply` — the trusted built-in snapshot. Existing post-merge duplicate/validation
rules then apply unchanged. Caller-supplied snapshots are never used.

**Publish-only equality (F008).** In the publish transaction only (`definitions.rs`
publish, after the existing company-first locks), for the new root bundle's own snapshots:
lock every snapshot agent row and its linked skill rows `FOR SHARE`, re-derive with the same
pure mapping, and require exact equality for every agent and skill snapshot in the bundle
(not only literal ones), plus model-connection availability. A concurrent edit fails the
publish (retry recaptures); a forged snapshot is never persisted. Embedded child bundles
were verified at their own publication and are immutable; they are not re-derived.
`supported_publication` (binding save, admission, child admission, admission replay)
drops the blanket agent/skill refusal and does **not** compare current agent/skill rows:
stored bundles are trusted frozen content, so agent edits never block old versions, their
bindings or parents. It keeps today's tool-registration checks for every tool, unchanged.

**Selection.** `PublishedBundle::resolve_agent(company, key)` returns the frozen agent with
exactly its saved tools/skills in order; skill requirements are validated against the
agent's own tools at publication (error names skill and missing tool). Remove
`SelectionSource`/profile fields, `CapabilityProfile`, registry `facts.profiles`, the
`agent.run` `capability_profile` input, `ai.classify` profile-enum support, workflow
`capability_ceiling` (domain definition, compiler wire, outcome/graph) and
`ResolvedAgent::validate_current`. Action-level `ActionPolicyContext.capability_ceiling`
stays. `decision.agent` uses only the agent's instructions and model (tool-free, as the
phase plan's bounded choice). Compiler/bundle revision is bumped with the matching SQL
admission revision; the previous revision is rejected (no fallback).

**Runtime current authority (F012, F013).** The store's `current_on` runs inside model
admission and every `PrepareInvocation` (including replay), before any local workflow lock,
under `FOR SHARE`: agent row still owned by the company; `workflow_tools` still contains
every frozen tool name; every frozen `skill_id` is still linked to the agent; the company
model connection still offers the frozen model. Skill text/required-tool edits are not
revocations (the frozen content is used). Native preparation (`prepare_agent`) no longer
calls `current_on` (it would follow the run lock); the store checks are authoritative.
Additions after publication have no effect until republish; a removal refuses the next
model admission or invocation preparation with terminal `agent.rejected` and no provider
entry or new dispatch. If the refused preparation replays an invocation that was already
dispatched, its marker, receipt or uncertainty stays owned by ActionService
recovery/reconciliation and the shared retry predicate (agent.run joins
`marker_owned_effect_kind` once every agent effect passes ActionService, per D4); the
checkpoint records no result and grants no progression.

## Invariants and failure behaviour

- Effective capabilities ⊆ saved definition at publication ∩ current definition at
  execution. No step, profile, classifier or skill adds one.
- Unknown, unregistered, ambiguous, invalid-name or legacy-only entries fail publication
  explicitly; nothing silently drops or falls back.
- Lock order: publish — existing company-first publication locks, then agent then skill
  rows `FOR SHARE`. Execution — authority class (reviewers, agent/skill-link/model rows)
  → local run/execution/job → checkpoint → root usage. Admission and child admission take no
  agent/skill row locks.

## Affected contracts

Init migration (agents, skills, shape function); agent and skill use cases/writers;
publication types/capabilities/validation/preflight and capture directory composition;
registry facts/families/schemas/budget; compiler wire/domain definition/outcome/graph;
persistence publish (definitions.rs), `authority::supported_publication`, agent
preparation and store `current_on`; fixtures/tests using profiles or ceilings (replaced by
agent-definition tests). 06C removals add: `ai-agents` builtins catalogue,
`granted_tool_ids`, skill tool steps, `agent_mcp_selections`, and `Binding::Legacy` with its
callers.

## Verification plan (F014)

Pure: snapshot mapping, canonical skill order, `TypeName`↔SQL shape parity, dedup/conflict
merge, finite agent universe (literal, enum ref, non-finite ref refused). Capture/publish:
ordered tools/skills; agent with no tools/skills gets none; missing skill requirement,
missing model or unavailable model connection, legacy tool-step skill, digit-leading slug,
unregistered/ambiguous HTTP and MCP grants all refuse; step-direct tool also granted to the
agent dedups. SQL owner: forged snapshot refused; competing agent edit and competing skill
relink during publish (real concurrent transactions) fail one publish; an old version still
binds and admits after an agent edit. Writers: unrelated agent/skill saves preserve
`workflow_tools`/`required_tools`. Runtime: tool removed, skill unlinked, model removed each
refuse before provider entry and before dispatch, including a competing revocation vs
admission race; additions after publication invisible; replay of an already-dispatched
invocation after revocation leaves the receipt with ActionService and grants no
progression. End to end: scripted provider `agent.run` with an HTTP tool, replay without
duplicate provider/effect calls. Then fmt, offline all-target check, strict Clippy, fresh
migration, SQLx prepare/check; full phase06 gate at the end.
