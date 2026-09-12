# Phase 2 — Write-path and retention shapes

Two Class A changes to statements that do more round trips or read more rows than the shape of the
data requires. Neither adds an index. Neither changes a result.

This is the lowest-value phase in the plan and it says so up front: both items are correct as
written and merely wasteful. They are here because "fix all of it" was the ask, and because both
fixes are small enough that the arithmetic favours doing them now rather than tracking them. If you
are triaging, land phases 3 and 5 first.

---

## 2.1 Set-based inserts for caller-supplied collections

Six loops in the persistence layer issue one `INSERT` per element:

| Site | Collection | Bounded by |
|---|---|---|
| `src/adapters/persistence/agent.rs:316` | `write.skill_ids` | agent edit form |
| `src/adapters/persistence/agent.rs:331` | `write.sub_agent_ids` | agent edit form |
| `src/adapters/persistence/channel.rs:372` | `write.agent_ids` | channel edit form |
| `src/adapters/persistence/channel.rs:470` | assigned agents | channel edit form |
| `src/adapters/persistence/channel.rs:608` | `write.agent_ids` | channel edit form |
| `src/adapters/persistence/task/queue.rs:414` | outreach channel targets | **task payload** |
| `src/adapters/persistence/thread/message.rs:747` | message participants | **inbound envelope** |
| `src/adapters/persistence/thread/inbound.rs:220` | binding validation per association | **inbound event** |

**Only the last three are worth changing.** The first five are bounded by a human editing a form —
an agent has a handful of skills, a channel a handful of agents — and a loop of five inserts inside
an open transaction is not a problem worth a rewrite. Leave them, and do not let a later reader
"finish the job" without re-reading this paragraph.

The last three are driven by data that arrives from outside: an email's `To`/`Cc` list becomes
`message_participants` rows, an agent's outreach declaration becomes `task_channel_targets` rows, and
every association on an inbound event is validated against `channel_bindings` with its own round
trip. Those are the ones where a statement per element is a real cost, and where `src/AGENTS.md`'s
bounded-work rule applies most directly.

The third is a *read* loop rather than an insert loop and needs a different rewrite, so it is handled
separately in 2.1b below.

**Change.** Replace each loop with one statement over `unnest`. `task/queue.rs:414` becomes:

```sql
INSERT INTO task_channel_targets (
        task_id, company_id, channel_id, thread_id, recipient_role, position
) SELECT $1, $2, target.channel_id, target.thread_id, target.recipient_role, target.position
    FROM unnest($3::uuid[], $4::uuid[], $5::text[], $6::int[])
      AS target(channel_id, thread_id, recipient_role, position)
   ON CONFLICT (task_id, channel_id) DO NOTHING
```

with the four arrays built from the same `enumerate()` the loop already does. `message_participants`
(`thread/message.rs:736`) follows the same shape.

**Three things to get right.**

*`unnest` with multiple arrays requires equal lengths*, or Postgres pads the short ones with nulls
and you insert garbage. Build the arrays in one pass from one iterator so they cannot diverge, and
assert the lengths match before binding.

*Nullable columns need the array type to be nullable-aware.* `thread_id` in
`task_channel_targets` is nullable; `Vec<Option<Uuid>>` binds to `uuid[]` correctly in sqlx, but
verify it against the pinned version rather than assuming.

*`ON CONFLICT … DO NOTHING` behaves differently in a set insert.* In the loop, each conflicting row
is skipped individually and the loop continues. In the set form that is still true — `DO NOTHING`
is per-row — but if the *input* contains two rows with the same `(task_id, channel_id)`, the set
form raises `ON CONFLICT DO UPDATE command cannot affect row a second time`-class errors in the
`DO UPDATE` case and silently keeps one in the `DO NOTHING` case. Deduplicate the input before
binding, and add a test that feeds a duplicate channel twice.

**Test.** For each of the two: insert a collection of three elements, assert the rows and their
`position` ordering; insert a collection containing a duplicate, assert one row survives; insert an
empty collection, assert the statement is a no-op rather than an error. Scope every assertion to the
ids the test created — the suite shares one database and whole-table counts are racy.

### 2.1b Validate every association in one statement

`src/adapters/persistence/thread/inbound.rs:220` runs one `SELECT EXISTS(…)` per association to
prove each `(binding_id, channel_id)` pair names an active binding in the company, returning
`AppError::NotFound` naming the first pair that fails. One round trip per recipient association, on
the inbound path, inside the ingest transaction.

The set form asks the same question once and returns the first offender:

```sql
SELECT assoc.binding_id, assoc.channel_id
  FROM unnest($1::uuid[], $2::uuid[]) AS assoc(binding_id, channel_id)
 WHERE NOT EXISTS (
     SELECT 1 FROM channel_bindings AS binding
      WHERE binding.id = assoc.binding_id AND binding.company_id = $3
        AND binding.channel_id = assoc.channel_id AND binding.status = 'active')
 LIMIT 1
```

No row means every association is valid; a row carries exactly the two values the existing error
message formats. **Keep the error text identical** — it names the binding and the channel, and
tests or operators may match on it.

One behavioural difference to accept deliberately: the loop reports the *first association in input
order* that fails, while the set form reports whichever the planner finds first. If any test asserts
*which* invalid association is named when there are several, add `ORDER BY assoc.ordinality` using
`unnest(...) WITH ORDINALITY` rather than weakening the test.

