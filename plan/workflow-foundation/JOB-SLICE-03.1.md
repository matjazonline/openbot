# 03.1 job integration slices

## Current bounded source-association prerequisite

Reconciled against original03, shared EXPANSION and BRIEF remaining table. Root approved this
prerequisite before the broad Rust/nullable sweep. Guard and channel NOT NULL remain unchanged.
Add DB-boundary prevention for response_drafts/message_deliveries/human_approvals links to workflow
jobs: these are the three owners whose task deletion loses the live discriminator. Retain existing
scoped FKs and immutable queue identity; task_attempts stays shared. Add explicit legacy selection
to approval locking and human-completion selection, shared checked selection to draft/delivery
writers. Standalone deliveries carry no task association. Other auxiliary records/read/control/
recovery/projection paths remain subject to the complete legacy audit, not accepted by this slice.
Acceptance: raw INSERT and UPDATE rejection for each owner, valid legacy/taskless sources, shared
attempt acceptance, real task-first deletion with notification listener and no workflow wakeups,
writer helper missing/foreign/workflow rejection, migrated DB/full stock-stack/static/SQLx gates
and independent actual-code review. No admission writer, activation or03.2 work.

Root approved splitting the job integration after uncapped discovery. Whole03.1 and the job
slice remain incomplete until every legacy query/nullable consumer is excluded and reviewed.

## Dormant schema and direct-trigger isolation

Add `background_tasks.queue_kind` (`legacy` default / `workflow`) and a nullable
`workflow_execution_id` scoped by company to the existing workflow execution owner. Workflow
payload is exactly version1 + execution UUID; legacy payloads and required channels are unchanged.
A named CHECK permits only legacy rows in deployed schema until the next migration, after the
complete legacy exclusion audit. This prevents partially integrated workflow jobs reaching old
workers. Tests drop that activation guard ONLY inside their own disposable database transaction,
then roll back. No production writer, claim, transition, or nullable channel is enabled.

Direct background-task ownership/status/attention/notification/harness triggers are gated to
legacy rows. Queue kind and execution association are immutable after insert; retries retain their
logical execution identity and task_attempts remains the sole attempt ledger. All existing source
uniqueness and lease constraints remain. Foreign/dangling execution, malformed/non-ID payload,
legacy workflow-link misuse, mutation, migration and trigger-side-effect regressions get DB tests.
No new claim protocol is introduced by dormant schema; competing claims remain mandatory for the
next full exclusion slice and atomic admission writer.

## Remaining job and writer contracts

Next migration must remove the temporary legacy-only guard only after explicit predicates cover
every legacy read/claim/control/recovery/projection, attempt write, DB routine and nullable channel
consumer. It adds conditional nullable association integrity and fallible legacy row decoding.
Current complete persistence inventory: /private/tmp/workflow-jobs-complete-persistence.json
(174groups/313matches, each top-level persistence file/subdirectory independently queried, zero
truncation). This found thread/views.rs omitted by the previous capped inventory. Application
16matches and HTTP5 are uncapped in /private/tmp/workflow-jobs-{application,http}-inventory.json.
Nullable row references: /private/tmp/workflow-jobs-nullable-inventory.json. Migration routines
are unindexed and separately inventoried from migrations/20260817000000_init_schema.sql.

SQL routine consumers beyond direct row triggers still require audit: removed-principal task
release; agent-harness usage checks; notification enqueue and source resolution; task-chain and
ownership notifications; channel/agent deletion cleanup and guards. Direct trigger gating does
not establish full routine isolation.

After complete job acceptance, existing BRIEF remaining-scope table governs one atomic production
admission transaction: company/actor/source/resource locks, exact saved replay and command/source
equivalence, frozen bundle/config/input/limits/deadline, bounded committed history membership,
run state, execution and ID-only first job. Child-action/unsupported resources fail closed.
No03.2 execution activation or worker runtime is in scope.

## Ownership

Implementer /root/workflow_jobs UUID01a0e6cf-b856-72a0-b0d7-d7556cb72ec7 Astra/medium;
reviewer /root/workflow_jobs/reviewer UUID01a0e6cf-fc81-7a72-94ea-65412e9493e1 Astra/medium.
Root owns acceptance/PROGRESS/RESUME. Task PostgreSQL restarted with escalation on55439;
/private/tmp/workflow-admission-pg-e3aa is reused, no existing database changed.

## Dormant slice final checkpoint (2026-09-28)

Implemented additive migration20260928073000_workflow_jobs_dormant.sql; APPLIED and immutable.
Only other source edits are nested module registration in run_schema_tests.rs and new
job_schema_tests.rs. Existing work preserved; no commit/reset/deploy. Production workflow rows
remain impossible through background_tasks_workflow_disabled. channel_id stays NOT NULL.

Independent actual-code review found two verification gaps, both corrected: existing foreign
company execution rejection plus legacy-link/discriminator mutation; observable actionable
notification generation and legacy DELETE withdrawal. Correction actual-code PASS without
remaining findings. Direct activity/harness/pg_notify exclusions were code-reviewed; no separate
runtime notification-listener or competing harness-lock test is claimed. The dormant guard makes
these latent isolation paths inaccessible to production pending the full legacy integration.
No new claim protocol introduced here. Competing legacy/workflow claims, all indirect database
routine isolation, nullable decoding and conditional association integrity remain mandatory next.

Commands/logs all use prefix /private/tmp/workflow-jobs-dormant-:
- migrate.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx migrate run PASS. Isolated OwnDatabase fixtures also apply all7migrations from scratch.
- tests.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
  workflow_job_schema -- --nocapture: corrected4PASS/0ignored.
- workflow.log: same environment and cargo command with workflow filter:194PASS/0ignored.
- full.log: same environment cargo test --locked --offline --lib:
  1919PASS/0failed/22existing ignored,60.48seconds. No new ignored tests.
- check.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets PASS.
- clippy.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings PASS.
- prepare.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx prepare -- --all-targets PASS, run sequentially after offline builds/tests;
  .sqlx unchanged afterward.
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check, git diff --check, graft build PASS.

First pg_ctl sandbox start failed; escalated task-only restart succeeded. All subsequent DB
commands ran with escalation. All build/test/prepare processes finished. Task PG remains running:
/private/tmp/workflow-admission-pg-e3aa, port55439, socket/private/tmp, max_connections200,
DBworkflow_admission, log/private/tmp/workflow-admission-postgres.log. Root explicitly accepted
ownership for transfer to replacement; do not remove during reuse. No existing DB modified.

Context: implementer startup21,619/258,400=8.37%06:59:36Z; correction44.59%115,222 at07:10:02Z;
parent final boundary123,569/258,400=47.82%07:12:55Z. Usage/runtime sources. Natural rotation,
not threshold stop. Reviewer correction63,607/258,400=24.62%07:10:08Z; final evidence sample
reported to root separately. Reviewer quiesces before implementation subtree retirement.
Graft reported savings at least3,075,804tokens for implementer; reviewer at least134,880 separately
(tool estimates, not measured model-token/cost savings; JSON-only inventories excluded).

Whole03.1 and full job slice remain IMPLEMENTING. Next worker must complete exhaustive legacy
Rust-query and database-routine exclusion BEFORE removing deployment guard or making channel
nullable. Start from uncapped inventories above; refresh needed source spans rather than repeat
accepted codec/schema/binding/read-port work. Production admission writer follows only after
complete job slice acceptance. Root exclusively owns PROGRESS/RESUME/README and acceptance.

## Indirect-routine isolation checkpoint (in review)

Root approved a bounded prerequisite before the Rust/nullable audit. New additive migration
20260928080000_workflow_legacy_routines.sql is APPLIED and immutable. It isolates ten indirect
legacy routines: target-channel cleanup, active-agent harness guards, removed-principal release,
actionable enqueue/outreach/ownership notifications, instruction/attention/chain/ownership wakeups.
Explicit legacy predicates retain existing fences; immutable queue identity makes selected CTE
rows remain legacy. Ownership-ledger immutability intentionally stays all-task aggregate protection.
Agent deletion's principal release can only select legacy owners; workflow shape already prohibits
legacy owners. Target-channel deletion excludes workflow jobs; direct association FKs remain intact.
No activation guard removal, channel nullability change, new claim protocol or workflow writer.

New job_routine_tests.rs is nested in job_schema_tests.rs and exercises mixed legacy/workflow
harness changes and principal removal, target-channel cleanup, persisted notification source events
and real PgListener payload isolation. Tests use isolated OwnDatabase fixtures. Two tests roll back
hypothetical workflow rows. The listener test deletes them, flushes deferred triggers, restores the
deployment guard, then commits only to deliver notifications. No workflow row becomes committed.
First run1PASS/2FAIL was fixture setup (missing principal; pending deferred triggers before ALTER);
corrected fixture setup and ownership-version assertion before rerunning. No production fix inferred
from those fixture failures. Static check initially passed; final evidence and review follow below.

