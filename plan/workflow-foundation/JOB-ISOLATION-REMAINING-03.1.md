# Remaining job isolation — bounded audit and implementation queue

2026-09-28. Audit/expansion only; no production/schema/test edits in this assignment.
Original03 admission/sole-job ownership and BRIEF-03.1 remaining table remain authoritative.
Accepted task SQL, direct/indirect routines, three source guards and projection/index slices are
reused, not reopened. Workflow creation stays disabled and channel_id stays NOT NULL. Do not
start03.2 or admission writes from this queue. Root owns acceptance; next worker implements.

## Inventory and completeness boundary

Fresh uncapped graft JSON inventories, all zero file/hit truncation:
- /private/tmp/workflow-isolation-application-current.json:16hits (same as prior application).
- /private/tmp/workflow-isolation-http-current.json:5hits (same as prior HTTP).
- /private/tmp/workflow-isolation-nullable-current.json:19hits (two added test references since17).
- /private/tmp/workflow-isolation-auxiliary-a.json:43hits, provisioning/schedule/start/completion/handoff.
- /private/tmp/workflow-isolation-auxiliary-b.json:219hits, remaining auxiliary names.
- /private/tmp/workflow-isolation-app-entry.json: typed provisioning/schedule entry references.
Accepted persistence audit remains /private/tmp/workflow-jobs-complete-persistence.json and
JOB-QUERY-REMAINING-03.1.md; no new exhaustive background_tasks query sweep claimed.

Task-only migrated database catalog snapshots:
/private/tmp/workflow-isolation-task-fks.txt contains17direct FK owners;
/private/tmp/workflow-isolation-task-columns.txt contains19task_id-bearing tables and indirect FKs.
Catalog inspection closes the blind spot in searching only literal background_tasks: independent
auxiliary readers/writers can exist without spelling that table. The19tables are exactly17direct
owners plus task_harness_invocations (scoped indirect FK) and thread_handoff_events (historical,
no task FK). Notification source_id and checkpoint JSON provenance continue to use accepted
source-resolution/guarded harness contracts; they are not extra execution owners.

## A. Legacy-only auxiliary association closure (implement first, guard still installed)

All direct FK owners except task_attempts are legacy-only. Keep the existing scoped FKs and
cascades; add explicit DB-boundary visible-legacy association checks for the remaining owners.
The immutable queue discriminator means classification cannot race an update. As in85000, checking
only for a visible workflow row is insufficient: an invisible/uncommitted row could satisfy a
later waiting FK. Require a visible legacy row, then let the existing FK serialize deletion.
Reject missing/foreign/workflow task links; permit legitimate taskless states only where defined.
Do not use disabled triggers or weakened workflow shape in fixtures.

