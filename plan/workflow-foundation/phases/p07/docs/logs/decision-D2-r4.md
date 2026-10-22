# P07-D2 — Agent workflow tools (B2 architecture gate), revision 4

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Author A, 2026-10-22. Builds on B1 as accepted in phase (digest `d82fa028…5c11`, D1 rev 4).
Revision 4 answers gate round WF-P07-R009 (revision 3, sha256 `b13f51e9…b335`): **F026** —
dispatch applies the Run-mode rule (name **and** `workflow_id`), so a re-pointed name refuses a
running agent's next call like a revocation; D2.3 step 3 and a D2.7 test.

Revision 3 answered gate round WF-P07-R008 (revision 2, sha256 `46301f5e…3451`):

| Finding | Resolution | Where |
| --- | --- | --- |
| F024 | Publish mode applies only to the root's own freshly captured workflow-tool snapshots; stored child bundles are checked in Run mode; tests for a child whose tool was re-pinned and revoked | D2.2, D2.7 |
| F025 (optional, adopted) | Run mode matches name **and** `workflow_id`: re-pointing a name to another workflow, or reusing a revoked name for another workflow, does not re-enable old pins | D2.2, D2.7 |

Revision 2 answered gate round WF-P07-R007 (revision 1, sha256 `1e11108a…ff27`):

| Finding | Resolution | Where |
| --- | --- | --- |
| F018 | User decision **P07-Q2**: revoke stops, re-pin and archive don't; two check modes (publish vs run) | D2.0, D2.2, D2.3, D2.7 |
| F019 | Competing-claimant and recovery tests for the `child_parked` protocol | D2.7 |
| F020 | Registration recheck at dispatch, taken before the run locks; revoked → typed refusal | D2.3 |
| F021 | One tool-kind classifier used at every site that today reads `connection: None` as HTTP | D2.2a, D2.7 |
| F022 | A same-name captured step tool and a workflow registration are refused as ambiguous | D2.2 |
| F023.1 | Why marker-free tool calls are safe for a marker-owned kind; doc updates | D2.3a |
| F023.2 | The `waiting → Parked` dispatch branch is dropped (a defect under a live fence) | D2.3 |
| F023.3 | No admin approval flag, deliberately | D2.1 |
| F023.4 | WF-C003 names p08 (routes) and p09 (UI, inspection, sample execution) | D2.8 |