Implementer /root/workflow_job_isolation UUID01a0e6dd-9a65-7a62-8307-69a2667f0573,
Astra/medium; startup24117/258400=9.33%07:14:49Z, review boundary93323=36.12%07:21:33Z,
usage/runtime sources. Nested reviewer capacity verified before edits:
/root/workflow_job_isolation/reviewer UUID01a0e6dd-def1-7f80-800f-a128903d23fd,
Astra/medium; startup23690/258400=9.17%07:15:04Z. Implementation paused for actual-code review.
Task PG reused on55439; migration log/private/tmp/workflow-jobs-routines-migrate.log.
Full query/attempt/nullable audit and competing legacy claims versus workflow fixtures remain
mandatory; this checkpoint is not whole job-slice or03.1 acceptance.

### Mandatory blocker before workflow-job enablement

This checkpoint isolates LIVE task associations only, NOT every indirect deletion path. Review
identified task-first deletion as unresolved: `response_drafts_task_fk` and
`message_deliveries_task_fk` use ON DELETE SET NULL(task_id); `human_approvals_task_fk` cascades.
After the task disappears, `enqueue_actionable_notification_event` and
`notify_attention_changed` cannot recover its workflow discriminator from surviving sources (and
approval DELETE may occur after task removal). The PgListener test explicitly removes auxiliary
sources first; it DOES NOT establish task-first deletion isolation.

Preferred integration seam: legacy auxiliary records must never attach to workflow jobs. Audit
all writers/controls with explicit queue_kind predicates, and enforce that association at the DB
boundary for legacy-only owners where appropriate (do not constrain task_attempts to legacy;
it remains the shared ledger). An alternative requires an explicit durable provenance/cleanup
contract. No generic authority store or second queue. Resolve and test the chosen contract with
real task-first deletion BEFORE dropping background_tasks_workflow_disabled. The complete legacy
query/attempt/nullable audit and concurrent claims remain independently mandatory.

Reviewer found two live-source gaps in80000: response_review actionable notifications and
approval/delivery/review attention wakeups. Additive20260928081000_workflow_notification_sources.sql
corrects these without editing the applied migration. Actual-code correction PASS (reviewer
78531/258400=30.39%07:26:02Z). New actual-table source fixtures cover each workflow/legacy/taskless
association. Extended fixture first hit the existing canonical-binding uniqueness constraint;
corrected to reuse the channel's actual binding rather than insert a duplicate. Final evidence due.

### Final live-association checkpoint evidence

All commands finished; no implementation edits after the final fixture correction. Both new
migrations80000/81000 APPLIED and immutable; OwnDatabase fixtures apply all9migrations from scratch.
Prior staged/unstaged/untracked work preserved; no commit/reset/deploy. Production workflow rows
remain impossible through background_tasks_workflow_disabled; channel_id remains NOT NULL.
Independent actual-code correction PASS, including canonical-binding fixture reuse; no remaining
finding within the explicitly bounded live-association scope. Task-first deletion above remains
an enablement blocker, so full indirect-isolation/job-slice/whole03.1 is NOT accepted here.

Evidence prefix /private/tmp/workflow-jobs-routines-:
- migrate.log + migrate-final.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx migrate run PASS for80000 then81000, each additive.
- tests-final.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
  workflow_job_routines -- --nocapture:3PASS/0ignored.
- full.log: same environment cargo test --locked --offline --lib:
  1922PASS/0failed/22existing ignored,60.50s. Includes197workflow-name tests, allPASS;
  no additional workflow-only rerun needed and no separate workflow-filter log claimed.
- check-final.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets PASS.
- clippy-final.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings PASS.
- prepare.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx prepare -- --all-targets PASS sequentially after offline checks/tests;
  git status --short .sqlx empty afterward.
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check, git diff --check, graft build PASS.
  Final post-prepare git diff --check also PASS; graph600files.

Final substantive context118654/258400=45.92%07:29:56Z, usage/runtime sources. Natural rotation
before the broad Rust/nullable segment, not threshold stop. Reviewer latest actual-code report
79778/258400=30.87%07:29:08Z; final evidence acknowledgment recorded separately. Graft tool estimate
saved2656355tokens implementer, reviewer2653263 separately (not measured model cost savings).

TaskPG remains RUNNING for root/next-worker transfer: /private/tmp/workflow-admission-pg-e3aa,
port55439, DBworkflow_admission, socket/private/tmp, max_connections200,
log/private/tmp/workflow-admission-postgres.log. No task build/test process remains. Do not touch
existing databases. Next worker must complete uncapped Rust background_tasks/task_attempts/nullable
inventory and close the task-first deletion blocker before activation guard removal. Real competing
claims/control/recovery/projection tests remain required. Atomic admission writer comes only after
full job checkpoint acceptance. Root exclusively owns PROGRESS/RESUME/README acceptance.

Final independent evidence acknowledgment PASS: reviewer83342/258400=32.25%07:30:38Z,
usage/runtime sources; confirmed full suite and SQLx logs/.sqlx unchanged, now stopped/quiescent
with no resources. Implementer stops after this record and returns ownership to root; no new scope.

## Source-association/task-first deletion prerequisite checkpoint (2026-09-28)

Implemented additive migrations20260928084000_workflow_legacy_source_associations.sql and
20260928085000_workflow_source_task_visibility.sql, both APPLIED and immutable. They prevent
response_drafts/message_deliveries/human_approvals from attaching to workflow tasks, closing the
three provenance-losing task-first deletion paths identified above. Existing scoped FKs still
govern tenant identity/deletion; task_attempts remains shared and the regression inserts a workflow
attempt successfully. Other auxiliary associations and the complete legacy Rust/nullable audit
remain outstanding. No activation guard removal, nullable column, claim/runtime or admission writer.

84000 initially rejected visible workflow rows; self-audit identified a concurrent uncommitted
task INSERT visibility gap.85000 requires a visible legacy row, rejecting before a FK can wait
and accept an unchecked discriminator. A two-connection regression proves the invisible task
rejects immediately and the same legacy task is accepted once committed. No workflow row commits
in any test; hypothetical rows still use rollback or are deleted before restoring the guard/commit.

All3source INSERT inventories are uncapped, zero truncation:
/private/tmp/workflow-jobs-{draft,delivery,approval}-writers.json. Added legacy selection to
approval/transitions::lock_subject and task/operations::complete_human_task; shared
legacy_task::require_legacy_task_on guards delivery::insert_delivery_on and
response_review::create_review_draft_on. Standalone delivery writer has no task association.
No broad read/control/recovery/projection audit acceptance is implied by those writer predicates.

New job_source_association_tests.rs covers all3owners raw INSERT and UPDATE from both legacy and
taskless fixtures, scoped/missing/workflow writer helper rejection, shared attempts and concurrent
visibility. Existing real PgListener regression now deletes tasks FIRST and checks legacy sources'
FK detachment/cascade plus no workflow notifications; it no longer constructs the now-forbidden
workflow auxiliary fixtures. Initial test8PASS/1FAIL was clone INSERT of a generated column;
corrected fixture enumerates only non-generated columns. Corrected focused9PASS predates the third
new concurrency test; full suite below includes all3new tests and the updated listener test.

Independent actual-code review PASS with no findings; reviewer
/root/workflow_legacy_queries/reviewer UUID01a0e6ed-a3aa-7ad2-ad41-2a4d132a655f Astra/medium,
77324/258400=29.92%07:40:41.805Z (usage/runtime sources). No edits during review. Implementer
/root/workflow_legacy_queries UUID01a0e6ed-482b-78a3-ad2a-f858a4d37b1a Astra/medium,
startup24243/258400=9.38%07:32:02.280Z; final substantive100910=39.05%07:42:15.901Z.
Natural handoff before broader Rust audit, not threshold stop. Root owns acceptance/PROGRESS/RESUME.

Evidence prefix /private/tmp/workflow-jobs-sources-:
- migrate.log + migrate-final.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx migrate run PASS,84000 then85000. OwnDatabase fixtures apply all11migrations afresh.
- tests-final.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_job
  -- --nocapture:9PASS/0ignored before final concurrency regression addition.
- full.log: same environment cargo test --locked --offline --lib:
  1925PASS/0failed/22existing ignored in60.57s, includes final concurrency regression PASS.
- check-final.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets PASS.
- clippy.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings PASS.
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check, git diff --check, graft build PASS.
SQLx final evidence acknowledgment follows. All DB commands used escalation on task cluster;
existing databases untouched. Original staged/unstaged/untracked work preserved, no commit/reset/
deploy. Graft estimated savings2820729 implementer,199623 reviewer (not measured model cost).

