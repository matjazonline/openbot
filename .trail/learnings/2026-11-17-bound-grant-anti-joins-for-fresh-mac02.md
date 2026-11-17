---
title: Bound grant anti-joins for fresh-table generic plans
author: Mac02
date: 2026-11-17
branch: feature/plan03_04
id: l-b685235f
scope: repo
status: active
src: agent
touches:
  - docs/departments.md
  - docs/inter_channel_agent_communication.md
  - docs/transport_architecture.md
  - migrations/20260817000000_init_schema.sql
  - plan/04-root-unit/COORDINATION.md
  - plan/04-root-unit/integration/main-60b9a28b/docs/HANDOFF.md
  - plan/04-root-unit/integration/main-60b9a28b/docs/TASK.md
  - plan/04-root-unit/phases/p01/docs/HANDOFF.md
  - src/adapters/http/pages/channel_settings.rs
  - src/adapters/http/pages/channels.rs
---

## Decided

Repeat the bounded desk set inside the grant insert's permission anti-join. This leaves its company/desk/principal/capability/provenance identity unchanged while giving PostgreSQL an index bound for generic prepared plans against fresh-table statistics. Preserve existing transaction locks, unchanged grants and delta-only writes.

## Tried and ruled out

A populated temporary clone failed to reproduce repeated production desk creation because its statistics and generic-plan shape differed. The historical real-table counterexample explains why the redundant-looking predicate is intentional: with 46000 grants, the original query materialized 44000 company rows and rejected 85999000 join pairs in 4021 ms; constraining the inner read to the desk set materialized 2000 rows and rejected 1999000 pairs in 98 ms. The separate live sample caught BuffileRead during the original insert. Raising work_mem or adding an index would obscure the missing bound rather than own it in the query.

## Watch out

Small cardinality tests and fully populated clones can miss the early fresh-table prepared-plan regime. Rollback-only EXPLAIN on an already complete desk proves read-plan behavior, not new-row triggers or end-to-end creation cost. Measure those through the actual production writer, retaining exact mutation counts and one settings revision step per affected desk. Distinguish quiescent measurements from diagnostic runs with concurrent EXPLAIN.

In the code: src/adapters/persistence/department_desks.rs INSERT_MISSING and src/adapters/persistence/access_mode_scale_tests.rs. Historical evidence: plan/04-root-unit/phases/p02/docs/logs/B1-live-reconciliation-plan.{sql,txt}, B1-scale-active-query.txt, B1-final-scale.txt and B1-insert-planner-experiment.txt. PostgreSQL18, work_mem4MB, 2000 desired grants. The note checker misclassified this causal historical comparison as transient status; retaining the counterexample is intentional.
