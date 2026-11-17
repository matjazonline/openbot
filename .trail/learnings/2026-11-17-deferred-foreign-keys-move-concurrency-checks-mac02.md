---
title: Deferred foreign keys move concurrency checks to commit
author: Mac02
date: 2026-11-17
branch: feature/plan03_04
id: l-7b3a6b63
scope: repo
status: active
src: agent
touches:
  - ".gitattributes"
  - docs/authentication-operations.md
  - plan/.archive/memory-service/05-openbot-integration.md
  - plan/04-root-unit/02-access-modes.md
  - plan/04-root-unit/COORDINATION.md
  - plan/04-root-unit/README.md
  - src/adapters/persistence/account_identity_tests.rs
  - src/adapters/persistence/agent_publication/tests/publication.rs
  - src/adapters/persistence/agent_retire_tests.rs
  - src/adapters/persistence/company/member_retirement.rs
---

## Decided
When a referenced-row race relies on a foreign-key lock, exercise the actual constraint check before claiming that insertion has acquired the lock. A deferred foreign key checks at COMMIT. In a test that must retain the checked writer while observing a real competing delete, SET CONSTRAINTS ... IMMEDIATE runs that same check while keeping the transaction open. Also test the reverse order: an insert after deletion must fail at its deferred commit and leave no dangling row.

## Tried and ruled out
The old agent retirement test inserted a principal and waited for its concurrent agent deletion to block. After tenant teardown required the peer foreign key to be deferred, no backend blocked: insertion no longer performed that check. Waiting longer would not establish the intended race. Restoring immediate enforcement in production would undo the teardown design.

## Watch out
Deferring an integrity constraint changes both error timing and lock timing. Keep the competing production deletion and the failed late COMMIT assertions; a sequential missing-parent check alone does not prove the race. Production constraints and stack/timeout budgets stay unchanged.

In the code: src/adapters/persistence/agent_retire_tests.rs, agent_retire_races_agent_principal_upsert; migrations/20260817000000_init_schema.sql, principals_company_agent_fk. Evidence: plan/04-root-unit/phases/p03/docs/logs/B1-more-failure-diagnostics.txt and B1-correction-focused.txt.
