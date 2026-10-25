# Workflow reconciliation safety

The application never treats an operator note as permission to repeat an uncertain external write.
Production reconciliation has no installed evidence verifier. It records an annotation; an operator
must inspect the external service and use the documented cancellation/fresh-run procedure when safe.
A trusted verifier can be installed at the application composition boundary; the integration test
uses that supported service contract without making a verifier available through HTTP input.

## Historical episode boundary

Reassessed against phases01–09 at `5d4aab1`. The phase10 [P10-D1 decision](../plan/workflow-foundation/phases/p10/docs/logs/p10-d1-r1.md)
contains exact source spans and the owner inventory; independent review WF-P10-R001 accepted its
bounded obstruction argument. This is not a universal SQL reachability proof.

Every scheduled reconciliation command creates its claim episode atomically through
`workflow_action_bind_schedule`. Reusing a committed command's episode identity encounters an
immediate primary/unique key before the deferred historical-transaction guard. A fresh command
through the supported owner creates fresh witnesses and its binding together. None of the current
owners removes the append-only binding while preserving its authentic scheduled command.
Consequently the exact historical-witness diagnostic through an otherwise-valid public episode
INSERT remains **unexecuted**, as explicitly permitted by phase10's independently reviewed
obstruction disposition. Layered production-function coverage does not establish that diagnostic.

Retained SQL coverage separates three facts:

- The unchanged production guard accepts genuine current witnesses and rejects a historical pair
  on a reference relation; catalog checks verify attachment to the public table.
- Public wrong-scope/ordinal tests and genuine scheduling exercise the real public boundary.
- The duplicate-candidate test reaches the actual public run-update trigger after explicitly
  adversarial committed topology preparation. Its scheduling histories are owner-created; the
  extra same-run execution is not normal application history. The attack rolls back, and disposal
  removes preparation.

Admission creates one execution in a new run. The single successor writer completes the predecessor
in the same locked transaction. Decisions, waits, I/O completion and recovery share that writer.
Child/workflow-tool/repeat calls create separate runs. Thus no supported owner now creates two live
executions in the same run. The adversarial duplicate test remains necessary defense in depth;
new legitimate parallel owners would require an owner-created fixture and competing-worker tests.

## Application integration regression

`workflow_cutover_http_historical_evidence_never_authorizes_an_extra_effect` uses a dedicated
PostgreSQL database, real HTTP workflow publication/binding/admission and an isolated scripted
HTTP server. Production HTTP dispatch and reconciliation resource authorization are used unchanged.
It observes a failed request, captures final rejection, schedules once, reconstructs service/runtime
objects, replays the command, races claimants and dispatchers, then loses a response after one effect.
Old-command replay, stale revision and a historical verifier observation cannot create another
attempt, reservation or effect. Fresh applied evidence recovers a receipt and completes the run
without another HTTP request. Assertions cover calls, effects, attempts, evidence consumption,
receipts, claim episodes and durable run/execution/job state.

Reconstructing owners simulates loss of process-local state; this test does not claim a killed OS
process or an otherwise-valid public historical episode INSERT. Other crash-boundary and SQL
attack tests remain separate evidence. Current commands/results are in phase10 TASK/REVIEW.
