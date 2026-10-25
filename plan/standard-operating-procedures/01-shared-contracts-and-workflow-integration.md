# 01 — Shared SOP contracts and workflow integration

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Establish one contract that lets groups 02–07 extend ordinary workflows without competing for
ownership of execution state. This is the blocking contract gate for parallel implementation.

## Current integration anchors

| Concern | Existing owner / source |
| --- | --- |
| Graph, execution identity, transitions | `src/domain/workflow/{definition,state,transition}.rs` |
| Source fields, validation, compilation | `src/application/workflow/compiler/{wire,compile}.rs` |
| Step contracts and authoring help | `src/application/workflow/registry/` |
| Immutable agent/skill/tool snapshots | `src/application/workflow/publication/{types,storage}.rs` |
| Draft editing, publication, binding configuration | `src/application/workflow/lifecycle/` and `src/application/workflow/binding/` |
| Human submission, proposal hash, comments, reviewer assignment | `src/application/workflow/decisions.rs` and `src/adapters/persistence/workflow/decisions.rs` |
| Leases, durable suspension, maintenance | `src/application/workflow/{worker,wakeups,waits}.rs` |
| Shared action authorization and effect evidence | `src/application/workflow/actions/` |
| Existing capacity and operational metrics | `src/application/workflow/{capacity,metrics}.rs` |
| Isolated samples | `src/application/workflow/sample/` |
| Authoring, decisions, run UI | `src/adapters/http/pages/{workflow_authoring,workflow_decision,workflow_runs}/` |

Check these owners and their subsystem `AGENTS.md` before implementing. Some language-document
introductions still describe early compiler phases; use the current source and
[operations guide](../../docs/workflow-operations.md) to establish supported runtime behavior.

## Batch A — Definition, identity, and responsibility contracts

1. Add a typed, optional `sop` section to workflow source, with procedure purpose, expected business
   outcome, owner/coordinator policy, named responsibilities, and selected measure keys. Add typed
   per-step SOP content for guidance, verification, and handoff/exception policies. Freeze the
   field names, bounds, and normalization in this batch before tracks split. Do not create a
   second `ProcedureDefinition` or put executable behavior in an unvalidated metadata object.
2. Include SOP sections in compiler validation, compiled identity, immutable publication storage,
   diffs, authoring help, and sample inputs. Reject unknown fields and unsupported policy values.
   Guidance edits require republishing; runtime never dereferences a mutable SOP document.
3. Define role keys as domain newtypes. A role describes responsibility and maps to an existing
   company user/group reviewer or the selected saved agent. It does not grant membership, tools,
   skills, model access, or coordinator authority. A coordinator must be a currently authorized
   human. Group membership and authority are rechecked when a person acts.
4. Freeze concrete run responsibility bindings at admission using the existing binding/readiness
   boundary; validate referenced principals and resources within the company. The published
   definition owns role requirements; run admission owns resolved selections. Human roles feed
   ordinary `decision.human` reviewer selection. Agent roles refer to the published agent snapshot.
   Define the validated admission snapshot and its identity now; do not add an alternative start
   path for SOPs. Later handoffs are explicit audited amendments owned by 04.
5. Use existing human decision steps for human work, with declared choices such as `completed`,
   `needs_revision`, or `needs_help` and a structured data schema. Ordinary comments do not complete
   work. Existing child workflows and bounded repeats express rework; this plan adds no parallel
   DAG scheduler or generalized task-assignee table.
6. Bound roles, guidance units, check counts, text, evidence references, aggregate source size,
   result size, and policy collections at decode, publication, HTTP, and execution boundaries.
   Start within existing source/context limits. If a limit must rise, document and retain the
   early-failing CI budget that detects growth before exhaustion.

Focused checks: add `sop_contract_` tests for invalid references, oversized and unknown fields,
stable publication identity, changed-guidance hashes, company isolation, and a non-SOP workflow.
Run compiler, publication, binding, and sample regressions because this batch crosses shared
interfaces. See [08](08-integration-verification-and-release.md) for the command convention.

## Batch B — Evidence, events, and integration fixture

1. Define an evidence reference as company, run, execution/decision or canonical artifact identity,
   exact version/digest, and a declared purpose. It points to source records; it is not copied
   customer text, an arbitrary URL to fetch, or a bearer credential. Resolve current visibility
   before reads. Define typed schemas for required evidence and verification receipts for 03.
2. Freeze an event/projection contract for activity start/end, decision opened/settled, handoff
   offered/accepted/expired, exception raised/resolved, and terminal outcome. Existing workflow
   events remain authoritative for execution. New coordination events describe coordination only;
   do not add another run-status/event stream that must be reconciled with the engine.
3. Record command idempotency, expected revisions/digests, and actor attribution for new mutations.
   Follow existing lock order; a new path cannot lock a decision and then its run if the existing
   cancellation path takes them in reverse. Freeze the order with the persistence owner.
4. Define bounded, tenant-scoped read ports for guidance, responsibility, evidence, timeline,
   measures, and debrief references. Ports belong to the consuming application layer. Projections
   may cache reads but never decide authorization or workflow transitions from stale aggregates.
5. Build a reviewed-invoice fixture with extraction, deterministic validation, human review,
   bounded correction, and a scripted final effect. Include a missing-evidence case, changed
   artifact, unavailable reviewer, cancellation, and two independent cases in one thread. Samples
   stay inside the existing sealed sample boundary with no production writes or deliveries.
6. Record the ownership and dependency contract in this directory as implementation resolves it.
   Add future plan files to `NO_BACKWARD_COMPATIBILITY_NOTES` if the file set changes.

Focused checks: add `sop_contract_` round-trip and fixture tests, tenant/ACL negative tests, and
publish/edit/retry tests proving a run retains the admitted definition and role snapshot.
For SQL changes migrate a fresh test database, regenerate `.sqlx/`, and run affected DB tests.

## Gate for parallel implementation

- One compiled/published workflow contains all SOP configuration used by that version's runs.
- No new procedure run, step occurrence, result-acceptance engine, worker queue, or legacy decoder.
- The shared fixture compiles, publishes, and runs through the actual engine and isolated sample
  path; ordinary non-SOP workflows still work.
- Field schemas, projection/event semantics, source anchors, transaction order, and test fixtures
  are committed before groups 02–06 implement against them concurrently.
