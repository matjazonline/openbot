# Phase 6 — Principal deletion, and the handoff to the evidence gate

Two different kinds of work. The first is a real defect on an interactive path and ships here. The
rest is a decision record: three index candidates that this plan deliberately does **not** ship,
written into `plan/db_improve/05-deferred-until-traffic.md` with enough of the case made that
whoever has production numbers can close them in an afternoon.

---

## 6.1 Deleting a principal is O(every task they ever owned), before the foreign keys are even checked

Nothing in `src/` runs `DELETE FROM principals`. Principals go away by cascade:

- **Removing a company member** — `DELETE FROM company_members`
  (`src/adapters/persistence/company_invite.rs:369`) → `principals_company_user_fk … ON DELETE
  CASCADE`.
- **Deleting an agent** — `DELETE FROM agents WHERE id = $1` (`src/adapters/persistence/agent.rs:715`)
  → `principals_company_agent_fk … ON DELETE CASCADE`.

Both are interactive, user-initiated operations. What they set off, in order:

**First, a `BEFORE DELETE` trigger walks every task the principal ever owned, one row at a time.**
`release_tasks_for_removed_principal()` opens

```sql
SELECT task.*, OLD.display_label
  FROM background_tasks AS task
 WHERE task.company_id = OLD.company_id AND task.owner_principal_id = OLD.id
   FOR UPDATE
```

and loops. There is **no status predicate**, so this is every task, including ones completed months
ago. Per row it issues an `UPDATE background_tasks`, conditionally an `UPDATE task_attempts`, and an
`INSERT INTO task_ownership_events`. `background_tasks` carries eight row triggers and
`task_ownership_events` carries three, so a principal with 5,000 lifetime tasks generates on the
order of fifteen thousand statements and their trigger cascades inside one transaction, holding
`FOR UPDATE` on all of them.

`background_tasks_unsettled_owner_idx (company_id, owner_principal_id) WHERE status IN (…unsettled…)`
looks like it covers this and does not: the trigger's `SELECT` carries no status predicate, so the
planner cannot prove the partial index applies. The same query shape was captured on 2026-09-10:

```
Index Scan using background_tasks_company_updated_idx on background_tasks
  Index Cond: (company_id = '…'::uuid)
  Filter: (owner_principal_id = '…'::uuid) AND (owner_principal_kind = '…'::text)
```

— a scan of the whole tenant's tasks, filtered.

**Then twenty-one foreign-key checks run, none of them indexed.** After the trigger, Postgres
enforces every inbound reference to the deleted principal row. Twenty-one of them have no index able
to lead on their principal column: `channels.preferred_reviewer_principal_id`,
`delegation_control_commands.actor_principal_id`, `human_approvals.approver_principal_id`,
`internal_note_tombstones.actor_principal_id`, `internal_notes.author_principal_id`, all three of
`manual_handoffs`'s (`responsible`, `created_by`, `resolved_by`),
`notification_events.actor_principal_id`, `notifications.recipient_principal_id`,
`response_draft_publications.published_by_principal_id`, all four of `response_drafts`'s
(`author`, `created_by`, `updated_by`, `reviewer`), all three of `response_reviews`'s (`reviewer`,
`decided_by`, `notification_actor`), `task_agent_instructions.requested_by_principal_id`,
`task_approval_waits.owner_principal_id`, `task_harness_runs.owner_principal_id`, and
`task_outreaches.created_by_principal_id`. Each is a full scan of that table.

`response_drafts.reviewer_principal_id` is the instructive one. It *has* an index —
`response_drafts_reviewer_pending_idx (company_id, reviewer_principal_id, created_at, id) WHERE
status = 'pending_review'` — and it is useless here for the same reason as above: a referential
check carries no status predicate, so a partial index cannot serve it. **Partial indexes never
satisfy foreign-key enforcement.** Write that on the wall; it is the single most load-bearing fact
in this phase and it is not obvious from reading the migration.

### What to change here, and what needs an owner

**Ships in this phase — make the trigger set-based.** The row-at-a-time loop has no reason to exist.
The three statements it runs per row are all expressible as one statement each over the same row
set:

```sql
WITH released AS (
    UPDATE background_tasks SET … WHERE company_id = OLD.company_id AND owner_principal_id = OLD.id
    RETURNING id, company_id, ownership_version, status, execution_generation
)
```
then one `UPDATE task_attempts … FROM released WHERE … released.status = 'processing'` and one
`INSERT INTO task_ownership_events … SELECT … FROM released`. The triggers still fire per row —
that is Postgres, not the statement — but the plpgsql loop, the per-row planning and the `FOR
UPDATE` cursor all go away, and the row set is computed once.

Two details that will bite. The `ownership_version + 1` must be computed from the value read in the
same statement, which `RETURNING` gives you — do not re-read the row. And `task_ownership_events`
has `UNIQUE (task_id, sequence)`; the set form must produce the same `sequence` the loop did, which
is `ownership_version + 1` per task, not a running counter.

**Needs an owner — should the release cover completed tasks at all?** Adding
`AND status <> 'completed'` (or restricting to the unsettled set the partial index already names)
would make `background_tasks_unsettled_owner_idx` usable and change the cost from lifetime tasks to
open tasks. But it changes what the system records: a completed task would keep pointing at a
principal row that no longer exists, which the foreign key forbids — so it is not a free predicate,
it implies a decision about whether historical ownership is retained (and therefore whether that FK
becomes `ON DELETE SET NULL` with a denormalised label). `task_ownership_events` already preserves
the history independently, which is an argument that the live column need not. **Do not decide this
in review.** Write the question down, take it to whoever owns the ownership model, and land the
set-based rewrite meanwhile — it helps under either answer.

