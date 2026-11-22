# 08G-P02 reviewer findings

Owner: reviewer B. Shared context for every entry below unless stated otherwise:

- Date 2026-11-21. Workflow `implement-opus-reviewed-2_4`. Phase `08G-P02` (p02), batch B1, stage **architecture gate**.
- Review round note: `08G-P02-D1-R1-N1` in [agent-review-notes.md](agent-review-notes.md). Task [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md).
- Reviewer: Fable (`claude-fable-5-2`) / medium. Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`.
- Reviewed: decision revision **D1-r1** (retained copy `logs/08G-P02-D1-r1-decision.md`, sha256 `d15af646…dffa0f3`), at HEAD `42bc327e61674dcd5114f3c948454036dfd51506`, artifact digest `dfb7097b05f83153178c265aad32a87fb36534839a74fa3c624daf5f930bed34` (exclusion `plan/08-goals/phases/p02/docs`). No implementation exists.
- All defects below originate in the **gated decision**, not in an implementation. Evidence is code inspection at the stated HEAD; nothing was executed.

---

## 08G-P02-D1-R1-F1 — `unit.archived` is masked by D1.10 for a binding whose run-as is the unit's own lead

- **Classification:** actionable defect · medium · correctness / requirements (D1.5 inconsistent with D1.10 and with 02B.2).
- **Evidence:**
  - D1.5 places the check "right after `management::authorize_association_on`".
  - `src/adapters/persistence/workflow/management.rs:116-126` (`authorize_association_with_locks`): after the company lock it calls `management_role_on(actor, target)` and returns `missing()` (`AppError::NotFound`) when the actor does not manage the desk's unit. At admission the actor is the binding's stored `run_as` (`src/application/workflow/message_admission.rs:497`, `actor: binding.run_as`), not the sender.
  - D1.10 makes roles held on an archived unit stop managing it. A binding activated by the unit's own lead (the ordinary delegated case; `bindings.rs:228` requires "run-as must currently manage its unit") therefore fails `authorize_association_on` with `NotFound` as soon as the unit is archived, before the D1.5 statement runs.
  - `message_admission.rs:462` classifies `NotFound` as `Refused("unavailable")`; the schedule path records it through `record_refusal` (`schedules.rs:836`, `materialization_status='failed'`).
  - Required (02B.2): admission "refuses with `unit.archived`"; schedules "not recorded as failures". Observed under D1-r1: the code depends on who `run_as` is — `unit.archived` only when run-as manages from above (owner, admin, ancestor lead). For an occurrence claimed just before the archive, D1.6's delete-instead-of-failed branch is not reached and a `failed` row with a refusal metric is written.
- **Impact:** wrong refusal code and a failure row for the most common binding shape; the planned tests pass if their fixture's run-as is the owner, so the verification plan would not detect it.
- **Required change:** read the unit state under the company lock *before* the management-role check (e.g. between `authorize_person_with_locks` and `management_role_on`, or as a separate step ahead of `authorize_association_on`), and add a discriminating case to `unit_archive_runs_`: binding run-as = the archived unit's own lead, message and schedule both, asserting `unit.archived` and no `failed` schedule row.
- **Instruction lesson:** observation — two decisions in one gate interacted (authority narrowing changes which error an earlier check returns). Candidate (scope: architecture gates): when a decision narrows an authority predicate, list every caller that evaluates that predicate for a *stored* actor and state the resulting error ordering.
- **Resolution history:** 2026-11-21 open.
  - 2026-11-21 recheck `08G-P02-D1-R2-N1` (D1-r2, same HEAD/digest): **fixed in the decision** — check moved ahead of the management-role check; mapping row 02B.2 has run-as = own lead and = owner. Placement wording raises `08G-P02-D1-R2-F2`.

## 08G-P02-D1-R1-F2 — Run-opening paths other than `admit` are not covered; a run can be opened on an archived unit

- **Classification:** actionable defect · high · requirements / correctness / concurrency (D1.4, D1.5; 02 Outcome "admits no work", 02B.1, `unit_archive_competing_` "never a run on an archived unit").
- **Evidence:** D1.5 puts the only refusal in `WorkflowAdmission::admit` (`workflow/admission.rs:11`). The startup map §1 lists only that path. Other writers that open a run on a desk association:
  1. **Operator retry reopens a finished run in place.** `workflow/controls.rs:49` (`retry`) → `control` → `control_retry::retry_on` (`workflow/control_retry.rs:56-59`): `UPDATE workflow_runs SET state='running' …` and the job back to `pending`; trigger `stamp_workflow_run_finished` (`init_schema.sql:9451-9463`) clears `finished_at`. Authority is company lock + `association::authorize_association` (desk access); no unit state is read. After an archive (which required zero open runs) a failed run on an archived unit's desk can be retried and executes.
  2. **Native MCP admission.** `local_agents/protocol/mcp.rs:46` → `workflow/native_mcp/mod.rs:89` (`admit_on`) calls `admission_write::insert_run` with `association: owner.association`, which `source_facts_on` (`native_mcp/mod.rs:38-51`) derives from the session's conversation thread (`threads.channel_id`). It does not pass through `admit`. It does take the company row (`local_agents/delivery.rs:23`, `authorize_company_with_locks`), so it serializes with archive, but nothing refuses it.
  3. **Schedule "run now".** `workflow/schedules.rs:445` (`capture_now`) inserts a `schedule_runs` occurrence for a desk schedule with no unit check. Under D1.6 the later admission refusal deletes the occurrence whose id was already returned to the caller.
  4. Runtime child runs (`workflow/children.rs:232`, `create_on`) also bypass `admit`; they inherit the parent's association and need an open parent, so they are safe **only if** 1–3 are closed. D1.4 should state that dependency.
- **Impact:** work runs on an archived unit; the invariant "archived ⇒ no open run on its desks" that D1.3/D1.4 rely on (and that 02C "nothing writes" builds on) does not hold; a later restore/delete or a second archive sees open runs on an already archived unit.
- **Required change:** extend the decision with a complete inventory of writers that create or reopen `workflow_runs` rows (and schedule occurrences), each with a disposition: refuse with `unit.archived` under the company lock (retry control, native MCP admit, `capture_now`), or justified unchanged (children). State whether starting a direct local-agent session on a conversation of an archived unit's desk (`local_agents/management/writes.rs:56`) is refused or out of scope and why. Add focused cases: retry of a failed run after archive is refused; native MCP call on such a conversation is refused; `capture_now` is refused.
- **Instruction lesson:** observation — the enforcement map was built by searching for the named function `admit`, not for writers of the table. Candidate (scope: verification-contracts "Contract impact"): for an "admits nothing" rule, enumerate by `INSERT`/state-reopening `UPDATE` on the run table, not by entry-point name. Supporting evidence: `grep "admission_write::"` finds three callers besides `admit`.
- **Resolution history:** 2026-11-21 open.
  - 2026-11-21 recheck `08G-P02-D1-R2-N1`: **fixed in the decision** — D1.5a inventories retry, native MCP, session start, run-now, claim_due, children, resumes with dispositions and tests. The added backstop trigger has its own defect, `08G-P02-D1-R2-F1`.

## 08G-P02-D1-R1-F3 — D1.8's configuration inventory is incomplete and its locking premise does not hold for every listed kind of write

- **Classification:** actionable defect · medium · requirements / architecture (02B.4; 02A/02B "Restore and delete are the two writes allowed. Delete keeps its existing rules").
- **Evidence:**
  - **Transport bindings.** 02B.4 refuses "configuring or activating a binding … on its desks". D1.8 names only the workflow binding (`save_binding_on`). `integration/binding.rs:36` (`create_binding`), `:170` (`set_binding_status`, which re-activates) and `integration/email_binding.rs:100,139` write `channel_bindings` for a desk. These transactions take the company row only `FOR KEY SHARE` (`binding.rs:179`) or not at all, and no department row lock, so D1.8's helper premise "reads `archived_at` of the already-locked row" is false there: the check would race an archive unless the helper itself locks the department row (archive holds it `FOR UPDATE`).
  - **Delete reachability.** `department/lifecycle.rs:212-290` refuses deleting a unit that still owns desks ("Move them to another department or delete them first"), skills/connections, or child units. D1.8 routes desk edits through `require_active_desk_on` in `update_channel_on`, which is also the desk re-assignment path (`channel_edit.rs:126-145`), and says nothing about desk delete (`channel.rs:487-529`). As written, moving a desk *out* of an archived unit is refused and desk delete is undefined, so an archived unit that owns a desk can be deleted only by restoring it first. The decision must say which of desk delete / desk move-out / child delete are allowed on an archived unit (02C.2 already allows moving a resource out for the same reason).
  - **Unlisted writes needing a disposition:** schedule delete and binding archive/delete (D1.8's allowed list names only "deactivating a binding, disabling a schedule"); unit budget caps (`unit_budgets.rs:26`, a unit-scoped management write; 02C.3 says rows are kept but not whether caps may change); desk grant and address writes outside `update_channel_on` (`channel.rs:156,240`, `participant.rs:815`, `account_identity.rs:238`); desk reply completion/proposals (`workflow/channel_reply_completion.rs:96`, `channel_reply_proposals.rs`).
- **Impact:** paths the requirement names stay open or race the archive; delete — one of the two writes the requirement keeps — may be unreachable in practice; implementation would discover these one at a time after dependent code exists.
- **Required change:** replace the prose list with a contract-impact table (per verification-contracts): every writer of unit- or desk-scoped configuration, its lock context, and refuse / allow / owned-by-B2. Define the helpers' own locking for callers that do not hold the company write lock. Decide desk delete and move-out explicitly.
- **Instruction lesson:** observation — a refusal rule stated as an enumerated list drifted from the writers of the tables. Candidate (scope: this workflow's architecture gate): a "refuse every write of kind X" decision must carry the table-writer inventory, not the requirement's noun list. Hypothesis, single occurrence.
- **Resolution history:** 2026-11-21 open.
  - 2026-11-21 recheck `08G-P02-D1-R2-N1`: **fixed in the decision** — D1.8 table with lock context, self-locking helpers, U4 desk delete/move-out, transport bindings, budgets; grant/address writers deferred to implementation with a test per writer (accepted).

## 08G-P02-D1-R1-F4 — Cancel-and-archive is underspecified: authority widening, audit, lock precondition, bound

- **Classification:** actionable defect · medium · architecture / security / recovery (D1.3; proposal 3; 02B.1).
- **Evidence:**
  - **Authority.** Operator cancel today requires *conversation access* to the run (`controls.rs:99`, `association::authorize_association`), and `workflow/management.rs:1` states configuration authority grants no history access. D1.3 lets whoever manages the parent cancel runs on every desk below, including `members` desks they hold no grant on. That is a new authorization rule; it is acceptable only if stated as a decision with its limit (cancel without read) and a test with a manager who lacks the desk grant.
  - **Audit.** D1.3 calls `cancel_tree_on` directly. The actor-bearing `control_cancel` run event and the `workflow_control_commands` receipt are written by `control`/`audit` (`controls.rs:118-141,167-174`), not by `cancel_tree_on`; children get `cancelled_by_parent`, roots get nothing. Root runs would end `cancelled` with no record of who or why (`src/adapters/persistence/AGENTS.md`: audited transitions carry typed reason and actor explicitly).
  - **Lock precondition.** `cancel_tree_on` assumes the caller holds the run head `FOR UPDATE` (taken in `control`, `controls.rs:85-93`) before it reads the `child_run` waits; D1.3 does not say the archive takes it, nor that it re-reads state under the lock (a run may finish, or a child may be parked, between the count and the cancel).
  - **Root selection and recount.** "Cancel each open root run" does not define the set (open runs with `parent_run_id IS NULL` on subtree desks?) nor require a recount of open runs before archiving; native broker runs (`native_mcp_calls.cancel_requested` is their normal cancel path, `mcp.rs:63-73`) are not dispositioned.
  - **Bound.** The whole cancel runs inside the bounded department-write transaction; `src/AGENTS.md` "Bound work at every external boundary" needs a stated limit or outcome when the subtree has more open runs than one transaction can cancel (otherwise the unit can never be archived from the page).
- **Impact:** an unaudited, wider-than-today termination power, and a transaction whose correctness depends on unstated locks.
- **Required change:** state the rule and its limit; record a per-root actor event (reuse `control_cancel` or a typed new kind naming the archive); lock each run head before `cancel_tree_on`, skip terminal ones, recount to zero before the archive; give a bound and its refusal; add `unit_archive_` cases for cancel-and-archive (events present, manager without desk grant, rollback leaves nothing cancelled) and a competing case against a running job.
- **Instruction lesson:** cause unknown beyond "reuse of an inner helper without its caller's obligations". Candidate (scope: verification-contracts "Contract impact"): when reusing an inner `_on` helper, list the side effects and locks its existing caller supplies. Already present in general form; no change supported by one occurrence.
- **Resolution history:** 2026-11-21 open.
  - 2026-11-21 recheck `08G-P02-D1-R2-N1`: **fixed in the decision** — user decision U3 states the authority and its limit; per-root `control_cancel` event with actor; head lock, terminal skip, recount, bound 64, native broker refusal, lock order, tests incl. manager without grant and a running-job competing case.

## 08G-P02-D1-R1-F5 — D1.6 consumes one-off schedules and stamps `last_run_at` while the unit is archived

- **Classification:** actionable defect · low · correctness (D1.6; 02B.2 "skipped by the sweeper and not recorded as failures").
- **Evidence:** D1.6: "advances `next_run_at` exactly as today but inserts no `schedule_runs` row". Today's advance (`workflow/schedules.rs:691-716`) sets `last_run_at = CURRENT_TIMESTAMP`, and for a non-interval schedule sets `next_run_at = NULL` and `enabled = false`. So a one-off schedule that falls due while the unit is archived is silently disabled and shown as run, and every skipped interval schedule shows a run time for a run that never happened. The stated reason (failure modes: "schedule backlog on restore") is already prevented by the existing `floor(...)+1` step, which skips all missed slots at once.
- **Impact:** a restore does not return schedules to the state they had; a one-off is lost without a trace.
- **Required change:** specify the skip so it changes no schedule state other than what is required (e.g. exclude archived-unit schedules in `claim_due`'s `WHERE`, accepting one catch-up run of an interval schedule after restore; or advance interval schedules only, never touching `last_run_at`/`enabled`, and leave one-offs due). State the chosen restore behaviour and test it (`unit_archive_runs_`: one-off still pending after restore; `last_run_at` unchanged).
- **Instruction lesson:** cause unknown; no change supported.
- **Resolution history:** 2026-11-21 open.
  - 2026-11-21 recheck `08G-P02-D1-R2-N1`: **fixed in the decision** — archived units excluded in `claim_due`'s WHERE, no schedule column changes, restore behaviour stated and mapped.

## 08G-P02-D1-R1-F6 — Verification plan omits criteria the decision itself relies on

- **Classification:** actionable defect · low · verification (architecture gate accepts the plan).
- **Evidence (plan paragraph vs originals):**
  - 02B.3 mail and relay refusal: no planned case for the inbound bounce (`routing.rs:1048`), the Rust relay pre-filter (`ingest/relay.rs:192`) or the SQL functions `relay_desk_can_participate` / `internal_relay_source`. The rule is implemented in both Rust and SQL; persistence AGENTS asks for a database-backed equivalence case.
  - D1.4 names "archive vs mail commit (gate)" as a competing test; the Verification plan paragraph lists only admission vs archive and archive vs archive.
  - 02D.4 (agent whose every department is archived is managed by root managers, and not by the archived unit's ancestors' leads) is absent from `unit_archive_authority_`.
  - No case for restore re-enabling work (message admitted, schedule fires, mail accepted after the revision bump), which is the other half of "`channels.enabled` is not touched".
  - The cases required by F1, F2, F4 and F5.
  - `channel.call`: the plan's `unit_archive_runs_` case has no producer. The NOT APPLICABLE disposition is supported by the code (`grep 'channel\.call' src` → one fixture comment; `plan/agent-workflow-call/README.md:9`). It must stay a recorded NOT EXERCISED criterion in the acceptance mapping, not disappear from it.
- **Required change:** add the cases to the plan and put the acceptance mapping table (criterion → production path → decisive assertion) in TASK before implementation, as `references/verification-contracts.md` requires.
- **Instruction lesson:** the gate text had a prose plan instead of the mapping table the workflow asks for; with the table the missing rows would have been visible. Candidate (scope: this skill's architecture gate): require the acceptance-mapping table, not prose, in the gate. One occurrence.
- **Resolution history:** 2026-11-21 open.
  - 2026-11-21 recheck `08G-P02-D1-R2-N1`: **fixed in the decision** — acceptance mapping table present with mail/relay, archive-vs-mail, 02D.4, restore rows; `channel.call` kept as NOT EXERCISED.

---

## Optional suggestions (do not block the gate)

### 08G-P02-D1-R1-S1 — Enforce the reverse direction of the tree invariant in the database
- **Classification:** optional suggestion · architecture. D1.2 leaves "archiving a parent while a child stays active" to the application. 02B.6 rests role resolution on the invariant, and persistence AGENTS says application checks are not substitutes for constraints. A `CONSTRAINT TRIGGER … AFTER UPDATE OF archived_at … DEFERRABLE INITIALLY DEFERRED` refusing an archived row with an active child would close it without ordering problems inside the one-statement subtree update (a `BEFORE ROW` trigger cannot, since row order within the statement is unspecified). 02A.2 only requires insert, re-parent and restore, hence optional.
- **Resolution history:** 2026-11-21 open (optional).
  - 2026-11-21 recheck: adopted (D1.2a).

### 08G-P02-D1-R1-S2 — State the trigger's lock
- **Classification:** optional suggestion · concurrency. `guard_active_unit_target` (D1.9) is a backstop only if it reads the department row with a lock (`FOR KEY SHARE`, as the sibling `guard_department_principal_kind` does at `init_schema.sql:16136`) and the archive keeps locking subtree rows `FOR UPDATE` as D1.3 says; a plain `UPDATE` lock (`FOR NO KEY UPDATE`) would not conflict with it. Worth one sentence in D1.9 and a trigger-vs-archive competing case.
- **Resolution history:** 2026-11-21 open (optional).
  - 2026-11-21 recheck: adopted (D1.9a).

### 08G-P02-D1-R1-S3 — `cancel_open` as an enum
- **Classification:** optional suggestion · style. `archive(scope, unit, cancel_open: bool)` is a behaviour-selecting flag (`src/AGENTS.md` "No flag parameters"); prefer `OpenWork::{Refuse, Cancel}`.
- **Resolution history:** 2026-11-21 open (optional).
  - 2026-11-21 recheck: adopted (`OpenWork` enum).

### 08G-P02-D1-R1-S4 — B1 decisions that constrain B2 and neighbours
- **Classification:** optional suggestion · contract impact. (a) Routing must read the unit state in the same statement that reads `settings_revision` (`channel_gate.rs` module doc), i.e. in the channel read, or the D1.3 revision bump protects nothing. (b) The archive must take desk gates before any desk row lock (`channel_edit.rs:124-125`). (c) `resource_default_unit` (`init_schema.sql:16047`) and `workflow_creating_department` (`:20482`) pick a unit from role/membership rows without regard to `archived_at`; say whether they skip archived units. (d) 02C.1 calls uploads a configuration write, but `knowledge_documents.unit_id` is outside D1.9's trigger tables; record for B2 that knowledge upload, resource create-in and unit budget writes need the same refusal. (e) The mail directory (`routing.rs:833`) lists desks by `enabled` only.
- **Resolution history:** 2026-11-21 open (optional).
  - 2026-11-21 recheck: adopted (D1.3 gate order, D1.7a, D1.10a; (d) recorded for B2).

---

# Recheck round 08G-P02-D1-R2 (decision revision D1-r2)

Context: 2026-11-21, workflow `implement-opus-reviewed-2_4`, phase `08G-P02`, batch B1, stage architecture-gate correction recheck, note `08G-P02-D1-R2-N1`, reviewer Fable (`claude-fable-5-2`) / medium, worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`. Reviewed text: `logs/08G-P02-D1-r2-decision.md` (sha256 `9acd8b9d…fcde5ad73`) at HEAD `42bc327e61674dcd5114f3c948454036dfd51506`, digest `dfb7097b05f83153178c265aad32a87fb36534839a74fa3c624daf5f930bed34`. Both originate in the gated decision (introduced by the r2 correction). Inspection only.

