---
title: Native and workflow peer references need a shared teardown boundary
author: Mac02
date: 2026-11-16
branch: feature/plan03_04
id: l-3837ec57
scope: repo
status: active
src: agent
touches:
  - ".sqlx/query-3aae25001a1a4325144df6ee4a3417b2b7d664b7bc00ff9cf0f2673547941593.json"
  - ".sqlx/query-5579ba07b5d4a75e98a5e36c87a3dcb71755bab888c43218d57efe9c5d4176ab.json"
  - ".sqlx/query-5b347734a5d6837b09f05ccf76fc0535323956a094679e3fdbbb58ea9651885b.json"
  - ".sqlx/query-7f0c6f79e79b96566a4c77f21c04db642505801108467c9890eae68cb6970eab.json"
  - ".sqlx/query-8127ec1c51175ba5c06188bca73b59906729cb3ff90636a7352454f3413d6d92.json"
  - migrations/20260817000000_init_schema.sql
  - src/adapters/http/pages/attention.rs
  - src/adapters/http/pages/company_departments.rs
  - src/adapters/http/pages/dashboard.rs
  - src/adapters/http/pages/department_ui_tests.rs
---

## Decided
Keep native/workflow owning cascades intact and defer the bounded NO ACTION peer references across them until transaction completion. Every committed tenant, origin and identity pin stays enforced. Exercise real populated workflow-native and direct-native histories through company and owned-account deletion; orphan inserts alone do not prove the lifecycle works.

## Tried and ruled out
Ordering channels, departments and principals is necessary for the root tree but does not order every native and workflow cascade. Keeping local-call native peers immediate after fixing only the dispatch edge still failed on the real profile FK. Treating all four local-call native peers as one cross-owner group then passed both actual teardown paths. Do not infer nested RI queue timing from a single obvious parent relationship.

## Watch out
After deferring a FK, retain inherited independent-FK regressions at a forced-immediate or commit boundary. Exact named-FK rollback probes distinguish overlapping peers. The local call's operation_id must equal invocation_id; its immediate intent owner and frozen guards prevent arbitrary foreign-operation-id substitution. Do not weaken those guards or claim an isolated tenant proof when a stronger owner guard preempts it. Foreign profile/session/machine tuples and precise FK definitions provide their stated evidence, with that operation-id limitation explicit.

In the code: migrations/20260817000000_init_schema.sql workflow_local_agent_calls and native session/operation peers; src/adapters/persistence/root_dispatch_fk_test_support.rs; workflow/local_agent_continuation_tests.rs; Plan04 p01 logs/D1-r7.md and B1-root-fifth-tests.txt (44 passed on fresh schema5).