Final acknowledgment PASS: reviewer78207/258400=30.27%07:43:22.216Z, usage/runtime sources;
quiescent, no edits/resources. Full suite includes200workflow-name tests, no separate filter rerun
needed. prepare.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
cargo sqlx prepare -- --all-targets PASS sequentially after offline builds/tests; .sqlx unchanged.
Final post-prepare git diff --check PASS. Graph602files. All task build/test processes finished.
TaskPG remains RUNNING; ownership transfers to root: /private/tmp/workflow-admission-pg-e3aa,
port55439/socket/private/tmp/max_connections200/DBworkflow_admission,
log/private/tmp/workflow-admission-postgres.log. Preserve during reuse; eventual cleanup task-only.
Next worker resumes complete Rust background_tasks/task_attempts/nullable audit from uncapped
inventories above. The3source deletion blocker is closed within this bounded reviewed slice;
other auxiliary associations, claims/controls/recovery/projections and fallible legacy decode remain
mandatory before activation guard removal. Atomic admission writer follows full job acceptance.

## Core Rust queue slice reconciliation (in progress)

Root approved queue.rs/operations.rs legacy isolation plus fallible nullable row decode. All SQL
background_tasks reads/claims/status writes/recovery/projections and task_attempts operations in
these two modules deliberately scope to legacy; retain worker/generation/ownership/lease fences.
Shared attempts remain available for future workflow runtime through independent workflow paths.
No schema change or activation; channel NOT NULL and deployment guard stay. Test isolated mixed
fixtures, concurrent legacy claimants, direct lifecycle rejection, shared attempt preservation and
missing-channel decode. Full DB stock2MiB/static/SQLx and independent actual-code review required.
Other task modules board/collaboration/controls/counts/harness_runs/instructions/mcp_journal/
outreach/ownership plus external consumers remain outstanding; complete inventories listed above.
Auxiliary association audit is NOT accepted by these task-row predicates.

## Core Rust queue checkpoint (2026-09-28; final SQLx acknowledgment pending)

Implemented legacy predicates throughout task/queue.rs and task/operations.rs background-task
reads, claims (including fairness counts), status/lease writes, recovery, lists and attempt ledger
entry points. Legacy enqueue explicitly records its discriminator; attempt begin fails when its
legacy selection matches nothing. Shared workflow attempt rows remain valid at the DB boundary.
BackgroundTaskDb.channel_id is now optional and conversion rejects missing channel with row ID;
domain BackgroundTask.channel_id remains required. channel_gate::task_gate_key_on now excludes
workflow tasks before nullable decoding or assignment locking. No migrations changed; deployment
guard and channel NOT NULL remain installed. No production admission writer or03.2 activation.

Independent actual-code review found omitted ancillary entry queries in current modules. Corrected
get_outreach_context/complete_outreach, target-list reads, request-message writes (reject unsupported
workflow association before canonical insert), cancellation helper and outreach-lock selection.
Predicates preserve existing generation/worker/ownership/live-lease fences. Remaining broader
auxiliary association DB guards and other modules are not accepted. Durable source-group checklist:
[JOB-QUERY-REMAINING-03.1.md](JOB-QUERY-REMAINING-03.1.md), built from uncapped inventory with
exact indexed symbols/line hints (refresh spans when editing). No repeated broad discovery needed.

New workflow/job_core_query_tests.rs has3isolated DB tests: simultaneous real legacy claimants
observe a workflow row and claim legacy once; read/control/target exclusions; workflow recovery
and attempt preservation while legacy recovery works; nullable conversion failure. These tests
commit hypothetical workflow fixtures only in OwnDatabase disposable databases, necessary for
independent claimant visibility; production/task base DB guard remains unchanged. Shared raw
workflow attempt INSERT is accepted; legacy begin/finish/query cannot alter or expose it.
Initial test compile corrected argument order and actor field; first DB run denied sandbox and
was rerun escalated. First escalated1PASS/2FAIL reflected fixture missing agent principal and
machine_id; corrected3PASS before final helper corrections. Final full suite below includes all
latest corrections. No ignored tests added.

Independent actual-code correction PASS no findings, reviewer
/root/workflow_query_sweep/reviewer UUID01a0e6f9-511c-75c1-a2c6-09bdf618c6fa Astra/medium,
89174/258400=34.51%07:54:08.262Z. Implementation paused for actual reviews. Implementer
/root/workflow_query_sweep UUID01a0e6f9-0518-7571-8979-dec7ac6a3961 Astra/medium; startup
24167/258400=9.35%07:44:44.650Z; final substantive120341=46.57%07:57:07.165Z, usage/runtime
sources. Natural rotation after final checks, no further scope.

Evidence prefix /private/tmp/workflow-core-:
- tests-corrected.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
  workflow_job_core -- --nocapture:3PASS before final ancillary helper corrections.
- full.log: same environment cargo test --locked --offline --lib:1928PASS/0failed/22existing
  ignored in76.20s, including latest3new tests. OwnDatabase tests migrate all11migrations afresh.
- check-final.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets PASS.
- clippy-final.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings PASS.
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check, git diff --check, graft build PASS;603files.
- prepare.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx prepare -- --all-targets running sequentially after all offline tests/builds; final
  outcome and metadata status recorded below.

TaskPG remains RUNNING; root explicitly accepted ownership transfer at final handoff:
/private/tmp/workflow-admission-pg-e3aa port55439/socket/private/tmp/max_connections200/
DBworkflow_admission/log/private/tmp/workflow-admission-postgres.log. Existing DBs untouched.
Preserved prior staged/unstaged/untracked work; no commit/reset/deploy.

Final SQLx/evidence acknowledgment PASS: prepare.log completed30.18s; .sqlx unchanged after
regeneration, post-prepare git diff --check PASS. Reviewer92277/258400=35.71%07:58:43.847Z,
usage/runtime sources, quiescent with no edits/resources. Implementer final boundary
125095/258400=48.41%07:58:41.822Z; natural retirement, no next work. All build/test/prepare
processes finished. Graft tool estimate saved2893374tokens implementer (not actual model-token
or cost savings). Root owns bounded acceptance and the running taskPG cleanup obligation.
Whole03.1 and full job/nullable/auxiliary isolation remain IMPLEMENTING; resume exact remaining
source-group checklist, then nullable conditional association schema only after complete audit.

## Remaining task-module reconciliation (2026-09-28; implementing)

Root accepted all remaining task-module consumers as one segment. Explicit legacy exclusion added
throughout board/collaboration/counts, controls, instructions, ownership, harness reads/locks and
outreach tally. Preserve status/lease/generation/ownership fences and shared attempt ledger.
MCP journal operations call the checked harness execution lock; remaining channel_gate functions
only acquire ordered advisory channel keys. Ancillary independent harness/ownership reads also
need discriminator checks, not just task-row selectors. Production guard/NOT NULL remain; no
migration/admission/runtime activation. External consumers and remaining auxiliary DB association
guards are still outstanding. Task-specific uncapped inventory:
/private/tmp/workflow-task-consumers-inventory.json (173matches/22files; zero truncation).
One new mixed disposable-DB projection regression checks workflow-only chains are absent and
shared correlations neither expose workflow task detail/collaboration nor inflate board/counts.
Existing control/harness/instruction tests plus full DB suite establish preserved legacy behavior.
Initial compile corrected test field names terminal_since and counts.total_tasks; final checks due.

## Remaining task-module checkpoint (2026-09-28; final SQLx acknowledgment pending)

Implemented explicit legacy predicates in task/{board,collaboration,controls,counts,harness_runs,
instructions,outreach,ownership}.rs. Includes recursive child lookup, overlapping-correlation
rollups, independent checkpoint/ownership reads, attempt updates and guarded control entry points.
MCP journal operations inherit checked execution locking; channel_gate's other functions only
acquire ordered advisory keys. No schema/migration edits, admission writer or activation; guard
and channel NOT NULL remain. The shared attempt ledger remains unchanged at the DB boundary.

Independent review found2ancillary reads: standalone chain status events and ask-owner stored
instruction replay before task locking. Both now require a legacy task. Three new mixed isolated
DB regressions cover workflow-only/shared-correlation board/detail/collaboration/counts and raw
auxiliary event/replay exclusions, while preserving legacy event/replay behavior. Hypothetical
workflow fixtures commit ONLY in disposable OwnDatabase fixtures, never task base/production DB.
Initial test builds corrected field names and PrincipalId import; no DB test failed or was ignored.
Existing legacy controls/harness/instructions exercised by full suite. No new concurrency protocol;
previous accepted core competing-claimant regression remains in full suite. Task sourcegroups are
checked in JOB-QUERY-REMAINING; the existing counts test fixture group remains unchecked because
it was not separately inspected. External consumers, remaining auxiliary DB association guards and
conditional nullable schema/full job acceptance remain outstanding. No new broad inventory needed.