## 08G-P02-D1-R2-F1 — The run backstop trigger cannot fire on a reopen

- **Classification:** actionable defect · low · correctness (D1.5a backstop).
- **Evidence:** D1.5a declares `guard_run_unit_active BEFORE INSERT OR UPDATE OF finished_at ON workflow_runs`. No writer sets `finished_at`: it is assigned inside `stamp_workflow_run_finished`, attached as `workflow_run_finished BEFORE INSERT OR UPDATE OF state` (`init_schema.sql:9448-9475`). PostgreSQL fires a column-specific trigger only when the column is in the statement's `SET` list; a change made by another `BEFORE` trigger does not count. The reopen statement is `UPDATE workflow_runs SET state='running', …` (`control_retry.rs:58`), so the backstop never fires for the one reopen path it exists for. The INSERT arm does work. The mapped check `unit_archive_refuses_run_trigger` exercises only a raw INSERT, so it would pass.
- **Required change:** declare it `BEFORE INSERT OR UPDATE OF state` and decide on state (`OLD.state` terminal and `NEW.state` not terminal), not on `finished_at`, so it does not depend on trigger name order either; add a raw `UPDATE … SET state='running'` case on a finished run of an archived unit to the mapping.
- **Instruction lesson:** observation — a derived column was used as a trigger event. Candidate (scope: persistence guide): a trigger keyed on a column another trigger writes must be keyed on the source column. One occurrence.
- **Resolution history:** 2026-11-21 open.
  - 2026-11-21 recheck `08G-P02-D1-R3-N1` (D1-r3, same HEAD/digest): **fixed in the decision** — trigger is `BEFORE INSERT OR UPDATE OF state`, decided on terminal→non-terminal state; raw `UPDATE … SET state='running'` case added to the mapping.