**Deferred — the twenty-one indexes.** Do not add them. Twenty-one indexes on tables that take
writes on every note, draft, review, notification and outreach is a large permanent cost for a path
that runs when somebody removes a teammate. The right move is to measure the path once there is
data:

1. Land the set-based trigger.
2. Time the two delete paths against a database with a realistic principal history — which is
   exactly what the skewed seeder in `05-deferred-until-traffic.md` is for.
3. Add indexes **only** for the tables the timing shows dominate, and only after capturing the plan.

Record that method, and the twenty-one-name list above, in `05-deferred-until-traffic.md` as a new
section. The list is the valuable part: it took a catalog query to produce and nobody should have to
write that query twice.

---

## 6.2 `manual_handoffs` has no query-supporting index

`manual_handoffs` carries exactly two indexes, both identity keys: `manual_handoffs_pkey (id)` and
`manual_handoffs_company_id_id_key (company_id, id)`. It is read by
`company_id + channel_id = ANY(…) + status = 'open'` in the attention feed
(`src/adapters/persistence/attention.rs:87`, handoff branch), so every read walks every handoff row
the tenant has ever created through the `(company_id, id)` index.

**The case for `(company_id, channel_id, status)`, made in advance so it can be decided quickly.**
The usual objection to a new index — write amplification — is weak here: handoffs are created when a
human escalation happens and updated when it resolves, which is a human-rate event, not a queue-rate
one. That makes the write cost close to nothing and the read benefit proportional to accumulated
handoff history. It is the strongest of the three candidates in this phase.

**It is still Class C and still does not ship here**, because "handoffs are rare" is an assumption
about a product that has no users yet, and the gate exists precisely to stop assumptions like that
from becoming permanent schema. Record the candidate definition, the read shape, and the write-rate
argument in `05-deferred-until-traffic.md`, and note that a partial `WHERE status = 'open'` variant
is probably better than the plain three-column form — the feed only ever reads open handoffs, and
`notification_from_attention_source` is the only other consumer worth checking.

---

## 6.3 `task_attempts (started_at)` — update the record, do not ship the index

This candidate has been at the gate since 2026-09-01
(`docs/query-evidence/2026-09-01-thread-index-and-task-attempts.md`, and
`05-deferred-until-traffic.md` §4). The audit found nothing that changes its status, but Phase 3
changes its *urgency*, and that belongs in the record:

- Before Phase 3, three statements scanned `task_attempts` on every five-second tick of every
  connected dashboard tab (`ATTEMPT_STATS_SQL:242`, `LATENCY_BODY:161`, `RETRY_RATE_BODY:263`).
- After Phase 3's snapshot cache, they run once per tick for the whole fleet of tabs.

That is the outcome `05-deferred-until-traffic.md` §2 predicted when it said to "evaluate the cache
first — it scales with operator count rather than data size and may make the index unnecessary".
Append that result to §4: the read pressure is now proportional to tick rate rather than to viewer
count, so the index's case is weaker than it was, and the remaining question is only whether a single
scan per tick of an unpruned, ever-growing table is acceptable at real cardinality.

**Also record the retention gap.** No statement in `src/` deletes from `task_attempts`. Rows leave
only by cascade from `background_tasks`, and `dashboard.rs:185` records in its own comment that
`background_tasks` "is never pruned" — the queue-depth reconstruction depends on that. So the table
only grows, and every dashboard window query pays for that growth regardless of the window it asks
for. A retention sweep modelled on `delete_retained` (`src/adapters/persistence/inbound_event/mod.rs:515`
— batched, ordered by `(processed_at, id)`, backed by a matching partial index) may be a better
answer than an index, and the two interact: a table with retention needs a smaller index, and a
table with an index prunes faster. Neither should be decided without the other on the table.

---

## Deliverables

This phase produces one code change and one document change.

**Code:** `release_tasks_for_removed_principal()` in
`migrations/20260817000000_init_schema.sql` becomes set-based. Both local databases recreated with
`./scripts/reset-db.sh --all`.

**Document:** a new section in `plan/db_improve/05-deferred-until-traffic.md` carrying (a) the
twenty-one-name unindexed-foreign-key list and the "partial indexes never satisfy foreign-key
enforcement" fact, with the captured plan; (b) the `manual_handoffs` candidate with its write-rate
argument; (c) the updated `task_attempts` §4 entry and the retention gap; and (d) the open question
about whether completed tasks should retain a deleted principal's id.

## Tests

- Removing a company member who owns tasks in every status releases all of them, writes one
  `task_ownership_events` row per task with the correct `sequence`, and fails the `task_attempts`
  rows of exactly the tasks that were `processing`. This test must exist **before** the set-based
  rewrite, so it can be run against both implementations.
- Deleting an agent takes the same path and produces the same result.
- A principal with zero owned tasks deletes cleanly — the set form must not fail on an empty
  `RETURNING`.
- Scope every assertion to the ids the test created; the suite shares one database.

## Acceptance criteria

- `release_tasks_for_removed_principal()` contains no `FOR … LOOP`.
- The ownership-event `sequence` values are identical to those the loop produced, proven by a test
  written against the current implementation first.
- No index is added in this phase.
- `05-deferred-until-traffic.md` carries all four records, and each names the evidence that would
  close it.
- `./scripts/reset-db.sh --all` succeeds; `cargo test` and
  `cargo clippy --all-targets -- -D warnings` green.