Independent actual-code and focused-regression PASS, no remaining finding within this bounded
scope. Reviewer /root/workflow_task_consumers/reviewer UUID01a0e707-5fe6-76a2-bc56-7a043ed9afbb,
Astra/medium; startup23629/258400=9.14%; final97193=37.61%. Implementer
/root/workflow_task_consumers UUID01a0e707-1daf-7850-abf3-2c23b60d4f30 Astra/medium;
startup24118=9.33%; substantive119824=46.37%08:11:06.978Z, usage/runtime sources.
Reviewer available before edits; edits paused for both actual-code reviews. Root owns acceptance.

Evidence prefix /private/tmp/workflow-task-consumers-:
- tests-corrected-final.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
  workflow_job_task_ -- --nocapture:3PASS/0ignored.
- full.log: same environment cargo test --locked --offline --lib:
  1931PASS/0failed/22existingignored in60.60s.
- check-corrected.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets PASS.
- clippy.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings PASS.
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check, git diff --check, graft build PASS.
- prepare.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx prepare -- --all-targets running sequentially after offline builds/tests; final below.

Task PG RUNNING, ownership transfers to root: /private/tmp/workflow-admission-pg-e3aa port55439,
socket/private/tmp/max_connections200/DBworkflow_admission,
log/private/tmp/workflow-admission-postgres.log. Existing databases untouched; all pre-existing
staged/unstaged/untracked changes preserved, no commit/reset/deploy. Root ownsPROGRESS/RESUME/README.

Final evidence acknowledgment PASS: reviewer97841/258400=37.86%08:13:17.785Z,
usage/runtime sources, quiescent with no edits/resources. prepare.log PASS15.63s; .sqlx unchanged
and postprepare git diff --check PASS. All build/test/prepare processes finished. Implementer
final substantive123523/258400=47.80%08:13:19.041Z, natural retirement before another segment.
Graft estimated savings2729164tokens implementer (not measured token/cost savings). TaskPG remains
RUNNING for root transfer. This is bounded task-module acceptance only, not full job/03.1 completion.

## External action consumers reconciliation (2026-09-28; implementing)

Bounded segment: approval/transitions, response_review/commands, attention and notification.
Explicit legacy selection covers task locks/updates, review evidence and shared attempts, actionable
projections, operational metrics and notification resolution. Optional taskless sources remain
visible; a non-null workflow task must never become taskless through a filtered LEFT JOIN.
Attention command replay requires a legacy source before returning an auxiliary event. Existing
authorization, expiry, ownership and generation fences remain. No migration or enablement change.
Acceptance: mixed disposable-DB projections/control/evidence/notification regressions plus existing
legacy/taskless suite; full DB at2MiB, offline all-target check/Clippy, fmt/diff/graft, sequential
SQLx prepare and independent paused-tree review. Other external groups/auxiliary DB association
guards remain unchecked. Reviewer verified before edits; root accepted this reconciliation.

## External action consumers checkpoint (2026-09-28; final gates pending)

Six production files approval.rs/approval/transitions.rs,response_review.rs/commands.rs,
attention.rs,notification.rs now explicitly exclude workflow tasks in direct reads/locks/updates,
shared-attempt evidence, notifications, attention projections/metrics and auxiliary command replay.
Optional taskless sources preserved; handoff-run attention association additionally guarded.
No migration/nullability/enablement changes. Remaining external groups and auxiliary DB guards
remain outstanding; no whole03.1 acceptance. Source fixtures module hoisted once from routine
tests to job_schema_tests; new job_external_query_tests.rs nested in job_core_query_tests.

Independent reviewer found two missing regression assertions: real shared-attempt evidence and
matching auxiliary attention replay. Added both. Final actual-code PASS after correcting replay
expected version. Three targeted isolated-DB tests cover mixed jobs, legacy/taskless approval,
review and failed-delivery projection retention, operational counts, task command/replay rejection,
notification source rejection and workflow attempt evidence rejection with legacy counterpart.
Initial failures were fixture assumptions (auto-agent-owned tasks are not actionable; workflow
owner mutation violates shape), corrected; compile generic-string inference and duplicate-module
Clippy issues corrected. No ignored tests added. Production query review had no defect finding.

Implementer /root/workflow_external_consumers UUID01a0e714-aa51-7f63-be3d-0977e52b76fc
Astra/medium startup24157/258400=9.35%; latest124331=48.12%08:26:11.710Z usage/runtime.
Reviewer nested/reviewer UUID01a0e714-f678-7e03-a4a1-51d3ab719c3b Astra/medium
startup23759=9.19%; finalactual85410=33.05%08:26:41.615Z usage/runtime; quiescent.
Reviewer capacity verified before edits, edits paused for each actual-code review.

Evidence prefix /private/tmp/workflow-external-:
- tests-corrected-final.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
  workflow_job_external_ -- --nocapture:3PASS/0ignored. Final version-relative replay assertion
  revised afterward (fixture version1 meant same expected value); final full suite includes it.
- check-final.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets PASS.
- clippy-final.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings PASS.
- fmt-final.log/diff-final.log/graft.log: cargo fmt --all -- --check,git diff --check,graft build PASS.
- full.log: same DB/stock-stack environment cargo test --locked --offline --lib RUNNING.
- prepare remains due sequentially after builds/tests.

Exact pre-slice six-file copies /private/tmp/workflow-external-baseline/; preserve all preexisting
staged/unstaged/untracked work. All migrations through85000 unchanged. TaskPG RUNNING at55439
/private/tmp/workflow-admission-pg-e3aa/socket/private/tmp/max_connections200/DBworkflow_admission;
log/private/tmp/workflow-admission-postgres.log. Root owns transfer/cleanup and acceptance records.

Final full.log PASS1934/0failed/22existingignored at2MiB60.57s. Explicit six-file code-audit
source groups checked after independent actual-code PASS. Final SQLx prepare.log RUNNING
sequentially; all offline builds/tests finished. Implementer128082/258400=49.57%08:28:35.125Z
natural retirement; root must verify prepare completion/.sqlx/diff and obtain reviewer evidence
acknowledgment or assign final gate to replacement before accepting segment. No open code finding.

Final SQLx/evidence acknowledgment PASS: reviewer independently verified prepare.log finished
16.16s and .sqlx clean, full1934PASS22existingignored and static logs. Reviewer quiescent
87237/258400=33.76%08:29:24.533Z; directly informed root. No code finding or known running
build/test/prepare remains. Implementer naturally retires at last substantive49.57%.

## External projection reconciliation (2026-09-28; implementing)

Root accepted bounded scope after re-reading original03/shared expansion/BRIEF remaining table:
channel_assignment, company_invite, dashboard (including shared attempt joins), schedule and
thread inbound/reply/views. Explicit legacy predicates preserve ownership/lease fences and
optional taskless behavior. Inspect unchecked fixture-only groups before closing their audit rows.
No migration, nullable-channel, enablement, admission-writer or03.2 work; remaining auxiliary DB
and application/HTTP/nullable audit plus whole-job acceptance precede production admission.
Meaningful mixed disposable-DB projection/control tests, prior unaffected competing-claimant
evidence, offline all-target check/Clippy, stock-stack full suite, SQLx prepare, fmt/diff/graft and
independent paused-tree actual-code review are required. Exact seven-file baseline:
/private/tmp/workflow-projections-baseline. TaskPG restarted at55439 with existing task directory.
Implementer /root/workflow_projections UUID01a0e72c-3189-7ad3-8be1-66ba7bba4d84
Astra/medium; nested independent_review UUID01a0e72c-8776-7921-9cd0-4b560fbd5905
Astra/medium verified before edits (startup9.33% /9.12%). Root owns queue acceptance.

## External projection checkpoint (2026-09-28; final gates pending)

Seven production files channel_assignment.rs,company_invite.rs,dashboard.rs,schedule.rs,
thread/inbound.rs,thread/reply_publication.rs,thread/views.rs now exclude workflow jobs from
legacy removal selection/relocks, member work-at-stake, all task/attempt dashboard aggregates,
schedule and thread projections. Existing fences/scopes/bounds retained. Removal relock extracted
to keep touched function small. No migration/nullable/enablement changes. Two SQLx macro metadata
entries replaced for company_invite. Fixture-only groups independently inspected and scoped;
JOB-QUERY remaining audit rows checked following independent actual-code PASS. This is not
auxiliary DB guard/application/HTTP/nullable acceptance or whole-job/03.1 completion.

New job_projection_tests.rs nested once in job_core_query_tests exercises reachable mixed valid
workflow/legacy rows: global/company dashboard queue/throughput/depth/outstanding and shared
attempt tokens/latency/retries; thread correlation fallback retaining message/legacy task;
member delegated asks excluding workflow auxiliary outreach; channel assignment removal preserving
workflow row while stopping legacy. Workflow shape is never weakened. Prior accepted competing
claimant evidence remains applicable; no concurrency protocol changed.