Requirements: plan 07 §"Child calls and workflow tools" (last paragraph) and §Acceptance ("Agent
workflow tools resume the original checkpoint after a child human decision"); TASK criterion 3.
Product decision **P07-Q1** (user, 2026-10-22): a **named tool registration** pins a workflow
version and is granted to agents by name like HTTP tools; publication freezes the pin; upgrading
is an explicit admin edit picked up on republish. User requirement: **a test that the workflows
listed in an agent's tools are from the agent's company.** Product decision **P07-Q2** (user,
2026-10-22): **revoke stops, re-pin doesn't** — re-pinning a registration or archiving the
pinned workflow leaves already-published workflows on the version they froze until republished;
revoking (deleting) the registration refuses new runs and new tool calls by running agents.

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

Registrations deliberately have **no admin approval flag** in B2: the pinned workflow's own
steps (decisions, protected actions) govern what it does, and a registration is itself an admin
act. All three run `authority::authorize_company` (the HTTP connections' authority) first. The
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
1. an already captured same-name snapshot (existing rules) — **unless the company also has a
   workflow-tool registration of that name and the captured snapshot is not that workflow tool:
   refuse as ambiguous** (as for a same-name HTTP registration, step 3);
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

**Checks, two modes (P07-Q2).** `workflow_tools::registered_on(tx, company, tool, mode)`:
- `Publish` (`FOR SHARE`): the company's registration of that name pins exactly the snapshot's
  workflow, version and params, and the version is still selectable. **Scope (F024):** only the
  workflow-tool snapshots the root bundle captured in this publication. A re-pin or an archive
  between capture and publish refuses that publication (the caller retries and captures the new
  pin).
- `Run` (`FOR SHARE`): a registration of that name **pinning the same `workflow_id`** exists in
  the company, whatever version and params it pins now (F025). Scope: admission, admission
  replay, binding save/activation readiness, dispatch (D2.3), and, at publish, the workflow-tool
  snapshots inside **stored child bundles**. Stored children were verified at their own
  publication and are not re-derived, consistent with `agent_publication::verify_on`.
`supported_publication` walks the closure and chooses the mode per bundle: `Publish` for the
root's own snapshots during a publish, `Run` otherwise.

The frozen pin is part of the immutable published closure, so a re-pin to another version of
the same workflow, or an archive, does not affect workflows published before it. They move only
on republish, and a new parent that pins such an existing child publishes and runs the child's
frozen pin. Revocation (deleting the row) refuses new runs and new tool calls, and a new parent
pinning a child whose tool was revoked does not publish. Re-registering the name for the
**same** workflow re-enables those runs. Re-pointing the name to a different workflow, or
reusing a revoked name for one, does not: old pins of the previous workflow stay refused. The
name now means a different tool, so for old pins it acts as a revocation.
`verify_children` hash-checks the captured tool children like step children at publish.

## D2.2a One tool-kind classifier (F021)

`ToolSnapshot::kind(&self) -> ToolKind`, with `ToolKind::{Message(MessageKind), Mcp(slot),
Http, Workflow(&WorkflowToolPin)}`, is the only place that classifies a snapshot. Every site that
today infers HTTP from `connection: None` matches it exhaustively, so a site that misses the new
kind fails to compile:
- `authority::supported_publication`;
- `resources::check_readiness` (admission, admission replay, binding save/activation);
- `agent_preparation::targets`;
- `prepared_tool`'s target/snapshot consistency check;
- `message::is_trusted_message_snapshot`.
The `AgentToolDispatch` and `worker.rs` docs name the four kinds.

## D2.3 Runtime

**Target.** `AgentToolTarget::Workflow { pin: WorkflowToolPin }`; `agent_preparation::targets`
maps workflow-tool snapshots to it through `ToolSnapshot::kind`.
`AgentActionTools` gains a `WorkflowToolDispatch` port and routes the target to it. Preparation
(`current_on`) keeps checking the agent's grants by name only; registrations are checked at
dispatch (below), so a revocation refuses the call, not the whole agent step.

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
0. Read the company's registration of `name` `FOR SHARE` **before any run lock** (authority
   before run, the existing lock order; a revocation or a re-point takes the company lock and
   the registration row, never a run).
1. Lock the run, execution and job under the fence (existing fence predicates).
2. If a `completed` `child_run` wait of this execution names this call, return
   `Completed(result)` with `result` recomputed from immutable facts (D2.4): the replay after the
   continuation. It is answered even if the registration was revoked meanwhile: the call began
   before the revocation. A `waiting` wait for this call cannot coexist with a live fence (the
   park closed the job), so it is a defect (`Err`).
3. Apply the Run-mode rule (D2.2) to what step 0 read. If there is no registration of `name`
   whose `workflow_id = pin.workflow_id` (revoked, or re-pointed to another workflow), return
   `Completed({"error":{"code":"workflow.tool_revoked","message":…}})` and create nothing (P07-Q2:
   "running agents: tool refused"; F025/F026: a re-point acts as a revocation for old pins). A
   registration of the same workflow at another version or params passes: the call runs the
   frozen pin.
4. Otherwise validate `arguments` against the pinned child's input schema. A violation returns
   `Completed({"error":{"code":"workflow.invalid_input","message":…}})` and creates nothing, so
   the model can correct the call (deterministic for the same arguments).
5. If the job's `continuation_count ≥ 63`, return
   `Completed({"error":{"code":"workflow.tool_limit",…}})` and create nothing: a parked call
   must be able to continue.
6. Child configuration: bundle = the parent bundle's frozen child for `pin.version_id`, params =
   `pin.params`, resources = the parent run's resources restricted to the child's slots (B1
   `child_binding`, generalized to `(version_id, params)`); a refusal here is a defect (`Err`).
7. `children::create_on` with `ChildStart { cause: Call(call) | Round(ordinal), … }` writes the
   child run (inherited association, actor, correlation, deadline), its first job, admission
   identity and the parent's `child_run` wait with `child_tool_call_id = call` and
   `parked_attempt_number = fence.attempt`, in the same transaction.
8. Close the fenced attempt with `stop_reason = 'child_parked'`, set the job `completed`, the run
   `waiting` / `child_run`, audit `wait_parked`; return `Parked(wait)`.
The supervisor confirms through a new `confirm_suspension` arm: the fenced attempt closed with
`child_parked` and the wait is this execution's tool-call `child_run` wait for that attempt.

### D2.3a Why a marker-free call is safe in a marker-owned step (F023.1)

`agent.run` is a marker-owned kind: its effectful tool calls go through `ActionService`, whose
dispatch marker lets reconciliation decide an uncertain outcome. A workflow tool call needs none
of that. The child run, its parent wait and the parked attempt commit in **one** fenced
transaction, so the call is either fully started or not started at all; it is never uncertain.
A replay finds the call's wait by the saved call identity, and the child's own effects are
governed by the child's own steps and markers. So nothing about the call itself is left to
reconcile. The dispatcher never returns `Uncertain` for this kind. The `worker.rs` and
`AgentToolDispatch` docs say so.

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
continuation. A dispatch competing with a cancel or with lease expiry serializes on the parent
run/job locks: whichever commits first wins, and the other finds a cancelled run or a dead fence
and writes nothing. The child inherits the parent's deadline; restart needs nothing (all state
is in rows). A revocation or re-point does not affect a call already parked: its child finishes and the
continuation delivers the result.

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
registering company, frozen at publication; revocation (or re-pointing the name to another
workflow) stops new runs and new tool calls,
while a re-pin or an archive leaves published workflows on their frozen pin (P07-Q2).
The child receives only the call arguments as input, the registration's static params, and the
parent's resources for its declared slots. Budgets, deadlines and cancellation are inherited.
Recursion through tools is rejected at publication. Continuations stay within 63 per execution.

