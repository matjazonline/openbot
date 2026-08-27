# Preserve Memory Scope Isolation

## Goal

Make memory scope resolution monotonic: when a requested narrow identity is unavailable, the
operation may omit that scope or fail safely, but it must never widen to company memory.

## Current Risk

`resolve_scopes` currently maps missing agent and user identities to the company collection.
Scheduled runs always lack a sender, so enabling user memory on a scheduled channel reads and
writes company-wide memory. Missing agent configuration has the same widening behavior.

Memory scope is a data-isolation boundary, not a relevance hint. A warning does not make a wider
read or write safe.

## Design

- Keep company scope available only when the company flag is explicitly enabled.
- Resolve agent scope only when an agent ID is present.
- Resolve user scope only when a normalized sender identity is present.
- Return unavailable requested scopes as typed diagnostics, for example
  `UnavailableMemoryScope::Agent` and `UnavailableMemoryScope::User`.
- If no requested scopes can be resolved:
  - recall returns `Ok(None)` without calling the provider;
  - persistence returns a skipped report without calling the provider.
- Preserve the existing highest-weight deduplication only when two explicitly resolved scopes map
  to the same collection. Do not use deduplication to create fallback behavior.
- Emit structured counters for skipped scopes using provider/company/channel/task identifiers, but
  never sender addresses or memory content.

## Implementation Steps

1. Change `src/domain/entities/memory.rs` so `resolve_scopes` returns a named result struct containing
   `resolved` and `unavailable` fields.
2. Remove both branches that substitute the `company` collection for a missing agent or sender.
3. Update `MemoryCoordinator::recall` and `MemoryCoordinator::persist` to handle an empty resolved
   set as a successful no-op.
4. Replace the current "fell back to company" warnings with "scope skipped because identity is
   unavailable" events and metrics.
5. Update inbound and scheduled call sites to retain their present identity inputs; scheduled runs
   should continue passing no sender rather than inventing one.
6. Correct `plan/hydradb_memory_completion_plan.md` and deployment/product documentation that still
   describe fallback widening.

## Tests

- Unit-test all combinations of company/agent/user flags with present and absent identities.
- Prove a missing sender never produces the `company` collection unless company memory is also
  explicitly selected.
- Prove a missing agent never produces the `company` collection unless company memory is selected.
- Test scheduled recall and persistence with only user memory enabled: the provider receives zero
  calls and the scheduled agent still runs.
- Test mixed scopes: an unavailable user scope does not suppress an explicitly enabled company or
  available agent scope.
- Assert skipped-scope logs/metrics contain no email address or message content.

## Acceptance Criteria

- No code path translates an unavailable user or agent identity into company scope.
- Recall and persistence remain successful no-ops when every requested scope is unavailable.
- Automated tests cover both inbound and scheduled execution.
- Documentation no longer promises or recommends scope fallback.

