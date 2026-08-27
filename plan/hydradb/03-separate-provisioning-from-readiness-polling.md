# Separate Provisioning From Readiness Polling

## Goal

Allow normal asynchronous HydraDB provisioning to take its documented amount of time without
consuming the failure retry budget or repeatedly issuing create requests.

## Current Risk

Each job claim increments `attempts`. A successful create followed by `is_ready == false` becomes a
retry after two seconds, and the job is terminal after eight claims. A healthy database that needs
roughly fifteen seconds to initialize can therefore be marked failed.

## State Model

Use explicit provisioning phases:

- `create_pending`: no successful create has been acknowledged.
- `waiting_ready`: create was accepted or already existed; only status is polled.
- `ready`: terminal success.
- `failed`: terminal failure with a safe reason.

Track separate values:

- `failure_attempts` for retryable transport/provider failures;
- `readiness_deadline` for the maximum total initialization window;
- `next_poll_at` for status polling;
- the existing execution generation/lease for claimant fencing.

A non-ready status is expected state, not a failed attempt.

## Implementation Steps

1. Add an additive migration for phase, failure-attempt count, readiness deadline, and next poll
   time. Backfill existing pending/leased jobs compatibly.
2. Replace the overloaded `attempts` meaning in domain and persistence types with named fields.
3. In `create_pending`:
   - call `provision` once per retry attempt;
   - treat conflict/already-exists as accepted;
   - atomically transition to `waiting_ready` and set a readiness deadline.
4. In `waiting_ready`:
   - call only `is_ready`;
   - schedule the next bounded poll when not ready;
   - increment failure attempts only for classified retryable request failures;
   - fail terminally only when the readiness deadline or failure budget is exhausted.
5. Add jitter to retry and poll scheduling so multiple application instances do not synchronize.
6. Make manual retry reset the appropriate failure/deadline state without creating a second job.
7. Expose safe distinctions in monitoring and UI: creating, waiting for readiness, retrying a
   provider error, timed out, and ready.

## Configuration

- Introduce a production-safe readiness deadline or use a documented constant based on provider
  guarantees.
- Bound minimum and maximum poll intervals.
- Keep request timeout separate from the total readiness deadline.

## Tests

- A fake provider remains non-ready for longer than eight polls and then becomes ready; the job must
  complete successfully with zero provider-failure attempts.
- Assert `provision` is not called again during ordinary readiness polling.
- Test transient status failures increment the failure counter and apply bounded jittered backoff.
- Test readiness-deadline exhaustion produces one observable terminal transition.
- Test manual retry from terminal timeout and provider failure.
- Test two claimants cannot poll or transition the same generation concurrently.
- Test restart while waiting preserves the original readiness deadline.

## Acceptance Criteria

- Healthy non-ready responses never consume the provider failure budget.
- Create and status polling are distinct durable phases.
- Provisioning duration is governed by an explicit deadline rather than claim count.
- Metrics and tests distinguish expected polling from actual failures.

