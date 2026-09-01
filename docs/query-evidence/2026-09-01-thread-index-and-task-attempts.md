# Thread-Index and task-attempt index decision — 2026-09-01

## Stored-value audit

The local development database was audited before the equality-only lookup was enabled:

```sql
SELECT COUNT(*) FILTER (WHERE thread_index IS NOT NULL),
       COUNT(DISTINCT thread_index) FILTER (WHERE thread_index IS NOT NULL),
       COALESCE(MAX(octet_length(thread_index)), 0)
  FROM email_messages;
```

Result: `0 | 0 | 0`. Consequently the same-parser classification is zero canonical values, zero
non-canonical valid values, zero invalid values, and zero over-limit values. No backfill is needed
for this database. A non-empty staging or production database must be classified with
`ThreadIndex::parse` before deploying the equality-only reader; a non-canonical valid count above
zero requires the separately reviewed, resumable backfill described in the implementation plan.

## Query and index decision

The `Thread-Index` predicate is changed for correctness from encoded-text prefix matching to a
bounded `text[]` of binary ancestors using `email_message.thread_index = ANY($2)`. This makes a
btree access path possible but does not, by itself, show that one improves the complete
channel-scoped join.

No index migration is included in this change:

- The local database has no representative `email_messages`, `thread_messages`, or
  `task_attempts` population from which to capture meaningful `EXPLAIN (ANALYZE, BUFFERS)` output.
- The production statistics and skewed-seed tooling required by
  `plan/db_improve/03-capture-query-statistics.md` is not present yet.
- Therefore neither the partial `email_messages (thread_index)` candidate nor a
  `task_attempts (started_at)` candidate has passed the repository's evidence gate.

Before either index is proposed, retain the complete before/after plans, buffer activity, relation
and index sizes, write rate, and window selectivity for the scope/window matrix in
`plan/db_improve/01-index-thread-index-and-task-attempts.md`. The absence of a migration here is the
intentional outcome of that gate, not an assertion that either index is unnecessary.
