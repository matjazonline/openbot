---
title: Recheck consumed knowledge owners before cached model continuation
author: Mac02
date: 2026-11-17
branch: feature/plan05-scopes
id: l-0bea6cf9
scope: repo
status: active
src: agent
touches:
  - migrations/20260817000000_init_schema.sql
  - plan/05-scopes/COORDINATION.md
  - scripts/tests/knowledge-browser.mjs
  - src/adapters/embedding/retrieval_probe.rs
  - src/adapters/http/pages/company_departments.rs
  - src/adapters/http/pages/department_ui_tests.rs
  - src/adapters/http/pages/knowledge.rs
  - src/adapters/http/pages/knowledge_tests.rs
  - src/adapters/http/routes/knowledge/mod.rs
  - src/adapters/http/routes/knowledge/tests.rs
---

## Decided
Check the selected agent's consumed knowledge unit set in the existing current-authority prefix before cached checkpoints admit another model request. Keep historical source items immutable, and keep model usage settlement on its existing independent accounting path.

## Tried and ruled out
Tool-port replay checks and a missing-scope-row continuation test passed, but they did not cover a legitimate department deletion that leaves historical scope rows intact. After a report document was moved and its empty department deleted, the actual resumed runtime sent cached protected facts to the scripted provider. Its strict zero-exchange fixture failed with an unexpected extra request. Adding the consumed-unit check to current preparation authority made that real lifecycle regression pass without rewriting the proof or relaxing the provider assertion.

## Watch out
A completed tool callback is not necessarily called again when a checkpoint resumes. Missing rows and retained rows whose owner disappeared are different cases. Reuse the sealed per-actor unit loader, preserve the exact no-data unavailable exception, and do not gate usage settlement on a revocation check.

In the code: src/adapters/persistence/workflow/agent_preparation.rs current_on; knowledge.rs require_consumed_units_on; run_scopes.rs knowledge_units_on; knowledge_scope_runtime_tests.rs knowledge_scope_search_cached_model_continuation_refuses_deleted_report_owner. Evidence: plan/05-scopes/phases/p03/docs/logs/B1-deleted-owner-continuation-red-r1.txt (unexpected provider request) and B1-knowledge-scopes-r5.txt (that regression passes; unrelated streamed-body test proxy still failed).
