# Plan 1 — Durable Procedure Execution for Human–Agent Teams

## Outcome

Add versioned standard operating procedures (SOPs) as a durable coordination layer above
`background_tasks`. A procedure says what business result must be produced and in what order; a
procedure run records where one case is in that process; a task assigns one currently actionable
piece of work to a human or agent; a skill supplies agent-specific instructions for doing that
work.

This plan delivers the smallest useful end-to-end flow:

```text
published procedure version
  -> start procedure run from a thread
  -> activate eligible step occurrence(s)
  -> create one owned task for each active occurrence
  -> human or agent submits a structured result
  -> accept the result and advance the run atomically
  -> complete, cancel, or escalate the run
```

The foundation deliberately supports the operating techniques in
[Plan 2](02-operational-techniques.md). Plan 2 extends the definition schema, result types, event
vocabulary, and projections established here. It must not introduce a second workflow engine,
step table, handoff model, checklist task, or analytics event stream.

## Existing contracts to preserve

- `BackgroundTask` remains the executable work item and owns queue status, retries, worker leases,
  business ownership, due time, and technical attempt history.
- `TaskOwner` continues to represent a human, an agent, or unassigned work. SOP roles resolve to
  this existing ownership model rather than defining another kind of assignee.
- Task transfer changes who performs the current step occurrence. It does not advance the SOP.
  The existing private handoff instruction remains execution metadata for that transfer.
- Approval, outreach, response review, and delivery retain their current state machines. An SOP
  step may wait on or reference them, but must not mirror their states in procedure JSON.
- Skills remain agent recipes compiled by harness adapters. An SOP selects and snapshots a skill;
  it does not turn every procedure step into a `SkillInstruction`.
- Correlation IDs, canonical messages, task status events, ownership events, and durable delivery
  guarantees continue across work started by an SOP.
- Procedure definitions and runs are tenant-scoped. The domain and application layers remain
  independent of SQLx, Axum, transports, and harness-specific types.

## Vocabulary and ownership of state

Use distinct names in code and storage so business progress is not confused with worker state:

| Concept | Durable owner | Meaning |
| --- | --- | --- |
| `Procedure` | `procedures` | Stable company-owned identity, name, and lifecycle |
| `ProcedureVersion` | `procedure_versions` | Immutable published definition used by runs |
| `ProcedureRun` | `procedure_runs` | One business case following exactly one version |
| `StepOccurrence` | `procedure_step_occurrences` | One activation of a defined step; rework creates another occurrence |
| `StepResult` | `procedure_step_results` | Immutable submitted output and evidence for one occurrence |
| `BackgroundTask` | existing queue | The work assigned to the human or agent performing that occurrence |
| `SkillSnapshot` | inside the published version | Exact agent instructions and tool references selected at publication |

A task attempt is a technical retry. A step occurrence is a business attempt. Retrying a provider
call stays within the same task and occurrence; requested rework creates a new occurrence and task,
preserving the rejected result as history.

## Definition model

### Procedure and version lifecycle

Add a stable `procedures` row with company, slug, name, description, owner principal, active
published version, timestamps, and `archived_at`. Add `procedure_versions` with:

- procedure/company IDs and a monotonic version number;
- state: `draft`, `published`, or `retired`;
- `schema_version`, bounded JSONB definition, canonical hash, author principal, change summary, and
  timestamps;
- a database rule and application guard that make a published version immutable; and
- uniqueness on `(procedure_id, version_number)` and on one active draft per procedure.

Editing updates a draft behind an edit version. Publishing validates and freezes it in one
transaction, switches the procedure's active version, and records an event. Runs always point to a
published version and never silently move when a newer version is published.

Use one bounded version document for ordered authoring content. Static step definitions are edited
and published together, while runtime state is normalized into rows. The domain parses the JSONB
into a tagged `ProcedureDefinitionV1`; application code never navigates it through ad hoc JSON
pointers.

### `ProcedureDefinitionV1`

