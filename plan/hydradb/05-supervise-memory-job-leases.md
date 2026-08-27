# Supervise Memory Job Leases and Provider Calls

## Goal

Keep ownership for the entire external provider operation, cancel real work on lease loss or
shutdown, and prevent configured request timeouts from exceeding safe worker deadlines.

## Current Risk

The memory worker renews each lease once before calling HydraDB. Provider requests may outlive the
fixed lease because timeout configuration has no upper bound. Another worker can then reclaim the
row while the stale worker continues making external side effects.

## Design

- Represent ownership as a named `MemoryJobLease` containing job ID, operation generation/token,
  and expiry.
- Execute the provider future directly under a supervisor; do not detach it into an untracked task.
- While the operation is active, select over:
  - provider completion;
  - a heartbeat interval shorter than one third of the lease;
  - shutdown cancellation;
  - an absolute operation deadline.
- A failed renewal means ownership is lost: cancel/drop and await the provider future, record lease
  loss, and perform no completion/failure transition.
- Every completion, retry, readiness update, and attempt record remains conditional on the current
  generation and a live lease.
- Check boolean persistence outcomes; a rejected stale transition is observable rather than
  silently treated as success.

## Implementation Steps

1. Add an operation generation UUID if the current token is not retained as an immutable generation
   across every transition.
2. Introduce a reusable lease-supervision helper in the application layer, parameterized by renew,
   cancellation, deadline, and the provider future.
3. Use it for provision, readiness status, and cleanup calls.
4. Propagate the process shutdown signal into active work, not only poll-loop sleeps.
5. Derive request timeout and worker operation deadline from validated configuration. Enforce finite
   upper bounds and require enough margin for heartbeat/cleanup.
6. Add jittered retry scheduling and retain bounded attempts/backoff for genuine failures.
7. Record counters for completed, retry, terminal, timeout, shutdown interruption, and lease loss.
8. Coordinate the operation deadline with deletion quiescence in
   `02-fence-deletion-against-provisioning.md`.

## Tests

- Use a slow fake provider and competing workers; heartbeats prevent a second claim while the first
  owns the job.
- Force renewal failure and prove the provider future is cancelled and no stale completion occurs.
- Reclaim an expired generation and prove the original worker cannot renew, complete, or fail it.
- Test shutdown during provision, readiness polling, and cleanup; all worker handles finish within
  the drain budget and durable state remains recoverable.
- Validate timeout values at their accepted boundaries and reject unsafe values at startup.
- Test a provider that never resolves reaches the operation deadline without blocking the lane
  indefinitely.

## Acceptance Criteria

- No provider future continues after confirmed lease loss.
- Active provider operations renew ownership throughout their lifetime.
- Every operation has a finite validated deadline shorter than the recoverability window.
- Stale transition attempts are measured and tested.
- Graceful shutdown owns and joins active memory work.

