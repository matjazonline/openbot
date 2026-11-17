---
title: Compile shipped workflow samples in focused checks
author: Mac02
date: 2026-11-16
branch: plan08_ph1
id: l-63f31d65
scope: repo
status: active
src: agent
touches:
  - docs/departments.md
  - docs/workflow-language-v1.md
  - migrations/20260817000000_init_schema.sql
  - src/adapters/http/pages/attention.rs
  - src/adapters/http/pages/company_departments.rs
  - src/adapters/http/pages/dashboard.rs
  - src/adapters/http/pages/department_ui_tests.rs
  - src/adapters/http/pages/workflow_authoring/bindings.rs
  - src/adapters/http/pages/workflow_authoring/reviewer_picker.rs
  - src/adapters/http/pages/workflow_runs/provenance.rs
---

## Decided
When adding a registered workflow sample, include the existing all-fixture compile/freeze, standard-input and storage round-trip tests in its focused proof before whole-batch review.

## Tried and ruled out
The sponsor feature's scripted Deployment built its human step from the registry example, so it inherited the required literal data_schema. The manually authored shipped YAML omitted that binding. Its sponsor runtime and rendering tests passed, while the later full suite failed three shared fixture tests with registry.literal at /steps/review/with/data_schema. Running the existing fixture matrix after adding explicit data_schema and feedback_required passed all 16 selected checks.

## Watch out
A generated equivalent procedure proves the runtime behavior it executes, but it does not prove the published sample compiles. Keep the actual sample in Fixture::ALL and exercise that supplier through the compiler and frozen storage reader. Architecture or source-review PASS does not replace the pending sample-publication assertion.

In the code:
- src/application/workflow/fixtures/role-sponsor-approval.yaml
- cargo test --locked --offline --lib -- workflow_fixtures_ workflow_storage_restores_all_frozen_fixtures_and_child_identity
- plan/08-goals/phases/p01/docs/logs/B1-full-core-failures.txt and B1-R3-fixtures.txt
