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
