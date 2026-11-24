---
title: "A run's dependency manifest root means the run that owns the manifest, never the budget tree root"
author: Mac02
date: 2026-11-22
branch: plan08_ph1
cost: { minutes: 1, tokens: 1788 }
id: l-11ae6c52
scope: repo
status: active
src: agent
touches:
  - migrations/20260817000000_init_schema.sql
  - src/adapters/persistence/workflow/channel_reply.rs
---

## Decided
- Every reader of a run's dependency-ownership row treats its root run as the run that owns the manifest (the scope root), never as the causal/budget tree root. Role bindings, follow decisions and provenance all key on it that way. The budget root is always read from the budget table.
- So a run can own its own manifest inside someone else's budget tree (the channel.call child case) without changing any reader. Only the foreign key that equates the two roots has to give way.

## Tried and ruled out
- Renaming the column to make the scope explicit: about 15 SQL statements would change, while none of them reads it as the budget root.

## Watch out
- The FK (company, run, root) to the budget table is what forces scope root = budget root. Replace it only together with a guard that admits exactly three cases: a root run, a call child backed by an invocation record, and an ordinary child inheriting its parent's exact scope. Otherwise a random child could claim a fresh scope.
- Responder settings for a desk are one mutable row. They are serialized only by locking the desk's channel row FOR UPDATE after users FOR SHARE. A snapshot reader must take person locks before the channel share lock to keep that order.

In the code:
- migrations/20260817000000_init_schema.sql: run_dependency_manifests, check_agent_manifest_members, capture_channel_reply_context
- src/adapters/persistence/workflow/agent_manifest/{mod.rs capture_on, storage.rs attach_child_on}
- src/adapters/persistence/workflow/channel_reply.rs configure_reply_on
- plan/agent-workflow-call/phases/p02/docs/logs/AWC-P02-D1-r1-decision.md