The initial definition contains:

- a bounded set of named role slots, each with a stable `role_key`, display name, allowed principal
  kinds (`human`, `agent`, or both), and whether it must be bound when a run starts;
- 1–64 steps with a stable `step_key`, title, concise instructions, responsible role key, expected
  result type, due-duration policy, prerequisite step keys, and activation policy;
- optional start form fields with stable keys, primitive/structured types, bounds, required flags,
  and sensitivity classification;
- terminal success criteria expressed as required accepted step keys; and
- optional labels used for presentation, never for authorization or transition decisions.

V1 supports a bounded acyclic dependency graph. Most procedures will be linear, but allowing
multiple prerequisites now avoids a schema replacement when Plan 2 introduces team briefs,
parallel checks, and review paths. Publishing rejects cycles, duplicate keys, missing role or step
references, unreachable required steps, unbounded schemas, unsupported result types, and graphs
whose theoretical active fan-out exceeds a configured limit.

Activation policy in V1 is deliberately small: `all_prerequisites_accepted` or
`any_prerequisite_accepted`. Conditional expressions, scripts, arbitrary code, and BPMN import are
out of scope. A later tagged policy can be added without reinterpreting stored V1 documents.

### Extension points reserved for Plan 2

Reserve these names and locations in the schema contract rather than adding a free-form `metadata`
escape hatch:

- procedure-level `coordination`, `measures`, `work_in_progress_policy`, and
  `improvement_policy`; and
- step-level `job_breakdown`, `verification`, `handoff_policy`, `exception_policy`,
  `decision_outcomes`, and `review_policy`.

V1 refuses these fields instead of accepting and ignoring behavior the author may rely on. Plan 2
introduces them as typed sections in V2, validates their meaning, and adds their transitions.
Unknown fields and schema versions fail closed at publication and execution boundaries. A V1-to-V2
editor conversion copies the common core explicitly; published V1 bytes never change.

### Skill reproducibility

For an agent-only or mixed role step, an optional skill binding identifies a company-visible
skill. Publishing resolves it and embeds a bounded `SkillSnapshot` containing skill ID, slug,
name, description, trigger, instructions, referenced tool IDs, and a content hash. Starting and
resuming a run use that snapshot even if the library skill is later edited or removed.

Factor harness compilation so both a current `Skill` and a procedure's validated `SkillSnapshot`
compile through the same inner representation. Continue to enforce tool grants at runtime. A
snapshot never grants a tool the selected agent or company policy no longer permits; such a run
parks as a configuration exception rather than broadening authority.

## Runtime state

### Procedure runs

Add `procedure_runs` with:

- ID, company and procedure-version IDs, optional initiating thread/message IDs, and inherited
  correlation ID;
- state: `running`, `waiting`, `completed`, `cancelled`, or `needs_attention`;
- bounded structured start inputs plus their schema/hash;
- a human coordinator principal responsible for unresolved exceptions;
- monotonic `version`, start/complete/cancel timestamps, and bounded terminal reason; and
- an idempotent external start key scoped by company and trigger source.

The run state is a projection maintained in the same transaction as step transitions. `waiting`
means there is unfinished business but no runnable task; `needs_attention` means progress requires
an explicit coordinator action. Neither state is represented by a sleeping worker.

Add `procedure_run_role_bindings` so each required role key resolves to an eligible principal for
that run. Store both the principal ID and a display snapshot. Binding or rebinding is a
version-fenced, idempotent, audited command. Rebinding changes future task creation; moving an
already active task uses the existing task-transfer operation so its lease is revoked correctly.

### Step occurrences and tasks

Add `procedure_step_occurrences` with company/run IDs, definition step key, occurrence number,
business state, assigned role key, due time, activation and resolution timestamps, and a monotonic
version. Enforce uniqueness on `(procedure_run_id, step_key, occurrence_number)`.

Business states are:

- `blocked`: prerequisites are not yet satisfied;
- `ready`: eligible for activation inside the transition transaction;
- `active`: one current background task owns the work;
- `waiting`: its source approval, outreach, or review state is parked elsewhere;
- `accepted`: an accepted result satisfies this occurrence;
- `superseded`: preserved history after explicit rework creates a replacement;
- `waived`: a coordinator ended the occurrence with an audited reason; and
- `cancelled`: its containing run was cancelled.

Do not copy task `pending`, `processing`, failure, lease, or retry fields into this table. A task's
technical failure moves the occurrence/run to `needs_attention` through an explicit reconciler;
the procedure engine does not infer business acceptance from `TaskStatus::Completed`.

Link SOP-created tasks with normalized `procedure_run_id` and `procedure_step_occurrence_id`
foreign keys and a `TaskSource::ProcedureStep` equivalent in the queue contract. The durable task
payload contains identifiers and bounded execution inputs, never the procedure definition or
mutable workflow state. Enforce at most one nonterminal task for an occurrence.

### Results and evidence

Add immutable `procedure_step_results` containing occurrence/company IDs, submission sequence,
submitter principal, result kind, bounded structured value, schema/hash, optional summary,
evidence references, command ID, and timestamp. Results hold references to canonical messages,
response drafts, deliveries, approvals, outreach responses, or separately managed attachments;
they do not duplicate message bodies or binary files.

V1 result kinds are `structured`, `decision`, `artifact_references`, and `no_output`. The
definition supplies the expected shape. Validation happens before persistence and is repeated by
database bounds where practical. Add immutable `procedure_step_result_reviews` to record acceptance
or rejection, reviewer authority, the exact result hash, command ID, and an optional bounded
reason. A submitted result never changes in place; acceptance is reflected in the review record and
the occurrence transition. Automatic acceptance inserts its review in the submission transaction.

An agent's natural-language final response is not sufficient completion evidence. The procedure
execution adapter requires the agent to call a scoped `submit_step_result` tool using the declared
result shape. The tool remains fenced by task lease, task ownership version, occurrence version,
and run version. A human uses the same application command through a form.

## Transition engine and atomicity

Implement one pure domain transition function. Given the immutable definition and a snapshot of
run/occurrence/result state, it returns a named outcome such as:

- reject the command with a typed reason;
- accept the result and activate a bounded set of successor steps;
- park the run because no step is currently actionable;
- complete the run because all terminal success criteria are satisfied; or
- require coordinator attention.

Application use cases load the state, authorize the actor, invoke the pure decision, then ask a
purpose-specific persistence port to commit it. Every mutating command carries company ID, stable
command UUID, expected run version, expected occurrence version where relevant, actor, and typed
reason. Command reuse with identical content returns the original outcome; reuse with a different
fingerprint conflicts.

The following become visible together or not at all:

- starting a run, binding its roles, materializing initial occurrences, creating initial tasks,
  appending events, and enqueueing notifications;
- accepting a result, closing/superseding its task, changing occurrence/run state, activating
  successors, creating their tasks, appending events, and enqueueing notifications; and
- cancelling a run, revoking active task leases, cancelling occurrences, appending events, and
  cancelling unsent procedure notifications.

Lock the run first, then affected occurrences and tasks in a documented stable order. Never hold a
database transaction open across an agent/provider invocation or outbound transport call.

## Events and audit

Add an append-only `procedure_events` ledger with company/run IDs, monotonic sequence, optional
step occurrence ID, command ID, typed event kind, typed actor kind/ID, bounded reason, related
record IDs, previous/new run and occurrence versions, and timestamp.

Initial event kinds cover publication, run start, role binding/rebinding, step activation, result
submission/acceptance/rejection, rework creation, waiting/resumption, task failure escalation,
waiver, cancellation, and run completion. Content stays in its source record; events contain IDs
and safe summaries. Database constraints make actor shape, event references, versions, and event
immutability explicit.

Procedure events are the source for history and future process analysis. PostgreSQL notifications
and SSE carry only company/run identifiers and act as wake-ups; readers reload authorized state.

