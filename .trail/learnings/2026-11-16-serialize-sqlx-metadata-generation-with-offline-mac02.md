---
title: Serialize SQLx metadata generation with offline builds
author: Mac02
date: 2026-11-16
branch: plan08_ph1
id: l-0f5aac95
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
Run cargo sqlx prepare only while the worktree's offline compilation and test compilation are idle. Cargo's build-directory lock does not protect the checked-in offline cache during the CLI's setup.

## Tried and ruled out
Queuing SQLx regeneration behind an active offline test build looked safe because both cargo children share a target directory. The installed SQLx CLI deleted all 28 .sqlx entries before its cargo child acquired the build lock. Waiting for Cargo therefore exposed a temporarily empty offline cache to the active build. Stop only the identified task-owned regeneration, restore the previously unchanged entries, and rerun generation after the other compiler is done. Do not change source queries or skip the metadata check to work around this tooling race.

## Watch out
A clean prepare --check already proves that the current cache matches that migrated schema, but it does not change the ordering requirement for a later regenerating prepare. Never regenerate during a frozen independent review: source-cache files are part of the artifact even when expected to regenerate byte-for-byte.

In the code:
- src/AGENTS.md and src/adapters/persistence/AGENTS.md require metadata regeneration.
- Evidence: plan/08-goals/phases/p01/docs/logs/B1-R2-sqlx-prepare.txt; TASK check-execution records. The task-owned queued regeneration was stopped and the unchanged 28 cache files restored from the verified original HEAD; git status .sqlx was clean afterward.
