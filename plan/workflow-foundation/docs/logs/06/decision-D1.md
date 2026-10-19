# 06-D1 — capability selection contract (06A)

2026-10-18, implement-astra-reviewed-2.1. Proposed; no dependent code yet.
No backward compatibility: fresh development/test database reset where needed;
no backfill, dual path or payload compatibility decoder.

## Batch boundaries

06A: complete publication/compiler/runtime-pure capability contract, with explicit
workflow ceiling, ordered frozen defaults/profiles, skill requirement enforcement,
trace-ready resolved content and focused tests. This is a stable contract for06B,
not an operational agent handler claim.
06B: workflow-owned durable Rig model/tool execution and explicit context/memory/
classifier/agent-decision handlers. Gate its persistence/concurrency/budget/recovery
changes separately after actual interfaces are traced. Reuse Rig checkpoints and
existing workflow fence/root budget/action/approval ownership.
06C: migrate remaining harness callers and remove ai-agents/runtime selection and
executable skill instructions; complete combined phase06 verification. Phase08 still
owns ingress replacement; no old orchestration bypass may be introduced by06.

## Original requirements

06-agent-context-memory-and-capabilities.md, especially Skills and capability
profiles and Acceptance; phase02 publication contracts stay enforced. No classifier
is required. A routing-only classifier changes nothing about another agent step.

## 06A decision

1. Add optional top-level YAML `capability_ceiling: {tools: [...], skills: [...]}`.
Both lists are required when the object is present; empty means none, omission means
no additional restriction. Null, duplicate names, unknown fields and over-bound lists
are errors. Tool/skill IDs use existing TypeName and MAX_SELECTIONS bounds. Store it
in WorkflowDefinition, so the normalized source/manifest hash and restored publication
include it. Existing formats need no decoder compatibility.
2. Add the pure resolver on PublishedBundle, consuming an agent key plus the explicit
optional profile input (absence distinct from null). It only selects captured
AgentSnapshot/ToolSnapshot/SkillSnapshot entries. Named and inline profiles both replace
default lists. Unknown/malformed selections fail closed. Selection order comes from
the saved defaults or selected profile; no sorting of execution order.
3. AgentSnapshot.tools/skills are the frozen agent grants/defaults. Selection is
intersected with those grants and optional workflow ceiling, preserving order.
No profile grants authority. Validate every remaining selected skill's required
 tools against effective tools, failing with its name and missing requirement.
4. Resolver returns typed source (agent_defaults/profile), optional named profile,
frozen model/instructions, selected skill snapshots, operational ToolSnapshots and
ordered IDs. No credentials, live content or classifier ambient state. The resolver
also validates caller company against bundle ownership. Unknown agent fails.
5. Current revocation belongs to the execution boundary.06A supplies a pure method to
validate the frozen selection against current allowed tool/skill sets, failing on any
revocation; it never reloads instructions, adds capabilities or changes selection.
06B must persist the initial resolved selection, enforce current policy before model
calls/tool dispatch, and fail closed on lookup errors. ActionService remains final
transactional authority for effects. This method alone is not dispatch permission.
6. Consolidate profile parsing/skill requirement rules with publication validation
where fitting, rather than duplicating fallback logic. Existing validation remains at
publication and activation.06A does not add a new policy database or service.

## Failure behavior and invariants

Explicit null/unresolved/unknown profile cannot turn into defaults. An empty agent
or profile offers no tools/skills. Profiles cannot widen saved grants or ceilings.
Missing requirements are errors, never an implicit grant. Model/instruction edits
cannot mutate a previously resolved result. Current revocation refuses its use;
new grants cannot widen it. Source choice and order survive serialization for06B.
All new parsing/selection work is bounded by publication catalogue limits.

## Verification and acceptance

Focused compiler/publication/resolver tests cover omission, named/static/dynamic
profiles, explicit empty/null/unknown/malformed, missing one list, duplicates,
unknown dependency, agent with none, profile outside grants, empty/omitted/partial
ceiling, missing required tool, order, cross-company rejection, frozen content after
original mutation, serialize/restore, current revocation and new-grant non-widening.
Existing classifier-routing/profile tests remain. fmt, diff check, offline all-target
check and strict Clippy. No SQL changes in06A; database migration/full suites remain
mandatory at phase06 completion. B reviews actual entire06A diff and callers after
focused gates. Root accepts exact frozen artifact only after PASS.
