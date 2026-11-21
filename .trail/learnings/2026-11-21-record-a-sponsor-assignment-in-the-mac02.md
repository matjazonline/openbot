---
title: "Record a sponsor assignment in the administration history only when a holder row is inserted, not when only the decider flag changes"
author: Mac02
date: 2026-11-21
branch: plan08_ph1
cost: { minutes: 27, tokens: 239908 }
id: l-3811c6e6
scope: repo
status: active
src: agent
touches:
  - src/adapters/persistence/department/roles.rs
---

## Decided
When merging the delegated-administration audit trail with sponsor roles, a sponsor assignment records `role_assigned` only when the upsert actually inserted a new holder row. Moving or setting the decider flag on an existing sponsor updates the row but is not a role assignment, so it records nothing. Root-authority checks and audit records for role assign/clear live in the same module as the sponsor decider logic, so every role write path goes through both.

## Tried and ruled out
Using `rows_affected() == 1` (the plain-role pattern) for the sponsor upsert: `ON CONFLICT ... DO UPDATE` reports one affected row on update too, so re-marking an existing sponsor as decider would have written a spurious `role_assigned` history event.

## Watch out
If decider handover should become auditable, add a distinct administration event rather than reusing `role_assigned`.

In the code: src/adapters/persistence/department/roles.rs `assign_sponsor_on` returns `RETURNING xmax=0` (true only for an insert); `write_role_on` fills `Placed.role_assigned`/`replaced`, which `audit::record_role_placement_on` consumes.
