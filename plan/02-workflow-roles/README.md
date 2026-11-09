# 02 Workflow roles: procedures name a role, admission picks the principal

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-09. Second of four organization plans; depends on
[01-departments](../01-departments/README.md) being implemented. Followed by
[03-principal-addresses](../03-principal-addresses/README.md) and
[04-root-unit](../04-root-unit/README.md).

## Intended behavior

A workflow step that needs a principal can name a department role instead of a specific agent or
person. `agent.run` and the other agent-backed steps accept `role: lead` in place of `agent:`;
`decision.human` accepts role references among its reviewers. The role is resolved when a run is
admitted, against the department that owns the desk the run arrived on, walking up to the nearest
ancestor that holds the role. The resolved principal and its current published snapshot are frozen
into the run. The same published procedure then serves every department.

Today `agent.run` pins one agent at publication and freezes that agent's snapshot into the version
(`AgentReference`, `src/application/workflow/publication/types.rs:19`). That stays available: a
step may still name an agent. The role form adds a second way to fill the slot, not a replacement.

## Decisions (user, 2026-11-09)

1. **Roles are a fixed vocabulary.** `DepartmentRole` gains `Reviewer` beside `Lead`, and
   `department_roles_role_check` is widened to match. A company does not define its own role
   names; adding one is a domain variant, a check widening and a test. `lead` keeps its single
   holder; `reviewer` may have several.
2. **Resolution happens at admission, once, and is frozen.** The run records which department
   resolved the role, which principal held it and which agent revision was captured. Later role
   changes affect later runs only. Agent deactivation and deletion remain live authority events
   exactly as they are for pinned agents.
3. **Resolution walks up the hierarchy.** If the owning department does not hold the role, the
   nearest ancestor that does supplies it, within the depth bound of 8. A desk with no department,
   or a company-wide binding, cannot activate a workflow that names a role; the binding
   configuration refuses it with a message naming the step.
4. **An `agent.run` role must resolve to an agent; a `decision.human` role must resolve to people.**
   A `lead` who is a person cannot fill `agent.run`; the run fails at admission with a definite,
   bounded error, and the desk's reply path reports it the way a missing agent is reported today.
5. **Reviewers become principals.** `workflow_decision_reviewers` and the `decision.human`
   `reviewers` input move from user ids to principal ids, so a role that resolves to person
   principals plugs in without translation. Agent principals are still refused as reviewers.
6. **Publication freezes the requirement, not the holder.** The compiled version records which
   steps name which roles and in which mode (`AgentUseMode`), the same way it records agent
   references. Capability checks that today run at publication against the pinned agent run at
   admission against the resolved agent, with the same rules.

## What does not change

- Pinned agents, their publication snapshots and the forged-snapshot refusals
  (`workflow_agent_publication_refuses_forged_snapshot_and_keeps_frozen_version`,
  `src/adapters/persistence/workflow/agent_runtime_tests.rs:289`).
- The binding's `run_as` actor (`src/application/workflow/binding/mod.rs:29`). The resolved agent's
  tools, skills, connections and knowledge are checked for that actor, as for a pinned agent.
- Workflow immutability: nothing in a running run dereferences a mutable role table after
  admission.

## Documents

1. [Role references and publication](01-role-references-and-publication.md).
2. [Admission resolution and run snapshots](02-admission-resolution-and-snapshots.md).
3. [Reviewers as principals and authoring UI](03-reviewers-and-ui.md).
4. [Integration and final verification](04-integration-and-final-verification.md).