## 08G-P02-D1-R2-F2 — D1.5 places the archived check in a helper that allowed writes also use

- **Classification:** actionable defect · low · architecture (D1.5 inconsistent with D1.8 and U4).
- **Evidence:** D1.5 says the check is "implemented in the management helper so ordering is structural". `management::authorize_association_on` / `authorize_association_with_locks` is also the authority step of writes D1.8 allows on an archived unit: desk delete (`channel.rs:498`, U4), schedule delete and disable (`schedules.rs:167-184` `managed`), binding deactivate (`bindings.rs:53`), and of `authorize_run_with_locks` (`management.rs:145`, used by `message.rs:78` and account-hint reads). A check inside the shared helper refuses all of them with `unit.archived`.
- **Required change:** state that the check is a separate step (or an explicit opt-in argument) used by `admit` and the other refusing writers of D1.5a/D1.8, taken after the company lock and before the role check, and that the shared helper stays unconditional. The mapping's "allowed rows succeed" should name desk delete, schedule disable/delete and binding deactivate.
- **Instruction lesson:** cause unknown; no change supported.
- **Resolution history:** 2026-11-21 open.
  - 2026-11-21 recheck `08G-P02-D1-R3-N1`: **fixed in the decision** — separate step `unit_state::refuse_archived_desk_on` called only by refusing writers, shared helpers unchanged; `unit_archive_allows_*` rows added.

