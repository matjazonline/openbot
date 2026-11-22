---
title: "Agents have no access level, so a hand-off between agents with no person on the thread runs at level 0"
author: Mac02
date: 2026-11-22
branch: plan08_ph1
cost: { minutes: 36, tokens: 232709 }
id: l-76e0f7d8
scope: repo
status: active
src: agent
touches:
  - src/adapters/persistence/department/archive.rs
  - src/adapters/persistence/department_archive_tests.rs
  - src/adapters/persistence/workflow/unit_archive_runs_tests.rs
  - src/application/workflow/message_admission.rs
  - src/adapters/persistence/workflow/schedule_archive_tests.rs
  - src/adapters/persistence/workflow/action_reconciliation_claim_sql_candidate_support.rs
  - plan/08-goals/phases/p02/docs/logs/08G-P02-B2-sweep.md
  - src/adapters/persistence/workflow/knowledge.rs
  - src/domain/entities/access_level.rs
  - src/adapters/persistence/workflow/children.rs
---

## Decided
Nothing was changed or decided. This records how levels and unit scope behave for agent-to-agent work. Read in the code, not run.

- Access level is one company-wide scale and is set on people only. Agents and the system count as no reader.
- A run's ceiling comes from the person who started it, the run-as person for a schedule, or the least-cleared person on the thread. With no person on the thread the audience is empty and falls to level 0.
- A message-started run is refused when the trigger message is labelled above that ceiling.
- A child run inherits its parent's ceiling and copies its sealed unit scopes.
- Knowledge and memory scope follow the desk's unit; an agent's tools follow the units the agent is placed in.

## Tried and ruled out
Nothing was tried.

## Watch out
A thread with only agents on it has no cleared reader, so the receiving run reads public data only, and a hand-off whose message is labelled above level 0 is refused. An agent called from another unit's run reads that run's units, not its own.

In the code:
- `src/adapters/persistence/workflow/knowledge.rs`: `reader`, `thread_audience_on`, `admission_levels_on`
- `src/domain/entities/access_level.rs`: `AccessLevel::lowest` returns `PUBLIC` for no readers
- `src/adapters/persistence/workflow/children.rs`: `create_on`, `insert_child_on`
- `src/adapters/persistence/workflow/run_scopes.rs`
- `department_members_level_on_root` in the init schema