| Owner | Current contract / remaining work |
| --- | --- |
| response_drafts, message_deliveries, human_approvals | Already guarded by84000+85000; retain unchanged. Nullable task links valid. Draft/delivery detach on task deletion; approvals cascade. Existing task-first deletion/listener tests are accepted evidence. |
| task_attempts | Shared sole attempt ledger. NO legacy-only association trigger. Accepted legacy BEGIN/FINISH/read predicates retain fences. New workflow attempt insertion must still succeed after every new guard. |
| task_outreaches | Legacy-only root for targets/replies and quorum notifications. Add visible-legacy INSERT/UPDATE guard; preserve scoped task FK and harness-invocation FK. Accepted entry operations are already fenced, but raw auxiliary associations were intentionally allowed in earlier mixed tests. |
| task_harness_runs | Legacy-only checkpoints, guarded task execution writer and discriminated readers already accepted. Add DB association guard. task_harness_invocations inherits legacy provenance through scoped(company,task,run) FK; do not create a second owner. |
| task_agent_instructions, start_agent_task_commands | Legacy-only instruction/replay rows. Add DB guards. Accepted ask/start/claim guards remain; replay rows must not turn into a bypass. |
| task_channel_targets | Legacy-only routing associations. Add DB guard; keep compound tenant/channel FK and ordered uniqueness. |
| task_ownership_events, task_status_events | Legacy-only ledgers. Add association checks on INSERT; retain existing immutability protection and owner-delete cascade behavior. Do not make historical aggregate protection legacy-only accidentally. |
| delegation_control_commands, human_task_completions | Legacy-only command/result journals. Add association guards; retain idempotency and immutable/replay behavior. Accepted outer task authorization/fences remain. |
| task_approval_waits | Legacy-only; scoped approval/task and optional invocation FKs already tie its identity to guarded roots. Include in direct-owner guard coverage or explicitly prove transitive enforcement; preserve wait/link replay semantics. |
| agent_channel_provisions | Legacy-only provisioning receipt; writer already calls lock_task_execution_on before replay/creation (agent_channel.rs:24-64). Direct FK is task-id-only, so do not assume a composite FK checks tenant. DB guard must also compare task tenant with provisioned channel/agent owners; preserve atomic provisioning and replay. |
| schedule_runs.task_id | Legacy materialization result link; optional while pending/materializing/failed, non-null when materialized. record_run_task(schedule.rs:468-494) currently accepts any task UUID after run worker/generation/lease fence. Add explicit legacy/scoped task check against the schedule's company/channel/thread, preserving fence and no partial mutation on mismatch. Direct FK is task-id-only; use the actual schedule owner, not an inferred tenant. workflow admission may reuse occurrence identity later without populating this legacy task link. |
| thread_handoff_runs | Legacy drafting association, NOT workflow execution ownership. DB association guard needed. start_handoff_run_on(thread_handoff.rs:536-655) is reached from accepted guarded ask/start task paths, but independent handoff projections/mutations currently rely on this provenance. Preserve(company,task) and handoff generation uniqueness. |

Related associations: task_outreach_targets/task_outreach_replies inherit through scoped outreach FK;
task_harness_invocations inherits through scoped harness run FK; task_approval_waits invocation
link must remain same company/task/run. No duplicate queue or independent provenance table.
Use additive migrations only; all migrations through20260928090000 are applied/immutable.

Tests: enumerate raw INSERT and UPDATE rejection for each newly guarded owner (including tenant
mismatch for two single-column task FKs), legacy acceptance, optional taskless acceptance, and a
shared workflow attempt. Retain real invisible-task race regression or extend its table-driven
coverage: reject immediately before uncommitted task insert becomes visible; accept committed
legacy. Exercise deletion/cascade/SET NULL with real PgListener, especially outreach descendants
and immutable ledger deletion. Adapt earlier raw-workflow auxiliary fixtures to expect rejection
or replace them with reachable mixed inputs; do not drop new guards merely to keep old assertions.

## B. Handoff historical provenance and independent consumer checks

thread_handoff_events.task_id is nullable and deliberately has no task FK (init schema3494).
insert_handoff_event(thread_handoff.rs:349-389) writes optional task id. Event rows are immutable
and survive task deletion: do not add a cascading FK or filter history through a currently-live
background_tasks join, which would erase legacy history. New non-null references need scoped
visible-legacy provenance at insertion, with task-row locking if necessary because no FK provides
the deletion serialization. Existing historical rows remain valid after their task is removed.
Taskless opened/regenerated/claim/release/resolve/dismiss events remain valid. Confirm failure
paths record their event while the task is still live; if an event is intentionally written after
delete, validate against the already trusted handoff run provenance before deletion instead.

Inspect these independent auxiliary entry seams in the next implementation slice (not accepted
merely from the task SQL sweep):
- open_handoff_generation_on:72-150 updates/supersedes existing handoff runs from inbound messages.
- handoff_run_for_task_on:670-687, complete_handoff_run_on:716-785,
  fail_handoff_run_on:826-891, resolve_handoff_for_draft_on:899-945,
  thread_handoff_draft:1258-1293. They read/mutate the handoff owner, not background_tasks.
  With a trusted DB legacy-only run association these are safe by construction; prove that
  premise and preservation of taskless handoff behavior before closing their audit rows.
- schedule_runs record_run_error/claim paths touch materialization state; their optional task_id
  does not make them workflow executors. Preserve pending occurrence processing.
- agent_channel_provisions is fenced before replay, so no separate table-wide discriminator
  sweep is needed once the DB association invariant is enforced.