## 08G-P02-D1-R2-S1 — Notes for implementation (optional)

- **Classification:** optional suggestion. (a) `claim_due` with the new join needs `FOR UPDATE OF` the schedule alias only. (b) The backstop trigger and child creation take `FOR KEY SHARE` on a department row while holding run locks; cancel-and-archive holds the row `FOR UPDATE` and then waits for run heads, so a deadlock abort is possible there — the planned running-job competing case should assert a bounded, retryable outcome. (c) U3 and U4 are recorded in the D1-r2 text but not under TASK's "User decisions" heading.
- **Resolution history:** 2026-11-21 open (optional).
  - 2026-11-21 recheck `08G-P02-D1-R3-N1`: adopted (`FOR UPDATE OF schedule`, bounded outcome in the running-job case, U3/U4 under User decisions).

---

# Batch review round 08G-P02-B1-R1 (B1 implementation)

Context for the entries below: 2026-11-22, workflow `implement-opus-reviewed-2_4`, phase `08G-P02`, batch B1 (02A.1–4, 02B.1–5, 02D.1–4), stage **batch review**, note `08G-P02-B1-R1-N1`, reviewer Fable (`claude-fable-5-2`) / medium, worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`. Reviewed: working tree on HEAD/review base `42bc327e61674dcd5114f3c948454036dfd51506`, digest `98cbdbc7f5ecd0ba310fbed0770954e7fe66a04f4ed48594942a92b8403ae887` (exclusion `plan/08-goals/phases/p02/docs`). Task [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md).

## 08G-P02-B1-R1-F1 — A session started before the archive still takes new prompts on an archived unit's conversation

- **Classification:** actionable defect · low · requirements / correctness (02 Outcome "admits no work"; TASK decision "A direct local-agent session on an archived unit's conversation counts as work and is refused").
- **Evidence:** `src/adapters/persistence/local_agents/management/writes.rs`: the archived check was added to `start` only (after the replay branch). `control` (same file, `SessionCommand::Send { prompt }` → `LocalCommand::Send`, an effect command) authorizes through `authorized_session_on` and never reads the unit state. Archiving does not count or end sessions (`department/archive.rs` `open_work_on` counts `workflow_runs` only), so a session started before the archive keeps accepting prompts indefinitely; only its native MCP calls are refused (`native_mcp/mod.rs`). Observed by inspection; no test sends a prompt after an archive (`unit_archive_runs_local_session_…` covers start only).
- **Origin:** the gated inventory (D1.5a) named only "session start"; the gap is in the gated decision and in my gate review, carried into the implementation.
- **Required change:** refuse an effect `Send` on a direct session whose conversation's desk is in an archived unit with `unit.archived` (same helper, before the operation is written); keep `Inspect`, `Stop` and `Archive`, which stop or read. Add a case: session started, unit archived, `Send` → `UnitArchived` and no operation row; `Stop` succeeds; after restore `Send` succeeds.
- **Instruction lesson:** observation — an inventory row named one function of a lifecycle (start) instead of every command that produces work on it. Candidate (scope: this workflow's gate inventories): list commands, not entry functions. Second occurrence of the pattern recorded in `08G-P02-D1-R1-F2`.
- **Resolution history:** 2026-11-22 open. Follow-up appended to `08G-P02-D1-R1-N1`/`R3-N1` via note `08G-P02-B1-R1-N1`.
  - 2026-11-22 recheck `08G-P02-B1-R2-N1` (digest a10ec4c2…1012ad): **fixed** — `require_active_association_on` in `local_agents/management/writes.rs` runs in `start` and for every effect command in `control`; extended test asserts `Send` → `UnitArchived` with no operation row, Inspect/Stop succeed, `Send` succeeds after restore. Test executed by me (in the 32).

## 08G-P02-B1-R1-F2 — The plan's Deployment-harness message check is replaced by a stand-in that does not exercise the message path

- **Classification:** actionable defect · low · verification.
- **Evidence:** `plan/08-goals/02-unit-lifecycle.md` Focused checks: "`unit_archive_runs_`: with the scripted-model `Deployment` harness, a message to an archived unit's desk admits nothing and leaves no run". D1-r2's mapping row 02B.2 repeats it ("message via Deployment harness with run-as = unit's own lead and = owner"). Delivered instead: `message_admission_classifies_by_producing_condition` (a pure `classify` unit case) and `unit_archive_runs_admission_refuses_…` (calls `WorkflowAdmission::admit` directly with a manual trigger). Nothing drives an accepted message through intent claim → `MessageAdmission` → `record_binding_outcome` on an archived desk, so the recorded outcome `unit.archived`, the absence of a run, and the run-as = own-lead case on a real message binding are unexercised. TASK "Stand-ins and gaps" states the substitution; a stand-in does not fulfil an explicit behavioural check (verification-contracts).
- **Required change:** add the Deployment-harness case (message to an archived unit's desk: no run, binding outcome refused `unit.archived`; binding run-as = the unit's own lead), or obtain and record an explicit agreed disposition for dropping the plan's named check.
- **Instruction lesson:** none new; existing rule applied.
- **Resolution history:** 2026-11-22 open.
  - 2026-11-22 recheck `08G-P02-B1-R2-N1`: **fixed** — `unit_archive_runs_message_to_an_archived_units_desk_admits_nothing` on the `Deployment` harness asserts the binding outcome row `(refused, NULL, unit.archived)`, zero runs, model never called, and a later message bounced with no intent. The run-as = own-lead arm stays at the admission layer; accepted, since the check precedes the authority step for every actor and that ordering is asserted there. Executed by me.

## 08G-P02-B1-R1-F3 — The archive/restore routes and the wire shape of the two refusals have no test

- **Classification:** actionable defect · low · verification (02B.1 "refuses with `unit.archive_open_work` and a count"; D1-r2 contract impact "HTTP 409, codes").
- **Evidence:** new routes `POST /api/companies/{company_id}/departments/{department_id}/archive|restore` (`src/adapters/http/routes/department.rs`) and new response arms in `src/adapters/http/app_error_impl.rs` (409, `code`, `count`, `cancellable`). `grep` for `/archive"`, `archive_department_json`, `unit.archive_open_work` under `src` finds no test; the only assertion on a wire code is `"unit.archived"` inside `classify`. The code string `unit.archive_open_work`, the count field, the 409 status, the optional-body handling (`Option<Json<ArchivePayload>>`, `deny_unknown_fields`) and `cancel_open_runs` → `OpenWork::Cancel` are unexercised.
- **Required change:** a route-level test: archive with an open run → 409 `{code:"unit.archive_open_work", count:1, cancellable:true}`; with `{"cancel_open_runs":true}` → 200 and archived; empty body → `Refuse`; configuration write afterwards → 409 `unit.archived`; restore → 200.
- **Instruction lesson:** cause unknown; no change supported.
- **Resolution history:** 2026-11-22 open.
  - 2026-11-22 recheck `08G-P02-B1-R2-N1`: **fixed** — `unit_archive_http_routes_refuse_open_work_cancel_on_request_and_restore` asserts 409 + code + count + cancellable, default/explicit false, unknown field rejected without cancelling, cancel → 200, restore under archived parent → 409 `unit.archived`, restores → 200. Stand-in limit: it uses a `cfg(test)` `lifecycle_router()` with the same handlers and paths, so registration in the production router is by inspection only. Executed by me.

