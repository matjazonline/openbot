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

## Implementation notes (2026-09-11)

What landed, and where it differs from the text above. The reasons are recorded so the next
reader does not "fix" the code back to match the plan. Plans, seeded row counts and the race
reproduction are in `plan/db_improve/05-deferred-until-traffic.md` §5.

- **Option A, chosen by the owner, for a different reason than the one given above.** Under load
  the worker claims one slot at a time: `run_bounded_task_loop` asks for as many tasks as it has
  free slots, and once every slot is busy each finished task frees exactly one. With a limit of
  one, the old statement, A and B all return the same row, the oldest due task in the queue. So
  fairness barely separates the options; cost does. A's cost depends on neither a tuning factor
  nor how many parked tasks there are. B's window would step over every pending person-owned or
  unassigned task on each claim, because those linger and sit at the old end of a `run_at`-led
  index. Narrowing B's index to stop that would have cost the same schema change and reset as A.
  The one-slot observation is finding 15; the rule change it calls for is the follow-up recorded
  after these notes.
- **A double claim the audit did not find is fixed here** (finding 14). The old statement's pending
  check lived only in the ranking subquery. `FOR UPDATE` locks rows chosen under the statement's
  snapshot, and when one was claimed and committed by another worker in between, Postgres followed
  it to its new version and re-checked only conditions written against the locked table, here just
  `id`. The second worker re-claimed the task and overwrote its `execution_generation`. The
  rewrite repeats `status = 'pending' AND run_at <= CURRENT_TIMESTAMP AND owner_principal_kind =
  'agent'` in `claimable`, against the row it locks. The channel-assignment `EXISTS` is not
  repeated: an assignment removed just before the lock is indistinguishable from one removed just
  after the claim commits, which no claim can prevent.
- **The loose scan stays recursive; the readable `DISTINCT` form was not written.** On 16.14 it
  planned as `Unique` over an index-only scan of every pending agent row. On the seed that was
  21,118 rows read to find 60 companies. The claim above that PostgreSQL 18's skip scan serves
  `DISTINCT` is wrong for the reason phase 2 recorded: skip scan needs a condition on a later
  index column.
- **`ROW_NUMBER()` stayed inside the `LATERAL`,** as sketched. Its window streams: on the seed
  each slice's index scan stopped after `$1` rows.
- **`due_company` is named `pending_company`,** because it lists companies with any pending agent
  work, due or not. Putting `run_at <= CURRENT_TIMESTAMP` into the enumeration would make each
  descent walk that company's not-yet-due rows. Leaving it out costs a company with nothing due
  one empty descent in its slice.
- **`FOR UPDATE OF task`,** where the old statement said `FOR UPDATE`. The same rows are locked,
  because an unqualified locking clause already skipped CTE references; naming the table says so.
- **The index was widened in place.** `pg_indexes` counts 16 on `background_tasks` before and
  after. No other reader of the old leading column used it; the grep is below and in §5.
- **A racing claim can come back short.** A task another claim holds locked still takes one of
  its company's `$1` places, where the old statement's sort ran on past it. Only claims from
  different machines can race: within a process the loop awaits each claim, and `claim_task` by id
  is called only from tests. The worker reads a short batch as an empty queue until its next poll
  (500 ms) or task-ready wakeup. Not "fixed" by looping, per the note above.
- **The new tests live in `task/claim_tests.rs`**, not `tests.rs`, which is already 7,157 lines.
  `seed_company_and_channel` and `seed_channel_agent` became `pub(super)` for them. Two things in
  that file are deliberate:
  - **Each test gets a database of its own**, through `test_support::own_database`: a unique name,
    migrated, and dropped when the handle falls out of scope, panic or not. Asked for on
    2026-09-12. The claim sweeps every row of the database it runs against, so on the shared test
    database these tests had to hold `UNSCOPED_CLAIM`, backdate their tasks by three centuries to
    outrank the neighbours, and delete their companies on the way out. Even then a panicking test
    left a claimable task for the next test's claim to take, which is exactly what happened on the
    first run: one expected failure cascaded into two unrelated ones. On a database with nothing
    else in it, each test states every row that exists and asserts the exact set of claimed tasks.
  - The race test's paused worker connects with `enable_hashjoin` and `enable_mergejoin` off. On
    a near-empty table the planner builds the `UPDATE`'s hash on `claimable`, which locks the
    whole batch before the first update and closes the window the test holds open. The setting
    orders execution; it does not change what is re-checked.
