# Gate AWC-P05-D2 r1 — Batch B2 (doc 05 Batch B): inspection, samples, signals

- **Batch.** Phase p05, batch B2 = doc 05 Batch B (B.1–B.5).
- **Base.** HEAD `fe7791d3`, with p01–p04 and p05 B1 accepted in phase. B1 was accepted at `22fdcacd…ba13`.
  Coordination is r22.
- **Gated decisions**, each hard to unwind once dependent code exists:
  - D3 changes the public sample request contract and the sample identity, and adds an authorized capture read.
  - D1 adds view read models that render cross-scope links.
  - D5 names public metrics.
- D2 and D4 are listed for completeness.
- The facts come from a survey of the current code; file:line references are given where they decide something.

## D1 — Call views on the caller and child runs (B.1)

**Model.** `RunView` gains two parts, read by the existing `run_view` in the same transaction under the same
visibility:

- `channel_calls: ChannelCallPage { items, truncated }`, the caller side. It holds this run's calls, newest first,
  at most `MAX_STEP_ITEMS` (50), fetched as limit+1.
- `called_by: Option<ChannelCallOrigin>`, the child side. It is present when this run is a call's `child_run_id`.

`StepView` gains the same page, filtered to its execution.

**Each call item** (`ChannelCallItem`):

- call id and caller execution, linked to the step view;
- calling agent id and revision;
- desk: its id, plus its name when the viewer may see the desk (otherwise "a desk");
- responder binding and its revision;
- child run;
- the child's own scope manifest;
- the call's wait deadline;
- `phase`.

**Phase.** `phase` is a pure function, unit-tested over every case, of the call's result row, the child run's state
and waiting reason, and the child job's lease:

| Phase | Source |
| --- | --- |
| `Settled { closure, code }` | A result row exists. `code` is the denied code or the error code. |
| `ChildRunning { stalled }` | The child is queued or running. `stalled` means its job is `processing` with an expired lease, or `pending` past its due time by more than the existing queue-age threshold. |
| `AwaitingFinalReview { decision }` | The child is waiting with reason `decision`. The decision link is drawn only if the viewer may open that decision. |
| `ChildWaiting { reason }` | The child is waiting for anything else. |
| `ResponseReady` | The child succeeded, the caller has not consumed the result, and the caller's wait is still open. |
| `ChildEnded { state }` | The child failed or was cancelled, not yet consumed. |

`AwaitingFinalReview` versus `ResponseReady` is the doc's "child execution complete vs response awaiting approval".
Review happens inside the child before it succeeds. `ChildRunning { stalled: true }` separates a stuck worker from a
waiting review.

**Origin** (`ChannelCallOrigin`, on the child): caller run, caller execution, calling agent, desk, call id, and the
caller's manifest (`agent_manifest_id`).

**Authorization.**

- Nothing renders unless the viewer already passed `visible_run` for the page's run.
- Every link to another run (child, caller, review) uses `run_chip` with `RunLinkAccess::Authorized(set)`. The set
  comes from one batch `run_visibility` read over the referenced run ids, and an unauthorized id renders as a plain
  label.
- A call child is a fresh scope on the desk's association (AWC-C005), so a caller-side viewer often cannot open it.
  That is intended.
- Desk names come from the existing channel visibility read.

**Content.** No arguments, fields, result content, reply text, contract or approval-policy body is read into either
model. Under `payload_restricted`, the page keeps only the ids, phase and codes, the same as the children list.

**Rendering.**

- Detail page: a "Channel calls" section under Children, and a "Called by" line above the lineage line for a call
  child.
- Step page: the same section for the execution.
- Truncation shows "Only the newest 50 calls are listed".

## D2 — Dependency view follows the explicit scope (B.2)

`provenance.rs:35-37` says "Selected at root admission and retained by retries and children". It becomes:

- **Ordinary run:** "Selected at root admission; kept by retries and ordinary children."
- **Run whose `run_dependency_manifests.root_run_id` is itself and that is a call child:** "This run's own
  dependency scope, selected when the call from run {caller} launched. The caller's selection is manifest {m}." The
  caller run renders as a `run_chip`.
  - Its selected agent is labelled as this run's, never as the caller's root selection.

Data: `run_dependency_manifests` (exists) plus `workflow_channel_calls.agent_manifest_id`. Lineage and
`workflow_run_parents` are unchanged, so cancellation and tracing keep the causal tree.