## 08G-P02-B1-R1-F4 — The cloudroom e2e failures are an unverified affected path, not "unrelated"

- **Classification:** actionable defect · low · verification record.
- **Evidence:** `logs/08G-P02-B1-lib-suite.txt:5462-5476` lists six cloudroom tests (TASK says seven): `local_agent_tests::e2e::*` ×2, `e2e::early::*` ×3, `retention::peer::*` ×1. They panic with "counted peer exited before event" / `NotFound`; the log holds six `ERR_DLOPEN_FAILED` lines, so the environment cause is plausible. But these tests drive local-agent session start and native MCP admission, both changed by B1 (`local_agents/management/writes.rs`, `workflow/native_mcp/mod.rs`), and they never reached that code. Under the workflow's verification scope an affected check that could not run is an unresolved gap, not a confirmed-unrelated failure. The other dispositions hold: the trigger-inventory failure is a B1 regression, fixed (one-line test change, rerun passes); `doop_agent_current_preparation_…` asserts version 7 against `PREPARED_AGENT_VERSION = 8` at HEAD in files outside the diff (unrelated); the four timing failures pass in isolation in `logs/08G-P02-B1-lib-rerun.txt`.
- **Required change:** relabel the six as NOT EXERCISED affected checks (correct the count), and run them on a working Node module build before the phase is accepted, or record an agreed disposition. No source change.
- **Instruction lesson:** existing rule ("if the relationship is uncertain, retain it as an unresolved verification gap"); no change supported.
- **Resolution history:** 2026-11-22 open.
  - 2026-11-22 recheck `08G-P02-B1-R2-N1`: **fixed** — after an environment-only `npm rebuild` of ignored plugin modules the six tests ran and passed (`logs/08G-P02-B1-R2-cloudroom.txt`, 6 passed, 3 ignored by design); TASK disposition and count corrected. Log inspected, not rerun by me.

