These rules govern orchestration in `src/application/` in addition to `src/AGENTS.md`. Application
code coordinates domain decisions through ports; it does not become a transport or persistence
adapter.

# Define ports where they are consumed

Persistence, mail delivery, storage, clocks, and provider calls used by application services are
traits owned by the application (or by the domain when genuinely domain-level). Their concrete
SQLx, lettre, Axum, cloud, and network implementations live under `src/adapters/` or `src/infra/`.
Do not import an adapter merely to reach the trait it defines; relocate the port and have the
adapter implement it.

Keep ports cohesive. Split a broad trait instead of adding optional methods with safe-looking
defaults. Lease renewal, authorization, durable writes, and state transitions have no default
implementation: every production adapter and mock must implement them explicitly.

# Durable effects form one logical commit

If a response requires an outbox row, a stored thread message, and a task payload transition, make
those changes agree in one transaction exposed through a purpose-specific port method. A crash
must not leave an email delivered without the corresponding durable state or cause a retry to pay
for a second provider call whose output can no longer be stored.

Every retryable workflow uses a stable idempotency key derived from the logical operation, not the
attempt. Document unavoidable at-least-once delivery and preserve the stable outbound Message-ID.
Fire-and-forget is not acceptable for bounces, stop notices, confirmation messages, or any other
notification whose loss changes user-visible behavior; enqueue it durably.

# Losing ownership cancels the real work

Wrap the work itself in cancellation. Do not spawn an inner agent future and then lease only the
outer `JoinHandle`: dropping that handle leaves the agent running. On lease loss or task
suspension, cancel and await the inner operation before another worker may begin, and make every
side-effecting step re-check current ownership immediately before committing.

Lease expiry and abnormal worker death consume an attempt. Cap retries, apply backoff, and move a
poison task to an observable terminal state. Completion after lease expiry must not leave a row in
an endlessly reclaimable state.

# Bound execution and distribute capacity

Agent/provider execution has a wall-clock deadline in addition to lease heartbeats. Bound tool
rounds, prompt history by tokens rather than message count, and fetched history to the columns the
prompt uses. Load shared history once per logical thread rather than inside a per-channel loop.

Use a bounded worker pool with explicit global and per-tenant concurrency or fairness. A stuck run
must not block all task processing, and one tenant's burst must not starve every other tenant.
Token counters are not budgets by themselves: enforce configured spend/rate limits before invoking
the provider.

# Keep failure states distinct from replies

An execution error is task state, not customer-authored response text. Do not save or email raw
provider errors as an agent reply, and do not detect retry state by matching a message prefix.
Classify errors explicitly as retryable, terminal, suspended, or successfully replied; only the
last category enters the outbound response path.

Any parked state such as approval, quorum, or scheduled materialization needs a deadline, a
sweeper, and a terminal transition. Pending queries must claim with a lease/backoff or exclude
failed rows so a fixed set of poison records cannot monopolize a batch and hot-spin the poll loop.

A full claim batch justifies the worker's `MoreWaiting`/zero-delay decision only when persistence
has transitioned those rows out of the claimable set. Failed or unchanged rows are not evidence of
additional backlog. For every retryable worker queue, add a poison-batch regression test: fill one
batch with failures, run two consecutive iterations without advancing time, and prove the second
iteration does not reclaim the same rows.

# Treat prompt input as untrusted data

Delimit inbound content from system instructions and label it as untrusted. Tool approval remains
the security boundary: select an approver by an explicit deterministic policy, propagate directory
lookup errors, and fail closed on malformed approval configuration. A phrase blocklist or optional
LLM guardrail may be defense in depth, never the authorization decision.