## D3 — `channel.call` in samples (B.3)

Today `sample/tools.rs:121-131` treats `ToolKind::ChannelCall` as an external tool. It needs a fixture output,
freezes no target, and does not simulate review.

**Request.**

- `SampleRequest` gains `call_targets: Vec<Uuid>`: the desks the sample may call, at most 10, deduplicated.
- `ScriptedToolCall` gains `approval: Option<bool>`, which only `channel.call` accepts.
- Both use `serde(deny_unknown_fields)`, as today.

**Capture** (`SampleCapture::capture`, persistence, the same transaction and 5 s bound). For each declared desk:

1. The actor must be able to view the desk through the existing channel visibility. Otherwise capture is refused
   with `NotFound`, revealing nothing.
2. Read the launch facts with the shared readers: writable for the sample's calling agent (check 3: public, or the
   agent principal's `participate` grant), enabled, unit not archived, a selected active message-triggered
   responder. Then apply `decide_target` without ancestry, because a sample has no run tree.
3. Freeze a `FrozenCallTarget`: `{desk, name, decision, responder: {binding, revision}, review_required, intake
   schema (declared fields)}`.
   - `decision` is `Proceed` or the typed error / denied code launch would answer.
   - No params, resources, prompts or contract body are read.

The frozen targets join `ConfigurationIdentity`, whose kind becomes `sample-only-v3`, so a changed desk changes the
sample identity.

**Execution** (pure; the `channel_call` module is added to the PURE allowlist in
`scripts/workflow-sample-boundary.py`). A scripted `channel.call` runs these steps:

1. Parse the arguments with `ChannelCallRequest::parse`, as live does.
2. If `channel_id` is not among the frozen targets, the sample stops with diagnostic `channel_call.sample_target_undeclared`.
3. If the frozen decision is not `Proceed`, the tool observation is that typed error or denied closure, as live.
4. Run the intake check against the frozen intake (D3 of p01), as live.
5. If `review_required` and `approval` is absent, the sample stops with `channel_call.sample_review_undeclared`.
   `approval: false` gives the `channel_call.review_rejected` error. `approval` on a target without review, or on
   any other tool, is refused as inconsistent.
6. Otherwise the fixture `output` is required (missing → the sample stops with
   `channel_call.sample_result_undeclared`). It must validate against the `channel.call` output schema and be a
   success closure (`done`, `denied`, `handed_off`, `no_reply`).

All three stops are the existing visible `Stopped` report with a typed diagnostic, never an empty success. The effect
is recorded as a new `SampleEffectKind::SimulatedCall`, not `InterceptedWrite`.

**Not simulated (stated in the report and the docs).** Child failure or cancellation, review expiry, deadline
expiry, and knowledge-level propagation. Samples cannot authorize production, as today.

**Zero effect.** A sample with `channel.call` creates no row in:

- `workflow_channel_calls`, `workflow_channel_call_results`, `workflow_runs`, `workflow_waits`;
- `threads`, `thread_messages`, `messages`;
- `channel_reply_*`, `workflow_decisions`, `background_tasks`, `agent_history_*`;

and makes no model or network request.

## D4 — Labels and logs (B.4)

- **Sample report:** each `SimulatedCall` effect carries `"untrusted": true` and the label "Simulated call result
  (author fixture)". The response preview keeps its "Preview only" notice.
- **Live views:** they carry no call content (D1), so no knowledge-level label is needed. Redaction stays all or
  nothing through `PayloadVisibility`.
- **Logs:**
  - consumption: `tracing::info!(company, run, execution, call, child, closure|code)`, next to the existing
    launch log;
  - refusal: `tracing::info!(company, run, execution, code)`.
  - Never arguments, fields, reasons, content or config.
- **Retention:** unchanged. Views read only live rows, which go with the tree (B1 docs).

## D5 — Bounded operational signals (B.5)

**Gauges.** `metrics.sql` gains one `calls` CTE, read from durable rows only: open caller waits (`workflow_waits`
with `state='waiting'` and a `child_tool_call_id` that names a `workflow_channel_calls` row) joined to the child run
and its job. It exposes, next to the existing workflow gauges:

