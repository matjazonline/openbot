# Phase 5 — `claim_pending_tasks`: claim cost proportional to the batch

The highest-value and highest-risk change in this plan. It rewrites the statement the task worker
runs on every poll, and it changes how a scheduling guarantee is implemented without changing the
guarantee. Do not start it until phases 1–4 have landed and the suite is green.

---

## What is wrong

`claim_pending_tasks` (`src/adapters/persistence/task/operations.rs:2065`) implements round-robin
fairness across companies with a window function:

```sql
WITH ranked AS (
    SELECT id, run_at, created_at,
           ROW_NUMBER() OVER (PARTITION BY company_id
                              ORDER BY run_at ASC, created_at ASC, id ASC) AS company_round
      FROM background_tasks
     WHERE status = 'pending' AND run_at <= CURRENT_TIMESTAMP
       AND owner_principal_kind = 'agent'
       AND EXISTS (SELECT 1 FROM principals AS owner
                     JOIN channel_agents AS assignment ON … )
), claimable AS (
    SELECT task.id FROM background_tasks AS task JOIN ranked ON ranked.id = task.id
     ORDER BY ranked.company_round, ranked.run_at, ranked.created_at, task.id
       FOR UPDATE SKIP LOCKED LIMIT $1
)
UPDATE background_tasks AS task SET status = 'processing', … FROM claimable …
```

A window function must see its whole partition before it can emit a single row, so nothing above the
scan can stop early. The plan captured on 2026-09-10 shows it:

```
Limit → LockRows → Sort → Nested Loop
  → Subquery Scan on ranked
      → WindowAgg
          → Sort  (Sort Key: company_id, run_at, created_at, id)
              → Index Scan using background_tasks_pending_ready_idx
                    Index Cond: (run_at <= CURRENT_TIMESTAMP)
                    Filter: (owner_principal_kind = 'agent') AND (SubPlan 1)
  → Index Scan using background_tasks_pkey on background_tasks task
```

Read it from the bottom. Every pending, due row is scanned. Each one runs `SubPlan 1` — a nested
loop into `principals` and a bitmap scan of `channel_agents`. The survivors are sorted in full, fed
through `WindowAgg`, joined back to `background_tasks` row by row, sorted *again*, and only then does
`LIMIT $1` apply. **Claiming ten tasks costs a full pass over the backlog plus two sorts of it, once
per poll, once per worker.** The cost is highest exactly when the backlog is deepest.

For contrast, the other three claim loops in this repository are textbook — `claim_deliveries`
(`src/adapters/persistence/delivery/queue.rs:53`), `claim_notification_events`
(`notification.rs:201`) and `claim_inbound_events` (`inbound_event/mod.rs:207`) each order by a
column list that matches a partial index exactly and put the `LIMIT` inside the locking CTE, so the
scan stops after `limit` unlocked rows. The task queue is the outlier, and it is the outlier because
it is the only one that promises fairness.

**This is a shape claim.** Nobody has measured how deep the backlog gets. What the plan proves is
that the query has no way to stop early, which is a property of the statement rather than of the
data.

## The guarantee that must survive

`pending_claims_take_one_company_round_before_a_second_task_from_a_backlog`
(`src/adapters/persistence/task/tests.rs:2507`) is the contract: a company with one due task is
served in the same batch as a company with a backlog, not behind it. Any rewrite that does not keep
that test green — unmodified — is wrong. Read it before writing SQL.

---

## Two options, and someone has to choose

### Option A — enumerate companies, take a bounded slice from each

Keep the semantics exactly. Replace "rank the whole backlog" with "ask each company with due work
for its first `$1` rows", which bounds the work at `companies-with-due-work × limit` instead of
`entire backlog`:

```sql
WITH RECURSIVE due_company AS (
    -- loose index scan: the first company with due work, then the next one after it, ...
    (SELECT company_id FROM background_tasks
      WHERE status = 'pending' AND owner_principal_kind = 'agent'
      ORDER BY company_id LIMIT 1)
    UNION ALL
    SELECT (SELECT t.company_id FROM background_tasks t
             WHERE t.status = 'pending' AND t.owner_principal_kind = 'agent'
               AND t.company_id > d.company_id
             ORDER BY t.company_id LIMIT 1)
      FROM due_company d WHERE d.company_id IS NOT NULL
), candidate AS (
    SELECT slice.id, slice.run_at, slice.created_at, slice.company_round
      FROM due_company AS c
      CROSS JOIN LATERAL (
          SELECT t.id, t.run_at, t.created_at,
                 ROW_NUMBER() OVER (ORDER BY t.run_at, t.created_at, t.id) AS company_round
            FROM background_tasks AS t
           WHERE t.company_id = c.company_id
             AND t.status = 'pending' AND t.run_at <= CURRENT_TIMESTAMP
             AND t.owner_principal_kind = 'agent'
             AND EXISTS (…)
           ORDER BY t.run_at, t.created_at, t.id
           LIMIT $1
      ) AS slice
     WHERE c.company_id IS NOT NULL
), claimable AS (
    SELECT task.id FROM background_tasks AS task JOIN candidate ON candidate.id = task.id
     ORDER BY candidate.company_round, candidate.run_at, candidate.created_at, task.id
       FOR UPDATE SKIP LOCKED LIMIT $1
)
UPDATE … FROM claimable …
```

The `ROW_NUMBER()` is still there but it now runs inside a `LATERAL` over at most `$1` rows, so the
window is bounded rather than global. The final ordering is unchanged, so the fairness test passes
by construction: taking each company's first row before any company's second row is exactly what
ordering by `company_round` then `run_at` does, and the candidate set contains every row that could
have won.

**Option A needs an access path per company, and today there is none.** The existing partial index is
`background_tasks_pending_ready_idx (run_at, created_at, id) WHERE status = 'pending'` — it leads
with `run_at`, so the `LATERAL`'s `company_id = …` cannot use it and each iteration would rescan the
whole pending set. The fix is to **widen the existing index in place rather than add one**:

```sql
CREATE INDEX background_tasks_pending_ready_idx
    ON public.background_tasks USING btree (company_id, run_at, created_at, id)
 WHERE status = 'pending' AND owner_principal_kind = 'agent';
```

Two changes: `company_id` moves to the front, and `owner_principal_kind = 'agent'` joins the
predicate — which also removes the per-row filter the current plan shows, and shrinks the index by
excluding every human-owned task.

**Before doing that, prove nothing else depends on the current shape.** Grep says the only consumers
of a `status = 'pending' AND run_at <= …` predicate without a company are this claim query and the
dashboard's `due_now` counter (`dashboard.rs:64`, and the same predicate inside `OUTSTANDING_SQL` at
`:313`). After Phase 3 the company-scoped form of `due_now` is served by the new leading `company_id`
and the global form is a full aggregate either way — so the widening is safe *provided Phase 3 has
landed*. Re-run the grep yourself; do not take this paragraph's word for it.

Widening a partial index on the hottest table in the schema is not free — `background_tasks` takes a
write on every claim, lease renewal and completion. It is index-count-neutral and predicate-narrower,
which is why it is defensible, but **it must be plan-captured before and after and recorded in
`plan/db_improve/05-deferred-until-traffic.md`** under the same rules everything else in this
repository follows. This is the one place in the whole plan where a schema change touches a hot
table.

*The recursive CTE is a loose index scan.* PostgreSQL 18 — what CI and production run — can do this
with a btree skip scan and a plain `SELECT DISTINCT company_id`, which is far more readable. Local
development is Homebrew 16.14 (`05-deferred-until-traffic.md`, "Verification Gaps"), where `DISTINCT`
degrades to a full scan of the partial index. Write the readable `DISTINCT` form, capture the plan on
18, and keep the recursive form in a comment with the version note — or align local Postgres to 18
first, which the deferred file already wants for unrelated reasons.

### Option B — bound the prefetch, accept weaker fairness

Change no index. Take a bounded window of the globally-oldest due rows and round-robin *within* it:

```sql
WITH window AS (
    SELECT id, company_id, run_at, created_at
      FROM background_tasks
     WHERE status = 'pending' AND run_at <= CURRENT_TIMESTAMP AND owner_principal_kind = 'agent'
     ORDER BY run_at, created_at, id
     LIMIT $1 * $4        -- fairness factor, e.g. 20
), ranked AS ( … ROW_NUMBER() OVER (PARTITION BY company_id …) FROM window … )
```

The scan is now bounded by `limit × factor` and the existing index serves the ordering directly.

**What it costs.** A company whose single due task is older than nothing else is still served — its
row is in the window. A company whose task is *newer* than `limit × factor` rows belonging to one
noisy tenant is not served this poll. It is served on a later poll, because the noisy tenant's rows
leave `pending` as they are claimed. So this is not starvation, it is bounded delay proportional to
how far the noisy tenant's backlog exceeds the window — and `pending_claims_take_one_company_round…`
would need its fixture checked against the chosen factor, which is exactly the kind of test edit this
plan otherwise forbids.

**Recommendation: Option A**, because the fairness guarantee is the reason this query is shaped the
way it is, and Option B trades it for an index the widening already avoids adding. But A costs a
touch to the schema and B does not, so this is a real decision with an owner, not a detail to settle
in review. Record the choice and the reasoning at the top of the rewritten function, in the voice the
existing comment there already uses.

---

## Things a rewrite must not disturb

**`lock_task_agent_harnesses` still fires per row.** The trigger takes `FOR SHARE` on the owning
agent and every channel-assigned agent on each transition into a live status
(`general_plan_instructions.md`, "Understood, no change proposed"). A batch claim of *N* rows
acquires those locks *N* times, before and after this change. Neither option moves that, and neither
should — it is what makes the harness interlock meaningful. What matters is that a rewrite does not
*increase* the number of rows the `UPDATE` touches, which neither does.

**The comment at `operations.rs:2072` is load-bearing.** It records why expired `processing` rows are
not stolen here — the loop that produced, and why `reap_expired_task_leases`
(`operations.rs:1020`) turns them back into pending rows while paying an attempt. Carry that comment
across verbatim. A rewrite that quietly re-widens the status predicate reintroduces a fixed bug.

**`FOR UPDATE SKIP LOCKED` can return fewer than `$1` rows.** True today and true after; the caller
already handles a short batch. Do not "fix" it by looping.

**Eight row triggers fire on the claimed rows** — `record_task_status_event` writes a
`task_status_events` row per claim, `notify_attention_changed` and `notify_task_chain_changed` issue
`NOTIFY`s. The claim `UPDATE` is much heavier than it reads. That is unchanged here and is the reason
raising the batch size is not a free alternative to fixing the scan.

---

## Tests

- `pending_claims_take_one_company_round_before_a_second_task_from_a_backlog` (`tests.rs:2507`)
  passes **unmodified**. If Option B is chosen and it does not, stop and escalate the fairness change
  rather than editing the test.
- A new test with three companies — one with a deep backlog, two with a single due task each — 
  asserting a batch of three returns one task from each company.
- A test that a task owned by a person (`owner_principal_kind <> 'agent'`) is never claimed, because
  Option A moves that predicate into the index and a mistake there would be silent.
- A test that a task whose owning agent is not assigned to the task's channel is never claimed —
  the `EXISTS` subquery's contract, which both options preserve but neither option's plan makes
  obvious.
- Existing lease, retry and reaping tests unchanged and green.

## Acceptance criteria

- The claim statement contains no window function over an unbounded input; the plan shows the row
  source bounded by `LIMIT` or by a `LATERAL` slice.
- Before-and-after `EXPLAIN` output is captured and recorded in
  `plan/db_improve/05-deferred-until-traffic.md`, labelled as shape checks against a near-empty
  database, per that file's acceptance criteria.
- If Option A: the index is *widened in place*, index count in `pg_indexes` for `background_tasks`
  is unchanged, both local databases recreated with `./scripts/reset-db.sh --all`, and the grep
  proving no other consumer of the old leading column is recorded in the commit message.
- The fairness test is green and unedited.
- `cargo sqlx prepare -- --all-targets` regenerated; `cargo test` and
  `cargo clippy --all-targets -- -D warnings` green.