## D2.7 Verification plan (focused, DB-backed on the phase cluster)

- Revocation and re-pin (P07-Q2):
  - after a re-pin, a workflow published with the old pin still admits runs and its agent still
    calls the old version; a republish captures the new pin; a re-pin between capture and
    publish refuses that publish;
  - archiving the pinned workflow does not stop published workflows; a new publication refuses
    it;
  - revoking refuses new admissions (and binding activation readiness), and a running agent's
    next call gets `workflow.tool_revoked` with no child, while a call parked before the
    revocation still delivers its result;
  - re-registering the name for the same workflow re-enables admissions; re-pointing it (or
    reusing a revoked name) to another workflow keeps old pins refused (F025);
  - after a re-point to another workflow, a running agent frozen on the old pin gets
    `workflow.tool_revoked` on its next call and no child is created, while a call parked before
    the re-point still delivers its result; after a re-pin to another version of the same
    workflow, the next call runs the frozen version (F026);
  - stored children (F024): a new parent that pins an existing child whose own workflow tool
    was re-pinned publishes and its run calls the child's frozen version; if that tool was
    revoked, the new parent does not publish.
- One classifier (F021): binding save/activation, admission and admission replay succeed for a
  workflow whose agent grants a workflow tool; an MCP step tool and a workflow registration of
  the same name refuse publication as ambiguous (F022).
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
  - cancel vs dispatch in both deterministic orders and racing (barrier): a cancelled run gets
    no child, or a created child is cascaded to `cancelled`;
  - lease expiry / `retire_expired_io` vs dispatch, including a lease that expires inside the
    dispatch transaction (the commit is refused by the fenced-completion guard; nothing remains);
  - cancel vs continuation (both orders): either the job is reopened and the run cancelled
    afterwards cleanly, or the continuation finds a settled wait and writes nothing;
  - a reopened attempt that fails before saving its result replays to the same result, with
    still exactly one child;
  - child failed/cancelled → typed error result to the model;
  - invalid arguments → typed error and no child;
  - continuation limit → typed error and no child;
  - invalid child output → wait `invalid_output`, run failed;
  - oversized output → typed error;
  - parent cancel cascades to the child;
  - restart mid-wait;
  - revoked registration → admission refused and the next call refused with a typed error
    (see "Revocation and re-pin" above).
- SQL guards: raw park/reopen without the tool-call wait refused (23514); a source key with both
  or neither of call/ordinal refused; the call/ordinal mismatch with the wait refused at commit.
- Refusals: `TriggerSource::Child` with a `ToolCall` cause is refused at admission like every
  child cause.
- Stack budget for the new seams (dispatch, `resume_on` branch, continuation).
- Focused filters: the B1 set plus the new modules; final phase gate per TASK.

## D2.8 Coordination (proposed; activated after B passes this revision)

- **WF-C003 → p08 and p09:** p08 exposes company-scoped HTTP routes over the `WorkflowTools`
  commands (register with expected revision, revoke, list) and over agent tool grants
  (`agents.workflow_tools` has no route today); p09 builds the admin UI for both, shows
  tool-call children in run inspection (parent call id) like step children, and covers
  workflow-tool children in sample execution.
- **WF-C001 r2 → p08:** every production `DefinitionService` composition nests
  `ChildPublicationDirectory` **outside** `AgentPublicationDirectory`, and every production
  `AgentActionTools` composition includes the workflow tool runtime.
