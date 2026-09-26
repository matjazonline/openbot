# 01.3 typed step outcomes

Original phase01 Implementation item3/README govern. Astra `/root/astra_01_3`, session `01a0ddff-124d-78e2-9c02-4a7c952a0b50`, expanded only. Preserve verified01.1/01.2. No backward compatibility, live entry-point rewiring, or later phase work.

## Existing seams

domain/workflow/transition.rs:11–15 RouteSelection includes engine FinalError; :29–62 select_route is authoritative target lookup. definition.rs:43–49 final_error requires exhausted retries. graph.rs:47–63 validated context limits. context.rs:212–255 contains bounded JSON validator (check_value callers evaluate/resolve). ids.rs:26–89 checked newtype macros. Exhaustive graft grep finds graph tests and module export for select_route even though callers query reported none. Trace changed symbols before edits.

## Contracts and pure behavior

- New StepOutcome has Completed(CompletedStep), Waiting(DurableWaitRequest), Failed(StepFailure).
- CompletedStep owns JSON output and restricted completion selection Success or Choice(ChoiceName), never unrestricted RouteSelection::FinalError. Business rejection is completed output/declared choice, not infrastructure failure.
- DurableWaitRequest owns typed stable WaitId and mandatory absolute deadline, checked against explicitly supplied current time (equal/past rejected). Request to register a logical wake identity for current execution, not proof of durable commit. No opaque JSON, untyped reason or successor ID. 01.4 adds semantic waiting reasons; later phases own complete event/decision/effect payloads.
- StepFailure has FailureClass Retryable/Terminal, checked bounded FailureCode, optional diagnostic text with enforced byte ceiling. No arbitrary provider/anyhow objects or customer output. Diagnostics internal, not inherently safe to log.
- Pure engine resolver takes validated workflow/current step/outcome, explicit engine-owned RetryEligibility Available/Exhausted, and needed current time. Distinct EngineDisposition retains original result and maps: completed via select_route; waiting to park only; retryable+available to retry only; terminal or exhausted retryable to declared final_error, otherwise terminal failure without route. Missing final_error is ordinary terminal failure. Unknown current step rejects in ALL branches. Unknown choice/wrong route kind propagate typed errors.
- Handler-facing outcome contains no StepId/TransitionTarget/job/scheduling capability; resolved targets exist only in engine disposition.
- Enforce completed output byte/depth/work bounds using definition context limits/platform caps before cloning/serialization. Reuse/factor existing value validator, do not synthesize fake input context. Constructors/private fields or checked resolver enforce invariants.
- Pure functions cannot read system clock. Run deadline cap/retry budget/backoff belong later. Future atomic commit must combine execution result, wait+notification or successor, run progress and audit. Identity is neither authorization nor global unscoped lookup authority.

## Scope and edits

Add outcome.rs + tests; extend ids/mod; reuse select_route in outcome mapping (possibly transition.rs); factor shared validator only if needed preserving context callers. No application port/service changes. No states/wait-reason categories/registries/resumption/action recovery/schema checks/workers/persistence now. If concrete dependency conflict forces opaque placeholders or01.4, stop and report rather than reorder.

Read root/src guides. Keep functions/files within thresholds, box large enum payloads if needed. No DB/reset/commit/deploy/dependency change. Inspect combined existing work and refresh graft after edits.

## Acceptance

Test sequential/both-choice completion preserving typed output; successful business rejection; unknown choice/wrong route kind/unknown step (including wait/failure); waiting preserves identity/deadline with no successor and rejects expired/equal deadlines; retryable+available never takes error route; exhausted retryable and terminal take declared final_error; both without route remain terminal. Exact/over output bytes, depth/work, code/diagnostic limits; mapping preserves context/previous committed output. Inspect no handler-selected targets/scheduling. No concurrency protocol added; prior race tests remain regression coverage.

```sh
cargo fmt --all -- --check
git diff --check
SQLX_OFFLINE=true cargo test --locked --offline --lib domain::workflow
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib domain::workflow
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib application::workflow
SQLX_OFFLINE=true cargo check --locked --offline --all-targets
```

No services/resources/SQLx regeneration/cleanup required. Full durable state/auth gates remain pending.

## Review 1 correction

outcome.rs:43,123–144,158 public docs must explain: wait requests durable registration for current execution, stable ID scoped by caller company/run and not authorization, construction not persistence; retry eligibility engine-owned; Terminal with Some(target) ends failed execution and follows error route, not necessarily run termination. Future transaction combines result/run progress/audit/wait+notification or successor. Reason categories and run-deadline cap remain next work. Behavior otherwise passes; reviewer independently confirmed22/22 at2MiB and whitespace. Documentation-only correction requires fmt/diff checks and rereview, not behavioral reruns.
