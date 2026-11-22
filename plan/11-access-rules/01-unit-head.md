# 01 Unit head

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

One function says whether a role holder heads a unit. Management and the one-step desk view both
ask it. Implements decision 5 of the [README](README.md). Continues in
[02](02-agent-home.md).

## Where it stands

Since part 02 of 08-goals a person who is sponsor decider manages the unit. The rule
`role = 'lead' OR (role = 'sponsor' AND decider)` is written out three times in the init schema:
`unit_management_role` (line 15823), `any_unit_management_role` (15842) and
`resource_default_unit` (16067). Three other places still name the lead alone:

| Place | Decides |
| --- | --- |
| `ELIGIBLE` in `src/adapters/persistence/department_desks.rs:101` | Who gets a desk's one-step grants |
| `src/adapters/persistence/knowledge/permissions.rs:139` | Whose direct children a person reads |
| `src/adapters/persistence/workflow/run_scopes.rs:149` | Whose direct children an acting agent reads |

So a person who is sponsor decider of Sales manages every unit under Sales and sees none of
their desks. In a unit led by an agent, the agent sees the sub-unit desks and the only human in
charge does not. Actions that need both management and a view of the desk are refused for that
person on a members-only desk until they add themselves as a member.

## Batch 01A One definition

No behavior changes in this batch.

1. Add one SQL function, `heads_unit`, over a `department_roles` row: true for `lead`, and for
   `sponsor` with `decider` when the holder's principal is a person. It reads the principal kind
   itself, so no caller restates it as a literal.
2. Use it in `unit_management_role`, `any_unit_management_role` and `resource_default_unit` in
   place of the three written-out copies. Their results must not change.
3. Add no Rust copy of the rule unless a caller needs the answer without a query. If one is
   added, cover both with one database-backed equivalence matrix.

## Batch 01B Desk view follows the head

1. In `ELIGIBLE`, the parent arm and the child arm use `heads_unit` in place of the bound
   `lead` value. A person who is sponsor decider then gets the grants a lead gets on the parent's
   desks and on each direct child's desks. An agent lead keeps them. An agent decider and an
   informed sponsor get none.
2. Desk grants are re-derived by `rederive_on` and `sync_department_grants_on`
   (`department_desks.rs:237`, `:287`), reached from `write_placement_on`
   (`src/adapters/persistence/department.rs:440`) and `clear_department_role_on`
   (`src/adapters/persistence/department/roles.rs:42`). Confirm that moving the decider flag, and
   naming a successor when a deciding sponsor is removed, runs the same sync for the unit, for its
   parent's desks and for its children's desks, in the same transaction. Add the call where it is
   missing.
3. `permissions.rs:139` and `run_scopes.rs:149` use `heads_unit`. Nothing a person reads
   changes, because a manager already reads the subtree it manages. Nothing an agent reads
   changes, because an agent decider does not head. The edit keeps the rule in one place.

These stay with the lead and must not be changed to the head: who answers the unit's desk
(`follow_lead_on`, `department.rs:381`), the second escalation hop
(`src/adapters/persistence/workflow/escalation_hops.rs:17`), and the lead shown in rosters
(`src/adapters/persistence/department_roster.rs:273`, `:359`).

## Batch 01C Documentation

`docs/departments.md`: the Administration section says a person manages a unit when they hold
`lead` on it or an ancestor (about line 289), and the desk section names the lead for the one-step
grants. Both name the head, defined once. Name only behavior the code has.

## Focused checks

- `unit_head_`: SQL matrix over lead person, lead agent, deciding sponsor person, deciding
  sponsor agent, informed sponsor, reviewer, and each on an archived unit. Every existing
  `manages_unit_` and `delegated_admin_` test passes unchanged.
- `unit_head_desk_`: a person who decides for the parent sees the child's desk; one who decides
  for a child sees the parent's desk; an agent decider and an informed sponsor do not; moving the
  decider flag moves the grants in the same commit.
- `unit_head_desk_competing_`: a decider move racing a desk edit leaves grants that match the
  committed decider, in both orders.
- Existing families still pass: `access_mode_grants_`, `department_lead_desk_`,
  `knowledge_scope_`, `run_scopes_`.