| Gauge | Meaning |
| --- | --- |
| `workflow_channel_calls_open` | open call waits |
| `workflow_channel_calls_review_waiting` | child waiting on `decision` |
| `workflow_channel_calls_response_ready` | child succeeded, caller wait still open |
| `workflow_channel_calls_stalled` | child job lease expired, or pending past due |
| `workflow_channel_calls_past_deadline` | call wait deadline passed |
| `workflow_channel_call_oldest_seconds` | age of the oldest open call wait |

The CTE scans only open waits, so its cost is bounded by open work, as the existing `waits` and `decisions` CTEs
are.

**Counter.** `workflow_channel_call_outcomes_total{outcome}`, with bounded labels `refused`, `launched`, `done`,
`denied`, `handed_off`, `no_reply`, `error`. It follows the `workflow_channel_reply_publications_total` precedent.
It is a rate signal only; stuck calls are found by the gauges and the D1 phases.

**Run events.** No new `workflow_run_events` kinds.

- The existing kinds already record the transitions on the right runs:
  - `wait_parked` / `wait_completed` on the caller;
  - `admitted`, `decision_opened`, `run_deadline_expired`, `parent_wakeup` and `step_final_error` on the child.
- A refusal leaves no row by design (p01). It is in the agent history tool result and the log/counter.
- Adding per-call kinds would need the `wait_id` check and the unique `(run, execution_id, event_kind)` index
  changed, because one execution can launch several calls in sequence. That schema change buys nothing the D1
  phases do not already give.

**Docs.** `docs/workflow-operations.md` "Channel calls" gains the phases, the gauges and how to tell a review wait
from a stalled child.

## Verification plan (B2)

| ID | Test | Assertion |
| --- | --- | --- |
| V1 | `channel_call_views_phase` (unit) | Every phase row of D1 from its inputs, including stalled and past deadline. |
| V2 | `channel_call_views_caller_and_child` (DB) | Caller page lists the call with the child link only when the child is visible. The child page shows "Called by". A step view lists it for its execution. Truncation at 50. No argument or content text in the HTML. |
| V3 | `channel_call_views_review_vs_stalled` (DB) | A child parked on final review shows `AwaitingFinalReview` with a decision link for a privileged viewer and a label otherwise. A child whose job lease expired shows `ChildRunning{stalled}`. A succeeded, unconsumed child shows `ResponseReady`. |
| V4 | `channel_call_views_foreign_scope` (DB) | A viewer who cannot see the child or the desk gets no link and no desk name. A viewer who cannot see the caller sees nothing. |
| V5 | `channel_call_views_dependency_scope` (DB) | A call child's provenance names its own manifest and links the caller; an ordinary child keeps the inherited wording. |
| V6 | `channel_call_sample_simulated_result` | Done, denied and no_reply fixtures validate and record `SimulatedCall` with the untrusted label. A frozen non-`Proceed` target answers its typed error. The intake check refuses bad fields. |
| V7 | `channel_call_sample_incomplete` | An undeclared target, missing review, missing result and an inconsistent `approval` each stop with their typed diagnostic. |
| V8 | `channel_call_sample_capture` (DB) | Capture refuses an invisible desk (NotFound). It freezes the decision for disabled, members-ungranted and no-responder desks. The identity changes when the responder changes. Production-table counts are unchanged across capture and execution. |
| V9 | `channel_call_views_metrics` (DB, `own_database`) | Each gauge counts its fixture state exactly. |
| V10 | existing | `python3 scripts/workflow-sample-boundary.py`; run-view, provenance and sample suites; `workflow-runs.mjs` and `workflow-samples.mjs` browser suites. A Rust-generated fixture of the call section is consumed by `workflow-runs.mjs`, following the `*_BROWSER_FIXTURES` dump pattern. |

Batch checks: stack budget on the affected filters, clippy 1.95.0, fmt, sqlx check, source_modules and CSS.

## Invariants

1. Views and samples never show call arguments, fields, results or desk configuration.
2. A link to another run, decision or desk is drawn only when the viewer may open it.
3. A sample with `channel.call` writes no production row and makes no external request. Every missing simulation
   input stops visibly.
4. Stuck calls are found from durable rows (gauges and phases), never from process counters.

## Failure modes

| Failure | Behaviour |
| --- | --- |
| Call view read fails | The run page fails as any `run_view` read failure does. There is no partial page. |
| Capture read fails or times out | Capture fails; no sample runs. |
| Fixture invalid | The sample stops with the existing validation diagnostic. |
| Metrics query slow | Bounded by open waits; it shares the existing metrics timeout. |