## Application behavior

Add cohesive application ports for definition persistence, procedure execution commits, and
read-only run projections. Correctness operations have no silently successful defaults.

The initial use cases are:

1. Create/edit/validate/publish/retire a procedure version.
2. Start a run from a visible thread, bind roles, and create the first work atomically.
3. Submit and accept a human or agent step result.
4. Reject a result and create an explicit rework occurrence.
5. Rebind a future role or transfer an active task through the existing ownership service.
6. Waive a step, cancel a run, or resolve a configuration exception as coordinator.
7. Read a company procedure library and an authorized run timeline/progress projection.

Agent execution receives only the current step's instructions, declared inputs, relevant accepted
results, skill snapshot, private current-task handoff, and scoped result-submission tool. Delimit
all user-authored inputs/results as untrusted data. Do not place the entire definition or unrelated
step outputs into the prompt.

When a result needs human review, the agent submits it and the task commits without publishing an
external reply. Delivery is modeled as a later explicit procedure step so approval and delivery
cannot be conflated.

## HTTP and product surface

- Add a company procedure library with draft editor, validation results, version history,
  publish/retire controls, and a preview of the execution graph.
- Add “Start procedure” to authorized thread actions. The start form collects declared inputs,
  coordinator, and required role bindings.
- Show a procedure-run panel on the thread: objective, version, coordinator, current steps,
  owners, due times, accepted outputs, waits, exceptions, and history.
- Reuse task ownership controls for active work. Make “transfer this task” and “rebind this role”
  separate actions with clear scope.
- Give human owners a result form generated from the declared result schema. Keep “submit internal
  result,” “publish reply,” and “add private note” as distinct commands.
- Notify newly assigned humans and coordinators through the existing durable notification/delivery
  paths. Notifications contain a secure deep link and minimal identifiers, not private result or
  handoff content.
- Every list, detail, event, result, and mutation route rechecks company membership and channel or
  thread visibility. Procedure ownership alone never grants access to a restricted thread.

## Operational bounds and recovery

- Bound definition bytes, start inputs, result bytes, evidence count, roles, steps, graph fan-out,
  active steps per run, open runs per company, and history page size.
- Apply global and per-company limits when procedure transitions create tasks. Exceeding a limit
  leaves durable `ready` occurrences and wakes a bounded materializer; it does not partially lose
  work or allocate without limit.
- Add a supervised reconciler that finds `ready` occurrences without tasks, stale active
  occurrences whose task reached a terminal technical failure, and waiting runs whose source state
  changed. Claim reconciliation rows with leases/backoff, exclude poison rows, and surface dead
  letters.
- A start, completion, or reconciliation retry uses the logical command/source key and cannot
  create duplicate runs, occurrences, tasks, results, events, or notifications.
- Removing a principal leaves historical snapshots intact, prevents future role resolution, and
  routes affected active/future work to coordinator attention through audited commands.

## Implementation sequence

### 1. Domain contract and definition validation

Add procedure entities, validated newtypes, tagged definition versions, graph validation, result
schemas, transition outcomes, and pure tests. Record the state-ownership table above in module
documentation. Do not add database or HTTP types to these modules.

### 2. Schema, immutable versions, and offline metadata

Add tenant-scoped tables, constraints, indexes driven by the planned queries, and immutable event
guards. Link background tasks to occurrences with normalized foreign keys. Regenerate and commit
SQLx metadata after changing SQL.

### 3. Publishing and run-start transaction

Implement draft validation/publishing, skill snapshot resolution, role eligibility, idempotent run
start, initial graph materialization, task creation, events, and durable notifications. Expose the
minimal manager authoring and thread start flows.

### 4. Human and agent result completion

Add the shared application command, human form, scoped agent tool, result validation, explicit
review/rework, atomic successor activation, and run completion. Refactor human task completion so
internal SOP completion does not imply an outbound message.