Tests: workflow association rejected before it can influence handoff generation/read/expiry,
legacy draft/complete/fail behavior and taskless handoff events preserved, event remains visible
and immutable after task deletion. Test the independently exposed expire_thread_handoff_draft path.

## C. Application/HTTP and nullable boundary

Fresh literal inventories reveal no additional production SQL in application or HTTP. Application
16hits are comments/port methods plus scoped DB test fixtures (external_reply/inter_channel/
structured_response); HTTP5hits are attempt presentation and UI port calls/comments. This is a
classification of the literal inventory, not a claim that all typed callers were exhaustively
re-reviewed. Boundary proof is the accepted legacy TaskPersistence and fallible domain decoder.

Checked relevant source: TaskWorker::process_claimed_task(task_worker.rs:487-523) accepts the
required-channel BackgroundTask from the accepted legacy-only claim path. UI TaskMonitorView::task
(ui_tasks.rs:982-1005) loads that same legacy-only get_task_by_id then scopes company/visible channel;
pane:1038-1092 calls the already discriminated attempt reader. Presentation does not query shared
attempts directly. No nullable channel should be introduced into legacy application/HTTP entities.
Retain transport task payload meaning; workflow payload remains version1+execution UUID only.

BackgroundTaskDb.channel_id is already Option<Uuid> (rows.rs:25-50), with fallible conversion at
rows.rs:258-267 rejecting missing legacy channel. TaskChainCardDb:133-160 contains aggregated
channel_names, not a channel UUID; no new optional field is needed there. Existing null-decoder
regression and accepted discriminated raw-row call sites can be reused. Before actual nullability
migration, regenerate SQLx against that schema: macro-generated channel fields may become Option
and need fallible conversion even when SQL's queue predicate logically guarantees a legacy row.
The company_invite owned-task macro is a known site; do not silence this with unwrap/default.

Next worker must turn final nullable acceptance into real DB fixtures: company-only workflow job
with NULL channel/thread; channel-associated workflow job; thread-associated workflow job; legacy
NULL rejection; mismatched company/channel/thread/execution rejection; all legacy claim/control/
recovery/projection paths skip workflow rows and still operate on legacy rows. Keep the domain
legacy entity required-channel and retain competing claimant tests. SQLx metadata must match the
post-migration schema and all-target compile must pass.

## D. Conditional association and enablement gate (separate, after A–C accepted)

Current workflow job shape/FK proves company+execution and exact ID payload only. When allowing
NULL channel, require legacy rows to retain channel and require workflow job channel/thread to
match the owning execution's run association (workflow_runs scoped associations in70000). A mere
CHECK(thread implies channel) is insufficient to prevent a same-company job pointing at a
wrong run channel/thread. Enforce exact NULL-aware association at the DB boundary and make parent
association/execution linkage immutable or validate their changes, so a later parent UPDATE cannot
invalidate a previously checked job. Preserve scoped FKs and existing legacy source uniqueness.
No workflow writer/claim/runtime added in this gate; production admission writer is separate03.1.

Only remove background_tasks_workflow_disabled after all remaining association/read/control/
recovery/projection/nullable criteria are verified and independently reviewed together. Run fresh
migrations, meaningful mixed DB/concurrency/rollback/deletion tests, full DB suite at stock2MiB,
locked offline all-target check/Clippy, sequential SQLx prepare, fmt/diff/graft. Reuse accepted
unaffected evidence; do not call this audit or its queue whole-job/03.1 acceptance.

## Audit status

Implementer01a0e73a-5134-7911-acd4-a413cfd2607d, latest milestone118939/258400=46.03%
09:06:49.686Z, usage/runtime sources. Audit only; no code correction. Artifact requires independent
completeness/contract review before acceptance. TaskPG55439 remains running for root transfer.

Independent audit/expansion PASS: reviewer01a0e73a-89ec-7e23-82cb-f23de81335d7 verified all17direct
owners plus indirect invocation and historical event owners against catalog/relevant source; no
missing owner or unsafe contract found. Review clarification: C's real nullable fixtures and D's
association constraints are ONE coordinated schema gate, with workflow creation disabled until
both pass. Include a real concurrent parent association/execution UPDATE versus job INSERT test,
not only sequential mismatch rejection. Parent mutation must not create a mismatched committed job.
Reviewer quiescent90289/258400=34.94%09:09:27.190Z, usage/runtime capacity sources.

