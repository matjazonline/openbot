---
title: Preparation format versions include the SQL preparation and checkpoint validators
author: Mac02
date: 2026-11-17
branch: feature/plan05-scopes
id: l-aa159407
scope: repo
status: active
replaces: l-b603fb20
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
Treat SQL history validators as strict readers of persisted agent preparation, alongside Rust deserializers and projections. A format bump needs an exhaustive init-schema search and a real preparation/checkpoint on a fresh migration.

## Tried and ruled out
Offline compilation passed while Rust wrote version 8 and both SQL readers still required 7. Tool and child tests then stopped before the provider with agent.unresolved or unused scripted exchanges. Updating selector fixtures could not fix that common failure. Aligning both SQL predicates and recreating the owned databases restored those paths. This replaces the earlier note solely to correct the SQL function's literal name: agent_history_preparation_valid, not agent_history_freeze_valid.

## Watch out
Keep strict SQL checks and scripted-model assertions. Do not accept both versions; this repository resets incompatible data. Trace the first actual preparation failure because workflow retry classification can hide its database cause.

In the code: migrations/20260817000000_init_schema.sql functions agent_history_checkpoint_matches and agent_history_preparation_valid; src/application/workflow/agent.rs PREPARED_AGENT_VERSION; plan/05-scopes/phases/p02/docs/logs/B1-scope-tests-r4.txt, B1-core-schema-refresh-r3.txt and B1-memory-focused-r2.txt (192 passed, 8 live checks separately required).
