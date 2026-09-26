# 01.4 run states and waiting reasons

Original phase01 item4/README govern. Expanded by `/root/astra_01_4`, session `01a0de0a-e0ba-7f63-9887-3e14227e9eb2`. Preserve verified01.1–01.3 and all uncommitted changes. No blocking product ambiguity.

## Contracts and transition semantics

Add pure RunState: Queued, Running, Waiting(WaitingReason), Succeeded, Failed, Cancelled. WaitingReason: Decision, Event, Timer, ChildRun, Effect, Reconciliation. Categories only; complete associated records/recovery later. Mandatory reason in checked DurableWaitRequest constructor/accessor, preserve stable WaitId and future absolute deadline. Add authoritative RunState to application RunHead; replace test TestRun.cancelled boolean.

Purpose-specific transition methods or typed transition enum, never arbitrary set_status. Queued start -> Running; Running park -> Waiting(request reason); Waiting resume -> Running. Apply existing resolved EngineDisposition only in Running: completed to step -> Running; completed End -> Succeeded; eligible retry -> Running; final failure routed to step -> Running; final failure routed End -> Succeeded (handled termination); unrouted final failure -> Failed. Engine terminal failure any nonterminal -> Failed; cancel nonterminal -> Cancelled. Cancel terminal returns unchanged/already terminal, preserving Succeeded/Failed/Cancelled. Other terminal transitions reject. Start Running/resume nonwaiting/apply outcome nonrunning reject without mutation.

Running is active logical run including queued successors/retries, not evidence of worker lease. Queued is initial admission. Succeeded means workflow executed successfully, not business approval: rejection retains output and can succeed. Handled final-error End retains failed execution but succeeds run. Reuse EngineDisposition/TransitionTarget; do not duplicate route/retry selection.

Wait parking must not bypass deadline check with a stale previously checked request; standalone park takes explicit now and rechecks. Pure decisions no system clock.

## Public documentation obligations

Pure state decisions do not persist/schedule/authorize/prove ownership. Durable resumption must validate scoped matching wait identity, deadline policy and consumed wake condition atomically. Outcome application needs current revision/live fence; future transaction combines execution result, run progress, audit, wait+notification or successor. Wait deadlines mandatory; sweepers/run caps/deadline policy later. Terminal states immutable; cancellation cannot erase committed outputs/receipts or undo effects.

## Application integration

Atomic admission creates Queued, replay never resets state. Claim excludes Waiting/terminal; first claim Queued->Running atomically with revision change, subsequent eligible Running claim need not change state revision. Cancellation checks expected revision/current authoritative state, invalidates work/ownership on nonterminal cancellation, preserves terminal states. Inspection returns authoritative state. Service retains CAS call after scoped lookup (no stale-head early return). No completion/resumption persistence methods or placeholder payloads now; full transactions phase03.

## Files/callers/ordered edits

Add domain/workflow/state.rs/export; update outcome.rs:48–71 and wait fixtures :344–370/:425–448; application/workflow/contracts.rs:158–164 RunHead; ports.rs:22–55 docs; tests.rs admission :166–213, inspection :218–232, cancel :237–264, claim :269–297; tests/cases.rs:226–301 fixtures. Graft callers missing edges; exhaustive grep found these constructors/exports/mocks, no production implementations. Reuse existing outcomes/routes/cancel results and race fixtures, not harness RunState.

Implement domain types/rules/tests; checked reason-bearing waits; head+port docs; in-memory integration/races; source audit/checks/graft refresh. Root/src/application guides apply. Parent owns progress/briefs. No live entry point changes, SQL/storage serde, runtime/auth/inventory/causality/shims/dependencies/commit/deploy/reset.

## Acceptance

All states/reasons exercised; each reason survives checked request->park->waiting. Matrix invalid origins/terminal immutability; business rejection output retained and Succeeded; retry/handled errors vs unrouted failure; expired/equal deadlines cannot park via alternate API. Admission inspected Queued, claim Running; replay preserves Running/terminal. Cancel Queued/Running/Waiting, preserve all terminals; claims exclude Waiting/terminals. Existing competing races plus actual barrier/join claim-vs-cancel test if atomic interaction changes: cancelled state has no pending ownership, no duplicate claim, stale revisions conflict. Handler still cannot pick arbitrary target/mutate prior outputs/schedule.

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=true cargo test --locked --offline --lib domain::workflow
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib domain::workflow
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib application::workflow
SQLX_OFFLINE=true cargo check --locked --offline --all-targets
```

No services/DB/SQLx/temp cleanup required. Full persistent concurrency and recovery are phase03.

## Review 1 corrections

1. outcome.rs:46/:183 still defer waiting reason classification; now implemented. Defer only associated durable records, run caps, recovery/deadline policies.
2. tests/state_cases.rs:166–202 barrier race does not force both orderings; join can cancel first, so no ownership does not prove revoking established claim. Keep actual competing test AND controlled coverage: claim commits then cancellation clears ownership; cancel commits then claim returns empty; revision observed before first claim conflicts afterward. Assert claim count/revisions/final no ownership for each. Behavior otherwise passes, reviewer37/37 workflow tests at2MiB+diff. Rerun appropriate checks on corrected tests.