## 08G-P02-B1-R1-S1 — Optional notes

- **Classification:** optional suggestion.
  - `archive_department_on` (`department/archive.rs:110-198`) is 88 lines with a marked phase ("open work → cancel → recount"); `src/AGENTS.md` asks to split past ~80. Extracting the open-work phase into a helper returning `Result<(), AppError>` would fit.
  - `unit_archive_competing_cancel_and_a_running_job` holds only the run row in the competing transaction, so it exercises the wait path. The deadlock arm (a job transaction that holds run locks and then needs the unit row `FOR KEY SHARE`, e.g. inserting a child run through `workflow_runs_active_unit`, against an archive that holds the unit `FOR UPDATE` and waits for the run head) is not produced; the `Err(AppError::Database(_))` branch is therefore untested, and a lock timeout would surface as `DatabaseTimeout`, which the test treats as a failure.
  - `pages/company_departments.rs:694` still decodes only `lead:` acting roles; a `sponsor:` audit entry falls to the fallback until B2 handles it.
- **Resolution history:** 2026-11-22 open (optional).
  - 2026-11-22 recheck `08G-P02-B1-R2-N1`: not taken in B1; `sponsor:` decode assigned to Batch 2 (02E), the other two carried to the final review. Remains open (optional).

---

# Batch review round 08G-P02-B2-R1 (Batch 2 implementation)

