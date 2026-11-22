# 08G-P02 task

Current state (2026-11-21): merge committed `73afd5a7`; coordination r3 committed `42bc327e`. B1 architecture gate D1-r1 under independent review (B spawned). Implementation NOT STARTED. Not accepted in phase; not integrated.

## Request and identity

- Original request (user, 2026-11-21): "pull origin/main where up to plan07 is already complete then use /implement-opus-reviewed-2_4 and continue where we left off". Phase 01 is committed (`6c63f292`); the next phase is p02.
- Requirements: `plan/08-goals/02-unit-lifecycle.md`, `plan/08-goals/README.md` (authoritative originals at plan root).
- Phase: p02. ID prefix `08G-P02`. Workflow: implement-opus-reviewed-2_4.
- Installed skill: `/Users/mac02/orca/workspaces/openbot/plan08_ph1/.claude/skills/implement-opus-reviewed-2_4`.
- Record root: `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/08-goals/phases/p02/docs`.
- Root/A: this Claude Code session (`plan08-ph1-51`), Opus 5.5 / `xhigh` — effort confirmed by the user 2026-11-21 (AskUserQuestion). Root acts as A.
- B: spawned 2026-11-21 for the D1 gate; requested Fable / `medium`, `general-purpose`, fresh context (no fork); actual settings readback unavailable. Agent ID `a51801b0670ea8aad` (in-process subagent; SendMessage resumes it).
- Context: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` added to `.claude/settings.local.json` `env` at user request 2026-11-21; effective for newly started sessions/agents. This root session started before the change, so its own threshold is unverified (default).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`.
- Phase starting commit: `73afd5a7` (merge of origin/main `a1cdb2d5` into `6c63f292`). Coordination commit `42bc327e` = D1 review base/HEAD. D1 snapshot digest `44d631232ac02c524293a02c936eaeebfafe71fce73a35014ac24976626001e5` (`logs/08G-P02-D1-snapshot.txt`).
- Pre-existing dirty changes at startup: none besides the in-progress merge.
- Snapshot exclusion (exact single path): `plan/08-goals/phases/p02/docs`.

## User decisions (2026-11-21)

- README proposal 2 CONFIRMED: a person who is the sponsor decider of a unit or an ancestor manages the unit (`manages_unit` arm, 02D.3). An agent decider does not.
- README proposal 3 CONFIRMED as written: archiving refuses while work is open (`unit.archive_open_work` with a count); the page offers to cancel the open runs and archive.
- U3 (after D1-R1): the parent's manager may cancel the subtree's open runs through cancel-and-archive even without desk access; audited per root run; reading the runs still needs desk access.
- U4 (after D1-R1): an archived unit's desks may be deleted, or moved out to an active unit by a manager of both units; moving a desk in and any other desk configuration are refused.

## Coordination

- Authoritative file `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/08-goals/COORDINATION.md`; previous writer /root session 01a1e095 (p01, stopped after handoff). No other session works in this worktree (ListAgents 2026-11-21). Writer handoff to this session recorded in revision 3.
- Revision 3 (writer handoff, p02 registration, confirmed proposals); pinned `logs/08G-P02-coordination-r3.md`. Applicable notices: none.

## Merge prerequisite (origin/main a1cdb2d5)

Plans 04–07 are on main. Conflicts in 19 files were resolved by layering p01 on main: role enum = lead, reviewer, sponsor, owner, admin; `is_root` and `escalation` both on `Department`; main's root authority and administration audit moved into the p01 `department/roles.rs` writes; sponsor upsert reports insert-only `role_assigned` (`RETURNING xmax=0`); escalation picker iterates `DepartmentRole::ALL` with "Use unit default"; ~70 test call sites in main's new tests moved to `DepartmentRoleAssignment`/`DepartmentRoleRemoval`; sponsor runtime fixture uses department membership instead of the removed `team` access mode; selector-bound test uses `MAX_REVIEWER_SELECTORS` (renamed `..._includes_every_role_...`).