Implementation remains future work; this is bounded audit acceptance only. Implementer naturally
retires after handoff (last substantive126027/258400=48.77%09:08:57.903Z). Graft estimate audit
676299tokens implementer and392226reviewer; implementer cumulative3248394 across both assignments.
No code/schema/test changed, so accepted projection/static/test evidence remains unaffected.
TaskPG remains running at55439 under existing task directory, transferred to root; no build/test
or audit query process remains. Root owns next-worker assignment and acceptance.

## Active implementation partition (2026-09-28)

Root approved A1 → A2 → B, each independently reviewed before advancing.
- A1: additive scoped visible-legacy guard for eleven remaining compound-FK owners;
  INSERT/UPDATE matrix, invisible-task race, deletion/cascade/listener proof, reachable old
  mixed fixtures. Existing source guards and shared attempts unchanged. Scope is a classification
  boundary; existing immutable journals retain their stronger mutation rules.
- A2: provisioning compares task/channel/agent tenant; schedules compare actual schedule
  company/channel and run thread, plus fenced record_run_task rejection without partial write.
  Preserve existing materialization CHECK semantics (materialized task deletion currently fails
  atomically despite SET NULL FK); do not invent a reset transition. Parent association mutation
  implications must be closed in this checkpoint.
- B: insertion-only historical handoff event guard with scoped visible legacy task row locked
  through commit; no live-task read filter or cascading FK. Verify entry seams, taskless behavior,
  independently exposed expiry, history survival/immutability, and deletion races.
All applied migrations through90000 immutable; no nullable channel/workflow enablement/admission
writer. C/D remain coordinated later gate. Use taskPG55439 and separate disposable test databases.

### A1 implementation evidence

Eleven scoped owners in A implemented/reviewed; see JOB-SLICE-03.1.md A1 checkpoint for exact
migrations93000/94000, tests, correction facts and gates. All-owner raw INSERT/UPDATE rejection,
legacy acceptance, shared attempts, invisible-parent and real deletion contention/cascades PASS.
Full1942PASS/22ignored at2MiB, offline alltarget/Clippy/SQLx/fmt/graftPASS; root owns acceptance.
A2 still requires agent_channel_provisions and schedule_runs tenant/association guards plus fenced
record_run_task checks. B still requires historical insertion provenance and independent handoff
entry/expiry evidence. Do not confuse A1 with completion of A/B or broader job enablement.

### A2/B reconciliation (2026-09-28, tenant-links worker)

A2 adds stored scope keys to the two legacy associations, derived from the actual task/schedule
for existing callers and explicitly supplied by production inserts. Composite FKs retain those
identities against later/concurrent parent changes; they are integrity keys, not execution state.
Provision receipts require task/channel/agent company equality. Schedule links require schedule
company/channel plus occurrence thread equality with a visible legacy task; pending/failed taskless
states remain valid. Migration audits existing rows and fails incompatible data without repair.
Existing single FKs/cascades and materialized CHECK remain, including atomic task-delete failure.
record_run_task adds a scoped legacy EXISTS without changing its worker/generation/live lease fence.
Test raw inserts/updates, foreign/missing/workflow/invisible parents, correct scope, taskless states,
parent mutation serialization, replay/fences and deletion. Root accepts A2 before B starts.
B remains an insertion-only scoped visible-legacy event provenance guard, with a task row lock
held to commit because historical events have no task FK. Never filter surviving history by a
live-task join. Check independent generation/read/fail/complete/expiry consumers and taskless events.
No workflow enablement, nullable jobs, admission writer, or03.2 work in either checkpoint.

### A2 implemented contract correction

A2 implementation/evidence: JOB-SLICE-03.1.md tenant association checkpoint. The initial live
schedule-channel scope was corrected after the existing supported schedule-move test failed.
Occurrence company/channel/schedule/snapshot identity is captured under schedule FOR SHARE and
is then immutable. The schedule FK preserves company only; a later channel move affects future
occurrences. task/thread links match the stored occurrence scope, not the current schedule channel.
This supersedes A2's earlier live-channel wording; it follows the actual frozen-snapshot producer.
Applied100000/101000 are immutable. Fresh-schema cutover only, as explicitly required by the user.
A2 correction independent review PASS; root accepts only after final gates. B remains pending.

