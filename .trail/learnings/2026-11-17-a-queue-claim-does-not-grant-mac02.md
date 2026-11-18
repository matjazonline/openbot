---
title: A queue claim does not grant conversation access
author: Mac02
date: 2026-11-17
branch: plan06
id: l-1944bdfc
scope: repo
status: active
src: agent
touches:
  - migrations/20260817000000_init_schema.sql
  - src/adapters/http/routes/department_tests.rs
  - src/adapters/http/routes/root_department_route_tests.rs
  - src/adapters/http/routes/workflows/ui/authoring/channel_response_tests.rs
  - src/adapters/persistence/agent_publication/mod.rs
  - src/adapters/persistence/department.rs
  - src/adapters/persistence/department/lifecycle.rs
  - src/adapters/persistence/department/root.rs
  - src/adapters/persistence/department_desks.rs
  - src/adapters/persistence/department_lead_desk_tests.rs
---

## Decided
Only an actual Message claim can use the automatic-message admission path without the human run-as person's desk View. A Schedule claim retains the normal human View check at both application preflight and the transactional persistence boundary. A later model guard cannot repair an admission that already created a run or queued a static effect.

## Tried and ruled out
Checking whether admission_claim was present incorrectly grouped Message and Schedule together. The actual schedule producer supplies a claim too; its source proof verifies the occurrence, binding and target, not the person's conversation access. Model tests that revoke View only after admitting the schedule cannot catch this boundary error.

## Watch out
When using an enum to select an authorization exemption, trace every variant's producers and independent source proof. Keep a real claimed-occurrence test that removes human View before admission while management remains valid, checks both boundaries and no written run, and restores View to prove the command is otherwise valid.

In the code: src/application/workflow/service.rs, src/adapters/persistence/workflow/admission.rs, src/application/workflow/schedule_admission.rs, and delegated_schedule_admission_tests.rs. Evidence: Plan06 p01 independent finding 06DA-P01-B1-F1 and its R2 correction.
