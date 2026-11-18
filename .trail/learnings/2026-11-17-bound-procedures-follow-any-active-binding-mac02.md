---
title: Bound procedures follow any active binding unit manager
author: Mac02
date: 2026-11-17
branch: plan06
id: l-cfa5b983
scope: repo
status: active
src: agent
touches:
  - migrations/20260817000000_init_schema.sql
  - src/adapters/http/routes/agent_api_tests.rs
  - src/adapters/http/routes/attention_authorization_tests.rs
  - src/adapters/http/routes/authorization_test_support.rs
  - src/adapters/http/routes/channel_authorization_tests.rs
  - src/adapters/http/routes/channel_classic_form_tests.rs
  - src/adapters/http/routes/department_tests.rs
  - src/adapters/http/routes/root_department_route_tests.rs
  - src/adapters/http/routes/schedule_channel_tests.rs
  - src/adapters/http/routes/ui_local_agents_tests.rs
---

## Decided

The user resolved the Plan06 README/01B.4 conflict: a procedure with active bindings
is managed by managers of ANY unit with a current active binding. The recorded
creating department governs only when no active binding exists. Publishing does
not require authority over every existing binding; creating/moving a binding still
requires authority over its actual target unit.

Creation commands retain the destination workflow identity. If that same-company
definition already exists, its current management authority applies before any
collision or replay result. Only actual absence uses the creating unit.

## Tried and ruled out

Always using the recorded department contradicts the user's chosen binding rule.
Identity-free creation authority exposes another unit's existing definition via
conflicts or repeated template-copy keys; independent D3 review found that gap.

## Watch out

Use the current binding configuration/version, excluding inactive and historical
revisions. Recheck current target authority on template-copy conflict replay, even
when the original creator still manages its former department. Keep configuration
management separate from conversation View and current authenticated provenance.

In the code: plan/06-delegated-administration/README.md decision3;
src/adapters/persistence/administration.rs and workflow/definitions.rs;
src/adapters/http/routes/workflows/definitions.rs; p01 D3/r4 review records.