Independent actual-code PASS and final correction PASS from nested independent_review
UUID01a0e72c-8776-7921-9cd0-4b560fbd5905 (Astra/medium), final fixture review25.26%
65,272/258,400 at08:50:55.149Z, token_usage_record.usage/task_started.model_context_window.
Edits paused during each review. Findings corrected: i64 latency expectations and non-vacuous
exact latency measurements. Fixture failures corrected: legacy task needs explicit owner and
ChannelWrite.agent_ids must be Some(empty) to actually remove assignments. No production defect.
Implementer UUID01a0e72c-3189-7ad3-8be1-66ba7bba4d84 latest44.21%114,247/258,400
08:50:40.868Z same measurement sources. Approximate graft savings this worker ~2.86M tokens.

Evidence prefix /private/tmp/workflow-projections-:
- prepare-initial.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx prepare -- --all-targets PASS (initial sandbox connection denied, escalated retry).
- tests-final2.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
  workflow_job_projection_ -- --nocapture RUNNING. Prior final.log3PASS/1fixtureFAIL.
- check.log/clippy.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets /
  cargo clippy --locked --offline --all-targets -- -D warnings PASS23.02s/30.47s; final
  fixture-only change requires fresh static gate.
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check,git diff --check,graft build PASS;
  final refresh after last fixture due. Full stock-stack library suite/final sequential SQLx due.

TaskPG RUNNING with unchanged task resource55439/socket/private/tmp/DBworkflow_admission;
root owns acceptance and cleanup/transfer. No commit/reset/deploy. No next slice begun.

### Projection final gate failure / handoff (2026-09-28 08:54Z)

All4focused tests PASS0ignored (tests-final2.log); final offline all-target check PASS14.18s,
Clippy-Dwarnings PASS19.81s, final fmt/diff/graft PASS. Full stock2MiB library suite
FAILED1937PASS/1FAIL/22existingignored60.63s (full.log). Failure is REAL, not waived:
thread::view_tests::task_lookup::a_full_message_page_uses_bounded_index_probes_on_skewed_history
L362: fallback now uses background_tasks_correlation_idx+Sort, ~1195rows/loop, instead of bounded
ordered index probe. Added queue_kind predicate is not covered by the existing index, preventing
index-only projection; prior migration20260912010000 includes only task_type. Full EXPLAIN JSON
is in full.log failure section. Prior production/query rationale lives in
docs/query-evidence/2026-09-12-message-task-lookup.md.

NO index/migration correction attempted yet. Likely fix: new additive migration replacing the
same named ordered index with queue_kind as an equality key before preference ordering, retaining
task_type INCLUDE and thread_id partial predicate; assess against original query evidence and
mixed workflow-heavy history so discriminator filtering cannot reintroduce unbounded scans.
Do not edit applied migrations or weaken test bounds. Exact source pointers:
migrations/20260912010000_index_thread_task_matches.sql:5; thread/views.rs THREAD_TASK_LOOKUP_SQL;
thread/task_lookup_tests.rs:279–381. Further independent actual-code index/test correction review,
focused regression + static/full DB/stock-stack/SQLx gates required.

Sequential gate chain stopped at full-suite failure; prepare-final.log not created. Two macro
cache replacements remain correct from initial prepare. All build/test/prepare processes finished.
TaskPG remains RUNNING for root transfer. Reviewer quiescent after final fixture PASS; latest
25.26% at08:50:55Z (later parent-measured25.56%), no unresolved scoped code review finding but
new full-suite performance regression NOT reviewed/corrected. Worker stops naturally at
47.86%123,662/258,40008:54:29.741Z (usage/runtime sources). Query checklist checkmarks mean
explicit discriminator audit only; this segment is NOT accepted until performance gate corrected.
No03.2/nullable/guard changes, no commit/reset/deploy. Graft total ~2.86M saved tokens.

## Projection index correction expansion (2026-09-28)

Reconciled original03/shared contracts/BRIEF remaining criteria and the existing query evidence.
Add migration20260928090000 replacing only background_tasks_thread_correlation_match_idx:
(company_id,thread_id,correlation_id,queue_kind,preference DESC,created_at DESC,id ASC), retaining
INCLUDE(task_type) and thread_id partial scope. Equality on queue_kind restores covering access
and prevents workflow rows from being scanned then filtered; INCLUDE alone would not prevent
that scan. Keep source uniqueness and original ordering; no bound increase/query hint/second owner.
Applied migrations through85000 are immutable. Normal transactional index rebuild blocks writes
and needs deployment sizing, as the original did; no deployment is performed here.

Keep original 200-message skewed-history guard unchanged. Add isolated migrated mixed workflow
history regression using production SQL (test-only internal re-export), both custom/generic plans,
legacy winner plus workflow-only correlation, no history sort and no discriminator rows filtered.
Retain production workflow-disabled constraint everywhere except disposable fixtures. All existing
projection behavior/tests are preserved. Required final gates: targeted lookup/projection tests,
stock2MiB full DB suite, locked offline all-target check/Clippy, sequential SQLx prepare, migrations,
fmt/diff/graft and paused-tree actual-code review of correction plus inherited slice integration.
Implementer01a0e73a-5134-7911-acd4-a413cfd2607d startup24094/258400=9.32%; reviewer
01a0e73a-89ec-7e23-82cb-f23de81335d7 startup23618=9.14%, verified before edits. Root acceptance
and all remaining auxiliary/application/HTTP/nullable/admission scope remain unchanged.

### External projection/index final evidence (2026-09-28)

Index correction20260928090000_index_legacy_thread_task_matches.sql is APPLIED and immutable.
It restores ordered covering probes with queue_kind as an equality key, not a filter. Original
200-message plan guard unchanged; new job_projection_index_tests.rs proves the legacy winner and
workflow-only absence cases under both custom/generic plans over12000workflow+10000legacy history.
Internal test-only re-export uses the actual production SQL. Updated query-evidence document
retains the visibility/statistics caveat and transactional index-build write-blocking risk.
No guard removal/channel-nullability/admission/03.2 change; no commit/reset/deploy.

Independent paused-tree actual-code PASS, no findings, covers correction and all seven inherited
projection diffs against /private/tmp/workflow-projections-baseline. Reviewer
/root/workflow_projection_index/review_projection_index UUID01a0e73a-89ec-7e23-82cb-f23de81335d7,
Astra/medium; review sample63715/258400=24.66%09:00:51.033Z, usage/runtime capacity sources.
Prior unrelated per-slice review/competing-claimant evidence remains valid; no claim protocol changed.

All final commands finished. Evidence prefix /private/tmp/workflow-projection-index-:
- migrate.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx migrate run PASS (90000); isolated fixtures also migrate from scratch.
- projection.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
  workflow_job_projection_ -- --nocapture PASS5/0failed/0ignored.
- lookup.log: same environment and command with task_lookup filter PASS4/0failed/0ignored,
  including unchanged a_full_message_page_uses_bounded_index_probes_on_skewed_history.
- full.log: same DB/stock-stack environment cargo test --locked --offline --lib
  PASS1939/0failed/22existingignored60.44s, including prior competing-claimant regressions.
- check.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets PASS18.11s.
- clippy.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
  PASS30.18s.
- prepare.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx prepare -- --all-targets PASS22.37s, sequential after tests/offline checks.
  sqlx-check.log + sqlx-before.json prove all45cache files unchanged by final prepare;
  inherited two company_invite macro replacements retained.
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check, git diff --check, graft build PASS;
  graph606files. schema.log verifies installed index, workflow_disabled and channel NOTNULL.

Implementer /root/workflow_projection_index UUID01a0e73a-5134-7911-acd4-a413cfd2607d
Astra/medium latest substantive81680/258400=31.61%09:03:05.635Z (usage/runtime capacity sources).
Graft reported2572095tokens saved implementer, at least~2.76M reviewer (tool estimates only).
Root exclusively owns acceptance/PROGRESS/RESUME. Bounded external projection slice is ready for
acceptance after final evidence acknowledgment; whole job integration and03.1 remain incomplete.
Next: auxiliary association/application/HTTP/nullable audit, whole-job enablement and review, then
actual atomic admission/history/authority/source replay integration. Do not start03.2.

TaskPG remains RUNNING for root transfer: /private/tmp/workflow-admission-pg-e3aa, port55439,
socket/private/tmp, max_connections200, DBworkflow_admission,
log/private/tmp/workflow-admission-postgres.log. No build/test/prepare process remains.

Final independent evidence acknowledgment PASS, no unresolved scoped findings; reviewer verified
all required logs and checkpoint claims, now quiescent at71469/258400=27.66%09:04:17.184Z,
usage/runtime capacity sources. Implementer final boundary84444/258400=32.68%09:04:18.532Z,
same sources. Root receives acceptance/resource ownership packet; no next slice started.