Checks (scratch DBs `openbot_merge08_mac02`, `_test` on localhost:5432):
- `SQLX_OFFLINE=true cargo check --all-targets`: PASS.
- `sqlx migrate run` on fresh DB: PASS. `cargo sqlx prepare --check -- --all-targets`: PASS.
- `cargo test --lib -- department sponsor root_unit delegated_admin role_snapshot escalation reviewer_picker workflow_authoring::bindings root_department administration`: 492 PASS / 3 FAIL (merge fallout above) → fixed; `role_sponsor sponsor_admission_selector_bound` 11 PASS.
- Second batch (`root_ delegated scope unit_resources unit_budget memory::tests workflow_role follow_review place_in placement`): 511 PASS / 2 FAIL, both merge fallout: role-list expectation without sponsor (`root_membership_assignment_canonical_roles_follow_stored_strings`) and step help not naming owner/admin (`workflow_role_source_sample_and_help_explain_the_same_requirement`; main made owner/admin selectable via `DepartmentRole::ALL` without updating help). Fixed (expectation + help role lists).
- `registry publication reviewer_assignment help`: 348 PASS / 1 FAIL `root_department_company_and_account_teardown_remove_populated_message_identity_and_action_history` (main's test, file untouched by this branch, 100 ms batch budget); PASS rerun in isolation. Recorded as load-sensitive, not a merge regression; not further investigated.
- `trail check`: no conflicts. Merge committed `73afd5a7`.

## Startup findings

Map: `logs/08G-P02-startup-map.md`. Deviations from the plan text:
- `channel.call` does not exist in the code (`grep -rln 'channel\.call' src` → only a fixture comment). 02B.3's call clause and the `unit_archive_runs_` channel.call case have no producer. Disposition (A, pending B gate): NOT APPLICABLE in p02; recorded for the `agent-workflow-call` plan as a requirement it must add (callee desk on an archived unit closes `denied` `unit.archived`). Reported to the user. No coordination notice exists for another plan; this is carried in HANDOFF.
- Workflow admission serializes on the company row (`FOR NO KEY UPDATE`), not the channel gate; archive takes the same company lock, so the archive/admission race is decided by that lock. Inbound mail takes the Admit gate with only `FOR KEY SHARE` on the company, so archive must bump desk `settings_revision` under the ChangeSettings gate (pattern `department_desks.rs` `rederive_on`).
- Schedules have no skip-without-failure outcome; disabled desks are not checked by admission or the sweeper today.

## Batches

- **B1 (gated): 02A + 02B + 02D.** State, archive/restore writes, refusals (admission, schedules, mail/relay, configuration, DB trigger), authority (archived roles ignored, sponsor decider arm, agent-in-archived-units). Includes the entity field and the API routes for archive/restore.
- **B2: 02A.5 listing argument + 02C + 02E.** Explicit include-archived on list reads, pickers/filters/directory/usage, memory/knowledge labels, resource move rule, department page actions, docs.

## B1 architecture gate — decision revision D1-r1

D1.1 Schema. `departments.archived_at timestamptz`, `archived_by jsonb`; `CHECK ((archived_at IS NULL) = (archived_by IS NULL))`, `CHECK (archived_by IS NULL OR valid_creation_provenance(archived_by))`, `CHECK (NOT (is_root AND archived_at IS NOT NULL))`. Edited in the single init migration (no compatibility; DB reset).

D1.2 Tree invariant (02A.2). `guard_department_shape` fires also on `UPDATE OF archived_at` and refuses (`23514`, constraint `departments_archived_parent`) any row with `archived_at IS NULL` whose parent is archived — covers insert, re-parent and restore. The reverse direction (archiving a parent while a child stays active) is prevented by the application archiving the whole subtree in one `UPDATE … WHERE id IN subtree AND archived_at IS NULL` (units archived earlier keep their timestamp). Invariant: an active unit's ancestor chain is all active; therefore role resolution/escalation (02B.6) need no change.

D1.3 Archive/restore write protocol. New `DepartmentPersistence::archive(scope, unit, cancel_open)` / `restore(scope, unit)` through `begin_department_write` (company `FOR NO KEY UPDATE`), then `require_parent_management_on` (02D.1, same as delete). Archive: refuse root (`BadRequest`), lock the subtree rows `FOR UPDATE` in id order, count open runs (`workflow_runs.finished_at IS NULL`) on desks whose `department_id` is in the subtree; if >0 and not `cancel_open` → `AppError::UnitArchiveOpenWork(count)` code `unit.archive_open_work` (409). With `cancel_open`, cancel each open root run via the existing `controls::cancel_tree_on` in the same transaction (authority = managing the parent; management covers configuration of every desk below, and the page states it), then archive. Then archive the subtree, record one `department_archived` administration event per newly archived unit, and bump every subtree desk's `settings_revision` under the ChangeSettings gate (sorted) so in-flight mail admission is superseded. Restore: unit alone; refuses if not archived; parent must be active (DB guard is the backstop, app refuses first with `unit.archived` naming the parent); records `department_restored`; bumps that unit's desks' revisions. `channels.enabled` is never touched.

D1.4 Race argument (02B.1). Admission takes the company row `FOR NO KEY UPDATE` before reading the desk; archive takes the same lock first. Either admission commits first (its run is open → archive refuses with the count) or archive commits first (admission then reads `archived_at` and refuses `unit.archived`). Two overlapping archives serialize on the company lock; the second archives only still-active rows. Competing-claimant tests: admission vs archive; archive vs archive of overlapping subtrees; archive vs mail commit (gate).

D1.5 Admission refusal (02B.2). New `AppError::UnitArchived` (code `unit.archived`, 409), classified to `BindingOutcome::Refused("unit.archived")` in `message_admission::classify` like other coded refusals. Checked in admission right after `management::authorize_association_on` (company lock held) by one statement reading `channels JOIN departments … FOR SHARE` for a desk association. Principal (agent-address) associations are not desks and are unaffected.

D1.6 Schedules (02B.2). `claim_due` joins the schedule's desk unit; for a due schedule on an archived unit it advances `next_run_at` exactly as today but inserts no `schedule_runs` row (skipped, no failure, no refusal metric). An occurrence claimed before an archive that admission then refuses with `unit.archived` is deleted under its claim instead of being recorded `failed` (no new status value).

D1.7 Mail and relays (02B.3). Routing treats a desk of an archived unit as `DisabledDesk` (same bounce). SQL `relay_desk_can_participate` / `internal_relay_source` gain `AND unit active`; Rust relay pre-filter mirrors it.

D1.8 Configuration refusals (02B.4). One persistence helper `require_active_unit_on(tx, company, unit)` (reads `archived_at` of the already-locked row) used by: update (name/slug/charter/description/escalation/parent — both the unit and a destination parent), members add/remove, roles assign/clear (incl. placement), create under an archived parent, desk create/assign into the unit (`prelock_ownership_on`), and a desk-level helper `require_active_desk_on(tx, company, channel)` for desk settings (`update_channel_on`, `configure_reply_on`), workflow binding configure/activate (`save_binding_on`, activity set to active), and schedule create/update/enable. Allowed on an archived unit: restore, delete, deactivating a binding, disabling a schedule (they stop work rather than configure it). Member/role removal on an archived unit is refused at the application (02B.4 lists members and roles), while the DB trigger allows deletes so principal retirement still works (02B.5).

D1.9 DB backstop (02B.5). Trigger `guard_active_unit_target` BEFORE INSERT OR UPDATE on `department_members`, `department_roles` (any update, incl. sponsor decider flip), and `channels` (INSERT, UPDATE OF department_id) refusing when the target department is archived (`23514`, constraint `unit_archived`). Deletes pass.

D1.10 Authority (02D). `unit_management_role`: the lead arm (and new sponsor arm) only counts holders on ancestors with `archived_at IS NULL` (roles on an archived unit do not manage; it is managed from above). New arm (proposal 2 confirmed): `holder.role='sponsor' AND holder.decider` on an active ancestor, returned as `sponsor:<unit>`; `membership_of` returns the person principal so an agent decider never matches. Ordering: owner > admin > nearest of lead/sponsor by depth (lead first on a tie). `valid_acting_role` accepts `sponsor:<uuid>`; Rust `ManagementRole::Sponsor(DepartmentId)`. `any_unit_management_role` and the lead-candidate CTEs that pick managed units include deciding-sponsor units. `agent_management_role`: departments with `archived_at` are ignored, so an agent whose every department is archived falls to the no-department root arm (02D.4).

Failure modes: missing `archived_at` check in a write path (mitigated by the DB trigger for rows and one helper per path, enumerated in tests); schedule backlog on restore (prevented by advancing `next_run_at` while skipped); cancellation partially failing (whole transaction rolls back, nothing archived).

Verification plan (B1): `unit_archive_` round-trip/subtree/root/guard/enabled-unchanged; `unit_archive_refuses_` one per 02B.4 write via use case + trigger cases + open-work count + retirement deletes; `unit_archive_runs_` message admits nothing (Deployment harness), schedule skipped w/o failure row; `unit_archive_authority_` parent manager vs own lead, archived roles, sponsor arm (person yes, agent no); `unit_archive_competing_` admission vs archive, archive vs archive. SQLx/migration/offline checks.

## B1 architecture gate — decision revision D1-r2 (supersedes D1-r1; responds to 08G-P02-D1-R1-F1…F6, S1…S4)

User decisions 2026-11-21 (AskUserQuestion, after D1-R1): (U3) the parent's manager may cancel the subtree's open runs through cancel-and-archive even without desk access; each cancellation is audited with the actor; reading the runs still needs desk access. (U4) An archived unit's desks may be deleted, or moved out to an active unit by a manager of both units; moving a desk in and any other desk configuration are refused. A direct local-agent session on an archived unit's conversation counts as work and is refused (A, from 02 Outcome "admits no work").

Unchanged from r1 (accepted by D1-R1-N1): D1.1 schema, D1.2 tree guard, D1.7 relay SQL/Rust, D1.9 trigger tables, D1.10 authority, the company-lock race argument for `admit`. Changes:

**D1.2a (S1 adopted).** Add `CONSTRAINT TRIGGER departments_archived_subtree AFTER INSERT OR UPDATE OF archived_at, parent_id ON departments DEFERRABLE INITIALLY DEFERRED`: an archived row with any active child refuses at commit (`unit_archived_subtree`). Both directions of the invariant are DB-enforced.

**D1.3 (F4, S3).** `archive(scope, unit, OpenWork::{Refuse, Cancel})`. Lock order: company `FOR NO KEY UPDATE` → `require_parent_management_on` → subtree department rows `FOR UPDATE` (id order) → ChangeSettings gates of subtree desks (sorted; gates before any desk row lock, S4b) → run heads. Open work = `workflow_runs` with `finished_at IS NULL AND channel_id IN subtree desks`. `Refuse`: any open → `UnitArchiveOpenWork{count}`. `Cancel` (U3): roots = open runs with `parent_run_id IS NULL`; if any open run is a native MCP broker run (cancel path is `native_mcp_calls.cancel_requested`, owned by the local agent) or roots exceed `MAX_ARCHIVE_CANCELLATIONS = 64` → refuse `UnitArchiveOpenWork{count}` with a distinct message (cancel from the run board / wait). For each root (id order): `SELECT … FOR UPDATE` the run head, skip if already terminal, insert a `control_cancel` run event carrying the archiving actor (same kind operator cancel writes, so history shows who), then `controls::cancel_tree_on` (children get `cancelled_by_parent`). Then recount open work; nonzero → refuse and roll back (nothing cancelled, nothing archived). Then archive the subtree, one `department_archived` event per newly archived unit, bump desk `settings_revision`.

**D1.5 (F1).** The archived check runs in `admit` *before* `management::authorize_association_on`: one statement under the company lock (`authorize_person_with_locks` is taken first by a new small step, or the check is the first statement after taking the company lock in the existing helper — implemented in the management helper so ordering is structural) reading `channels JOIN departments … FOR KEY SHARE OF departments`; archived → `AppError::UnitArchived`. Run-as that loses management because of D1.10 therefore still sees `unit.archived`.

**D1.5a (F2) Work inventory — every writer that creates or reopens a run or an occurrence:**

| Writer | Lock | Disposition |
| --- | --- | --- |
| `admission::admit` (message, manual, schedule, rerun) | company `FOR NO KEY UPDATE` | refuse `unit.archived` (D1.5) |
| `control_retry::retry_on` (operator retry reopens a failed run) | company + run head | refuse `unit.archived` before reopening |
| `native_mcp::admit_on` (local agent MCP call) | company (`authorize_company_with_locks`) | refuse `unit.archived` |
| local-agent direct session start (`local_agents/management/writes.rs:56`) | to confirm in impl | refuse `unit.archived` (U-derived) |
| `schedules::capture_now` (run now) | schedule row | refuse via `require_active_desk_on` |
| `schedules::claim_due` | `SKIP LOCKED` | WHERE excludes archived units (D1.6) |
| `children::create_on` (runtime child) | parent run locked | unchanged: needs an open parent, and an archived unit has no open run (D1.3 + this table) |
| wake/resume transitions (`wait_commit`, `child_waits`, `decision_approvals`) | run head | unchanged: act only on already-open runs |
Backstop: trigger `guard_run_unit_active` BEFORE INSERT OR UPDATE OF finished_at ON `workflow_runs`, firing when the row becomes/is inserted open (`NEW.finished_at IS NULL AND (TG_OP='INSERT' OR OLD.finished_at IS NOT NULL)`) on a desk (`channel_id`) whose unit is archived (`FOR KEY SHARE`) → `23514` constraint `unit_archived`. `From<sqlx::Error> for AppError` maps constraint `unit_archived` → `AppError::UnitArchived` (central, `adapters/persistence/mod.rs`), so any path the table missed still refuses with the right code.

**D1.6 (F5).** `claim_due` adds `AND unit active` to its WHERE (join `channels`/`departments`); an archived unit's schedules change no state at all (`next_run_at`, `last_run_at`, `enabled` untouched). After restore: a one-off still due fires once; an interval schedule whose slot passed fires once (one catch-up) and then follows its existing slot arithmetic. An occurrence claimed before the archive whose admission refuses `unit.archived` is deleted under its claim (no `failed` row, no refusal metric).

**D1.7a (S4a, S4e).** Mail routing reads unit state in the same statement that reads the desk and `settings_revision` (so the archive's revision bump under ChangeSettings supersedes in-flight commits); the directory (`routing.rs:833`) skips archived-unit desks like disabled ones.

**D1.8 (F3, U4) Configuration inventory.** Helpers lock for themselves so callers without the company write lock are safe: `require_active_unit_on(tx, company, unit)` = `SELECT archived_at FROM departments … FOR KEY SHARE` (conflicts with the archive's `FOR UPDATE`); `require_active_desk_on(tx, company, channel)` = same via the desk's `department_id`.

| Writer | Lock context | Disposition |
| --- | --- | --- |
| department update (name/slug/description/charter/escalation) `lifecycle::update_department_on` | company + row `FOR UPDATE` | refuse |
| re-parent (unit or destination archived) | same | refuse (destination also by DB guard) |
| create under parent `insert_department_on` | company + parent `FOR SHARE` | refuse (DB guard backstop) |
| members add/remove, roles assign/clear, sponsor decider, placement | company + row | refuse (app); DB trigger refuses insert/update, allows delete |
| restore / delete unit | company | allowed (delete keeps its rules) |
| desk create into unit / assign into unit (`prelock_ownership_on`) | company + target `FOR SHARE` | refuse; DB trigger on `channels.department_id` |
| desk update `update_channel_on` | gate ChangeSettings + row | refuse when current unit archived, unless the update moves the desk to an active unit (U4; existing move authority needs both units) |
| desk delete `channel.rs:487` | — | allowed (U4) |
| desk grant/address writers outside update (`channel.rs:156,240`, `participant.rs:815`, `account_identity.rs:238`) | per caller | refuse on archived desk where the write configures the desk; allowed where it is part of a deletion/retirement; each confirmed and tested in impl |
| responder `channel_reply::configure_reply_on` | channel `FOR UPDATE` | refuse |
| reply completion/proposals | run work | unchanged: no run can be open |
| workflow binding save/activate `bindings.rs` | company | refuse; deactivate/archive/delete allowed |
| transport binding create / set_status→active / email binding (`integration/binding.rs:36,170`, `email_binding.rs:100,139`) | company `FOR KEY SHARE` or none | refuse via `require_active_desk_on` (self-locking); pause/disable/unlink allowed |
| schedule create/update/enable/run-now | schedule/channel | refuse; disable/delete allowed |
| unit budget caps `unit_budgets.rs:26` | company | refuse on archived unit (rows kept, 02C.3) |
| knowledge upload into unit, resource create-in/move-in | — | B2 (02C.1/02C.2), recorded constraint (S4d) |

**D1.9a (S2).** The trigger reads the target department `FOR KEY SHARE`, conflicting with the archive's `FOR UPDATE` subtree lock.

**D1.10a (S4c).** `resource_default_unit` and `workflow_creating_department` ignore archived units (an archived-unit lead gets no default unit from that role).

### B1 acceptance mapping (F6)

| Criterion | Production path / case | Decisive assertion | Check | Status |
| --- | --- | --- | --- | --- |
| 02A.1 schema/root | archive root via use case; raw UPDATE | BadRequest; `departments_root_archive` CHECK | `unit_archive_root_is_refused` | planned |
| 02A.2 guard | insert/re-parent/restore under archived parent; raw parent archive with active child | constraint names | `unit_archive_guard_*` | planned |
| 02A.3/4 subtree, timestamps, restore one | archive parent with earlier-archived child; restore | child keeps its timestamp; only restored unit active; `enabled` unchanged | `unit_archive_round_trip_*` | planned |
| 02B.1 open work + cancel | Refuse with N open; Cancel by manager without desk grant; cancel rollback; native/bound refusal | count; `control_cancel` event with actor per root; nothing changed on rollback | `unit_archive_refuses_open_work`, `unit_archive_cancel_*` | planned |
| 02B.2 admission | message via Deployment harness with run-as = unit's own lead and = owner | `Refused("unit.archived")`, zero runs | `unit_archive_runs_message_*` | planned |
| 02B.2 other work writers | retry, native MCP admit, capture_now, local session; raw INSERT | `unit.archived`; trigger `unit_archived` | `unit_archive_runs_*`, `unit_archive_refuses_run_trigger` | planned |
| 02B.2 schedules | due one-off and interval on archived unit; claimed-then-archived | no occurrence, no failed row, schedule columns unchanged; restore fires once | `unit_archive_runs_schedule_*` | planned |
| 02B.3 mail/relay | inbound to archived desk; relay Rust filter; SQL functions | DisabledDesk bounce; Forbidden; SQL false | `unit_archive_refuses_mail_*`, `_relay_*` | planned |
| 02B.3 channel.call | — | — | — | NOT EXERCISED (no producer; agent-workflow-call) |
| 02B.4 config | one case per row of D1.8 table via use case | `unit.archived`; allowed rows succeed | `unit_archive_refuses_*` | planned |
| 02B.5 trigger | raw insert/update members/roles/channel department; delete passes; retirement deletes | constraint `unit_archived`; rows gone | `unit_archive_refuses_trigger_*` | planned |
| 02D.1/2 | parent manager archives/restores; unit's own lead cannot; archived roles don't manage | Ok / NotFound | `unit_archive_authority_*` | planned |
| 02D.3 sponsor arm | person decider manages unit and descendants; agent decider and informed sponsor do not; decider on archived unit does not | role `sponsor:<id>` / None | `unit_archive_authority_sponsor_*` | planned |
| 02D.4 | agent in only archived units: root manager manages, archived ancestor lead does not | role | `unit_archive_authority_agent_*` | planned |
| restore resumes work | after restore: message admitted, mail accepted, schedule fires | run exists; delivered | `unit_archive_round_trip_resumes_work` | planned |
| competing | admission vs archive; archive vs archive overlapping; archive vs mail commit; trigger write vs archive; cancel-and-archive vs running job | exactly one outcome; never an open run on an archived unit | `unit_archive_competing_*` | planned |

Contract impact beyond the tables: `AppError` gains `UnitArchived`, `UnitArchiveOpenWork{count}` (HTTP 409, codes `unit.archived`, `unit.archive_open_work`); `ManagementRole::Sponsor` consumers (display, audit `acting_role`, UI "as … of"); `AdministrationEvent::{Archived, Restored}` + SQL transition kinds and the department history pane labels; `Department.archived_at` (+ `archived_by` for the page) in every constructor (B2 owns list filtering).

## B1 architecture gate — decision revision D1-r3 (supersedes the two paragraphs below of D1-r2; responds to 08G-P02-D1-R2-F1, F2, S1)

**D1.5 (replaces the D1-r2 paragraph; R2-F2).** The archived check is a separate step, `unit_state::refuse_archived_desk_on(tx, company, association)`, called explicitly only by the refusing writers, never inside the shared `management::authorize_association_on` / `authorize_run_with_locks` helpers. In `admit` it runs after the company lock is held and before `management::authorize_association_on`; to make that ordering possible without changing the shared helper, `admit` first takes the company lock with the existing `authorize_person_with_locks` path's lock statement (re-taking an already-held row lock in the same transaction is a no-op), then calls the check, then the unchanged management authorization. Writers that D1.8 allows on an archived unit keep using the shared helpers unchanged and therefore succeed: desk delete (`channel.rs` delete → `authorize_association_on`), schedule delete and disable (`schedules.rs` `managed`), workflow binding deactivate/archive/delete (`bindings.rs`), transport binding pause/disable/unlink, restore and unit delete. Mapping rows added: `unit_archive_allows_desk_delete`, `unit_archive_allows_schedule_disable_and_delete`, `unit_archive_allows_binding_deactivate`, `unit_archive_allows_desk_move_out` (U4), each asserting success on an archived unit.

**D1.5a backstop (replaces the trigger sentence of D1-r2; R2-F1).** `guard_run_unit_active` is `BEFORE INSERT OR UPDATE OF state ON workflow_runs` and fires when the row is inserted non-terminal or moves from a terminal state (`succeeded|failed|cancelled`) to a non-terminal one (`queued|running|waiting`) — the retry reopen sets `state`, not `finished_at`. Mapping adds a raw `UPDATE workflow_runs SET state='running'` of a failed run on an archived desk → constraint `unit_archived`, alongside the raw INSERT case.

**R2-S1 adopted.** `claim_due` uses `FOR UPDATE OF schedule SKIP LOCKED` with the new join. The cancel-and-archive vs running-job competing case asserts a bounded outcome: either archive succeeds with the run cancelled, or the archive fails with a retryable database error (deadlock/lock timeout) leaving nothing changed — never a run left open on an archived unit. U3/U4 now also listed under "User decisions".

## B1 implementation status (checkpoint 2026-11-21)

Gate D1-r3 PASS (`08G-P02-D1-R3-N1`). Implemented (compiles, `cargo check --all-targets` clean; fresh migration applies):
- Schema: `departments.archived_at/archived_by` + `departments_archive_shape`, `departments_root_archive`; `guard_department_shape` refuses active-under-archived (`departments_archived_parent`, fires on `archived_at` too); deferred `departments_archived_subtree` (`unit_archived_subtree`); `guard_active_unit_target` triggers on `department_members`, `department_roles`, `channels(department_id)`; `guard_run_unit_active` on `workflow_runs` INSERT/UPDATE OF state; all guards lock the unit row unconditionally (`FOR KEY SHARE`) then test. `valid_acting_role` accepts `sponsor:`; transition kinds `department_archived/restored`. `unit_management_role` skips roles on archived units and adds the person sponsor-decider arm; `any_unit_management_role`, `resource_default_unit`, `workflow_creating_department`, `agent_management_role` updated. Relay SQL refuses archived desks (`department_is_archived`, stable, no lock).
- Rust: `Department.archived: Option<UnitArchive>`; `Channel.unit_archived` (read in `CHANNEL_SELECT` with `settings_revision`) + `takes_traffic()` used by mail routing, directory and relay pre-filter; `AppError::{UnitArchived, UnitArchiveOpenWork}` (409, codes), central constraint mapping; `ManagementRole::Sponsor`; `AdministrationEvent::{Archived, Restored}`; `OpenWork`; port/persistence/use case `archive`/`restore`; `department/archive.rs` (subtree lock, gates + revision bump, open-work count, cancel-and-archive ≤64 roots, native refusal, recount, audit) and `require_active_unit_on`/`require_active_desk_on`; `workflow::cancel_for_archive_on`.
- Refusals wired: admission (before management, company lock first), retry, native MCP admit, local session start, run-now, schedule create/update/enable, sweeper filter + post-claim `Materialized::Skipped` (`discard_skipped`), workflow binding (non-deactivate), transport binding create/re-activate, responder config, desk create/assign/edit (`prelock_ownership_on`; move-out passes), department update/create-under/members/roles/placement, unit budget set/clear. API: `POST …/departments/{id}/archive` (`cancel_open_runs`), `POST …/restore`.
- Disposition noted: config writes keep their authorization first, so a unit's own lead on an archived unit sees NotFound (no longer manages it), a manager above sees `unit.archived`; only admission puts the archived check before authority (F1). Canonical desk email binding changes only through desk writes (already refused).
- Next: B1 tests per the acceptance mapping, then focused runs and freeze for B review.

## B1 verification (2026-11-21)

Fixture identity: dev DB `openbot_merge08_mac02` and test DB `openbot_merge08_mac02_test`, both recreated fresh after the last migration edit; `SQLX_OFFLINE=true`. `cargo fmt` clean; `cargo sqlx prepare --check -- --all-targets` passes (B1 adds only runtime `sqlx::query` calls, no `.sqlx` change). Focused command: `cargo test --lib -- unit_archive` (+ `message_admission_classifies`): all pass: 29 `unit_archive_*` + `message_admission_classifies_by_producing_condition` = 30 (discovered list `logs/08G-P02-B1-focused-list.txt`; rerun after the inventory fix `ok. 30 passed`). Broad lib run (shared infrastructure: new `workflow_runs` trigger and run guards): `cargo test --lib` → `logs/08G-P02-B1-lib-suite.txt`: 4998 passed, 12 failed, 30 ignored (4433 s). Dispositions: (1) REGRESSION, fixed: `workflow_action_reconciliation_claim_sql_multiple_applicable_candidates` pins the BEFORE UPDATE row-trigger inventory of `workflow_runs` (`action_reconciliation_claim_sql_candidate_support.rs` `capture_catalog`); B1 adds `workflow_runs_active_unit`, so the inventory now names it. Rerun passes (`logs/08G-P02-B1-lib-rerun.txt`). (2) UNRELATED, pre-existing: `doop_agent_current_preparation_reaches_ready_before_model_io` expects prepared `version` 7, while `PREPARED_AGENT_VERSION` is 8 since `2edd0a73` (already in HEAD); neither file is in this phase's diff. It fails in isolation too. Left unchanged. (3) UNRELATED, environment: 7 cloudroom tests (`local_agent_tests::e2e::*` ×5, `cloudroom_e2e_crash_after_admission_before_peer_connect_dispatches_once`, `retention::peer::cloudroom_lifecycle_real_server_plugin_retention_*`) fail on `ERR_DLOPEN_FAILED`: `integrations/cloudroom-plugin/node_modules/fs-ext` was compiled for another NODE_MODULE_VERSION than the local Node v26.13.1 (147). No plugin or node file is in this phase's diff. Unverified here, to be re-checked at the final gate. (4) LOAD timing, not reproduced: `doop_connection_verification_identity_failures_stop_at_distinct_steps_without_writes`, `doop_result_final_read_foreign_identity_workspace_title_or_error_cannot_deliver`, `doop_result_invalid_created_path_segment_cannot_invoke_editor_or_deliver` (timeouts) and `workflow_mcp_revocation_and_rotation_racing_dispatch_never_call` (`DatabaseTimeout` persistence bound) all pass when rerun in isolation (`logs/08G-P02-B1-lib-rerun.txt`).

| Criterion | Test(s) | Decisive assertion |
| --- | --- | --- |
| 02A.1 root | `department::tests::archive_tests::unit_archive_root_is_refused` | port BadRequest; raw UPDATE → `departments_root_archive` |
| 02A.2 guard | `unit_archive_guard_refuses_active_units_under_an_archived_parent` | create/reparent → `UnitArchived`; raw insert/reparent/restore → `departments_archived_parent`; parent-only archive → `unit_archived_subtree` |
| 02A.3/4 round trip | `unit_archive_round_trip_keeps_earlier_timestamps_and_restores_one_unit` | child keeps earlier `archived_at`; parent archive records no event for the archived child; restore one; `enabled` unchanged; `takes_traffic` false/true; provenance user+owner |
| 02B.1 refuse | `workflow::…::unit_archive_runs::unit_archive_refuses_open_work_and_changes_nothing` | `UnitArchiveOpenWork{1,true}`, run still queued, unit active |
| 02B.1 cancel (U3) | `unit_archive_cancel_by_the_parent_manager_ends_open_runs_and_records_who` | parent lead (no desk grant) cancels; each run `cancelled` + one `control_cancel` with the lead |
| 02B.1 bound | `unit_archive_cancel_refuses_past_its_bound_and_cancels_nothing` | 65 open → `{65,false}`, all still open |
| 02B.1 native | `local_agents::…::unit_archive_runs_native_mcp_refuses_on_an_archived_units_desk` | live native call → Cancel refused `{1,false}` |
| 02B.2 admission (D1.5) | `unit_archive_runs_admission_refuses_on_an_archived_desk_and_resumes_after_restore` | unit's own lead: NotFound while active, `UnitArchived` once archived (check before authority); owner `UnitArchived`; command prepared before archive refused by persistence; zero runs written; restore admits |
| 02B.2 message code | `message_admission_classifies_by_producing_condition` | `UnitArchived` → `Refused("unit.archived")` |
| 02B.2 retry + trigger | `unit_archive_runs_retry_and_the_run_trigger_refuse_on_an_archived_desk` | retry `UnitArchived`; raw reopen satisfying the transition guard → `unit_archived`; raw INSERT → `unit_archived`; retry after restore Applied |
| 02B.2 native admit | `unit_archive_runs_native_mcp_refuses_on_an_archived_units_desk` | session started before archive: call → `UnitArchived`, no run; after restore runs |
| 02B.2 local session | `unit_archive_runs_local_session_refuses_on_an_archived_units_conversation` | `UnitArchived`, zero sessions; restore starts |
| 02B.2 schedules | `schedule_tests::archive::unit_archive_runs_schedule_is_untouched_while_archived_and_fires_once_after_restore`, `…_claimed_before_the_archive_is_dropped_not_failed` | drain empty, columns unchanged, no occurrence; create/update/enable/run-now `UnitArchived`; disable+delete Ok; after restore one-off and interval each fire once; claimed → `Skipped`, row gone, no `last_error`, no run |
| 02B.3 mail | `thread::tests::unit_archive_refuses_mail_to_an_archived_units_desk` | bounce with `disabled_slugs=[support]` |
| 02B.3 relay | `unit_archive_refuses_relay_to_an_archived_units_desk` | `relay_desk_can_participate` true→false→true |
| 02B.3 channel.call | — | NOT EXERCISED (no producer) |
| 02B.4 config | `unit_archive_refuses_configuration_of_an_archived_unit`, `unit_archive_refuses_binding_configuration_and_allows_deactivate`, `integration::tests::unit_archive_refuses_transport_binding_link_and_reactivation`, `unit_archive_refuses_responder_configuration` | each write `UnitArchived` and unchanged rows; deactivate/disable allowed |
| 02B.4 allowed (U4) | `unit_archive_allows_desk_delete_desk_move_out_and_unit_delete` | move-out Ok and desk takes traffic; desk delete; unit delete |
| 02B.5 trigger | `unit_archive_refuses_trigger_writes_and_allows_deletes` | raw member/role/desk → `unit_archived`; raw delete and retirement remove rows |
| 02D.1/2 | `unit_archive_authority_parent_manager_archives_and_archived_roles_do_not_manage` | child lead NotFound; parent lead archives (`lead:<parent>` audit); archived roles → None incl. `any_unit_management_role` |
| 02D.3 | `unit_archive_authority_sponsor_decider_manages_unit_and_descendants` | `sponsor:<unit>` for unit and child; informed → None; audit `sponsor:`; archived → None |
| 02D.4 | `unit_archive_authority_agent_in_only_archived_units_is_managed_by_root` | lead → None after archive; owner/admin manage |
| competing | `unit_archive_competing_overlapping_archives_agree`, `unit_archive_competing_trigger_write_waits_for_the_archive`, `unit_archive_competing_admission_and_archive_agree`, `unit_archive_competing_cancel_and_a_running_job`, `thread::…::unit_archive_competing_mail_commit_queued_behind_an_archive_is_refused`, `…_archive_queued_behind_a_mail_commit_applies_after_it` | one event per unit; writer blocked then `unit_archived`; exactly one winner; run cancelled + late result fenced (or rollback); mail Superseded with nothing stored / message stored then archive |

Stand-ins and gaps: the Rust relay pre-filter (`ingest/relay.rs:192`) is covered through `Channel::takes_traffic` (round trip) and the SQL function, not a relay end-to-end. Message admission is covered by `classify` plus the persistence admission test, not the Deployment harness. Desk grant/address writers (D1.8 row), traced with `trail callers`: `channel.rs` `insert_desk_addresses` and `insert_email_allowlist_grants` are reached only from desk create (`create` → `insert_channel_row_on` → `prelock_ownership_on`), desk edit (`update_channel_on` → `write_channel_settings_on`, after `prelock_ownership_on`) and department-desk provisioning (`insert_plain_desk_on`, at department create, refused under an archived parent), so each is already refused on an archived unit (asserted by the create/edit cases of `unit_archive_refuses_configuration_of_an_archived_unit`). `participant.rs:815` is a cross-tenant schema test, not a writer. `account_identity.rs:238` `adopt_outsider_places` moves an outsider's existing grants to the person who claimed the address. It is identity consolidation that preserves access already held, not desk configuration, so it stays allowed on an archived unit (disposition: allowed, no new access).

## B1 corrections for 08G-P02-B1-R1 (2026-11-22)

Artifact `a10ec4c20a9011c4af28694f27467765e02ed34dd8cf54d33e193229771012ad` (base `42bc327e`, exclusion `plan/08-goals/phases/p02/docs`, `logs/08G-P02-B1-snapshot-r2.txt`; delta vs r1: the files below plus the two new test files). Manifest `logs/08G-P02-B1-R2.evidence.json`.

- **F1 (local session `Send`)**: `local_agents/management/writes.rs` gains `require_active_association_on`. It resolves the desk of the session's association (conversation → `threads.channel_id`, workflow → `workflow_runs.channel_id`) and calls `require_active_desk_on`. Both `start` and every effect `control` command (`Send`; `is_effect`) call it. `Inspect`, `Stop` and `Archive` stay allowed. A replay of a recorded request returns before the check, as before. Test `unit_archive_runs_local_session_refuses_on_an_archived_units_conversation` (extended):
  - a session started before the archive gets `UnitArchived` on `Send`, and no operation row is added;
  - Inspect and Stop succeed;
  - after a restore, `Send` succeeds and a new start succeeds.
- **F2 (Deployment harness)**: new `workflow/unit_archive_ingress_tests.rs` (module of `message_ingress_tests.rs`), test `unit_archive_runs_message_to_an_archived_units_desk_admits_nothing`. It runs on the scripted-model `Deployment`, with the desk moved into a unit and a direct-trigger binding.
  - A message is accepted while the unit is active, then the unit is archived (a pending intent is not open work).
  - The production engine runs until the intent is `done`. The `workflow_message_intent_bindings` row is `("refused", NULL, "unit.archived")`, zero runs are written, and the model is never called.
  - A second message after the archive is bounced at ingest with no intent recorded.
  - The run-as = unit's own lead arm stays covered at the admission layer by `unit_archive_runs_admission_refuses_on_an_archived_desk_and_resumes_after_restore` (NotFound while active, `UnitArchived` once archived). The harness binds as the fixture owner.
- **F3 (HTTP routes)**: `routes/department.rs` gains a `#[cfg(test)] lifecycle_router()` serving only the two routes over `Arc<DepartmentUseCases>`. Test `unit_archive_http_routes_refuse_open_work_cancel_on_request_and_restore` drives real requests with a verified-session extension over the run fixture:
  - no body → 409 `{"code":"unit.archive_open_work","count":1,"cancellable":true}` and the unit stays active;
  - `{"cancel_open_runs":false}` → 409;
  - an unknown field → 4xx (not 409) and nothing is cancelled;
  - `{"cancel_open_runs":true}` → 200 with `department.archived.at`, and the run is `cancelled`;
  - after the parent is archived, restoring the child → 409 `unit.archived`;
  - restoring the parent, then the child → 200 with `archived` null.
- **F4 (cloudroom)**: the earlier failures were `ERR_DLOPEN_FAILED`: the ignored `integrations/cloudroom-plugin/node_modules` had native modules (`fs-ext`, `better-sqlite3`) built for NODE_MODULE_VERSION 137, while the local Node v26.13.1 needs 147. I ran `npm rebuild` there, which is an environment-only change to ignored files. The six tests then ran and PASS (`logs/08G-P02-B1-R2-cloudroom.txt`):
  - `e2e::cloudroom_e2e_competing_workers_native_thread_followup_controls_and_company_fences`
  - `e2e::cloudroom_e2e_cut_points_reconstruct_server_plugin_and_workers_without_blind_native_retry`
  - `e2e::early::cloudroom_e2e_committed_delivery_marker_before_socket_never_blindly_dispatches`
  - `e2e::early::cloudroom_e2e_crash_after_admission_before_peer_connect_dispatches_once`
  - `e2e::early::cloudroom_e2e_crash_before_admission_commit_reconstructs_workers_server_and_peer`
  - `retention::peer::cloudroom_lifecycle_real_server_plugin_retention_preserves_prepared_and_unseen_queued_work_once`

  The count is six, not seven as written above. The broad-run disposition (3) is superseded: these are not unrelated, they are now exercised and pass.
- **S1 (optional)**: not taken in B1. The `sponsor:` acting-role decode on the department page belongs to 02E (Batch 2). The other two suggestions are noted for the final review.
- **Checks**:
  - `cargo test --lib -- unit_archive message_admission_classifies local_agents::management routes::department`: 77 passed, 0 failed, 2 ignored (`logs/08G-P02-B1-R2-focused.txt`). The discovered `unit_archive` + classify names are 32 (`logs/08G-P02-B1-R2-focused-list.txt`).
  - `cargo fmt --check` clean.
  - `cargo sqlx prepare --check -- --all-targets` exit 0.
- **Root settings observation**: this session's environment reads `CLAUDE_EFFORT=high`. The user confirmed root as Opus/xhigh at the start, and no readback here distinguishes the two. Recorded, not resolved.

## B1 acceptance (2026-11-22)

Accepted in phase (not integrated, not committed). B's PASS `08G-P02-B1-R2-N1` names digest `a10ec4c20a9011c4af28694f27467765e02ed34dd8cf54d33e193229771012ad`. Root recomputed the same digest (exclusion `plan/08-goals/phases/p02/docs`, HEAD `42bc327e`). Manifest `logs/08G-P02-B1-R2.evidence.json` validates. Coordination r3 is unchanged (sha256 `4ea05356…`) with no notices. Open findings: none. Optional suggestion `08G-P02-B1-R1-S1` is carried forward to Batch 2/final review.

Carried limits:
- `channel.call` is NOT EXERCISED.
- Production router registration of archive/restore is verified by inspection only.
- The deadlock arm of cancel-and-archive is not produced by any test.
- The full suite is due at the final gate.

## Batch 2 implementation status (checkpoint 2026-11-22)

There is no architecture gate: everything reuses B1's accepted patterns (the `require_active_*` helpers, `FOR KEY SHARE` guard triggers, the `unit_archived` constraint mapping). Sweep: `logs/08G-P02-B2-sweep.md`. Each change is listed with A's decisions.

- **02A.5 listing argument**: `ArchivedUnits {Exclude, Include}` (domain `department.rs`) is taken by every read that lists units:
  - ports: `list_by_company`, `list_for_user`, `agent_departments`, channel `list_departments`, `managed_unit_choices`;
  - use cases: `list_departments`, `visible_departments`, `desk_departments`;
  - SQL: `departments_of_company`, `agent_departments_on`, roster `departments_on`, `unit_budgets_on`.

  Caller decisions:
  - desk lists, attention, dashboard, run board and reviewer chain → Include, because labels of past work need archived owners;
  - option lists use `DeskDepartments::choices(selected)`, which excludes archived units unless one is already selected;
  - mail bounce directory → Exclude;
  - company settings department count → Exclude;
  - agent directory, Agent Builder ListAgents and agent badges → Exclude;
  - run prompt roster children → Exclude (the owner and parent stay Include);
  - resource pickers → Exclude;
  - a unit's own model-connection form read → Include;
  - JSON `GET /departments?include_archived=true`.
- **02C.1 knowledge**: `Permissions::require_write_into`, which is `require_write` plus `require_active_unit_on`, is used for upload, preflight and the destination of a move. Moving out and deleting stay allowed (A's decision, consistent with U4, and needed because `knowledge_documents_unit_fk` is RESTRICT on unit delete). The knowledge picker excludes archived units unless one is selected.
- **02C.2 resources**: `move_resource_on` refuses an archived target (`require_active_unit_on`) and allows an archived source. Creating a resource in an archived unit is refused by the new trigger. Editing a resource that stays in an archived unit is not refused: it is not in the plan's 02B.4 list, and nothing can select the resource. Disposition: allowed.
- **02C.3 budgets/usage**: `UnitBudgetStatus.archived`; `UsageQuery.archived` comes from the usage page's `include_archived` checkbox; the dashboard panel uses Exclude; rows are marked "(archived)".
- **Schema backstop (02C)**: `guard_active_resource_unit()` on `knowledge_documents`, `skills`, `company_mcp_connections`, `workflow_http_connections` and `company_model_connections`, all BEFORE INSERT OR UPDATE OF unit_id, and on `unit_budgets` BEFORE INSERT OR UPDATE. Deletes pass.
- **02E.1 department page**:
  - archived units sit behind `<details id="department-archived">` "Show archived (N)", which is open when an archived unit is selected;
  - an "Archived" badge;
  - a lifecycle card (archive / "Cancel open runs and archive" after a cancellable open-work refusal / archived-at-by + restore, or "restore the department above it first"), which now also holds Delete;
  - an archived unit gets no edit, lead, sponsor, escalation, member, model or cap form;
  - the parent picker excludes archived units;
  - UI routes `POST /ui/companies/{c}/departments/{d}/archive|restore`;
  - S1: `sponsor:` acting roles are decoded.
- **02E.2**: department filters, the dashboard department list and run-board scope options exclude archived units. Desk owner pickers offer an archived unit only as the current owner, so move-out stays possible. Desk group headings, desk badges and attention group headings show "Name (archived)".
- **02E.3**: `docs/departments.md` gains an "Archive" section before "Delete", plus the JSON API rows and refusal codes.
- **Tests added**:
  - `unit_archive_listing_leaves_archived_units_out_unless_asked`
  - `knowledge::…::unit_archive_scopes_knowledge_takes_nothing_in_and_lets_documents_out`
  - `unit_archive_scopes_parent_lead_reads_the_archived_child_and_a_sibling_does_not`
  - `unit_archive_scopes_a_resource_moves_out_and_never_in`
  - `unit_archive_scopes_usage_shows_archived_caps_only_when_asked`
  - page tests `unit_archive_ui_*` (list, archived card, open-work card, filters and picker, usage page)
  - route test `unit_archive_ui_routes_archive_restore_and_refuse_on_the_card`
- **Checks so far**:
  - `cargo check --lib --tests` (SQLX_OFFLINE) is clean;
  - the migration applies on fresh DBs;
  - the first focused run (`cargo test --lib -- unit_archive`) was KILLED by Claude Code for low system memory (other sessions' builds on this host). It is not restarted without the user's go-ahead, and has no result.

## Batch 2 verification (2026-11-22)

The user said go to the rerun. To leave memory headroom, all runs use `-j 4 --test-threads 4`. Env: dev DB `openbot_merge08_mac02`, test DB `openbot_merge08_mac02_test` (localhost), `SQLX_OFFLINE=true`.

- `cargo test --lib -- unit_archive`, first rerun:
  - two compile errors in the new usage test (the closure moved the fixture); fixed with a borrow.
  - then 3 failures:
    - UI list test: the end marker of the active-tree slice was wrong; it now ends at `<details id="department-archived"`. Test bug.
    - listing test: `list_for_user` assumed the member held only the two new units, but the fixture places them in another unit. It now asserts containment. Test bug.
    - **resource test (a product defect)**: creating a skill in an archived unit returned `BadRequest("The skill relationship does not belong to the same company.")`. `skill::write_error` renamed every `23514` error, which hid the trigger's `unit_archived`.
      - Fix: `persistence::refused_for_archived_unit(&sqlx::Error)` is now the one predicate. `From<sqlx::Error>` and `skill::write_error` both use it, and `write_error` checks it first.
      - Other 23514 renamers were swept. `agent_capability::relationship_error` (tool bindings), `integrations::mod` (integration tables) and `decisions::closed_at_commit` (wait guard) write no table with a unit guard trigger, so they are left unchanged.
- Rerun: `cargo test --lib -- unit_archive` passes, 42 tests (`logs/08G-P02-B2-focused-first.txt`).
- Affected suites: `cargo test --lib -- knowledge unit_resources department model_usage unit_budget attention channel skill mcp administration dashboard roster provisioning agent_authoring reviewer_picker runs::` passes: 1490 passed, 0 failed, 7 ignored (`logs/08G-P02-B2-focused-affected.txt`).
- `cargo fmt --all -- --check` passes after `cargo fmt --all`, which made layout-only edits in two new test files (`logs/08G-P02-B2-fmt.txt`).
- `sqlx prepare --check`: the diff adds no `query!`-family macros, so `.sqlx` is unaffected. Deferred to the final gate (each run builds again).
- Not exercised in B2: the competing-claimant interleavings are B1's, and B2 adds no new lock protocol. The guard trigger takes the same `FOR KEY SHARE` lock as B1's `channels_active_unit`, whose race test is B1's.
- Artifact for B2-R1: base and HEAD `42bc327e61674dcd5114f3c948454036dfd51506`, digest `51dd88e18771de73170477a1a633f13bf5adeecb34f9c59835ed1fcc60849afa`, which excludes only `plan/08-goals/phases/p02/docs` (`logs/08G-P02-B2-snapshot-r1.txt`). The manifest `logs/08G-P02-B2-R1.evidence.json` is valid.
- Timing note: `cargo fmt --all` reformatted two new test files (layout only) while the affected-suite run was compiling. That run may therefore have built the pre-format layout, which is token-identical.

## B2 corrections for 08G-P02-B2-R1 (2026-11-22)

User decision on F3, 2026-11-22: **"Desk history only"**. Membership and roles in an archived unit grant reading its desk history and nothing else.

- **F1** (memory hits from an archived child are not marked):
  - `MemoryScope.archived` is the unit's live state, read by `mark_archived_on` in `memory_tool_checkpoint.rs`. The frozen `label` is unchanged because recall uses it as an identity key (`recall.rs` freeze match).
  - Tool results (search, timeline, details) and the recall prompt carry `"archived": true` on such items through `memory_tools::mark_archived`. The tool output schema allows it as an optional `{"const":true}`.
  - `RecallNamespace.archived` and `RecalledMemory.archived` carry the flag. The freeze copies it from the eligible scope.
  - Knowledge passages carry no unit label today (`{file, heading, text}`), so the knowledge clause has no label to mark. Disposition: not applicable.
  - A first attempt that appended " (archived)" to the label was rejected by the output schema's scope pattern, and is replaced by the flag.
- **F2** (the memory half of `unit_archive_scopes_` is missing): new `memory_scope_selector_tests::unit_archive_scopes_lead_reads_an_archived_childs_memory_marked_archived`.
  - The lead's agent reads `report-a`'s memory after its archive through the real use case, marked archived; the active `report-b` carries no flag.
  - The namespace was provisioned by the fixture beforehand and is not `removing` after the archive.
- **F3** (02C.4), per the user decision:
  - `workflow_target_departments` (init migration): the member and role arms skip archived units, so an archived placement places nothing for role binding, the role-context guard or run chains.
  - `run_scopes` lead children: a lead role on an archived unit grants no child reads.
  - Knowledge `Permissions`: placements and lead children skip archived units. The parent's lead keeps the one-step read, and root managers still read everything.
  - Agent Builder requester placement (`provisioning_builder.rs`): skips archived units.
  - Kept: desk grants (`department_desks.rs` ELIGIBLE), which are the desk-history reading. Root-unit `root_user_id` memberships are not archivable.
  - Tests:
    - `unit_archive_keeps_desk_history_readable_by_the_units_member`: `get_readable_channel` still succeeds after the archive.
    - `knowledge::…::unit_archive_scopes_roles_in_an_archived_unit_read_no_knowledge`: before the archive, a lead reads its own unit and its child. After it, the lead reads neither, and a top-level lead still reads the archived unit.
  - The run-scope lead-children change has no test of its own: it is the same SQL predicate as the knowledge change. It is exercised only by the existing run-scope suites, for no regression.
  - DBs recreated after the migration edit; migrate is clean.
- **F4** (the usage checkbox governs only caps): `load_calls` takes `query.archived` as `$7`. With Exclude, calls attributed to an archived unit are left out unless it is the drill-down unit. Department names of archived units read "Name (archived)".
  - The test now also asserts that Exclude yields no group and that Include yields `["support (archived)"]`.
- **S1**:
  - Run-board scope options: kept as `choices(None)`. They are the scopes a board can be scoped to anew, and a board already scoped by URL still renders its runs.
  - `managed_units` takes no listing argument deliberately: it feeds authorization sets, which archived units must stay in for management from above.
  - Resource edits in an archived unit are allowed and now documented in `docs/departments.md`.
- `docs/departments.md` Archive section updated for F3 and F4 and the resource-edit note.
- Checks:
  - `cargo test -j 4 --lib -- unit_archive memory_scope memory_tool memory_recall memory_tools unit_budget model_usage recall agent_memory knowledge run_scope role_binding role_context provisioning --test-threads 4` passes: 462 passed, 0 failed, 2 ignored (`logs/08G-P02-B2-R2-focused.txt`).
  - Wider affected run: see `logs/08G-P02-B2-R2-affected.txt`.
  - The wider affected run passes: 2825 passed, 0 failed, 16 ignored. `cargo fmt --check` passes.
  - Artifact for B2-R2: HEAD `42bc327e…`, digest `6de44ac6cdd14b58bf950e56b1332842be5b4d24585f50c3e196eb4d3d33e2b5`, same exclusion (`logs/08G-P02-B2-snapshot-r2.txt`). Manifest `logs/08G-P02-B2-R2.evidence.json` is valid.

## B2 correction for 08G-P02-B2-R2 (2026-11-22)

- **F1** (archive and restore skip the role-binding recheck):
  - `archive_department_on` and `restore_department_on` now lock the principals placed in the affected units, as delete does, and run `role_binding::check_principals_on` after the update.
    - An archive is refused when an active direct binding of an agent placed only there would lose its one department.
    - A restore is refused when it would make such an agent ambiguous.
  - The placed-principals query is shared (`archive::placed_principals_on`), and delete now uses it too.
  - Test `direct_role_binding_tests::unit_archive_rechecks_direct_role_bindings_on_archive_and_restore` covers both refusals, the archive after the binding is deactivated, and the activation while the unit is archived.
  - Scenario 2 (an in-flight direct-address run whose target loses its context):
    - Disposition: same behaviour as every existing roster write. `check_principals_on` inspects active bindings, not running runs. Member removal and department delete already have this exposure, and the role-context guard then refuses the manifest insert rather than running with a wrong context, so it fails closed.
    - No new test. It needs no new rule, being the same contract as the existing roster writes.
  - `docs/departments.md` gains one paragraph on the refusal.
- Checks:
  - `cargo test -j 4 --lib -- unit_archive direct_binding role_binding role_context department --test-threads 4` passes: 290 passed, 0 failed (`logs/08G-P02-B2-R3-focused.txt`).
  - fmt passes.
  - Digest `b066cce60ef4ee7a3928db2f5e2f6054aaa1cf922d129b44a2b61b7654123559` (`logs/08G-P02-B2-snapshot-r3.txt`); manifest `logs/08G-P02-B2-R3.evidence.json`.

## B2 acceptance (2026-11-22)

- B2 is ACCEPTED in phase (not committed) at digest `b066cce60ef4ee7a3928db2f5e2f6054aaa1cf922d129b44a2b61b7654123559`.
  - Root recomputed the digest and it matches. B's PASS is `08G-P02-B2-R3-N1`.
  - Coordination r3 is unchanged (`4ea05356`) and has no applicable notices.
  - Findings: B2-R1-F1…F4 and B2-R2-F1 are fixed; B2-R1-S1 is closed with recorded dispositions.
- Limits carried to the final gate:
  - The full suite and `sqlx prepare --check` have not run since Batch 2.
  - The archive's principal-lock order (after desk gates and run heads) has no race test. It runs under the company write lock that admissions and roster writes take first, so it is serialized with them.
  - `channel.call` is still NOT EXERCISED (from B1).
  - The in-flight direct-address run disposition is untested.

## Final gate (2026-11-22)

- **Run 1:** `./scripts/check.sh` with `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4`. Stopped at the user's choice about 55 minutes in, to restart with more test threads once memory allowed (about 20 GB free). Partial log: `logs/08G-P02-final-check-run1-stopped.txt`. No leftover processes or `openbot_check_*` databases.
  - Seen before the stop:
    - every gate through "Compile tests" passed, including fmt, offline compilation, migrations and the SQLx metadata check, except Clippy;
    - **Clippy FAIL**: 9 `redundant_field_names` errors in test files (`role: role` / `principal: principal`), none in this phase's diff. They came in with merge `73afd5a7` (plans 04–07), so they are unrelated and left unchanged;
    - the DB suite had 3736 ok before the stop, plus the known pre-existing `doop_agent_current_preparation_reaches_ready_before_model_io` failure (version 7 vs 8).
- **Run 2:** `./scripts/check.sh` with `CARGO_BUILD_JOBS=6 RUST_TEST_THREADS=8`, log `logs/08G-P02-final-check.txt`. It ran about 12:37–14:45 and exited 1. Per-gate results:
  - **PASS:**
    - formatting, patch whitespace, offline compilation, fetch locked deps;
    - fresh-DB migrations, SQLx metadata matches migrated schema, compile tests;
    - integration boundary / docs / conformance families;
    - workflow sample boundary, workflow evidence validators and retained evidence;
    - generated CSS, frontend protocols, browser protocols (native markup), ignored browser fixture producers;
    - deployment script tests, knowledge artifact preparation, doop runner boundaries and probe protocols;
    - local-agent plugin tests, runner test coverage;
    - lease budget, memory model preparation, memory backup/restore, OpenBot and memory integration.
  - **Database-backed test suite: 5026 passed, 1 failed, 30 ignored.** The one failure is the known pre-existing `doop_agent_current_preparation_reaches_ready_before_model_io` (version 8 vs 7). It is unrelated: it failed the same way before this phase's work and is recorded in run 1.
  - **Stack budget (2 MiB): 5024 passed, 3 failed.** The script itself reports "tests failed for a reason unrelated to the stack budget"; no overflow.
    - The known doop failure above.
    - Two load timeouts ("document timed out", waiting on an effect): `doop_agent_published_editor_approvals_and_worker_restart_reuse_one_canvas` and `doop_result_ignores_forged_urls_and_preserves_private_message_level`.
    - Both passed in the main suite on the same code, and both pass alone at `RUST_MIN_STACK=2097152` (`logs/08G-P02-final-doop-docs-rerun.txt`, 2 passed in 73.5 s). They are timing flakes under 8-thread load, and this phase does not touch doop documents.
  - **FAIL, pre-existing or environment (not this phase):**
    - **Clippy:** the 9 `redundant_field_names` errors from merge `73afd5a7` (see run 1).
    - **Transport abstraction boundary:** `rg` is not on PATH for plain bash here, so the fixture check passes vacuously and the self-test reports "gate accepted rig in application". Reproduced with `bash -x` ("rg: command not found"). This phase does not touch `scripts/`.
    - **Query statistics prerequisites:** local Postgres lacks `pg_stat_statements.track` (not in `shared_preload_libraries`).
    - **Reproducible plugin package:** Node v26.13.1 installed, pinned v24.21.0 required.
    - **Memory extracted service suite, memory standalone journey, attribution extracted service:** system Python 3.9 has no `tomllib`.
    - **Knowledge, memory and attribution production images:** no Docker daemon.
    - **Attribution browser integration:** `attribution_sso_browser_real_openbot_password_and_existing_session` fails at "independent secure cookies". The same failure is in `plan/04-root-unit/phases/p03/docs/logs/B1-full-check.txt`; it predates this phase, and the phase diff touches no attribution, user or session code.
    - **Managed Doop service:** `DOOP_TEST_UI_CHROME` is unset (no sandboxed system Chrome).
  - **Not verified here (unavailable):** clippy-clean build, the Docker image gates, the extracted memory/attribution suites, the managed Doop gate, query-statistics prerequisites and plugin reproducibility. They need a Node 24 / Python 3.11+ / Docker / Chrome / `pg_stat_statements` environment.
- **Concurrent commit during the gate:**
  - At 14:13 another session committed `ce0bb1e1` ("Plan access rules…": `plan/11-access-rules/*`, three `.trail/learnings/2026-11-22-*` files, and 6 lines adding the plan-11 files to `NO_BACKWARD_COMPATIBILITY_NOTES` in `src/main.rs`). It holds none of this phase's edits.
  - The gate's builds predate it.
  - No phase file was modified after the accepted R3 snapshot (mtime check against `logs/08G-P02-B2-snapshot-r3.txt`). The file list matches except that learning, which is now committed.
  - On the combined tree, `python3 scripts/check-integration-docs.py` PASS (`logs/08G-P02-final-integration-docs-rerun.txt`: 192 startup reset notices) and `cargo check --bin open_bots` PASS.
  - The current snapshot reads base `ce0bb1e1`, digest `90f18e31d3ee809fd39dab3abddc7efe67bcd78e6ccfc41e295e2be0ba5a2628`. The phase content is unchanged from accepted `b066cce6…`; only the base moved under it.
- **Gate-driven changes:** none, so there is no B recheck. Coordination r3 (`4ea05356…`) is unchanged, with no applicable notices.
- **Completion:** scratch DBs `openbot_merge08_mac02` and `openbot_merge08_mac02_test` dropped (no active connections; no `openbot_check_*` DBs left). Nothing committed, pushed or deployed: that needs user authorization.