### 5. Recovery, history, and operational UI

Add the reconciler, run timeline/progress projection, exceptions, cancellation, metrics, alerts,
and operational controls. Validate shutdown ownership if a new worker loop is introduced.

## Tests

### Pure domain tests

- Definition round-trip, bounds, tagged-version rejection, stable hash, graph reachability and
  cycle detection, missing references, role compatibility, result-shape validation, and terminal
  completion rules.
- Rework creates a new occurrence while technical retry does not.
- A task completion without an accepted result never advances business state.

### Database and concurrency tests

- Two simultaneous run-start commands for the same source create one run and one initial task set.
- Two competing result submissions or acceptances produce one winning version and one successor
  set; a stale command cannot advance the run.
- Result acceptance racing with task transfer yields one serial order, with no stale owner result
  accepted and no duplicate task.
- Cancellation racing with agent result commit either completes the step before cancellation or
  rejects the stale result; effects never straddle both outcomes.
- Parallel predecessor completion activates a shared successor once.
- Published definitions/events/results are immutable; all tenant and principal foreign keys fail
  closed across companies.
- Crash injection around each transaction boundary proves recovery without duplicate tasks,
  results, events, or notifications.

### Execution, authorization, and UI tests

- Human and agent owners receive the same declared inputs and must satisfy the same result schema.
- Skill snapshot execution remains reproducible after the source skill changes, while revoked tool
  authority parks safely.
- Restricted-thread access, coordinator actions, role rebindings, transfer visibility, evidence
  reads, and cross-company deep links are authorized independently.
- Internal result submission cannot accidentally publish a message; a later delivery step uses the
  accepted artifact and existing delivery guarantees.
- SSE loss and reconnect converge by re-reading durable state.

## Observability and initial measures

Emit bounded metrics for runs started/completed/cancelled/needs-attention, step activation and
acceptance, technical failure, rework count, active/waiting age, and transition conflicts. Derive
elapsed times from durable timestamps rather than in-process timers. Tag only bounded identifiers
such as result kind and procedure version number; do not put company, procedure, step, or content
values into metric labels.

Plan 1 records the timestamps and events needed by Plan 2 but does not claim performance gains.
Collect a baseline for lead time, waiting time, first-pass acceptance, human touch time where
available, technical retries, and outcome failures before enabling optimization recommendations.

## Rollout

1. Ship definition storage and validation behind a company feature flag.
2. Enable publish/start for internal pilot procedures with no external delivery step.
3. Enable agent result submission for one low-risk procedure and compare the event history with
   task/ownership ledgers.
4. Enable human review/rework and explicit delivery steps.
5. Turn on the reconciler and operational alerts before expanding company access.
6. Begin Plan 2 only after version pinning, concurrency fences, recovery, and baseline measures are
   demonstrated in production-shaped tests.

## Acceptance criteria

- One published SOP version can coordinate human and agent steps from a thread through a completed
  run without treating an outbound reply as generic completion.
- Every active step has one durable accountable task owner, and every transfer uses the existing
  ownership fence and private handoff record.
- The server, rather than an agent prompt, authoritatively validates results and advances the run.
- Existing runs continue against their frozen procedure and skill snapshots after edits.
- Rework, task retries, approval/outreach waits, and external delivery remain distinguishable in
  state, history, and metrics.
- Competing claimants and stale writers cannot create duplicate progress or effects.
- Plan 2 can add job breakdowns, checks, coordination rituals, exception policies, measures, and
  improvement cycles through the named versioned extension points and shared event ledger.

## Out of scope

- Arbitrary scripts, user-authored expressions, loops without explicit rework commands, BPMN
  execution/import, and an unrestricted visual programming language.
- Automatic generation or publication of SOP definitions by an agent.
- Cross-company procedure runs, anonymous role owners, or access inherited solely through an SOP.
- A second task queue, approval engine, notification center, or artifact/blob store.
- Performance recommendations and automatic SOP evolution, which belong to Plan 2.
