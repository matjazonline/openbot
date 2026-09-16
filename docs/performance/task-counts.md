# Live open-task counts

Measured on 2026-09-15, PostgreSQL 18.6 / Apple arm64, using
`scripts/tests/task-counts-query-plan.py`. The script extracts the actual runtime query from
`task/counts.rs` and creates temporary copies of the migrated tables, constraints, and indexes.
It writes no application data. Run it against a migrated database:

```sh
python3 scripts/tests/task-counts-query-plan.py postgres://mac03@localhost:5432/mail_agents_test
```

## Query plan

The [complete EXPLAIN ANALYZE / BUFFERS output and statistics](task-counts-query-plan.txt)
cover 416,000 tasks: four tenants each have 100,000 retained terminal tasks and 4,000 open tasks,
spread across 32 channels. Ownership is approximately 25% unassigned, 25% human, and 50% agent.
The principal table has 6,500 rows across 100 tenants (64 agents and one person per tenant).
The task heap is 82 MiB, and the principal heap is 744 KiB. Both were analyzed before measurement.

| Visible channels | Matching tasks | Task heap blocks visited | Execution time |
| --- | ---: | ---: | ---: |
| 32 | 4,000 | 111 | 5.391 ms |
| 8 | 1,000 | 111 | 1.462 ms |

Both plans use a bitmap index scan with **company and open-status conditions inside the index
condition**. They visit 4,000 open task entries rather than scanning the tenant's 100,000 terminal
rows. The 8-channel plan filters out 3,000 open tasks at the heap. This confirms history pruning;
channel filtering is not index pruning with this index. The join hashes only the tenant's 65
principals, using the tenant-prefixed principal index, rather than scanning all 6,500 principals.

The temporary clone calls the task index `background_tasks_company_id_status_created_at_id_idx`;
its definition is the existing production `background_tasks_company_status_created_idx`.
No additional index is warranted by this result. These temporary-table measurements use local
buffers and synthetic data; they are evidence about access paths, not production latency targets.
Revisit channel selectivity if a tenant develops a substantially larger open backlog.

## Full-pass measurement and cadence

`task_counts_refresh_duration_seconds`, labelled only by `outcome=changed|unchanged|error`, measures
the complete stream pass: management authorization, channel visibility, aggregate read, and HTML
rendering. Connection admission authorization is separate. The in-memory monitor retains fractional
values, count, sum, and cumulative fixed buckets; tracing records the value in the metric's units.
The controlled-reader stream test verifies that the full-pass metric records both snapshots.

The initial snapshot is immediate. Every subsequent pass starts at least five seconds after the
previous pass finishes, even when the fragment is unchanged. A 60-second skipped-tick recheck shares
that gate with wakes and lag. All scheduling is per connection; reconnects each read a fresh snapshot.

## Verification

The Rust tests cover status mapping, visible-channel filtering, ownership transfer, task-only wake
filtering, scheduler churn/lag/slow reads, stale initial reads, snapshot suppression, reconnect,
permission revocation, and recovery without a notification. The browser protocol suite runs the
shipped HTMX and SSE extension against a real HTTP/SSE server in Chrome/Chromium. It covers both
workspaces, both arrival orders, OOB list replacement, competing reads, missing-key clearing,
reconnect, and navigation cleanup. CI runs it with `npm run test:task-counts`.

The live server check used a separate disposable database on port 3001. Both workspaces updated
through pending → processing → completed; switching agents reapplied the current snapshot.
Chrome reported no page errors. Light and dark themes were checked on screenshots and computed
badge colors. Observed full-pass values were 0.0047–0.0139 seconds; successive changes were
recorded five seconds after the preceding completed pass. Sidebar names, addresses, and badges
wrap or truncate within the existing pane width.

Local verification passed: 1,712 library tests and 5 binary tests (22 existing ignored tests),
the same library suite at the stock 2 MiB stack under network isolation, four real-browser protocol
tests, eight existing JavaScript tests, transport-boundary checks, formatting, offline compilation,
Clippy, migrations, and SQLx metadata generation. The runtime query adds no `.sqlx` entries.