- **Run against the old statement** (with the new index in place), the three behaviour tests
  passed and the race test failed with "the paused worker re-claimed a task another worker already
  held".
- **A second race test keeps the shape production has** (2026-09-12).
  `concurrent_workers_never_receive_the_same_task_twice` runs eight workers against a three-company
  backlog for fifteen rounds, with no lock and no planner setting, and asserts that no task is ever
  returned to two of them. With the re-check removed it failed on all three runs it was tried
  against, once by 35 duplicates in 135 hand-outs. It cannot prove the window is hit on a given
  run, which is why the deterministic test stays. Note that the status ledger cannot be used for
  this at all: `record_task_status_event` returns early when the status does not change, so a
  second claim of an already-`processing` row writes no event — which is how the pre-existing
  `concurrent_workers_claim_once_and_a_failed_task_is_not_immediately_reclaimed` stayed green
  throughout.
- **The status ledger cannot detect a double claim, and is not where to fix that** (2026-09-12).
  `record_task_status_event` skips any write that leaves `status` unchanged, which is what
  `src/adapters/persistence/AGENTS.md` asks for: notification triggers fire on material state
  changes, not on writes that leave the notified state alone. Recording same-status writes would
  fabricate transitions for idempotent ones, such as a resume that sets `status = 'pending'` on an
  already-pending row. A double claim is a fence violation rather than a transition, so two tests
  guard the class instead:
  - `a_task_with_a_live_lease_is_claimable_by_nobody` — neither the batch claim nor the by-id
    claim takes a task that already carries a live lease, and neither overwrites the execution
    generation that fences the worker running it.
  - `every_statement_that_claims_a_task_requires_it_to_be_pending` — scans the crate's non-test
    sources for `execution_generation = gen_random_uuid()` and fails unless the statement around it
    also requires `status = 'pending'`. It catches a *new* claim path, including one no test of its
    own exercises. Checked by adding an unguarded statement to `task/queue.rs`: the test failed and
    named it by file and line.

  A trigger rejecting `processing → processing` with a changed execution generation would enforce
  this rather than detect it, and no legitimate path does that — only the two claims assign a
  generation, and both require `pending`. It was deliberately not added: it costs a migration and
  another database reset, and the statement is now fixed, tested and scanned. That is the thing to
  reach for if a future claim path cannot be expressed as "pending only".
- **Every test that held `UNSCOPED_CLAIM` now takes a database of its own, and the guard is gone**
  (2026-09-12). Sixty tests across seventeen files held it to serialise unscoped claims and sweeps.
  Isolation is the stronger property, so the static and its rationale were deleted along with the
  workarounds it forced: the foreign-row releases in the task and delivery tests, and the
  century-scale backdating that sorted a test's own row to the front. `Fixture::isolated` in
  `thread/test_support.rs` does the same for the inbound tests that commit through a claim. The
  suite's test time went from 7.1 s to about 17 s, which is what migrating one database per
  isolated test costs. Isolated pools are capped at five connections, because sixty of them in
  parallel would otherwise come within reach of the server's `max_connections`; the one test that
  needs more fan-out opens a pool of its own.
- **Both databases were recreated** with `./scripts/reset-db.sh --all`.
- **Verified:** `cargo test` passed 1,443 lib tests (the baseline was 1,439) and the binary's 5,
  none failed, 22 ignored. `cargo clippy --all-targets -- -D warnings` is clean. `cargo sqlx
  prepare -- --all-targets` wrote no change to `.sqlx/`, as expected for a runtime query. The
  fairness test is unedited.