## Remaining isolation audit handoff (2026-09-28)

Root accepted external projection/index slice. Subsequent audit-only bounded queue is
JOB-ISOLATION-REMAINING-03.1.md, independently reviewed PASS against catalog/relevant source.
It accounts for17direct taskFK owners plus indirect harness invocations and historical handoff
events, shared attempt exception, two tenant guard gaps, independent auxiliary consumers,
required-channel application/HTTP boundary and coordinated nullable/exact-association gate with
concurrent parent-update test. No code/schema changes or enablement. Implementer natural retirement
at last substantive48.77%, reviewer34.94% quiescent. TaskPG transferred to root running55439.

## Scoped auxiliary association checkpoint A1 (2026-09-28)

A1 from JOB-ISOLATION-REMAINING-03.1.md is implemented and independently actual-code reviewed;
root owns acceptance. Additive applied migrations93000/94000 require a visible scoped legacy
parent for INSERT or task/company reassociation across eleven compound-FK auxiliary owners.
Existing3source guards unchanged; task_attempts remains shared; harness invocations inherit the
scoped harness-run provenance. Existing FK cascades and ownership-journal immutability preserved.
Workflow-disabled guard and channel NOTNULL remain; no admission writer or03.2 work.

New job_auxiliary_association_tests.rs + job_auxiliary_fixtures.rs verify all11 raw INSERT/UPDATE
workflow/missing/foreign rejection, valid legacy owners, shared workflow attempt insertion,
all-owner invisible parent rejection before FK waits, committed legacy acceptance, and actual
observed FK lock contention against task deletion. Cascades remove all11owners and outreach
target/reply descendants, with real PgListener delivery/marker. Status fixtures include both
related_approval_id and related_outreach_id, exercising their SETNULL actions during deletion.
Old mixed fixtures in job_core_query_tests/job_projection_tests/job_routine_tests now use reachable
legacy associations and/or assert the workflow rejection before preserving reader assertions.

Independent reviewer initially found two missed impossible raw fixtures and a potential status
SETNULL/parent-delete visibility edge. Both fixtures corrected. Runtime reproduction with linked
status events against93000 PASSED, so no reproduced cascade bug is claimed. Followup94000 makes
association-only validation explicit: unchanged task+company UPDATE bypasses revalidation, while
independent immutable-journal triggers still run. Migration93000 was already applied and remains
immutable;94000 is additive. Correction actual-code PASS; no remaining findings.

Evidence prefix /private/tmp/workflow-auxiliary-a1-:
- full.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib -- --nocapture
  →1942PASS/22existingignored/0fail; all27workflow_job tests, including3new A1 tests, PASS.
- check-final.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets →PASS.
- clippy.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings →PASS.
- migrate.log/migrate-final.log: DATABASE_URL same task database cargo sqlx migrate run
  →93000/94000PASS; disposable own_database tests also exercise fresh full migrations.
- prepare.log: DATABASE_URL same task database cargo sqlx prepare -- --all-targets →PASS,
  run sequentially after offline compilation; .sqlx unchanged from inherited two replacements.
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check, git diff --check, graft build →PASS.
- tests.log was sandbox-localhost PermissionDenied and is NOT evidence; escalated tests-live.log
  had25PASS/2oldfixtureFAIL before correction. delete-regression.log proves the suspected cascade
  failure did not reproduce (1PASS against93000), not a failing-then-passing regression claim.

Implementer /root/workflow_auxiliary UUID01a0e747-c3e9-7553-a693-d2b119d99a9b Astra/medium;
latest substantive105672/258400=40.89%09:21:29.775Z, usage/runtime sources. Reviewer
/root/workflow_auxiliary/reviewer UUID01a0e748-0d5a-7532-9547-f954c54f86e6 Astra/medium;
correction review88999/258400=34.44%09:21:44.429Z, same sources. Final samples below/at handoff.
TaskPG/private/tmp/workflow-admission-pg-e3aa remains RUNNING port55439 DBworkflow_admission,
socket/private/tmp max_connections200 log/private/tmp/workflow-admission-postgres.log. Transfer
ownership back to root for next worker. No existing DB touched; no commit/reset/deploy.
A2(provisioning/schedule tenant guards) and B(historical handoff provenance) remain pending;
C/D nullable/exact-run association plus parent-update race is a later coordinated gate. Whole03.1
is not accepted. Natural rotation before A2/B; do not reuse retired subtree.

Final A1 reviewer evidence acknowledgment PASS:91045/258400=35.23%09:23:48.979Z;
reviewer quiescent, no edits/resources. Implementer final substantive112492/258400=43.53%
09:24:35.776Z (usage/runtime capacity sources); naturally retiring below50% before unrelated
A2/B rather than claiming a threshold stop. Counted graft estimated savings2,886,913tokens
implementer (estimate, not measured model-token/cost savings). All build/test/prepare/graft
processes finished; only explicitly transferred taskPG remains running. Root owns next assignment.

## Tenant association checkpoint A2 (2026-09-28)

A2 is implemented and correction actual-code reviewed PASS; final evidence completion below.
Additive applied100000/101000 migrations retain the original task FKs and cascades, add relational
company scope to agent_channel_provisions and company/channel occurrence scope to schedule_runs,
and require visible legacy parents for raw insertion/reassociation. Provision links match the
actual agent/channel tenant. Shared attempts and accepted A1 guards remain unchanged.

The initial live schedule-channel FK exposed a real current behavior regression in the full suite:
a schedule can move after occurrences exist.101000 corrects this additively. Claims restore their
saved ChannelSchedule snapshot (schedule.rs:115-127); execute_schedule_run uses that snapshot's
company/channel (application/use_cases/schedule.rs:552-706). Occurrence capture takes FOR SHARE on
the current schedule, storing immutable company/channel/schedule/snapshot identity; only company
is tied to the live schedule. Moves affect future occurrences, never already captured work.
record_run_task(schedule.rs:470-506) matches the occurrence's company/channel/thread and retains
worker/generation/live lease predicates. Composite task/thread FKs serialize later parent changes.
The plan/user require fresh-schema cutover, not backward compatibility or business-data migration.

New job_tenant_association_tests.rs and job_tenant_fixtures.rs cover6cases: raw INSERT/UPDATE
workflow/missing/foreign/owner mismatch rejection; taskless pending/failed states; existing
materialized SETNULL CHECK deletion failure with atomic rollback and provisioning cascade after
removing the occurrence; invisible-task immediate rejection plus committed acceptance; actual
record_run_task unchanged-row rejection for bad links/stale worker/generation/expired lease and
replay rejection; schedule movement before materialization; snapshot mismatch/immutability; real
parent-first schedule-move capture contention and child-first task-thread UPDATE contention.
Existing schedule CRUD/movement, provisioning atomicity/idempotency and A1 tests remain intact.
Shared auxiliary fixture module registration moved to their common parent; no accepted behavior
was changed. Reviewer's126line test finding resolved by separate matrix/taskless deletion tests.

Reviewer /root/workflow_tenant_links/reviewer UUID01a0e755-9e97-7410-9a99-4130f515176e
Astra/medium independently inspected corrections/callers/FKs and ran a rollback-only pending
schedule/thread company cascade check. Correction PASS104184/258400=40.32%09:41:20.875Z,
usage/runtime capacity sources. Implementer /root/workflow_tenant_links UUID
01a0e755-6bea-79d3-9836-a83669900bb8 Astra/medium, latest108885/258400=42.14%09:42:39.897Z.
B remains unimplemented; natural worker rotation planned after A2 evidence/acceptance.

### A2 unaccepted handoff — new fixed-channel user contract (09:44Z)

USER explicitly changed intended semantics: schedules are fixed to a channel; channel movement
need not be supported. This supersedes the movement-preserving correction/review above. A2 is
NOT accepted and requires a fresh worker correction; do not treat the following passing gates
as proof of the new contract. No fixed-channel correction has yet been implemented.

Next worker: graft callers/grep for SchedulePersistence::update and application update schedule,
inspect UI channel-change controls and relevant HTTP inputs. Enforce immutable schedule channel
at domain/application update boundary and persistence/DB; remove any supported channel-move UI.
Retain tenant scope, occurrence snapshot/task/thread provenance, raw visible-legacy guards and
lease fences. Preserve applied migrations100000/101000 unchanged; use an additive migration.
Do not blindly restore the earlier redundant schedule/channel FK if channel immutability itself
closes the invariant. Update the existing schedule CRUD movement test to assert rejection with
no mutation; revise A2's move-before-materialization/concurrent-move tests for fixed identity.
Prove raw SQL UPDATE and real concurrent capture/change cannot change channel, plus ordinary
schedule edits still work. Fresh-schema cutover, no backward compatibility/old-data repair.
Independent actual-code review plus fresh affected/full/static/SQLx/fmt/graft gates required.
B remains untouched and follows root A2 acceptance; no nullable/admission writer/03.2 work.

