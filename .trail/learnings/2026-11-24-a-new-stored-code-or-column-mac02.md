---
title: A new stored code or column needs a sweep of its nearest existing sibling everywhere it is stored
author: Mac02
date: 2026-11-24
branch: plan08_ph1
cost: { minutes: 1183, tokens: 7655575 }
id: l-98d0868b
scope: repo
status: active
src: agent
touches:
  - docs/departments.md
  - migrations/20260817000000_init_schema.sql
  - plan/08-goals/03-goal-units-and-tracking.md
  - plan/08-goals/COORDINATION.md
  - plan/08-goals/README.md
  - plan/08-goals/phases/p03/docs/HANDOFF.md
  - plan/08-goals/phases/p03/docs/REVIEW.md
  - plan/08-goals/phases/p03/docs/TASK.md
  - plan/08-goals/phases/p03/docs/agent-review-findings.md
  - plan/08-goals/phases/p03/docs/agent-review-notes.md
---

## Decided
When a change adds a new stored value next to an existing one (a refusal code beside `budget.unit_exhausted`, a column on `model_usage_calls`), search the repo for the nearest existing sibling literal or column and visit every hit before review. CHECK constraints, archive tables that must mirror a live row, and fixed-list tests hold copies the compiler cannot see.

## Tried and ruled out
Following call edges from the new code only. In Plan08 p03 this missed three times:
- the schedule refusal CHECK lacked `budget.goal_exhausted` (08G-P03-B2-F1);
- `model_usage_history` lacked `goal_unit_id`, so retention failed with "usage archive must exactly match its live owner";
- an earlier gate missed `date_trunc` on the new `total` period (08G-P03-D2-F1).

## Watch out
- Postgres `LEAST`/`GREATEST` ignore NULL, so `LEAST(sum(x), max)` over no rows returns `max`, not NULL. Write `LEAST(COALESCE(sum(x), 0), max)`.
- A trigger that fills a column only when it is NULL lets the writer choose the value. Assign it unconditionally when it must come from the row's owner.

In the code: `migrations/20260817000000_init_schema.sql` (`schedule_runs_refusal_check`, `model_usage_history`, `attribute_model_usage`), `src/adapters/persistence/workflow/storage_retention.rs::archive_usage`, `src/adapters/persistence/department/goal_rollup.rs`.
