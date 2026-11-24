# Gate AWC-P05-D2 r2 — delta on r1

r1 is `logs/AWC-P05-D2-r1-decision.md` (sha256 `de0e8612…78ca`). r2 answers the review AWC-P05-D2-R1-N1 (F1–F7).
Sections not named here stand as in r1.

## D1 — phase, replaced (F1, F2, F3)

`phase` is a pure function, unit-tested over every combination, of these inputs:

- the call's result row (present or absent);
- the caller's call-wait state (`waiting`, `completed`, `expired`, `failed`, `cancelled`);
- the child run's state and waiting reason, and whether its open decision is the final-reply approval;
- the caller and child jobs' health (`stalled` predicate, below);
- whether the caller's execution has completed, and the caller run's state.

The rules apply in order:

1. Result row → `Settled { closure, code }`.
2. Wait `waiting`, decided by the child:
   - queued or running → `ChildRunning { stalled: child }`;
   - waiting on a decision that is a final-reply approval → `AwaitingFinalReview { decision }`;
   - waiting for anything else, including a `decision.human` step → `ChildWaiting { reason }`;
   - succeeded → `ResponseReady { caller_stalled: false }`;
   - failed or cancelled → `ChildEnded { state }`.
3. Wait `completed` and the caller's execution not completed and the caller run not terminal →
   `AwaitingCaller { stalled: caller }`. This is the window between the caller's resume and its consumption.
4. Otherwise → `ClosedWithoutResult { wait, caller }`, which names the wait state and the caller run state. It
   covers:
   - a consumption that answered `channel_call.revoked` (by design it stores no result);
   - a caller cancelled after the child succeeded;
   - an expired or failed wait.
   The view text is "Closed without a stored result (wait {wait}, caller {caller})". It never claims a cause it
   cannot read.

**Definitions.**

- A decision is a final-reply approval when `workflow_decisions.reply_proposal_id IS NOT NULL` and it is the open
  decision of the child's wait. Anything else is a business decision (F2).
- `stalled(job)`: the job is `processing` with `lock_expires_at <= now`, or `pending` with `run_at` older than the
  existing queue-age threshold. The same predicate is applied to the child's job in rule 2 and the caller's job in
  rule 3 (F3).

**Responder shown only where authorized (F5).** The call item's responder binding and revision render only when
the viewer may read the desk's response settings. That is the authority the B1 desk card relies on,
`channel_reply_settings(company, desk, actor)`, read once per distinct desk on the page (at most 50). Otherwise the
item shows the desk (gated as in r1) and no responder.

## D3 — samples, amended (F4, F5, F6, F7)

**Capture authority (F5).** A declared target is captured only when the actor may read that desk's response
settings, through the same `channel_reply_settings` authority. Otherwise capture is refused with `NotFound`. The
author then may see the responder and review setting the simulation depends on.

**Report (F5).** The report shows, per simulated call, what a live call tells the agent:

- the tool observation (the typed error, denied closure or fixture result);
- plus the label.

The frozen responder binding is used for identity and is not printed.

**Result fields (F6).** Capture also freezes the responder version's `output_schema` `fields` sub-schema (01 C.5). A
fixture's `fields` must validate against it. Without a declared sub-schema, `fields` must be absent, as live.

**Boundary allowlist (F4).** `scripts/workflow-sample-boundary.py` gains these named symbols only:

- `channel_call::{ChannelCallRequest, check_intake, declared_fields, channel_call_schemas, TargetDecision,
  DeniedCode, ChannelCallError}`;
- the code constants the sample answers with.

`ChannelCallDispatch`, `ChannelCallToolCall` and the snapshot builders stay out. If a symbol also needs a domain
type, that is named too, never the module.

**Docs (F7).** Samples may target members-only desks the author may manage, although the agent's list shows only
public desks.

## D5 — signals, amended (F1, F2, F3, F7)

The `calls` CTE now scans calls whose caller execution is unfinished (`workflow_executions.completed_at IS NULL`)
or whose call wait is open. Both are bounded by open work. Gauges:

| Gauge | Meaning |
| --- | --- |
| `workflow_channel_calls_open` | Call wait open. |
| `workflow_channel_calls_review_waiting` | Child waiting on a final-reply approval decision (F2). |
| `workflow_channel_calls_response_ready` | Child succeeded and no result is stored, with the wait open or completed (F1). |
| `workflow_channel_calls_child_stalled` | Rule 2 with the child stalled. |
| `workflow_channel_calls_caller_stalled` | Rule 3 with the caller stalled (F3). |
| `workflow_channel_calls_past_deadline` | Call wait open past its deadline. |
| `workflow_channel_call_oldest_seconds` | Age of the oldest call in rule 2 or 3. |

The counter is unchanged. Run events are unchanged (F7 agrees).

## Verification plan, amended rows

| ID | Test | Assertion |
| --- | --- | --- |
| V1 | `channel_call_views_phase` | Every combination of result row × wait state × child state and decision kind × caller execution and run state × stalled, mapped by rules 1–4, including revoked (completed wait, finished execution, no row) and caller cancelled after child success. |
| V2 | `channel_call_views_caller_and_child` | Adds: a call wait has its `wait_parked` / `wait_completed` audit pair (F7). The responder renders only for a viewer who may read the desk's response settings (F5). |
| V3 | `channel_call_views_review_vs_stalled` | Adds a child parked on a `decision.human` step: `ChildWaiting`, not `AwaitingFinalReview`. Adds a caller resumed but with an expired lease: `AwaitingCaller{stalled}`. |
| V7 | `channel_call_sample_incomplete` | Adds fixture `fields` outside the frozen result sub-schema, and `fields` on a target without one: both refused. |
| V8 | `channel_call_sample_capture` | Capture by an actor who may view but not manage the desk → `NotFound`. The report prints no responder binding. |
| V9 | `channel_call_views_metrics` | Adds `decision.human` (not counted as review), `caller_stalled`, and `response_ready` with a completed wait. |
| V11 | `workflow-sample-boundary` | `python3 scripts/workflow-sample-boundary.py` passes, and fails if sample code names `ChannelCallDispatch` (checked by a negative fixture run in the test, or recorded manually if the script has no fixture mode). |