Last valid PRE-STEERING tree gates (prefix /private/tmp/workflow-a2-):
- full-final.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib -- --nocapture
  →1948PASS/22existingignored/0fail, including33workflow_job tests and6newA2 tests.
- check-corrected.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets →PASS.
- clippy-corrected.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings →PASS.
- migrate-live.log/migrate-correction.log: DATABASE_URL taskPG cargo sqlx migrate run →100000/101000PASS.
  Disposable own_database tests additionally exercise fresh full migration chain.
- prepare.log: DATABASE_URL taskPG cargo sqlx prepare -- --all-targets →PASS, sequential after
  offline check/Clippy/test compilation. .sqlx remains inherited two replacements, no A2 changes.
- fmt-final.log/diff-final.log/graft-corrected.log: fmt --check, git diff --check, graft build →PASS.
- full.log:1945PASS/1FAIL/22ignored revealed supported-move regression of100000; later corrected.
  full-corrected.log1948PASS preceded fixture-file extraction; full-final.log supersedes it.
- targeted.log: initial deferred foreign-fixture active-agent failure, fixed by disabling the unused
  foreign channel. jobs.log31PASS preceded the later snapshot/delete test split additions.
- migrate.log is sandbox permission failure only, not evidence; migrate-live.log is successful.

Last verified implementer UUID01a0e755-6bea-79d3-9836-a83669900bb8 at113068/258400=43.76%
09:44:30.952Z (token_count/runtime capacity); natural retirement before new cross-layer scope.
ReviewerUUID01a0e755-9e97-7410-9a99-4130f515176e last107741/258400=41.70%09:41:38.141Z,
quiescent; its earlier actual-code PASS applies only to pre-steering semantics. No deeper agents.
TaskPG/private/tmp/workflow-admission-pg-e3aa stays RUNNING port55439 DBworkflow_admission,
socket/private/tmp max_connections200 log/private/tmp/workflow-admission-postgres.log; ownership
transferred to root. All test/build/prepare/graph processes finished. Preserve every staged and
unstaged change; no commit/reset/deploy. Counted graft estimated savings2716593tokens for this
implementer turn (not measured model-token/cost savings).

Final handoff measurement:116291/258400=45.00%09:45:55.456Z implementer usage/runtime sources.
Reviewer confirmed completed by collaboration status; no further review running. Handoff diff
check PASS (/private/tmp/workflow-a2-diff-handoff.log). Root owns fresh subtree assignment.

## Fixed-channel A2 final checkpoint (2026-09-28)

Root accepted RECONCILIATION-03.1-FIXED-SCHEDULE.md expansion before edits. The user's
fixed-channel contract is now implemented; prior movement-preserving evidence is superseded.
Additive20260928102000 rejects raw schedule channel changes atomically. Applied100000/101000
remain untouched, including frozen occurrence scope/snapshot, tenant FKs and lease fences.
ScheduleUseCases rejects mismatched channel scope before writes; persistence no longer assigns
channel and matches company/channel in the UPDATE predicate. JSON route channel is scope, and
unknown body fields (including channel_id) are rejected. UI edit displays the fixed channel with
hidden scope; creation retains its selector. Other edits/run-as/cadence/pause behavior survives.
No compatibility shim or old business-data upgrade was added. Workflow jobs remain disabled and
channel NOT NULL; B/C/D/admission are still incomplete. No03.2 work.

Changed correction files: migrations/20260928102000_schedule_channel_immutable.sql;
src/application/use_cases/schedule.rs; adapters/persistence/schedule.rs;
adapters/persistence/workflow/job_tenant_association_tests.rs; adapters/http/pages/schedules.rs;
adapters/http/routes/{schedule.rs,ui_schedules.rs,schedule_channel_tests.rs}.
Modified long test/render logic was extracted into focused helpers. Existing A2 provisioning
and association work was independently rechecked with this correction, without reopening the
accepted whole legacy-query audit. The API integration test exercises creation on a selected
channel, mismatched channel/company rejection with full unchanged-row comparison, and legitimate
edit/reread. UI rendering verifies selector only at creation; app/persistence tests reject former
movement. Actual concurrent occurrence capture holds its schedule lock while reassignment waits,
then rejects with unchanged schedule and preserved occurrence. Existing task-thread contention,
raw association matrices, invisible-parent rejection, snapshot checks and lease fences remain.

Independent actual-code PASS and final evidence acknowledgment: nested reviewer
/root/workflow_fixed_schedule/reviewer, UUID01a0e76c-8b9b-72d1-8524-028752d48a08 Astra/medium.
No findings. Review sample89600/258400=34.67%10:00:29.444Z; final evidence sample
92083/258400=35.64%10:02:29.194Z, token_usage_record.usage/task_started.model_context_window.
Reviewer is quiescent; no edits or competing builds. Implementer UUID
01a0e76c-477d-7513-8410-c44e6131c54a Astra/medium, latest111064/258400=42.98%
10:02:26.785Z, same measurement sources. Natural rotation before B, not a threshold stop.
Root owns A2 acceptance and the remaining queue.

Exact logs use /private/tmp/workflow-fixed-a2- prefix:
- migrate.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx migrate run →102000PASS; disposable own_database tests exercise fresh migration chain.
- schedule-final.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib schedule -- --nocapture
  →37PASS/0fail (before final JSON body-rejection test; final full run includes it).
- full.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib -- --nocapture
  →1952PASS/22existingignored/0fail,60.58s.
- check-final.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets →PASS.
- clippy.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings →PASS.
- prepare.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx prepare -- --all-targets →PASS, sequential after full/check/Clippy.
  Cache remains inherited two replacements; no additional .sqlx entry changes.
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check; git diff --check; graft build →PASS.
- schedule.log: initial new API test compile failure (imports/creation-response tuple), corrected;
  superseded by schedule-final/full. check.log is earlier successful preliminary compile.

Task-only PG/private/tmp/workflow-admission-pg-e3aa remains RUNNING port55439,
DBworkflow_admission socket/private/tmp max_connections200 log/private/tmp/workflow-admission-postgres.log;
explicit ownership handback to root. All test/build/prepare/graph processes finished. No other
DB changed and no commit/reset/deploy/staging command issued by implementer. Current index changes
observed during work were preserved. Counted graph-estimate savings at least3161885tokens from
16 retained-count calls, plus two caller/skeleton outputs whose count lines were truncated;
reviewer reported~2997000tokens. These are graph baseline estimates, not measured billing savings.

Final implementer handoff sample112371/258400=43.49%10:03:20.425Z usage/runtime sources.
Reviewer latest observed95229/258400=36.85%10:02:37.035Z token_count/runtime sources;
collaboration status confirms completed. Both retire naturally after this handback.

## Historical handoff provenance B checkpoint (2026-09-28)

A2 fixed-channel root acceptance preserved. B adds insertion-only provenance in additive
20260928103000_handoff_event_task_provenance.sql (now applied/immutable): optional event.task_id
must name a visible company-scoped legacy task. FOR KEY SHARE holds that proof through commit
against deletion/company-key changes; immutable queue identity prevents discriminator changes.
No task FK or live-task history join is added. Taskless events remain valid; historical event
bytes and immutability survive task deletion. There is no old-data upgrade or compatibility shim.
workflow_disabled/channel NOT NULL remain installed; no admission writer, C/D or03.2 work.

Existing independent consumers remain unchanged after actual-code audit: generation opening
(thread_handoff.rs:72), run lookup(:670), completion(:716), failure(:826), resolution(:899),
draft lookup(:1258), exposed expiry(:1295). Their live run owners are already legacy-only through
scoped task/handoff FKs,93000/94000 auxiliary visibility/reassignment guard, and73000 immutable
queue identity. Generation supersession retains those associations; draft/reply reads never
reinterpret a workflow task as legacy. Failure/stop/reap callers (queue.rs:135,304,456 and
operations.rs:1083) UPDATE still-live tasks before failure event insertion, never delete first.
Schedule occurrence/error/claim ownership and fenced provision replay remain accepted A2 behavior.

B files: new migration103000; new workflow/job_handoff_history_tests.rs and
job_handoff_consumer_tests.rs; job_routine_tests.rs module registration; JOB-ISOLATION B
reconciliation and this checkpoint. All pre-existing staged/unstaged/untracked changes preserved.
No production Rust query changed. SQLx regenerated; cache retains only the inherited two replacements.

Six new own_database tests cover missing/real foreign-company/workflow/invisible-parent event
rejection; event writer holding a task key lock while DELETE waits; DELETE-first insertion waiting
then rejecting; retained unchanged history and refused UPDATE/DELETE afterward; workflow run
insertion rejection before direct lookup/completion/failure/expiry effects; legacy complete,
draft visibility, taskless resolution, and independently exposed idempotent expiry. Real lock
waits are observed in pg_stat_activity, not inferred from sleeps. Existing generation/claim/
release/dismiss/terminal-failure/stop/reap/approval flows pass under the final full suite.

