# Fence Company Deletion Against Provisioning

## Goal

Guarantee that deleting a company eventually leaves no HydraDB database behind, even when deletion
races with an already-started provisioning request or a worker dies mid-operation.

## Current Risk

Company deletion copies the remote ID into a cleanup job and then cascades the provisioning row.
An old worker may already be executing `provision`. Cleanup can observe `404`, mark itself complete,
and exit before the older provisioning request creates the remote database.

Database lease fencing protects PostgreSQL state, but cannot undo or order a remote side effect.

## Design

Use a durable desired-state/reconciliation record for every remote memory database:

- `desired_state = present | absent` is authoritative.
- The record and its execution generation survive company deletion while cleanup is outstanding.
- Provider operations are generation-fenced in PostgreSQL and always re-read desired state before
  and after the external call.
- Company deletion transactionally changes desired state to `absent`, prevents new provisioning,
  and schedules reconciliation before removing the company.
- Cleanup cannot treat an early `404` as terminal while an older provisioning execution may still
  be alive. It waits through the maximum provider-operation deadline, then verifies absence again.
- Once desired absence is confirmed after the quiescence window, the cleanup job and surviving
  lifecycle record may become terminal and eligible for retention cleanup.

This plan depends on the bounded, cancellable provider execution in
`05-supervise-memory-job-leases.md`; without a maximum operation lifetime there is no finite safe
quiescence period.

## Implementation Steps

1. Add an additive migration for a provider-neutral remote-resource lifecycle table, or extend the
   existing connection/cleanup model with equivalent surviving fields:
   - provider and remote database ID;
   - optional company ID;
   - desired state;
   - operation generation and lease fields;
   - `quiesce_until`, last safe error, timestamps, and coherent-state constraints.
2. Backfill every existing `memory_provider_connections` row as `desired_state = present` and every
   cleanup row as `desired_state = absent`.
3. Make provider selection/reselection transactionally set desired state to `present` and enqueue
   reconciliation idempotently.
4. In `delete_company_with_cleanup`, lock the lifecycle row, set desired state to `absent`, detach it
   from the soon-to-be-deleted company, and enqueue cleanup in the same transaction.
5. Change provisioning workers to check desired state immediately before calling HydraDB and again
   before committing readiness. If it changed to absent, schedule cleanup rather than readiness.
6. Change cleanup so `404` is success only after all older generations are expired/cancelled and the
   quiescence deadline has passed. Recheck after the deadline.
7. Keep deletion and provisioning calls idempotent by deterministic remote database ID.
8. Add metrics for desired-state changes, stale-generation suppression, quiescence waits, confirmed
   cleanup, and cleanup exhaustion.

## Tests

- A competing integration test pauses a fake provider inside `provision`, deletes the company,
  releases provisioning, and proves the database is subsequently deleted.
- Repeat with cleanup receiving `404` before provisioning finishes; cleanup must not become terminal
  until the later absence verification succeeds.
- Prove a stale provisioning generation cannot restore connection readiness after deletion.
- Prove process restart during each lifecycle phase resumes reconciliation.
- Prove company deletion and cleanup-job creation remain atomic.
- Add a real-database competing-claimant test for cleanup leases.

## Rollout

- Deploy the additive schema and dual-read compatibility first.
- Backfill and verify lifecycle rows for all existing connections and cleanup jobs.
- Switch workers and deletion to the reconciler model.
- Remove obsolete job paths only in a later migration after no old application version can write
  them.

## Acceptance Criteria

- Company deletion has a durable, test-covered path to confirmed remote absence.
- No early `404` can complete cleanup while an older provision operation may still finish.
- Stale workers cannot restore deleted resources to ready application state.
- Recovery works after crashes at every transition.

