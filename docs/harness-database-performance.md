# Harness persistence performance

The fixes retain task → run locking, sorted agent admission locks, transactional receipts,
and MCP authorization/revision checks immediately before remote dispatch.

- Admission locks now derive the owner/channel's agent IDs before joining `agents`.
- The configuration guard checks owner and channel relationships separately. Two partial
  task indexes cover unsettled tasks by company/owner and company/channel. Completed history
  does not enter these indexes; stopped, failed, and dead-letter tasks still fence changes.
- Checkpoint persistence compares the locked previous snapshot with the next snapshot and
  sends only changed invocation receipts in one batched statement. Checkpoint-only changes
  issue no receipt writes. The bounded conversation checkpoint still updates atomically.
- Selection reads fetch revision and IDs in one statement without a company row lock.
  Runtime catalog reads accept at most eight explicit IDs, including a single ID for invocation.

## Reproducible query plans

Run against an isolated test database:

```sh
python3 scripts/tests/harness-query-plans.py postgres://mac03@localhost/mail_agents_test
```

The script extracts current queries and indexes from the squashed baseline and historical queries
from `scripts/tests/fixtures/harness-fencing-before.sql`, builds temporary tables,
analyzes them, emits `EXPLAIN (ANALYZE, BUFFERS, TIMING OFF)` and table/index statistics, and
rolls back. It does not modify application tables. The fixture has 100 companies, 50,000 agents,
50,000 principals/assignments, and 500,000 tasks, approximately 90% completed. The target agent
has completed history but no unsettled tasks, exercising a negative guard lookup.

Measured on PostgreSQL 16.14, arm64 macOS, 2026-09-10:

| Query | Before | After | Before/after executor local buffer hits + reads |
| --- | ---: | ---: | ---: |
| Admission locks | 16.697 ms | 0.045 ms | 50,151 → 11 |
| Configuration guard | 116.035 ms | 0.042 ms | 92,330 → 14 |

Before, the admission query walked the agent primary-key index and rejected 49,999 agents;
the guard visited 49,999 unrelated unsettled tasks and scanned the principals table. After,
admission uses the principal/channel indexes followed by a primary-key lookup; the guard uses
nested-loop relationship lookups and the two partial task indexes, with zero heap fetches
for the missing unsettled tasks. Each new index was about 2.4 MiB for approximately 50,000
entries; the task heap was 56 MiB and each existing full-history composite index about 50 MiB.

These synthetic measurements demonstrate the access-path change, not a production latency
guarantee. The indexes add write/storage cost to unsettled tasks and are included in the fresh
database baseline. Check plans against deployment-sized history before rollout.

## Regression coverage

Database-backed tests exercise both orders of competing harness changes/task admission,
owner-only tasks, shared library agents, and response-contract fencing. Receipt tests compare
PostgreSQL row versions to prove unchanged receipts are not rewritten. Concurrent selection
readers must finish while a company writer holds its lock and return a coherent committed
revision/set. MCP integration runs include an invalid unselected schema, so accidentally
reintroducing a full-catalog runtime read fails the test.
