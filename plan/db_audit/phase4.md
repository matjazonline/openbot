# Phase 4 — Attention feed: push the responsibility filter into the branches

`ATTENTION_SQL` (`src/adapters/persistence/attention.rs:87`) is a six-branch `UNION ALL` over
`background_tasks`, `manual_handoffs`, `human_approvals`, `response_reviews`, `task_outreaches` and
`message_deliveries`, wrapped in three further CTEs — `ranked`, `after_cursor`, `counted`.

Two structural problems, one of which is fixable without changing a single returned row.

---

## 4.1 The `my_work` filter runs after the union

The view mode arrives as `$4` and the viewer's principal as `$3`. They are applied in `ranked`,
after all six branches have already produced their rows:

```sql
), ranked AS (
    SELECT raw.*, params.as_of, …
    FROM raw CROSS JOIN params
    WHERE ($4 = 'team_work'
           OR ($4 = 'my_work' AND raw.responsibility_kind = 'principal'
                              AND raw.responsible_principal_id = $3)
           OR ($4 = 'unassigned' AND raw.responsibility_kind = 'channel_team'))
)
```

The task branch does mention `$3`, but only as one arm of a disjunction:

```sql
WHERE task.company_id = $1 AND task.channel_id = ANY($2)
  AND ( ($14 AND $4 = 'my_work' AND task.owner_principal_id = $3
         AND task.owner_principal_kind = 'person')
        OR (task.status IN (…) AND … ) )
```

An `OR` widens; it never narrows. So the branch returns the general set regardless of view mode, and
the other five branches carry no principal predicate at all. **Opening "my work" therefore builds
every teammate's approvals, reviews, handoffs, delegation decisions and delivery failures for every
channel the viewer can see, and then discards them.** On a team of one this is free. On a team of
thirty it is thirty times the work for the same answer.

The five anti-joins in the task branch make this worse than a row count suggests: each surviving
task row runs `NOT EXISTS` against `human_approvals`, `response_reviews` (with a nested `EXISTS`
into `response_drafts`), `task_outreaches` and `message_deliveries`. Those probes are paid for rows
that `ranked` is about to throw away.

**Change.** Give each branch the predicate its own responsibility column implies, keeping `ranked`'s
filter as the backstop so the two can be diffed:

- **task**: `my_work` → `owner_principal_kind = 'person' AND owner_principal_id = $3`;
  `unassigned` → `owner_principal_kind <> 'person' OR owner_principal_id IS NULL`.
- **handoff**: `my_work` → `responsible_principal_id = $3`; `unassigned` →
  `responsible_principal_id IS NULL`.
- **approval**: `my_work` → `approver_principal_id = $3`; `unassigned` never matches, because that
  branch's `responsibility_kind` is `principal` or `external` and never `channel_team` — so under
  `unassigned` the whole branch can be skipped.
- **response_review**: `my_work` → `reviewer_principal_id = $3`; `unassigned` never matches, same
  reasoning.
- **delegation_decision** and **delivery_failure**: both derive responsibility from the owning task,
  so they take the task branch's predicate against `task.owner_principal_*`.

Write these as a `$4 <> 'my_work' OR <predicate>` conjunct per branch rather than three copies of
each branch. That keeps the statement one statement, and it keeps `team_work` reading exactly as it
does today.

**The two branches that can be skipped entirely under `unassigned` are the interesting ones** —
`AND $4 <> 'unassigned'` in the approval and review branches removes two table accesses from that
view outright. Prove the claim before relying on it: both branches hard-code
`responsibility_kind` to a non-`channel_team` value in their select list, so `ranked` already
discards every row they produce under `unassigned`. If a future edit makes either branch emit
`channel_team`, that conjunct becomes a silent bug — put the reasoning in a comment next to it.

**Test.** This is a pure refactor of a filter, so the test is equality: for each of the three view
modes, on a fixture with items of every source kind spread across two principals and an unassigned
team, assert the item set is identical before and after the change. Snapshot the "before" set from
the current implementation first, in the same commit, so the comparison is real rather than
remembered. `src/adapters/persistence/attention_tests.rs` already holds the fixtures.

---

## 4.2 The `LIMIT` cannot bound the sort — and that is mostly inherent

`after_cursor` orders by `due_rank, priority_rank, created_at, source_kind, source_id` and then
takes `LIMIT $12`. The first two are computed in `ranked`:

```sql
CASE WHEN raw.due_at < params.as_of THEN 0
     WHEN raw.due_at <= params.as_of + interval '24 hours' THEN 1 ELSE 2 END AS due_rank,
CASE raw.business_priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 ELSE 2 END AS priority_rank
```

`due_rank` depends on `as_of`, which is `COALESCE($5, CURRENT_TIMESTAMP)`. No index can supply that
ordering, so every page sorts the entire filtered set before returning a screenful.

**Do not try to fix this with an index.** A merged feed over six tables with a clock-dependent
ranking has no single sort order to index, and an expression index on `due_rank` is impossible
because the expression is not immutable. The honest options are:

1. **Accept it, having made the input smaller.** §4.1 shrinks the set being sorted by the size of
   the team for two of the three view modes. For `team_work` it changes nothing.
2. **Bound the input.** The feed is "what needs attention"; items older than some horizon are not
   what anyone is scrolling for. A `created_at >= CURRENT_TIMESTAMP - $n` conjunct in every branch
   turns an unbounded sort into a bounded one and would let each branch use its
   `(company_id, …, created_at DESC, id DESC)` index. **This is a product decision, not a database
   one** — it makes old open items invisible — so it needs an owner, not a migration.
3. **Materialise the feed.** `attention_source_events` already exists and
   `notify_attention_changed` already fires on every contributing table. A rollup table maintained
   by those triggers would make this a single indexed read. That is the design in
   `plan/db_improve/kanban_denormalized_rollup_table_optimization.md`; read it before proposing
   anything here, because it is the same shape of problem and the trade-offs are already written
   down.

**This phase implements option 1 only.** Options 2 and 3 are recorded so the next reader does not
mistake the remaining sort for an oversight. Add a comment above `after_cursor` saying exactly that
— the file is already written in that voice, and an unexplained full sort invites someone to
"optimise" it with an index that cannot exist.

`COUNT(*) OVER ()` in `counted` is *not* a defect: it runs over `after_cursor`, which is already
`LIMIT`-bounded, and the name `bounded_count` says so. Leave it.

---

## Sequencing with Phase 1

`phase1.md` §1.3 adds `company_id` to this same statement's delegation join and two anti-joins.
Both phases rewrite `ATTENTION_SQL`. Land 1.3 first and rebase this on it, or fold 1.3 into this
phase and note it in the commit message. Doing both independently guarantees a conflict in a
250-line SQL literal, which is the worst place in this repository to resolve one.

---

## Acceptance criteria

- Every union branch carries the responsibility predicate its own columns imply; `ranked`'s filter
  remains as a backstop and is documented as such.
- The approval and response_review branches are skipped under `unassigned`, with a comment recording
  why that is sound.
- All three view modes return item sets identical to the pre-change implementation, proven by a test
  written against a snapshot taken before the rewrite.
- A comment above `after_cursor` records that the sort is inherent, and names the two options that
  were considered and not taken.
- No index added, no cursor or pagination contract changed.
- `cargo sqlx prepare -- --all-targets` regenerated; `cargo test` and
  `cargo clippy --all-targets -- -D warnings` green.