Independent actual-code PASS, no findings: reviewer
/root/workflow_handoff_history/review_handoff_history, session
01a0e779-4e60-7442-9af2-1073d983bd42 Astra/medium, measured71110/258400=27.52%
10:18:22.429Z. Review examined migration, consumers/callers, original criteria and six targeted
DB results. Final evidence acknowledgment follows below. Implementer
/root/workflow_handoff_history, session01a0e779-089f-7732-8e46-5d8278a078a0 Astra/medium,
startup21593/258400=8.36%10:04:34.875Z; post-discovery91264=35.32%; post-edits102064=39.50%;
latest113053=43.75%10:20:18.775Z. All measurements from helper token_usage_record.usage /
task_started.model_context_window. Reviewer capacity verified before production edits, edits
stopped throughout review. Natural retirement after B; root owns acceptance and C/D assignment.

Exact commands/logs use /private/tmp/workflow-handoff-b- prefix:
- migrate.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx migrate run →103000PASS. Own database tests also exercise fresh migration chain.
- targeted-final.log: TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
  workflow_job_handoff -- --nocapture →6PASS/0fail1.48s. Final real foreign-company enhancement
  is additionally covered by full.log below; targeted.log initial import compile error corrected.
- full.log: same environment, cargo test --locked --offline --lib -- --nocapture
  →1958PASS/22existingignored/0fail60.58s at stock2MiB, including all six new tests.
- check.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets →PASS19.50s.
- clippy.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
  →PASS28.87s.
- prepare.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx prepare -- --all-targets →PASS16.03s; sequential after full/check/Clippy.
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check; git diff --check; graft build →PASS.
- schema.log: psql inspected event immutability+insertion triggers and preserved workflow_disabled
  and channel NOT NULL. Static/SQLx checks do not validate runtime SQL; real DB tests do.

Task-only PG/private/tmp/workflow-admission-pg-e3aa remains RUNNING port55439,
DBworkflow_admission socket/private/tmp max_connections200 log/private/tmp/workflow-admission-postgres.log.
Explicit ownership handback to root, as accepted; no other database touched. All build/test/
prepare/graph processes finished. No git add/commit/reset/deploy. Graph refresh is local ignored
cache. Counted implementer graft savings3375178tokens across19count lines, reviewer3004756;
these are graph baseline estimates, not measured billing savings.

Final B reviewer evidence acknowledgment PASS: full1958/22ignored and all static/SQLx/fmt/diff/
graft/schema gates inspected; no findings or remaining B verification. Reviewer quiescent for
retirement, final sample72938/258400=28.23%10:20:36.613Z (usage/runtime sources).
Final implementer handoff sample117368/258400=45.42%10:21:25.630Z usage/runtime sources;
reviewer latest observed75225/258400=29.11%10:20:49.562Z token_count/runtime sources.
Both retire naturally now; next worker must create its own nested reviewer.

## C/D nullable and exact job association gate checkpoint (2026-09-28)

C/D implemented; root acceptance pending. This supersedes earlier dormant/NOT NULL status:
104000 adds safety first, independent pre-enable whole-job integration review and staged full
suite passed, then105000 removes workflow_disabled and channel NOT NULL. Both new migrations
are applied and immutable. No admission writer, workflow claimant, execution runtime or03.2 work.

104000 enforces job company→visible scoped execution→visible scoped run, exact NULL-aware
channel/thread equality, and locks run then execution FOR SHARE through commit. Run
company/id/channel/thread and execution company/id/run/step/activation cannot change, including
before jobs exist. Existing scoped FKs, queue identity, ID-only payload, source uniqueness and
shared task_attempts owner remain. Legacy conditional channel CHECK is installed before physical
nullability changes; domain BackgroundTask remains required-channel. Both company_invite macro
projections use one synchronous fallible contextual decoder. Final SQLx metadata reflects nullable
channels; no unwrap, default, invented compatibility path or second ownership table.

Five new own_database tests: all company-only/channel/thread valid jobs and shared attempts;
NULL-aware INSERT/UPDATE mismatch matrix; immutable parent/activation identities; real observed
pg_stat_activity lock contention for run/execution UPDATE versus job INSERT in both orders;
all three workflow associations survive legacy claims, direct controls/reads and expired-lease
recovery byte-for-byte. Existing mixed claim/control/recovery/projection fixtures additionally
contain company-only and channel workflow rows alongside thread workflow and real legacy rows.
Existing competing legacy claimants, tenant/source/auxiliary/historical guards, notifications,
deletions and projection-index evidence rerun in the full suite. Run/history/projection fixtures
now establish valid association at INSERT rather than moving immutable workflow parents.

C/D files: migrations104000/105000; company_invite.rs; new workflow/job_association_tests.rs;
workflow/{job_schema_tests,run_schema_tests,job_core_query_tests,job_projection_tests,
job_projection_index_tests,job_routine_tests}.rs; SQLx cache. Two stale comments corrected in
job_external_query_tests and job_handoff_consumer_tests after gate removal. Existing staged,
unstaged and untracked A/B/workflow work preserved; no staging/commit/reset/deploy.

Independent actual-code pre-enable and final gate-delta PASS, no findings:
/root/workflow_job_gate/review_job_gate, UUID01a0e789-a0ce-7e42-8dd5-353622f6b3a3,
Astra/medium. Pre-enable28.63% at10:32:07Z; delta33.07% at10:36:39Z; final evidence
acknowledgment below. Edits frozen for both reviews. Original03/EXPANSION/shared03.1 and
accepted exhaustive A/B audits reused; review inspected current association protocol, nullable
boundaries, mixed consumer integration and actual tests. No whole03.1 acceptance inferred.

Exact evidence, prefix /private/tmp/workflow-job-gate-:
- migrate.log / migrate-final.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx migrate run →104000PASS24.15ms,105000PASS5.46ms. Own-database full-suite fixtures
  exercise the fresh migration chain. final-catalog.log confirms legacy channel CHECK, nullable
  channel, all3new association/identity triggers and absent workflow_disabled.
- targeted.log: TEST_DATABASE_URL=same task URL SQLX_OFFLINE=true RUST_MIN_STACK=2097152
  cargo test --locked --offline --lib workflow_job -- --nocapture →44PASS/0fail9.02s.
  This initial run predates fifth test/mixed nullable extras, superseded by both full runs.
- full-staged.log: same environment, cargo test --locked --offline --lib -- --nocapture
  →1963PASS/22existingignored/0fail60.61s before gate removal.
- full-final.log: same full command/environment →1963PASS/22existingignored/0fail60.59s
  on final nullable schema. All5new tests and full mixed legacy behavior PASS at stock2MiB.
- prepare-staged.log: DATABASE_URL=same task URL SQLX_OFFLINE=false
  cargo sqlx prepare -- --all-targets →PASS41.64s. prepare-final.log initially exposed second
  macro nullable decode mismatch (delegated asks). Corrected via shared fallible decoder;
  prepare-final-corrected.log same command →PASS19.72s. Final metadata nullable=true at both fields.
- check.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets →PASS0.96s.
- clippy.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
  →PASS47.14s. No stacks/limits raised.
- fmt.log: cargo fmt --all -- --check; diff.log: git diff --check; graft.log: graft build →PASS.

Implementer /root/workflow_job_gate UUID01a0e789-5541-7850-bcc4-7615dfb4c686 Astra/medium:
startup21577/258400=8.35%; discovery74513=28.84%; staged98430=38.09%; finalSQLx109003=42.18%;
static112295=43.46%; final suite114573=44.34% at10:39:06.234Z. Measurements use
context_usage.py depth0 token_usage_record.usage / task_started.model_context_window.
Nested reviewer capacity verified before edits. Natural retirement before admission work.
Counted graft savings estimate at least2584099tokens (some earlier truncated output excluded).

TaskPG /private/tmp/workflow-admission-pg-e3aa remains RUNNING127.0.0.1:55439, database
workflow_admission, socket/private/tmp, max_connections200; log/private/tmp/workflow-admission-postgres.log.
Ownership handed back to root after completion; no production database touched. Build/test
processes complete. Root owns PROGRESS/RESUME acceptance and next admission/snapshot assignment.

Final evidence acknowledged: same reviewer PASS/no findings after reading final1963PASS/22ignored,
all5new cases, Clippy/offline check/SQLx, catalog and checkpoint. Reviewer quiescent88780/258400=
34.36% at10:40:20.344Z; implementer final checkpoint117158/258400=45.34% at10:40:15.312Z.
PG55439 running ownership explicitly handed back to root. No remaining C/D verification gap;
root accepts before admission work. Subtree naturally retires; no further substantive assignment.
