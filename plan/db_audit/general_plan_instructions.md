# Database audit follow-up — shared instructions

Read this before any phase file. Each phase lives in `plan/db_audit/phase{n}.md` and assumes
everything here.

| Phase | File | Goal |
|---|---|---|
| 1 | [`phase1.md`](phase1.md) | Schema hygiene and tenant-scope doctrine — a dropped duplicate index, three unscoped predicates, one shared helper |
| 2 | [`phase2.md`](phase2.md) | Write-path and retention shapes — set-based inserts, per-machine pruning |
| 3 | [`phase3.md`](phase3.md) | Dashboard: kill the generic-plan scope hazard, cache the snapshot across tabs |
| 4 | [`phase4.md`](phase4.md) | Attention feed: push the responsibility filter into the union branches |
| 5 | [`phase5.md`](phase5.md) | `claim_pending_tasks`: make claim cost proportional to the batch, not the backlog |
| 6 | [`phase6.md`](phase6.md) | Foreign-key index gaps on `principals`, and the handoff to the evidence gate |

Each phase compiles, tests green, and is landable on its own. They are ordered by risk, not by
value: phases 1–2 are mechanical, 3–4 change query shape without changing results, 5 changes the
scheduling behaviour of the task worker and needs the most care, 6 is mostly a decision record.

---

## Where these findings came from

A schema and query audit on 2026-09-10. Method, so it can be re-run rather than re-argued:

1. The squashed init migration was applied to a scratch database
   (`migrations/20260817000000_init_schema.sql` → `mail_agents_schema_audit`), because the live
   development database had drifted — 79 tables against the migration's 76, and it was missing
   `background_tasks_unsettled_owner_idx` and `background_tasks_unsettled_channel_idx` entirely.
   **Audit the migration, never the local database.** Any catalog query below that disagrees with
   the migration is measuring drift.
2. `pg_catalog` was queried for foreign keys with no index able to lead on any of their non-tenant
   columns, for byte-identical index definitions, and for prefix-redundant indexes.
3. Every SQL string literal in non-test `src/` was extracted (854 statements) and swept for
   unbounded reads, `OFFSET`, optional-filter predicates, correlated subqueries and per-row loops.
4. Plan *shapes* were captured with `EXPLAIN` against the empty scratch database.

**Point 4 is the load-bearing limitation of this entire plan.** An `EXPLAIN` against an empty
database shows how a query is *structured* — whether a `LIMIT` can terminate a scan early, whether
a predicate reaches an index at all, whether a partial index is eligible. It says nothing about how
long anything takes. Every claim in these phase files is a structural claim. None of them is a
measurement.

## The evidence gate

`plan/db_improve/05-deferred-until-traffic.md` sets the rule this project already works to:

> A seeded plan is enough to rule a change *out*, never enough to rule one *in*.

and

> No change from this file lands without a captured plan justifying it.

That gate exists because the project is not in production, `pg_stat_statements` is accruing against
near-empty tables, and the skewed seeder and workload runner it specifies do not exist yet. Paying a
permanent write cost for a speculative read win is the failure mode it is guarding against.

**This plan does not relitigate that gate, and it does not smuggle indexes past it.** Instead every
finding is sorted into one of three classes, and the class decides the phase:

- **Class A — strictly reducing or correctness.** Removing a duplicate index, adding a tenant
  predicate that is already implied, replacing N statements with one. These cannot regress a plan,
  so no cardinality evidence is required. Phases 1 and 2.
- **Class B — shape change, same result set.** Rewriting a query so the database does less work for
  the same answer, adding no index and changing no contract. Justified by the plan shape alone,
  because the improvement is structural. Phases 3, 4 and 5.
- **Class C — new index or new contract.** Gate-bound. Phase 6 records the case, the write-rate
  argument, and the exact candidate definition into `05-deferred-until-traffic.md`, and ships
  nothing.

If a phase tempts you to move an item from C to B, the answer is no. Write down what evidence would
settle it instead.

## Findings map