For the commit message, which the acceptance criteria below ask to carry the grep: readers of the
old leading column, found by grepping non-test `src/` for `'pending'` and `run_at` on 2026-09-11.
`TASK_PRESSURE_SQL`'s `due_now` and the stuck-work census's `queue_overdue` are `COUNT(*) FILTER`
aggregates. `OUTSTANDING_SQL`'s pending arm is one side of an `OR`; its plans, empty and seeded,
are unchanged and never used the index. `board.rs`'s `MIN(run_at) FILTER` aggregates an already
selected chain. `CLAIM_TASK_SQL` and the `notify_agent_instruction` trigger go by primary key. The
`status IN (…)` lists in `attention.rs`, `board.rs` and `operations.rs` cannot use a single-status
partial index.

## Follow-up: fewest running first (2026-09-11)

This is a behaviour change, not a Class B rewrite: it changes which task a claim returns. The owner
chose it on 2026-09-11, after the one-slot observation in the notes above (finding 15). It adds no
index. It is justified by the scheduling it produces, not by plan evidence, so it sits outside the
A/B/C classes this plan uses for performance changes, and none of their gates apply to it.

- **The rule.** A candidate's `company_round` is now its place in its company's queue plus the
  tasks that company has `processing` under a live lease (`in_flight`). A free slot goes to the
  company with the fewest running, and ties go to the oldest waiting task. Each company's own queue
  stays oldest-first. With nothing running, it orders exactly as phase 5 did, which is why every
  earlier claim test, the fairness test included, passes unedited.
- **What it fixes.** With a limit of one, the worker's steady state once every slot is busy, phase
  5's ordering took the oldest due task anywhere. So a company with a deep backlog held every other
  company's newer task until the backlog drained. On the seed, company 1 had 8 tasks running and
  20,000 waiting. A limit-4 claim served companies 1, 60, 59 and 58 under phase 5's statement, and
  60, 59, 58 and 57 under this one.
- **Cost.** `in_flight` is an index scan of `background_tasks_processing_lease_idx` over live
  leases only: one row per busy slot, across every machine. On the seed it read 8 rows. The whole
  statement went from 876 to 879 buffers at limit 1, and from 1,357 to 1,360 at limit 4.
- **What is counted.** Only `processing` rows with `lock_expires_at > CURRENT_TIMESTAMP`. An
  expired lease's worker is gone, and the reaper is about to put its task back. Tasks in
  `pending_approval` or `waiting_for_third_party_reply` hold no worker slot.
- **No slot sits idle.** The round orders candidates and never drops one, so a company alone with
  work still gets every slot.
- **Racing claims can over-allocate by one.** Two claims that start together both see the same
  committed running counts, so both can take a company for idle and each give it a task. The next
  claim counts both.
- **Tests** (`task/claim_tests.rs`): `a_free_slot_goes_to_the_company_with_the_fewest_tasks_running`,
  `an_expired_lease_does_not_count_as_a_running_task`,
  `a_company_alone_with_work_fills_the_whole_batch` and
  `companies_running_the_same_number_of_tasks_are_served_oldest_first`. The last two could not be
  written against the shared test database, where another test's due task is a company with
  nothing running and outranks anything placed second. They became straightforward once each test
  got a database of its own.
- **Verified:** `cargo test` passed 1,450 lib tests (phase 5 left 1,443) and the binary's 5, none
  failed, 22 ignored — that count includes the follow-up's two tests, the realistic race test and
  the two guards above,
  and the run covers the suite-wide move to per-test databases. `cargo clippy --all-targets -- -D
  warnings` is clean, and `cargo sqlx prepare -- --all-targets` again wrote no change to `.sqlx/`.
  A full run leaves no `mail_agents_own_%` database behind.

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
