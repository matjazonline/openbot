# 02 Admission resolution and run snapshots

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

When a run is admitted on a desk, every role reference in its version is resolved to a principal
and, for agent steps, to that agent's current published revision, and the result is frozen into
the run. Continues in [03](03-reviewers-and-ui.md).

## Batch 02A Resolution

1. One pure function `resolve_role(department_chain, role, need)` in
   `src/application/workflow/roles.rs` takes the owning department and its ancestors (already
   loaded under the company lock), the role and whether an agent or people are needed, and returns
   `Resolved { department_id, principals }` or a typed refusal (`NoHolder`, `WrongKind`). It
   has no I/O and is unit tested across every combination.
2. Admission (`create`, `src/adapters/persistence/workflow/admission_write.rs:7`) loads the desk's
   department chain in the same statement set that reads the channel under its gate, calls
   `resolve_role` for every `RoleReference`, and for agent steps captures the resolved agent's
   current published revision with the capture path publication uses (`capture_agents_on`,
   `src/adapters/persistence/workflow/agent_publication.rs:48`), taking the same `FOR KEY SHARE`
   on the agent row so a concurrent delete either sees the run's reference or finds no agent.
3. A refusal admits nothing and records a bounded, definite failure on the intent, reported
   through the same path a disabled channel uses in the relay
   (`docs/inter_channel_agent_communication.md`, "Refusals"). The error names the step and the
   role, never the principal list.

## Batch 02B Storage

1. `workflow_run_role_bindings (company_id, run_id, step, role, department_id, principal_id,
   agent_revision_id NULL)`, composite foreign keys to runs, departments, principals and agent
   revisions; guarded immutable after insert like the run snapshot
   (`workflow_run_snapshot_immutable`). One row per resolved principal.
2. Agent execution (`agent_runtime`, `src/adapters/persistence/workflow/agent_runtime*.rs`) reads
   the agent for a step from the version's `AgentReference` when pinned, else from the run's role
   binding. Everything after that lookup is unchanged: the frozen revision's prompt, tools and
   grants are what run, and live authority events on the agent still stop it.
3. Run inspection pages and the board show the resolved holder with a "via role" marker.

## Focused checks

- `workflow_role_resolve_`: unit coverage of the pure resolver, including the ancestor walk,
  depth bound, missing holder, person holder for an agent step and agent holder for a human step.
- `workflow_role_admission_`: with the scripted-model `Deployment` harness, the same published
  version bound on two department desks runs two different lead agents; a role reassignment after
  admission does not change a running run; a reassignment before the next message does; a desk
  whose department loses its lead refuses admission with the named error and leaves no run.
- `workflow_role_competing_`: a concurrent agent delete and admission leave either a refused
  delete or a refused admission, never a run bound to a missing revision.