---

## 2.2 Prune runtime metric samples by machine

`src/adapters/persistence/runtime_metrics.rs:402`:

```rust
sqlx::query("DELETE FROM runtime_metric_samples WHERE sampled_at < $1")
```

`runtime_metric_samples` has exactly one index — `runtime_metric_samples_pkey (machine_id,
sampled_at)`. A predicate on `sampled_at` alone cannot lead on it, so every retention sweep reads
the whole table.

**Be honest about the size of this problem.** The table is bounded by the retention this very
function enforces: machines × samples-per-minute × retention-window. At a handful of machines
sampling every few seconds with a day of retention that is tens of thousands of rows, and scanning
tens of thousands of rows on a periodic sweep is nothing. This is on the plan because the fix is
five lines and removes a scan whose cost grows with fleet size, not because it is hurting anything
today. **Do not add an index on `sampled_at`** — that would be a Class C change, it would be
maintained on every sample insert, and it would be solving a problem the primary key can already
solve.

**Change.** Delete per machine, so each delete is a primary-key range scan:

```sql
DELETE FROM runtime_metric_samples AS sample
 USING (SELECT DISTINCT machine_id FROM runtime_metric_samples) AS machine
 WHERE sample.machine_id = machine.machine_id
   AND sample.sampled_at < $1
```

The `DISTINCT machine_id` is itself served by the primary key's leading column, and on the
PostgreSQL 18 that CI and production run it can use a btree skip scan. Local development is
Homebrew 16.14 (recorded in `plan/db_improve/05-deferred-until-traffic.md` under "Verification
Gaps"), where the same query is a full index scan of a small index — still cheaper than the heap
scan it replaces, and the shape is what matters.

If that reads as too clever, the alternative is honest and equally fine: have the caller pass the
machine identity it already knows and delete `WHERE machine_id = $1 AND sampled_at < $2`, accepting
that samples from a machine that never comes back are pruned only when some other sweep enumerates
them. Check `prune_before`'s callers with `graft callers prune_before` before choosing — if the
sweep already runs per machine, the second form is strictly simpler and should win.

**Test.** Samples for two machines, half of each older than the cutoff. Assert both machines lose
exactly their old rows and keep their recent ones — the bug the per-machine rewrite could introduce
is pruning only the first machine, and a single-machine test would not catch it.

---

## Implementation notes (2026-09-11)

What landed differs from the text above in four places. The reasons are recorded here so the next
reader does not "fix" the code back to match the plan.

- **`task_channel_targets.thread_id` is `NOT NULL`**, and so is `TaskTarget::thread_id`. The
  nullable-array concern in 2.1 does not apply. The arrays bind as plain `Vec<Uuid>`.
- **Array lengths are equal by construction, with no assert.** Each array is a `map` over the same
  deduplicated slice with no filter, so no two can differ in length. A duplicate channel is dropped
  in Rust, keeping its first occurrence and the position it was stated at. That makes the rows
  identical to what the loop wrote: positions `0, 1, 3` for `[A, B, A, C]`. `message_participants`
  needed no new dedup, because `resolve_participants` already drops a handle repeated within a role.
  Both helpers return early on an empty input, so the empty case costs zero round trips, as it did
  under the loop.
- **2.1b orders by `WITH ORDINALITY` unconditionally.** It is cheap, and it keeps the error naming
  the same association the loop named.
- **2.2 does not use the `SELECT DISTINCT machine_id` form.** A seeded shape check on local
  PostgreSQL 16.14 (a temp table, 4 machines × 7 days of 10-second samples) ruled it out. The
  `DISTINCT` subquery planned as `HashAggregate` over a heap `Seq Scan`. So the statement did the
  original's full heap scan *plus* the per-machine index scans. Under a generic plan it became a
  hash join over two heap scans. The claim above that PG18 skip scan serves the `DISTINCT` is also
  wrong. Per the PG18 docs (§11.3), skip scan needs a constraint on a later index column, which a
  bare `DISTINCT` does not have. The same docs imply that on PG18 the *original*
  `WHERE sampled_at < $1` is itself eligible for a skip scan on the key. What shipped enumerates
  machines with a recursive loose index scan (`min(machine_id)`, then the next key greater than
  it: one index descent per machine) and deletes `WHERE machine_id = ANY(ARRAY(…)) AND
  sampled_at < $1`. That planned as an `Index Scan using runtime_metric_samples_pkey` under a custom
  plan and as a `Bitmap Index Scan` on the same key under a generic plan. It never read the heap to
  find the rows. These are shape claims from a synthetic seed, not measurements.
- **The per-caller alternative in 2.2 was rejected.** `MachineId` is boot-local off Fly, so
  deleting only the caller's machine would never prune any earlier boot's samples, or any retired
  machine's.

## Acceptance criteria

- The two externally-driven insert loops and the association validation loop issue one statement
  each; the five form-driven loops are unchanged, and a comment at one of them records that this
  was a decision.
- Duplicate input to a set insert produces one row, covered by a test.
- The association validator returns the identical error text for an invalid pair, covered by a test.
- `prune_before` deletes through the primary key, and a two-machine test proves both are pruned.
- No index is added anywhere in this phase.
- `cargo sqlx prepare -- --all-targets` regenerated, `cargo test` and
  `cargo clippy --all-targets -- -D warnings` green.
