# 03 — Workflow references and durable run snapshots

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-agent-publication-and-skills.md). Publish workflows that reference reusable
agents, and capture exact agent revisions when admitting each new root run. Publishing an agent
changes future runs without changing an existing workflow version or a running execution.

## Ownership anchors

- [Agent selection/capture](../../src/adapters/persistence/workflow/agent_publication.rs),
  `agent_selection`, `capture_agents_on`, and `granted_tool`.
- [Published capability lookup](../../src/application/workflow/publication/capabilities.rs),
  `resolve_agent`: currently resolves from the workflow's frozen agent/tool/skill collections.
- [Admission writes](../../src/adapters/persistence/workflow/admission_write.rs), `create` and
  `insert_run`: transaction that creates a run, admission record, and first work.
- [Admission replay](../../src/adapters/persistence/workflow/admission_replay.rs): retries must
  return the original admitted execution and its stored selections.

## Batch 03A — Replace embedded agent definitions with references

1. Make published workflows retain authorized company agent identities and their source locations.
   Preserve the closed, bounded agent candidate universe for dynamic selection. Resolve authored
   slugs to durable identities so renaming an agent does not redirect a published reference.
2. Remove the requirement that workflow publication capture every agent tool and skill in global
   name-indexed collections. Keep workflow-owned steps/resources under existing workflow ownership;
   agent capabilities resolve exclusively through the selected agent revision.
3. Workflow publication validates agent identity/eligibility and its task/output interface. Treat
   a displayed active revision as a preview, not an execution pin. Run admission performs the
   authoritative active-revision and capability-readiness checks.
4. Keep existing step input/output schemas and model-only behavior for `classify`/`decide`.
   A more capable agent does not grant those steps tool execution or relax output validation.
5. Update workflow hashes, serialization, compiler/publication checks, diagnostics, fixtures, and
   callers coherently. Remove obsolete coupling rather than retaining old/new snapshot decoding.

Focused checks: `autonomous_agent_workflow_reference_` compiler/publication tests for a workflow
containing only `agent.run` and no MCP slots, two workflows using the same agent, closed dynamic
selection, cross-tenant references, slug changes, and unchanged output-schema enforcement. Run
the affected existing registry, publication, capability, and compiler suites at this shared seam.

## Batch 03B — Capture the dependency manifest at admission

1. Before committing a new root run, resolve its complete bounded reachable dependency set:
   workflow-selected agent identities, their active revisions, pinned workflow-tool children,
   explicit child workflow versions, and agents referenced by those child definitions. Capture
   possible declared branches, not just whichever branch happens to execute first.
2. Store one revision selection per company agent in the root run's immutable dependency manifest.
   Descendant runs inherit that manifest. Repeated use of an agent within the run tree therefore
   retains one revision even if its active revision changes while the tree is executing.
3. Select and verify the manifest consistently in the admission transaction and persist it with
   the run, admission identity, and initial work. Define lock ordering/revision checks for races
   with agent publication. Either commit a coherent selection or retry/refuse; never leave a run
   with missing or mixed dependency facts. Do not perform remote discovery under database locks.
4. Bound node count, depth, manifest bytes, and discovery/lookup work. Detect cycles in the combined
   agent/workflow dependency graph. Existing workflow-tool pins stay exact; never silently follow
   the latest registered child version to complete the closure.
5. Runtime-computed agent/child selections must belong to the declared captured universe. Reject
   an undeclared dependency instead of loading a newly active revision midway through execution.
6. Repeated admission with the same logical identity returns the stored run and manifest. Recovery,
   worker reconstruction, and continuation load those identifiers and never recapture current
   agent/skill configuration. A missing active revision or invalid dependency fails before a run
   can call a provider or commit an effect.
7. Preserve scheduling semantics: selection occurs when the logical run is durably admitted,
   not when a worker first claims it. An already admitted queued run retains its selected revision.

Focused checks: `autonomous_agent_admission_` database tests for v1 admission, v2 publication,
unchanged workflow version, and subsequent v2 admission; an older queued run remains v1. Race
duplicate admissions against publication with actual competing transactions. Cover crash rollback,
idempotent replay after an activation change, bounded/cyclic closure refusal, dynamic selection,
and parent/child revision consistency. Run affected admission and child-workflow regressions.

## Acceptance and evidence

The run record is sufficient to recover the exact agent and child-workflow configuration without
reading current definitions. New runs adopt active revisions; old runs retain their manifests.
Current authority remains a separate gate implemented in [04](04-tool-dispatch-and-live-authority.md).
Record focused counts/results and SQLx changes; final checks are in [07](07-integration-and-verification.md).
