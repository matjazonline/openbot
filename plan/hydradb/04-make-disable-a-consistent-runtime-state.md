# Make Provider Disable a Consistent Runtime State

## Goal

Make disabling HydraDB stop memory use immediately without breaking agent execution, while
preserving the remote connection and channel preferences for an idempotent re-enable.

## Product Semantics

- Disable means memory is suspended for the company.
- It does not delete the HydraDB database or erase channel memory settings.
- Recall becomes an empty/no-memory result.
- Persistence becomes a skipped result.
- Re-enabling may reuse a ready connection after confirming provider readiness.
- Deleting the company remains the operation that requests remote cleanup.

## Current Risk

Disabling clears only `companies.memory_provider`. Existing channels retain retrieval flags, so a
new run enters recall and fails before the agent executes. Conversely, queued payloads may carry an
older serialized `Company` value and continue using the retained ready connection.

## Design

- Introduce an application-owned query that returns the current active memory binding by joining
  the company selection to its connection. Do not decide enablement from a serialized `Company`
  snapshot.
- Represent the lookup result explicitly, for example `Disabled`, `NotReady`, `Ready(binding)`, and
  `Misconfigured`, rather than turning every state into `AppError`.
- `Disabled` is a successful no-op for recall and persistence.
- `NotReady` behavior must be explicit:
  - configuration-time channel writes still require readiness;
  - runtime recall should follow a documented retry/degrade policy rather than an accidental error.
- Use the same active-binding query for channel forms, channel validation, coordinator calls, and
  status APIs so UI and execution cannot disagree.

## Implementation Steps

1. Split the connection-read port from worker queue operations and add a focused
   `active_binding(company_id)` method backed by a company/connection join.
2. Refactor `MemoryCoordinator::ready_provider` into typed binding resolution using current database
   state.
3. Return `Ok(None)` from recall and a skipped report from persistence when disabled.
4. Update `ChannelUseCases::memory_ready` and memory interlocks to use the same binding resolver.
5. Make provider disable and provider selection purpose-specific transactional persistence methods.
6. Ensure re-enable checks the retained remote connection and queues readiness reconciliation only
   when necessary.
7. Add structured state-transition metrics without logging memory content or credentials.
8. Document suspension versus deletion in company settings and deployment documentation.

## Tests

- Enable memory on a channel, disable the company provider, and prove the next inbound and scheduled
  agents still run with zero provider calls.
- Reuse a queued payload containing the previous `Company.memory_provider`; current disabled state
  must win.
- Re-enable and prove the retained connection is reused idempotently.
- Test UI readiness and server-side channel validation agree in disabled, pending, ready, and failed
  states.
- Test disabling during a retry does not restore the provider selection from stale work.
- Test provider configuration disappearing from a deployment degrades safely and observably.

## Acceptance Criteria

- Disabling HydraDB never causes an otherwise valid agent task to fail.
- No queued snapshot can override current company selection.
- UI, API, channel validation, recall, and persistence share one state interpretation.
- Re-enable preserves the documented retained-connection behavior.