| # | Finding | Class | Phase |
|---|---|---|---|
| 1 | `claim_pending_tasks` cost is O(pending backlog), not O(limit) | B | 5 |
| 2 | `principals` has 21 inbound foreign keys, none indexed | C (mostly) | 6 |
| 3 | `task_attempts` has no `started_at` index; dashboard scans it 3× per tick per tab | C, mitigated by B | 3, 6 |
| 4 | `manual_handoffs` has no index beyond its two identity keys | C | 6 |
| 5 | Attention feed cannot use its own `LIMIT`; `my_work` filters after the union | B | 4 |
| 6 | Eight unbounded dashboard aggregates per tick; `($1 IS NULL OR …)` flips to a global scan under a generic plan | A + B | 3 |
| 7 | `human_approvals_expiry_due` is byte-identical to `human_approvals_pending_expiry_idx` | A | 1 |
| 8 | `runtime_metric_samples` retention delete cannot use its primary key | A | 2 |
| 9 | `fetch_thread_tasks` reads `background_tasks` with no `company_id` and no bound | A | 1 |
| 10 | Attention feed's delegation branch joins and anti-joins without `company_id` | A | 1 |
| 11 | Identical outreach-tally SQL in three places | A | 1 |
| 12 | Row-at-a-time statements for caller-supplied collections | A | 2 |
| 13 | `release_tasks_for_removed_principal()` walks every task a principal ever owned, one row at a time, on an interactive delete | A | 6 |
| 14 | `claim_pending_tasks` could re-claim a task another worker claimed and committed after its snapshot (found during phase 5) | A | 5 |
| 15 | Under load the worker claims one slot at a time, so the per-batch company round-robin degenerates to first-in-first-out across companies (found during phase 5) | Behaviour change, owner-approved | 5, follow-up |

## Understood, no change proposed

Two things look like findings and are not. Recording them here so the next audit does not spend a
day rediscovering them.

**`lock_task_agent_harnesses` takes `FOR SHARE` on agents on every task status transition.**
The trigger (`migrations/20260817000000_init_schema.sql`, function body reachable via
`pg_get_functiondef('lock_task_agent_harnesses'::regproc)`) locks the owning agent and every agent
assigned to the task's channel whenever `status`, `owner_principal_id` or `channel_id` changes to a
live value. It fires per row, so a batch claim of *N* tasks acquires those shared locks *N* times,
and any `UPDATE agents` serialises against live task churn. This is the intended interlock — it is
what makes `guard_active_agent_harness_change` meaningful — and it is invisible from the Rust side,
which is the only reason it is written down. Phase 5 must not change it, and must not let a rewrite
of the claim query quietly move it off the hot path.

**`background_tasks` carries eight row triggers, and `threads` and `messages` each carry five
unique indexes.** The trigger count is the event-sourcing design (`record_task_status_event` writes
a `task_status_events` row per transition); the index count is the cost of the composite tenant
foreign keys that `src/adapters/persistence/AGENTS.md` mandates. Both are deliberate. Neither is
free, and both belong in the write-rate arithmetic whenever Phase 6's candidates are weighed — an
index added to `background_tasks` is maintained on every claim, lease renewal and completion.

## Rules every phase inherits

**Schema changes edit the squashed migration in place.** There are no additive migrations in this
repo. `migrations/20260817000000_init_schema.sql` is edited directly, and both local databases are
recreated with `./scripts/reset-db.sh --all`. A phase that changes the schema and does not say
"recreate both databases" is incomplete.

**Regenerate the sqlx cache after touching any SQL**, per `src/AGENTS.md`:

```sh
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" \
  cargo sqlx prepare -- --all-targets
```

Note that only 25 statements are in `.sqlx/` because the overwhelming majority of this codebase uses
runtime `sqlx::query(` rather than the checked macros — 486 `sqlx::query(`, 243
`sqlx::query_scalar(`, 193 `sqlx::query_as::<`, against 25 macro call sites. **A typo in a rewritten
query in phases 3–5 will not be caught by the compiler.** Every rewritten statement needs a test
that actually executes it.

**Tenant identifiers stay in the predicate even when derivable.** `src/adapters/persistence/AGENTS.md`
is explicit: if a relationship is tenant-scoped, the tenant identifier participates. Phases 1 and 4
are applications of that existing rule, not new policy.

**Database tests share one database.** Whole-table count assertions are racy under the parallel test
runner. Assert on rows you created, scoped by the ids you created them with.

## Acceptance criteria for the plan as a whole

- Every finding in the map above is either implemented, or explicitly closed in
  `plan/db_improve/05-deferred-until-traffic.md` with the evidence that would reopen it.
- No index is added by phases 1–5. Phase 6 adds indexes only where a delete path is proven to
  scan and the write rate of the indexed table is argued in writing.
- No phase's justification rests on a timing number taken from an empty or synthetically seeded
  database. Shape claims are labelled as shape claims.
- `cargo test` and `cargo clippy --all-targets -- -D warnings` are green at the end of each phase,
  not only at the end of the plan.
