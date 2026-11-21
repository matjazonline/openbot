---
title: A unit manager reads and writes manual knowledge across its whole subtree
author: Mac02
date: 2026-11-19
branch: plan06
cost: { minutes: 1, tokens: 1862 }
id: l-c14eb3ad
scope: repo
status: active
src: agent
touches:
  - plan/06-delegated-administration/COORDINATION.md
  - plan/06-delegated-administration/phases/p01/docs/HANDOFF.md
  - plan/06-delegated-administration/phases/p01/docs/REVIEW.md
  - plan/06-delegated-administration/phases/p01/docs/TASK.md
  - plan/06-delegated-administration/phases/p01/docs/agent-review-findings.md
  - plan/06-delegated-administration/phases/p01/docs/agent-review-notes.md
  - plan/06-delegated-administration/phases/p01/docs/logs/06DA-P01-B4-R1-reviewer-focused.txt
  - plan/06-delegated-administration/phases/p01/docs/logs/06DA-P01-B4-R1-reviewer-invite-race.txt
  - plan/06-delegated-administration/phases/p01/docs/logs/06DA-P01-B4-R1-reviewer.evidence.json
---

## Decided
The user chose (Plan06 01B.6) that a person who manages a unit — lead of it or of any ancestor — gets manual knowledge read AND write (list, search, upload, move, delete) for every unit in that subtree. Level ceilings, hidden-layer and collision rules still apply. Frozen workflow run scopes and agent memory keep Plan05's ancestor chain plus one-step lead children; they do not widen.

## Tried and ruled out
Widening write only, which is the plan's literal wording. A lead could then upload into a grandchild unit but never list or delete what was uploaded, because list and delete need read. The user rejected that as incoherent management.

## Watch out
Plan05's knowledge tests originally asserted that a lead stops at direct children; two were deliberately changed (one renamed to ..._managed_subtree_and_level). Owner and root admin remain "All". The managed set reuses the SQL predicate unit_management_role rather than a second recursive walk, and is capped by MAX_RUN_READ_UNITS with the existing generic scope-limit error.

In the code:
- src/adapters/persistence/knowledge/permissions.rs `ordinary` (managed units added to read and write)
- Tests: src/adapters/persistence/knowledge/delegated_scope_tests.rs; knowledge_scope_search_manual_roles_ancestors_managed_subtree_and_level
