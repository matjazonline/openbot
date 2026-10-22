# P07-D2 — Agent workflow tools (B2 architecture gate), revision 1

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Author A, 2026-10-22. Builds on B1 as accepted in phase (digest `d82fa028…5c11`, D1 rev 4).
Requirements: plan 07 §"Child calls and workflow tools" (last paragraph) and §Acceptance ("Agent
workflow tools resume the original checkpoint after a child human decision"); TASK criterion 3.
Product decision **P07-Q1** (user, 2026-10-22): a **named tool registration** pins a workflow
version and is granted to agents by name like HTTP tools; publication freezes the pin; upgrading
is an explicit admin edit picked up on republish. User requirement: **a test that the workflows
listed in an agent's tools are from the agent's company.**

## D2.0 Scope

In B2: the registration (schema, application port, persistence, authority), publication freeze
of granted workflow tools (snapshot, child bundle capture, contracts, recursion, resources,
params), the runtime target (dispatch, child creation keyed by the saved model call, agent park,
continuation, typed result or error), inspection of tool-call children, and tests.
Not in B2 (notice **WF-C003 → p09**): HTTP/UI entry points for registrations and for agent tool
grants (today no route sets `agents.workflow_tools`), operations display of tool-call children,
and sample execution through workflow tools (p09's "zero production effects, including through
direct agent tools and children").

## D2.1 Registration

Table `workflow_tool_registrations` (init schema, edited in place):

| Column | Rule |
| --- | --- |
| `company_id uuid` | FK `companies(id)` ON DELETE CASCADE |
| `name text` | the tool name agents grant; the type-name shape `valid_workflow_type_names(ARRAY[name])` checks; NOT `message.%` (reserved built-ins) |
| `workflow_id uuid`, `version_id uuid` | composite FK `(company_id, workflow_id, version_id)` → `workflow_versions(company_id, workflow_id, id)`: **a pin can only name a version of the same company** (user requirement, enforced in the schema) |
| `params jsonb` | static child parameters; JSON object, ≤ 64 KiB |
| `revision bigint` | ≥ 1, optimistic concurrency for edits |
| `created_at`, `updated_at` | timestamps |

PK `(company_id, name)`. At most 256 registrations per company (checked under the company lock,
like HTTP operations).

Application port `WorkflowTools` (new `application/workflow/tools.rs`), implemented by
`PostgresPersistence` (new `adapters/persistence/workflow/workflow_tools.rs`):
- `register_workflow_tool(company, actor, WorkflowToolWrite { name, workflow, version, params,
  expected: Option<revision> }) -> AppResult<WorkflowToolRegistration>`: upsert by name;
  `None` creates (conflict if it exists), `Some(r)` replaces revision `r` (conflict otherwise).
- `revoke_workflow_tool(company, actor, name, revision) -> AppResult<()>`.
- `workflow_tools(company, actor) -> AppResult<Vec<WorkflowToolRegistration>>`.

All three run `authority::authorize_company` (the HTTP connections' authority) first. The
registration checks, in one transaction: the version is a **selectable** version of this
company (unarchived definition; a foreign or unknown version is the same non-disclosing
`NotFound`); `params` validates against the pinned bundle's parameter schema (`BadRequest`
otherwise). The registration does not check the child's resource slots: they depend on the
publishing parent (D2.2).

## D2.2 Publication freeze

**Snapshot.** `ToolSnapshot` gains `workflow: Option<WorkflowToolPin>`,
`WorkflowToolPin { workflow_id, version_id, params }`, serialized only when present (HTTP, MCP
and message snapshots serialize exactly as today, so their stored-JSON equality checks are
unchanged). For a workflow tool, `contract = { name, connection: None, input_schema: child
input schema, output_schema: child output schema }` and `policy = { capability:
"workflow.tool", policy_revision: registration revision, effect: Write, recovery: SafeRepeat }`.
`effect: Write` is conservative classification (the child may have effects); `SafeRepeat`
states that a repeated call is answered by the same child (D2.3). Workflow tools never require
approval in B2 (no approval flag; the child's own steps govern its effects and decisions).

**Capture.** `AgentPublicationDirectory::granted_tool` resolution order for a granted name:
1. an already captured same-name snapshot (existing rules);
2. `message.*` built-ins (existing);
3. **the company's workflow-tool registration of that name** — if the company also has a ready
   HTTP registration of that name, refuse ("registered both as a workflow and as an HTTP
   operation"); otherwise build the snapshot from the registration and the pinned version's
   stored bundle (read `FOR SHARE` at publish, plain at capture);
4. the HTTP registration (existing).
`ChildPublicationDirectory` must wrap `AgentPublicationDirectory` (it runs the inner directory
first) and now collects pins from step controls **and** from captured workflow-tool snapshots;
the 256-pin cap and the summed byte bound apply to the union; bundles are read in the same
batched queries.

**Contracts and graph** (`publication::dependencies` / validation, pure):
- A child bundle is used if a step control or a workflow-tool snapshot pins it
  (`publication.unused` otherwise).
- Each workflow-tool snapshot must equal what its pinned bundle states (`input_schema`,
  `output_schema` of the child) and its `params` must validate against the child's parameter
  schema (`publication.workflow_tool`).
- The child's resource slots must be declared by the publishing parent with the same kind and
  contract (the B1 `registry.child_resources` rule, here `publication.workflow_tool`), so the
  child receives the parent's resources by slot name only.
- The dependency graph gains edges parent → tool child for every workflow-tool snapshot an
  agent step of the parent can select; recursion (`dependency.cycle`) and the depth bound apply
  exactly as for `workflow.call`.

**Publish-time and admission checks.** `authority::supported_publication` treats a snapshot with
`workflow: Some(pin)` as a workflow tool, not HTTP: `workflow_tools::registered_on` requires the
company's registration of that name to pin the same workflow, version and params now, and the
version to still be selectable (`FOR SHARE`). It runs wherever `supported_publication` runs
(publish, admission and admission replay), so revoking or re-pinning a registration stops new
runs of workflows frozen with the old pin until they are republished, like an HTTP revocation.
`verify_children` hash-checks the captured tool children like step children.

## D2.3 Runtime

**Target.** `AgentToolTarget::Workflow { pin: WorkflowToolPin }`; `agent_preparation::targets`
maps workflow-tool snapshots to it, and `current_on` re-checks the registration as in
`registered_on` (a revoked or re-pinned tool fails preparation with the existing refusal).
`AgentActionTools` gains a `WorkflowToolDispatch` port and routes the target to it.

**Child identity.** The saved model call is the identity: `ModelToolCallId` is the harness's
saved `InvocationId` UUID, persisted in the checkpoint before dispatch (`PrepareInvocation`).
`ChildCause::Action(ActionRef)` (never admissible, unused at runtime) is **replaced** by
`ChildCause::ToolCall(ChildToolCallRef { execution: ExecutionRef, call: ToolCallId })` with
`ToolCallId(Uuid)`. The child source key stays 6 elements: `[child, run, exec, step,
call|null, ordinal|null]`, exactly one of the last two non-null. Workflow tool calls do **not**
go through `ActionService` (no intent, dispatch or receipt): the child run and its parent wait
are the durable record of the call.

**Dispatch** (`call_workflow_tool(fence, WorkflowToolCall { pin, name, call, arguments })`, one
transaction fenced to the claimed agent job, `Box::pin`ned at the seam):
1. Lock the run, execution and job under the fence (existing fence predicates).
2. If a `child_run` wait of this execution names this call:
   - `completed` → return `Completed(result)` where `result` is recomputed from immutable facts
     (D2.4): this is the replay after the continuation;
   - `waiting` → `Parked(wait)` (re-entry before confirmation).
3. Otherwise validate `arguments` against the pinned child's input schema. A violation returns
   `Completed({"error":{"code":"workflow.invalid_input","message":…}})` and creates nothing, so
   the model can correct the call (deterministic for the same arguments).
4. If the job's `continuation_count ≥ 63`, return
   `Completed({"error":{"code":"workflow.tool_limit",…}})` and create nothing: a parked call
   must be able to continue.
5. Child configuration: bundle = the parent bundle's frozen child for `pin.version_id`, params =
   `pin.params`, resources = the parent run's resources restricted to the child's slots (B1
   `child_binding`, generalized to `(version_id, params)`); a refusal here is a defect (`Err`).
6. `children::create_on` with `ChildStart { cause: Call(call) | Round(ordinal), … }` writes the
   child run (inherited association, actor, correlation, deadline), its first job, admission
   identity and the parent's `child_run` wait with `child_tool_call_id = call` and
   `parked_attempt_number = fence.attempt`, in the same transaction.
7. Close the fenced attempt with `stop_reason = 'child_parked'`, set the job `completed`, the run
   `waiting` / `child_run`, audit `wait_parked`; return `Parked(wait)`.
The supervisor confirms through a new `confirm_suspension` arm: the fenced attempt closed with
`child_parked` and the wait is this execution's tool-call `child_run` wait for that attempt.

## D2.4 Continuation and result

The child's terminal transition writes its `parent_wakeup` fact as in B1; polling offers the
parent's `child_run` wait. `child_waits::resume_on` branches on `child_tool_call_id`:
- Compute the tool result from the child's terminal fact:
  - `succeeded` → the child's terminal committed output. It must validate against the pinned
    child output schema; a violation is a defect: the wait fails `invalid_output` and the run
    fails, never routed (B1 F016 behavior). An output larger than 128 KiB serialized (the
    action receipt bound) becomes `{"error":{"code":"workflow.tool_result_too_large",…}}`.
  - `failed` → `{"error":{"code":"workflow.child_failed","message":"child run <id> failed"}}`;
    `cancelled` → `{"error":{"code":"workflow.child_cancelled",…}}`. Never a fabricated success.
- Complete the wait, set the run `running`, reopen the agent job (`reopen_job`:
  `continuation_count + 1`), audit `wait_completed`, return `WaitProgress::Continued`.
- Nothing new is stored: the reopened agent replays its saved call (`pending_call`), the
  dispatcher reaches step 2, recomputes the same result from immutable facts and returns
  `Completed(result)`, which the harness saves as the `InvocationResult`; later replays read the
  checkpoint. The model sees the child's output, or the typed error, as the tool result.

Replays and competition: the wait update is conditional on `state='waiting'` under the parent
locks, so competing resumers continue once (the loser replays). Cancelling the parent cancels
the child through the B1 cascade over waiting child waits and settles the wait without a
continuation. The child inherits the parent's deadline; restart needs nothing (all state is in
rows).

## D2.5 Schema changes (init schema, edited in place)

- `workflow_tool_registrations` (D2.1).
- `workflow_waits`: `child_tool_call_id uuid`, `parked_attempt_number integer`. CHECK
  `workflow_wait_child`: `reason = 'child_run'` ⇔ exactly one of `child_ordinal` (1–1000) and
  `child_tool_call_id`; `parked_attempt_number` non-null ⇔ `child_tool_call_id` non-null.
  Unique `(company_id, run_id, execution_id, child_tool_call_id)` where non-null.
- `check_workflow_parent_source`: accept `[child, run, exec, step, uuid, null]` or `[…, null,
  ordinal]`, never both or neither. The deferred `check_workflow_child_parent_wait` (B1) is
  **strengthened**: it also requires the parent wait's `(child_tool_call_id, child_ordinal)` to
  equal the child admission's source-key slots 4 and 5 (B1 compared run and execution only).
- `task_attempts_stop_reason_check`: add `child_parked`.
- `check_workflow_fenced_completion`: **park arm** — `stop_reason = 'child_parked'`, execution
  open, run `waiting`/`child_run`, `OLD.continuation_count < 63`, and a `waiting` tool-call
  `child_run` wait of this execution with `parked_attempt_number = OLD.attempt_ordinal`.
  **Reopen arm** — attempt `child_parked`, execution open, run `running`, a `completed` tool-call
  `child_run` wait of this execution with `parked_attempt_number = OLD.attempt_ordinal`, and its
  child terminal (`terminal_wakeup_sequence IS NOT NULL`).
- New deferred constraint trigger (like `workflow_model_tool_wait_continuation`): a tool-call
  `child_run` wait that completes only continues its agent job (execution open and the job
  `pending` at commit).
- `waits::row` excludes completed tool-call `child_run` waits (as for model-tool and approval
  waits).

## D2.6 Invariants

The child and its parent wait are written atomically, in the transaction that parks the fenced
agent attempt. There is one child per saved call (replays and competing dispatch find it). The
child's identity is the saved call, mapped before the child starts. The parent holds no worker
while it waits. The agent resumes the exact pending call, and the tool result is the child's
output or a typed error, never a fabricated success. A tool can only pin a version of the
registering company, frozen at publication; revocation stops new runs and new preparations.
The child receives only the call arguments as input, the registration's static params, and the
parent's resources for its declared slots. Budgets, deadlines and cancellation are inherited.
Recursion through tools is rejected at publication. Continuations stay within 63 per execution.

## D2.7 Verification plan (focused, DB-backed on the phase cluster)

- Registration: create/replace/revoke with revision conflicts; authority; 256 cap; archived or
  unknown version → `NotFound`; params refused against the child's schema. **Company (user
  requirement):** pinning another company's version through the service → the same `NotFound`
  as an unknown version and nothing written; a raw insert → 23503. **An agent of company A
  granted a tool name registered only in company B cannot publish**, while A's own
  registration freezes a child bundle of company A.
- Publication: snapshot contract equals the child's schemas; frozen params; resources by slot;
  name conflict with HTTP refused; `publication.unused` counts tool pins; recursion through a
  tool rejected; template-free end-to-end publish of a parent whose agent grants a workflow tool.
- Runtime, scripted model (no provider I/O):
  - the agent calls the tool → child created with exactly the arguments, frozen params, its
    slots' resources and the parent's deadline; the attempt is `child_parked`, the job is
    `completed` and the run `waiting`/`child_run`;
  - **the child performs a `decision.human`; after the decision the agent resumes the exact
    pending call (same invocation) with the child's output and finishes** (plan acceptance);
  - duplicate dispatch and replay → one child;
  - competing resumers → one continuation;
  - child failed/cancelled → typed error result to the model;
  - invalid arguments → typed error and no child;
  - continuation limit → typed error and no child;
  - invalid child output → wait `invalid_output`, run failed;
  - oversized output → typed error;
  - parent cancel cascades to the child;
  - restart mid-wait;
  - revoked registration → preparation and admission refused.
- SQL guards: raw park/reopen without the tool-call wait refused (23514); a source key with both
  or neither of call/ordinal refused; the call/ordinal mismatch with the wait refused at commit.
- Refusals: `TriggerSource::Child` with a `ToolCall` cause is refused at admission like every
  child cause.
- Stack budget for the new seams (dispatch, `resume_on` branch, continuation).
- Focused filters: the B1 set plus the new modules; final phase gate per TASK.

## D2.8 Coordination (proposed; activated after B passes this revision)

- **WF-C003 → p09:** admin UI/API for `WorkflowTools` registrations and for agent tool grants
  (`agents.workflow_tools` has no route); run inspection shows tool-call children (parent call
  id) like step children; sample execution must cover workflow-tool children.
- **WF-C001 r2 → p08:** every production `DefinitionService` composition nests
  `ChildPublicationDirectory` **outside** `AgentPublicationDirectory`, and every production
  `AgentActionTools` composition includes the workflow tool runtime.
