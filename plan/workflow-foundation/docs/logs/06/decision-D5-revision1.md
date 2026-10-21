# 06-D5 — agent-owned workflow capabilities, revision1 (PROPOSED)

Architecture gate for 06B2b, found during R1–R4 implementation. No backward
compatibility: edit `migrations/20260817000000_init_schema.sql` in place, reset identified
test databases; no backfill, legacy decoder or staged rollout.

## Evidence that forced this gate

- `adapters/persistence/workflow/authority.rs::supported_publication` refuses every bundle
  with agent/skill/profile snapshots ("Agents, skills and profiles have no authoritative
  durable owner yet ... (phase 06)"). No `agent.run` workflow can be published.
- `agents.granted_tool_ids` accepts only static legacy `CatalogueTool` ids
  (`use_cases/agent.rs::normalize_tool_grants`), so it cannot hold workflow tools, and no
  current grant source exists for revocation checks.

## User decisions (chat, 2026-10-19; recorded verbatim in TASK.md)

1. Tools and skills are defined on the agent. `agent.run` uses exactly the agent's saved
   tools and skills. No step-level narrowing: remove `capability_profile`, workflow
   profiles and classifier profile selection; also remove the workflow-level
   `capability_ceiling` (unused outside agent selection). An agent can never receive a
   capability beyond its own definition.
2. Skills are company library items attached to agents: prompt text plus declared
   required tools (new field).
3. Agent configuration is frozen at workflow version publication; republish picks up
   edits. Current revocation still applies at execution.
4. Legacy catalogue (`ai-agents` builtins) goes with the legacy runtime (06C/08). Our own
   native tools are repurposed as built-in workflow tools there; skill tool steps are
   removed then. New agent workflow-tool grants and skill required tools are added now.
5. Agent tools are named registered operations (HTTP), approved MCP tools and built-in
   message tools; the generic `http.request` stays a workflow step, not an agent tool.

## Decision

**Schema.** `agents.workflow_tools text[] NOT NULL DEFAULT '{}'`: ordered, unique, at most
32 workflow type names (SQL shape check). It is separate from legacy `granted_tool_ids`.
`skills.required_tools text[] NOT NULL DEFAULT '{}'`, same shape. Agent writes validate the
names syntactically only; registration is checked at publication and dispatch, because a
tool may be registered after the grant is saved.

**Capture (publication directory).** A persistence-owned `AgentPublicationDirectory`
derives `AgentSnapshot`/`SkillSnapshot` for every agent key referenced by the draft's
`agent.run`/`decision.agent` steps (key = agent slug, same company): agent UUID, system
prompt, explicit provider/model (an agent without both is refused with an actionable
error; no implicit company default), `max_output_tokens` = Rig default request output
bound, `workflow_tools` in saved order, skills in `agent_skills.position` order with
instructions = prompt texts joined in order and `required_tools`. A skill containing any
legacy tool step is refused for workflow use with an actionable error. Tool snapshots for
granted names: HTTP via the existing HTTP publication directory registration lookup; MCP
via the workflow's MCP selections, requiring exactly one declared MCP slot whose approved
tool has that name; message builtins via the trusted snapshot. A granted name that
resolves to none, or to several, refuses publication. Caller-supplied snapshots are never
used. It composes with the existing HTTP/MCP/message directories rather than replacing them.

**SQL owner.** `supported_publication` replaces its blanket agent/skill refusal: inside the
publication transaction, lock each referenced agent row and its skill rows `FOR SHARE`,
re-derive the snapshots with the same pure mapping, and require exact equality with the
bundle's agent and skill snapshots (and that each agent's tools equal its selection
universe). Profiles must be absent. Tool registration checks stay as they are. A concurrent
edit fails the publication (retry recaptures); a forged snapshot can never be persisted.

**Selection.** `PublishedBundle::resolve_agent(company, key)` returns the frozen agent with
exactly its saved tools/skills in order; skill requirements are validated against the
agent's own tools at publication (actionable error naming skill and missing tool). Remove
`SelectionSource`/profile fields, `CapabilityProfile`, registry `facts.profiles`, the
`agent.run` `capability_profile` input, `ai.classify` profile-enum support, workflow
`capability_ceiling` (domain definition, compiler wire, outcome/graph) and
`ResolvedAgent::validate_current`. Action-level `ActionPolicyContext.capability_ceiling`
(bundle tool policies) is unrelated and stays. Compiler/bundle revision is bumped with the
matching SQL admission revision; previous revision is rejected (no fallback).

**Runtime current authority.** `current_on` (already inside model admission before local
locks, and at every `PrepareInvocation` including replay) additionally requires, under
`FOR SHARE`: agent row owned by the company; `workflow_tools` still contains every frozen
tool; every frozen skill is still linked to the agent with unchanged required tools; the
company model connection still offers the frozen model. Additions after publication have
no effect until republish; removals refuse the next model call or dispatch (terminal
`agent.rejected`). ActionService still rechecks registration/connection/policy at each
effect. Frozen preparation, request fingerprints and D4 protocols are otherwise unchanged.

## Invariants and failure behaviour

- An agent's effective capabilities ⊆ its saved definition at publication ∩ its current
  definition at execution. No step, profile, classifier or skill can add one.
- Unknown, unregistered, ambiguous or legacy-only entries fail publication explicitly;
  nothing silently drops or falls back.
- Lock order unchanged: authority class (reviewers, agent/skill/model rows) → local
  run/execution/job → checkpoint → root usage. Publication locks agent/skill rows in the
  existing company-first publication transaction.

## Affected contracts

Init migration (agents, skills, shape functions); agent use case/write path and agent
settings persistence for `workflow_tools`; skill write path for `required_tools`;
publication types/capabilities/validation/preflight; registry facts/families/schemas/
budget; compiler wire/domain definition/outcome/graph; fixtures and tests that use
profiles or ceilings; persistence authority/agent directory/agent_preparation; 06A tests
that cover removed profile/ceiling behaviour are replaced by agent-definition tests.
UI editing of workflow tool grants and skill required tools is phase 09.

## Verification plan

Focused: pure snapshot mapping; publication capture (ordered tools/skills, missing
model, legacy tool-step skill, unregistered/ambiguous MCP tool); SQL owner equality with a
competing agent edit during publication and a forged snapshot; removed profile/ceiling
inputs rejected by the compiler; runtime revocation (tool removed from agent, skill
unlinked, model removed) refuses before provider entry and before dispatch; additions after
publication not visible; end-to-end scripted provider `agent.run` with an HTTP tool,
replay without duplicate provider/effect calls. Then fmt, offline all-target check,
strict Clippy, fresh migration, SQLx prepare/check; full phase06 gate at the end.