Context for the entries below: 2026-11-22, workflow `implement-opus-reviewed-2_4`, phase `08G-P02`, Batch 2 (02A.5, 02C, 02E), stage **batch review**, note `08G-P02-B2-R1-N1`, reviewer Fable (`claude-fable-5-2`) / medium, worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`. Reviewed: working tree on HEAD/review base `42bc327e61674dcd5114f3c948454036dfd51506`, digest `51dd88e18771de73170477a1a633f13bf5adeecb34f9c59835ed1fcc60849afa` (exclusion `plan/08-goals/phases/p02/docs`). No architecture gate was held for this batch, so every defect is an implementation or record defect. Task [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md).

## 08G-P02-B2-R1-F1 — Hits from an archived child are not marked archived (02C.1)

- **Classification:** actionable defect · medium · requirements.
- **Evidence:** `plan/08-goals/02-unit-lifecycle.md` 02C.1: "Results label such hits `reports:<slug>` as today, with the unit marked archived." `src/adapters/persistence/workflow/memory_tool_checkpoint.rs:226-246` still builds the scope label as `format!("{origin}:{slug}")` with nothing about the unit's state; the file is not in the diff, and no other change marks a memory or knowledge result from an archived unit. TASK "Batch 2 implementation status" lists upload/move refusal and the picker under 02C.1 and neither implements nor dispositions this sentence.
- **Impact:** an agent or lead reading a parent's `reports` scope cannot tell that a hit comes from a unit that no longer works.
- **Required change:** carry the unit's archived state into the label (or an adjacent field the result renders) for `reports` hits, and assert it in a `unit_archive_scopes_` case; or record an agreed disposition for dropping the sentence.
- **Instruction lesson:** observation — the sweep (`logs/08G-P02-B2-sweep.md`) inventoried list reads and writers but not result labels, so the one 02C sentence about output was never mapped. Candidate (scope: this workflow's batch mapping): map every sentence of the requirement section to a row, including ones about output text. One occurrence.
- **Resolution history:** 2026-11-22 open.
  - 2026-11-22 recheck `08G-P02-B2-R2-N1` (digest `6de44ac6…3e2b5`): **fixed** — `MemoryScope.archived` set from the unit's live state (`mark_archived_on`), carried as `"archived": true` on tool results and recall; the frozen label is unchanged. Knowledge passages have no unit label, so that clause is not applicable (accepted). Test executed by me.

## 08G-P02-B2-R1-F2 — The memory half of `unit_archive_scopes_` is missing and the retirement assertion cannot fail

- **Classification:** actionable defect · low · verification.
- **Evidence:** the plan's focused check: "`unit_archive_scopes_`: the parent's lead reads the archived child's memory and knowledge; a sibling does not; nothing writes; the namespace is not retired". `knowledge/scope_archive_tests.rs` covers knowledge only. Its retirement assertion counts `memory_namespaces … state='removing'` and `memory_lifecycles` for the child, but namespaces are created lazily on a memory write (`memory/namespaces.rs:226`) and the fixture writes no memory, so both counts are zero whether or not an archive retired anything. No test reads an archived child's memory through a parent lead's run scope. By inspection the behaviour is right (`department/archive.rs` never calls `retire_namespaces_on`; `workflow/run_scopes.rs` is unchanged), but inspection does not fulfil the named check.
- **Required change:** a case with a real unit namespace written before the archive: after the archive it is still active, a run of the parent's lead still has it in its `reports` scope, and a sibling lead's run does not.
- **Instruction lesson:** none new.
- **Resolution history:** 2026-11-22 open.
  - 2026-11-22 recheck `08G-P02-B2-R2-N1`: **fixed** — `unit_archive_scopes_lead_reads_an_archived_childs_memory_marked_archived` reads a pre-provisioned namespace after the archive through the use case and checks it is not `removing`. Executed by me.

## 08G-P02-B2-R1-F3 — 02C.4 has no disposition and no check

- **Classification:** actionable defect · low · requirements record / verification.
- **Evidence:** 02C.4: "Threads, runs, decisions and escalations stay readable under the desk grants that existed. Membership and roles in an archived unit grant that reading and nothing else." TASK's Batch 2 sections do not mention 02C.4. Readability holds by inspection (the archive changes no desk grant), with no test that a member or lead of an archived unit still reads its desk's threads and runs. "Nothing else" is not examined: `workflow/run_scopes.rs` (the `child JOIN department_roles AS holder … holder.role='lead'` query in `capture_agent_on`) still gives a principal who leads an archived unit the memory of that unit's children as a `reports` scope in runs elsewhere, and knowledge permissions still derive reads from roles on archived units. That may be intended under 02C.1 ("reads follow the existing rules"), but the two sentences pull in different directions and the choice is unrecorded.
- **Required change:** record the reading of 02C.4 A chose (which reads a role on an archived unit keeps) and, if it needs the user, ask; add one case showing a member of an archived unit still reads its desk history.
- **Instruction lesson:** same as F1 (unmapped requirement item).
- **Resolution history:** 2026-11-22 open.
  - 2026-11-22 recheck `08G-P02-B2-R2-N1`: **fixed as decided by the user ("Desk history only", per A's record)** — run-scope lead children, knowledge placements and builder placement skip archived units; desk-history and knowledge cases added and executed by me. The accompanying change to `workflow_target_departments` raises `08G-P02-B2-R2-F1`.

## 08G-P02-B2-R1-F4 — "Include archived departments" governs only the caps table

- **Classification:** actionable defect · low · requirements (02C.3 "Usage pages show archived units when asked to include them"; 02E.2 labels).
- **Evidence:** `query.archived` reaches only `unit_budgets_on` (`model_usage/report.rs:124-129,262-284`). The usage report itself (`load_calls`, `report.rs:211-242`) joins `departments` for the name alone, so with "Group by department" an archived department's calls are listed whether or not the box is ticked, and without "(archived)". The page test asserts the caps table only.
- **Required change:** either apply the choice to the department grouping (keeping totals correct, e.g. by folding or labelling), or at least label archived departments in that grouping and record that totals always include their calls; cover it in the usage page test.
- **Instruction lesson:** cause unknown.
- **Resolution history:** 2026-11-22 open.
  - 2026-11-22 recheck `08G-P02-B2-R2-N1`: **fixed** — `load_calls` takes the choice; archived units' calls are left out unless asked or drilled into, and named "(archived)"; test extended. Totals therefore differ with the box, which the docs now state.

## 08G-P02-B2-R1-S1 — Optional notes

- **Classification:** optional suggestion.
  - The run board's scope options use `choices(None)` (`routes/workflows/ui/runs/mod.rs`), while the attention filter and desk owner picker keep a selected archived unit; a board already scoped to an archived department loses its option.
  - `managed_units` (ids, `administration.rs`) lists units without the new argument. It feeds authority and resource listings where archived units must stay (to be moved out), so the omission is right, but TASK's 02A.5 list does not say so.
  - "Editing a resource that stays in an archived unit is allowed" is A's disposition against the Outcome sentence "accepts no configuration"; `docs/departments.md` does not mention it either way.
- **Resolution history:** 2026-11-22 open (optional).
  - 2026-11-22 recheck `08G-P02-B2-R2-N1`: dispositions recorded by A (run-board options kept; `managed_units` deliberate; resource edits documented). Closed as recorded.

---

# Recheck round 08G-P02-B2-R2 (Batch 2 corrections)

Context: 2026-11-22, `implement-opus-reviewed-2_4`, phase `08G-P02`, Batch 2, stage correction recheck, note `08G-P02-B2-R2-N1`, reviewer Fable (`claude-fable-5-2`) / medium, worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`. Reviewed: working tree on HEAD `42bc327e61674dcd5114f3c948454036dfd51506`, digest `6de44ac6cdd14b58bf950e56b1332842be5b4d24585f50c3e196eb4d3d33e2b5` (exclusion `plan/08-goals/phases/p02/docs`). Implementation defect introduced by the F3 correction; inspection only.

