# 02 Unit lifecycle

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Any unit except the root can be archived. An archived unit admits no work and accepts no
configuration, and everything it held stays readable under the rules that applied before. It can
be restored. Starts after [07-unit-resources](../07-unit-resources/README.md), so the rule is
written once against memory, authority and budgets. Continues in
[03](03-goal-units-and-tracking.md).

## Batch 02A State

1. `departments` (`init_schema.sql:15492`) gains `archived_at timestamptz` and `archived_by jsonb`,
   null together, the second in the shape `valid_creation_provenance` checks. The root from
   04-root-unit cannot be archived: `CHECK (NOT (is_root AND archived_at IS NOT NULL))`.
2. An active unit never sits under an archived one. `guard_department_shape`
   (`init_schema.sql:15597`) refuses inserting, re-parenting or restoring a unit whose parent is
   archived.
3. Archiving a unit archives its subtree in one transaction, under the company row lock every
   department write already takes. Units archived earlier keep their own timestamp.
4. Restoring a unit restores that unit alone, and its parent must be active. Children are restored
   one at a time.
5. The department entity (`src/domain/entities/department.rs`) carries `archived_at`; reads that
   list units take an explicit "include archived" argument, so no caller gets them by omission.

## Batch 02B What an archived unit refuses

1. **Archiving with open work.** The write refuses with `unit.archive_open_work` and a count when
   any run on a desk of the subtree is not finished. The check and the write share the company
   lock, so an admission racing the archive either commits first and refuses the archive, or is
   refused by item 2. The department page offers to cancel the open runs and archive.
2. **New runs.** Admission (`create`, `src/adapters/persistence/workflow/admission_write.rs:7`)
   reads the unit state in the statements that read the channel under its gate and refuses with
   `unit.archived`, reported the way a disabled desk is. `channels.enabled` is not touched, so a
   restore returns each desk to exactly the state it had. Schedules on the unit's desks are skipped
   by the sweeper and not recorded as failures.
3. **Calls and mail.** `channel.call` to a desk of an archived unit closes `denied` with
   `unit.archived`. Inbound mail and internal relays are refused as for a disabled desk.
4. **Configuration.** Writes to the unit's members, roles, charter, escalation default, name and
   parent, creating or assigning a desk into it, desk settings, and configuring or activating a
   binding or schedule on its desks are refused with `unit.archived`. Restore and delete are the
   two writes allowed. Delete keeps its existing rules.
5. **The database refuses what the application skips.** A trigger refuses inserts and updates on
   `department_members`, `department_roles` and `channels.department_id` that target an archived
   unit. Deletes pass, so principal retirement still removes its rows.
6. Role resolution and the escalation ladder need no change: an active desk's chain never passes
   through an archived unit, by item 02A.2.

## Batch 02C What is kept

1. **Memory and knowledge (05).** The unit's namespace and documents are not retired; retirement
   stays tied to deletion (`retire_namespaces_on`). Nothing writes them, because no run is admitted
   and uploads are a configuration write. Reads follow the existing rules, which means a parent's
   lead still reads an archived direct child through the one-step rule. Results label such hits
   `reports:<slug>` as today, with the unit marked archived.
2. **Resources (07).** Resources the unit owns stay where they are and are selectable by nothing,
   since every desk below is archived too. Moving a resource out to an active unit is allowed for a
   manager of both, so a credential is not stranded; moving one in is refused.
3. **Budgets and usage (07).** `unit_budgets` rows and usage attribution are kept. Usage pages show
   archived units when asked to include them.
4. **Desk history.** Threads, runs, decisions and escalations stay readable under the desk grants
   that existed. Membership and roles in an archived unit grant that reading and nothing else.

## Batch 02D Authority

1. Archive and restore need what delete needs under
   [06-delegated-administration](../06-delegated-administration/README.md): managing the parent.
2. `manages_unit` ignores roles held on an archived unit, so an archived unit is managed from
   above only.
3. If proposal 2 of the [README](README.md) is confirmed: `manages_unit` is also true for a person
   who is the sponsor decider of the unit or of an ancestor. The SQL behind the predicate gains
   that arm and the persistence rechecks from 06 pick it up with it. An agent decider is refused
   before the lookup, as an agent lead is.
4. An agent whose every department is archived is managed by root managers, as an agent in no
   department is.

## Batch 02E UI and docs

1. The department page (`src/adapters/http/pages/company_departments.rs`) gains Archive and
   Restore, with the open-work count and who archived it and when. The department list hides
   archived units behind "Show archived".
2. Department pickers, the agent directory's `DepartmentRef`
   (`src/application/agent/authoring_reads.rs:158`) and `department_filter`
   (`src/adapters/http/pages/attention.rs:36`) leave archived units out unless asked. The run board
   keeps showing past runs with their unit, labelled archived.
3. `docs/departments.md` gains "Archive" before "Delete" and says when to use which.

## Focused checks

- `unit_archive_`: archive and restore round-trip; the subtree is archived in one commit; the root
  is refused; an active child under an archived parent is refused on insert, re-parent and
  restore; desk `enabled` flags are unchanged after a restore.
- `unit_archive_refuses_`: one test per write in 02B item 4 through the use case, and one each
  through the trigger; open work refuses the archive and names the count; a retirement still
  deletes its rows.
- `unit_archive_runs_`: with the scripted-model `Deployment` harness, a message to an archived
  unit's desk admits nothing and leaves no run; a `channel.call` closes `denied`; a schedule is
  skipped without a failure row.
- `unit_archive_scopes_`: the parent's lead reads the archived child's memory and knowledge; a
  sibling does not; nothing writes; the namespace is not retired; a resource moves out and not in.
- `unit_archive_authority_`: the parent's manager archives and restores; the unit's own lead does
  not; roles on an archived unit do not manage; the sponsor arm, if confirmed.
- `unit_archive_competing_`: an admission racing an archive leaves either a refused archive or a
  refused admission, never a run on an archived unit; two archives of overlapping subtrees leave a
  consistent tree.
- Rendering tests for the actions, the toggle and the pickers.