### User steering supersedes A2 movement contract (09:44Z)

Schedules must be fixed to their channel; no channel movement support or backward compatibility
is required. A2 remains UNACCEPTED pending fresh worker correction across relevant application/
persistence/DB/UI seams and tests. Prior frozen-occurrence scope guards remain useful, but the
movement-preserving review is not new-contract acceptance. Exact handoff and last pre-steering
gates: JOB-SLICE-03.1.md A2 unaccepted handoff. Applied100000/101000 immutable; additive correction.
B still pending after root accepts corrected A2.

### B local reconciliation (2026-09-28, historical provenance worker)

A2 fixed channel accepted. B adds only an insertion trigger on thread_handoff_events:
non-null task_id requires a visible company-scoped legacy task, locked FOR KEY SHARE
until commit. No task FK, historical live-task join, backfill, compatibility shim,
nullable channel, workflow enablement, or admission writer. Taskless events unchanged.
Existing run ownership is proven by scoped task/handoff FKs, auxiliary INSERT/reassignment
visibility guard (93000/94000), and immutable queue identity (73000). Thus generation,
run lookup/completion/failure, draft lookup/resolution/expiry consume only legacy runs.
Failure/stop/reap callsites update still-live tasks before inserting their event; no
post-delete event producer was found. Retain these independent queries unchanged.
Acceptance: raw missing/foreign/workflow/invisible task rejection; both INSERT/delete
lock orders; immutable retained history; workflow run rejection before consumer effects;
legacy complete/fail/generation/resolve plus separately exposed expiry and taskless flows.
Use own_database fixtures and stock2MiB full DB suite, migrations, offline static,
SQLx/fmt/diff/graft, then actual-code independent review. Caller depth-all traversal
performed for event, open, lookup, completion, failure and resolution seams.
Implementer01a0e779-089f-7732-8e46-5d8278a078a0 and nested reviewer
01a0e779-4e60-7442-9af2-1073d983bd42 Astra/medium; capacity verified before edits.

### C/D staged implementation reconciliation (2026-09-28)

Root acknowledged staged approach. First104000 adds immutable run company/id/channel/thread and
execution company/id/run/step/activation, exact NULL-aware job→execution→run validation with
run-first SHARE locks through commit, and conditional legacy channel check. Existing scoped FKs,
source uniqueness, shared attempts, workflow_disabled and channel NOT NULL remain. Disposable
own_database fixtures drop only deployment gates to exercise actual company/channel/thread rows;
existing projection fixtures now create valid associations rather than moving workflow parents.
After independent combined isolation review, a separate additive migration may drop both gates;
then regenerate SQLx against final nullable schema and run complete stock2MiB/static/migration gates.
No admission writer or runtime claims. Parent UPDATE/job INSERT contention must be observed for
run and execution, including both transaction orders. SQLx macro decoder remains fallible.
Implementer01a0e789-5541-7850-bcc4-7615dfb4c686, reviewer01a0e789-a0ce-7e42-8dd5-353622f6b3a3,
Astra/medium; capacity verified before edits; startup8.35%/8.17%; discovery implementer28.84%.

### C/D implemented gate status

C/D checkpoint and exact evidence: JOB-SLICE-03.1.md C/D nullable and exact job association gate.
104000 safety constraints independently reviewed with the real disposable nullable fixtures and
full1963PASS/22ignored before105000 removed workflow_disabled/channelNOTNULL. Final actual
nullable schema SQLx, static and full1963PASS/22ignored at stock2MiB all pass; independent final
gate delta review PASS. Thus dormant/NOTNULL statements above describe earlier checkpoints,
not current schema. Legacy channel CHECK/domain requirement remains, scoped FKs/source uniqueness
and shared attempts retained. No production admission/runtime writer; whole03.1 remains pending.
Root owns C/D acceptance and the following admission/history work. TaskPG55439 handed to root.