## 08G-P02-B2-R2-F1 — Archive and restore now change an agent's placement for role binding, without the check every other placement change runs

- **Classification:** actionable defect · medium · correctness / architecture.
- **Evidence:**
  - The correction makes `workflow_target_departments` (`init_schema.sql:16921-16938`) skip member and role rows in archived units. For an agent's direct address that function is the binding's department context (`workflow/role_binding.rs:142-165` `context_on`), the expected value in `guard_workflow_role_context` (`init_schema.sql:16947-16976`, fired on every `agent_dependency_manifests` insert of a root run), the escalation origin, and the run-scope chain (`run_scopes.rs:96`).
  - Every roster write that can change that result re-validates active bindings under the company lock with `role_binding::check_principals_on` and refuses the write when an agent's active binding with a role step, an escalation policy or own-unit memory would be left without one unambiguous department: `department.rs:480,485,527`, `department/roles.rs:82`, `department/lifecycle.rs:329`.
  - `department/archive.rs` calls it in neither `archive_department_on` nor `restore_department_on`. Archiving a unit now removes that unit from the context of every agent placed in the subtree; restoring adds it back.
- **Consequences (by inspection, no test):**
  1. An agent placed only in the archived subtree keeps an active direct-address binding that needs a department; its next admission is refused for a missing department — the state `check_principals_on` exists to prevent ("deactivate bindings before changing these placements").
  2. Agent-address runs are not open work for the archive (it counts desk runs only), so such a run can be in flight; a later manifest insert for it compares the captured department with a now-empty expectation and raises `workflow_guard:guard_workflow_role_context`.
  3. A restore can make an agent that is also placed in an active unit ambiguous, breaking a binding activated while the unit was archived, again without refusal.
- **Required change:** decide and implement one rule for both writes — e.g. run `check_principals_on` for the agents placed in the affected units (refusing the archive or restore with the existing binding message), or count/handle open agent-address runs — and add cases: archive with an agent whose only unit is in the subtree and an active role-bearing direct binding; restore creating ambiguity; an in-flight agent-address run across an archive. If refusing is not wanted, record the agreed behaviour and what the binding and the run then do.
- **Instruction lesson:** observation — a shared SQL predicate was narrowed and its writers' invariant check was not extended to the new writer. Candidate (scope: persistence guide, "One decision, one place"): when a derivation gains a new input column, every writer of that column inherits the derivation's re-validation. One occurrence.
- **Resolution history:** 2026-11-22 open.
  - 2026-11-22 recheck `08G-P02-B2-R3-N1` (digest `b066cce6…123559`): **fixed** — archive and restore lock the placed principals (shared `placed_principals_on`, also used by delete) and run `role_binding::check_principals_on` after the update; `unit_archive_rechecks_direct_role_bindings_on_archive_and_restore` covers the archive refusal, archive after deactivation, activation while archived, and the restore refusal (executed by me). Scenario 2 (in-flight direct-address run) left as the existing contract of every roster write, failing closed at the role-context guard: accepted as a recorded disposition, untested.
