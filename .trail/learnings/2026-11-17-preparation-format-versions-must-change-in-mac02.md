---
title: Preparation format versions must change in database validators too
author: Mac02
date: 2026-11-17
branch: feature/plan05-scopes
id: l-b603fb20
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
Treat SQL history validators as strict readers of a persisted agent preparation, alongside Rust deserializers and projections. When bumping that format, exhaustively inspect the init schema as well as source callers and exercise a real preparation and checkpoint after a fresh migration.

## Tried and ruled out
Successful offline compilation did not prove the version migration complete. Rust wrote version 8 while two SQL validators still required 7. Many unrelated-looking memory tool and child tests then stopped before the provider with agent.unresolved or unused scripted exchanges. Updating selector fixtures alone could not fix that shared preparation failure. Aligning both SQL predicates and recreating the owned test databases restored those production paths.

## Watch out
Do not bypass the SQL checks, accept both versions, or weaken scripted-model assertions. The repository resets incompatible data. Follow the first preparation failure down to its database rejection; a workflow's retry classification can hide the specific SQL cause.

In the code: migrations/20260817000000_init_schema.sql, agent_history_checkpoint_matches and agent_history_freeze_valid; src/application/workflow/agent.rs PREPARED_AGENT_VERSION; plan/05-scopes/phases/p02/docs/logs/B1-scope-tests-r4.txt, B1-core-schema-refresh-r3.txt and B1-memory-focused-r2.txt (192 passed, 8 live-service checks separately required).
