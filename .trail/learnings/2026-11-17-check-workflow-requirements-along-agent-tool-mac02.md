---
title: Check workflow requirements along agent tool edges before a run starts
author: Mac02
date: 2026-11-17
branch: feature/plan05-scopes
id: l-9091bbc9
scope: repo
status: active
src: agent
touches:
  - migrations/20260817000000_init_schema.sql
  - plan/05-scopes/COORDINATION.md
  - scripts/memory-integration.sh
  - services/memory/.sqlx/query-dff27301651c1c546029eef97662c134b1b0ad246255bfd7cde2d950856679ed.json
  - services/memory/migrations/0001_init.sql
  - services/memory/openapi.json
  - services/memory/src/adapters/postgres/memory.rs
  - services/memory/src/adapters/postgres/mod.rs
  - services/memory/src/application/contract.rs
  - services/memory/src/application/memory.rs
---

## Decided
Validate each reachable workflow's local-unit requirement in the bounded admission dependency closure, using the already resolved base department. Ordinary published children and workflows reached through selected agents' workflow tools share this preflight boundary.

## Tried and ruled out
Checking only the root PublishedBundle and its children missed agent-tool workflow edges. A real regression test admitted a company-wide root with an own-unit memory tool descendant; the guard would otherwise fail only when that descendant executed, after earlier effects could occur.

## Watch out
A scope_unavailable refusal at the eventual memory step protects access but does not establish admission-before-effects behavior. Keep the negative root-admission test and real desk positive case, including unchanged run/job/delivery counts and zero provider calls for the refusal.

In the code: src/adapters/persistence/workflow/agent_manifest/{mod,capture}.rs stores the resolved department and checks each workflow node; src/application/workflow/memory.rs::needs_local_unit handles the published bundle portion. Tests: src/adapters/persistence/workflow/autonomous_memory_scope_tests.rs. Evidence: plan/05-scopes/phases/p02/docs/logs/B1-R2-own-preflight-red-r2.txt (Created before correction), B1-R2-focused-r1.txt (both negative and positive pass).
