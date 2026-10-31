# Atomic admission E/F checkpoints

2026-09-28, continuation after accepted C/D (JOB-SLICE-03.1.md:953).
Root accepted this bounded breakdown before production edits. Preserve all existing
staged/unstaged/untracked work; no staging, commits, reset, deployment or03.2 runtime.
Implementer /root/workflow_atomic_admission session01a0e79a-bb4a-7021-8fca-2d7fe8663c3e
Astra/medium startup25315/258400=9.80%10:41:28Z. Own nested reviewer
/root/workflow_atomic_admission/reviewer session01a0e79b-20e6-76c3-8dfd-803cfc6fe965
Astra/medium ready37977/258400=14.70%10:41:57Z; capacity verified before edits.
Measurements use usage/runtime sources. Root owns PROGRESS/RESUME acceptance.

E: production WorkflowAdmission owns one short company-serialized transaction. Reuse
company/member/principal and association locks; check actual source owners, exact current
active selection for new runs, saved bundle/resource authority for replay. Command-key
identity includes trigger UUID while canonical source identity permits fresh-key redelivery.
Persist exact bundle/input/params/resources/revision/limits and one fixed24h deadline,
first execution, ID-only background task, bounded committed history, source/command aliases
and final audit. No attempt owner or runtime transition introduced. Binding association
may narrow Company→Channel→Thread, never widen a configured Channel/Thread. Child action
source fails closed until phase04 owns it. History capture must be correct in E already.
E acceptance: DB snapshot roundtrip; same/different-key competing transactions; stale config,
changed-input/trigger/association conflicts; binding changes/removal/archive replay; first-job
shape; final/deferred rollback; correct committed membership and bounded overflow. Independent
E review before F. Deadline is internal named constant, never reset on replay; existing
bundle limits still govern steps/context. Company lock ends with admission commit and is
never retained for workflow execution.

F: adversarial coverage for actor/channel/source/saved-resource revocation under observed
locks, missing/foreign/inconsistent message/occurrence/parent sources and child-action failure;
older-created uncommitted membership committing after capture stays excluded, later admissions
see it; independent message runs. Whole03.1 actual-code integration review and full DB stock2MiB,
locked offline check/Clippy, SQLx prepare, migrations, fmt/diff, graph refresh. Reuse accepted
schema/job evidence when assumptions unchanged; no whole03.1 claim before combined gate.

Source pointers: workflow/contracts.rs:80–184; service.rs:44–74;
persistence/workflow/admission_binding.rs:44–193; authority.rs:6–52;
association.rs:6–61; binding_rows.rs:20–96; resources.rs:11–72.
Applied migrations through20260928105000 immutable. PG ownership: running task-only
/private/tmp/workflow-admission-pg-e3aa port55439 DBworkflow_admission, max_connections200.

## E implemented and reviewed; final broad gate pending at worker rotation

Production files: admission.rs (company-serialized owner/CAS/narrowing/24h constant),
admission_replay.rs (command identity, canonical aliases, exact saved re-restore and
resource authority), admission_source.rs (message membership/schedule occurrence/parent
execution locks and Action fail-closed), admission_write.rs (snapshot/run/execution/job/
alias/final audit), admission_history.rs (one MVCC statement, deterministic pinned
membership,10001-row overflow detection). Existing admission_binding.read_saved_binding
and bindings.verify_selection only became pub(super); mod/tests registrations added.
No new migration and no modifications to applied migrations. Binding scope narrowing is
an explicit root-accepted inference: Company accepts authorized related scopes, Channel
accepts itself/its threads, Thread accepts only itself. Replay compares saved input and
association before alias writes; configuration/head changes do not change equivalence.

Nine real isolated-DB tests in admission_tests.rs/admission_history_tests.rs cover:
exact bundle/limits/params/input/resources/revision/actor/trigger/correlation/job payload,
fixed deadline replay; actual competing same-key and fresh different-key same-source
admissions; stale selection and changed input; replay after head update/removal/archive;
message redelivery with different trigger IDs, changed-trigger same-command conflicts,
independent messages; populated-history final-write AND deferred-commit rollback;
older-created membership committed after observed audit barrier excluded while next
admission sees it;10001-membership overflow rollback; command/alias changed-association
conflicts; actual DB5x5 scope narrowing matrix with11valid admissions.

Independent reviewer actual-code correction PASS (latest completed sample100448/258400,
38.87%10:55:19Z, usage/runtime). Initial review found two test gaps: late fixture canonical
message INSERT held company FK lock, so it could not reach capture; and missing association
matrix. Corrected by precommitting canonical content and holding only membership, plus
matrix/replay conflict tests. Corrected9test run PASS. Three subsequent Clippy-only test
idioms removed (.into and explicit derefs), independently re-inspected PASS, no behavior
change. Reviewer idle/quiescent; final observed102695/258400=39.74%10:57:03Z.

Evidence (all /private/tmp/workflow-admit-e- prefix):
- tests-corrected.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
  workflow_admit_sql →9PASS (2.12s); tests.log preserves earlier6PASS1fixtureFAIL.
- clippy-corrected.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets
  -- -D warnings →PASS; clippy.log preserves the3corrected test idiom warnings.
- prepare-final.log: DATABASE_URL=same cargo sqlx prepare -- --all-targets →PASS.
- check-final.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets →PASS.
- migrate.log: DATABASE_URL=same cargo sqlx migrate run →PASS (no new migrations).
- full.log: initial full command omitted TEST_DATABASE_URL;351tests failed because auto-
  derived workflow_admission_test does not exist,1621PASS22ignored. This is unverified
  full-suite evidence, not an application regression or a pass.
- Corrected full command RUNNING at rotation in unified exec session13932:
  DATABASE_URL=same TEST_DATABASE_URL=same SQLX_OFFLINE=true RUST_MIN_STACK=2097152
  cargo test --locked --offline --lib >full-corrected.log. Same task-only database is
  intentionally explicit; no development/business DB touched. Upon success the existing
  set-e batch runs fmt --check→fmt.log, git diff --check→diff.log, graft build→graft.log.
  Root must await this process and inspect results; no final broad-gate claim yet.

Remaining F: actual actor/channel/source/saved-resource revocation with observed lock
contention; missing/foreign/inconsistent message/schedule/parent source matrices; explicit
child-action unsupported regression; deactivation replay and saved-resource changes;
whole03.1 actual-code/evidence integration review. E production code reviewed, but root
acceptance awaits final broad gate evidence. No03.2 work. Tests are nested under
admission_tests so F can reuse its isolated AdmissionFixture/Capture/NoRuntime privately.

Worker final substantive sample128239/258400=49.63%11:00:34Z (usage/runtime). Rotating
conservatively now; only handoff saving after this sample. No additional edits/tasks.
Reviewer quiesced. Explicit PG handback to root: RUNNING /private/tmp/workflow-admission-pg-e3aa,
port55439 DBworkflow_admission, socket/private/tmp max_connections200; active verification
batch13932 also transferred. No commit/stage/reset/deploy. Counted graft savings≈2752643
from22reported estimate lines (not measured billing savings).

## E accepted; F authority continuation

Root accepted E on2026-09-28 after final independent evidence acknowledgment:
full-corrected.log1972PASS0FAIL22ignored60.76s at stock2MiB; fmt/diff clean
(independently rerun); graft.log successfully wired621cards. Final check/Clippy/prepare
logs completed. Original batch exit no longer available after session disappearance;
terminal success plus downstream graft supports recorded set-e batch completion.
New implementer01a0e7ae-49eb-7c82-8178-6a35bebc362e Astra/medium; nested reviewer
01a0e7ae-ae7b-7673-891f-af13d150a7a1 Astra/medium established before edits.
Reviewer E acknowledgment33269/258400=12.88%11:03:26Z; implementer discovery
77920/258400=30.15%11:05:19Z (usage/runtime sources). F in progress.

F implemented: admission_authority_tests.rs and admission_source_tests.rs add9 real
isolated-DB tests (18combined admission tests). Actor membership, exact channel grant,
and saved MCP grants each revoke under an observed blocker for new admission, original
command replay and fresh alias; canonical message membership deletion blocks new
admission; occurrence deletion blocks all3delivery modes. Every denial asserts the
entire run/execution/job/admission/command/history/audit count projection unchanged.
Saved-resource changes prove replay uses the admitted resource after head replacement
and deactivation, permitting replay while only the replacement is revoked and rejecting
when the original is revoked. MCP tests use the established company-owner lock protocol.
Message sources reject missing/real foreign/wrong-thread/no-thread/wrong-channel rows;
schedule sources reject missing schedule/occurrence, mismatched owner, foreign owner or
occurrence and inconsistent association. Parent sources reject missing run/execution,
wrong run/step and actual foreign-company rows; a valid exact parent succeeds and child
Action fails closed. Only test fixture seams changed (with_binding factory, mcp/helper
visibility); no F production code or schema changes.

Fresh independent F+whole03.1 actual-code integration review found no substantive finding
beyond2private-module test imports, corrected to public workflow reexports. Reviewed
source/company/association locks, saved restore/resource authority, exact selection,
snapshot/job/history/alias/audit atomicity and new fixture/concurrency assertions against
original03 admission and BRIEF remaining criteria. Unaffected accepted A–D/schema/job and
E codec/race/rollback/history evidence reused. Reviewer latest actual-code final sample
77873/258400=30.14%11:11:17Z (usage/runtime). Corrected combined test log
/private/tmp/workflow-admit-f-tests-corrected.log: TEST_DATABASE_URL and DATABASE_URL both
postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true
RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_admit_sql
→18PASS0FAIL5.51s. Initial f-authority.log/f-tests.log preserve corrected fixture import/
request-clone compile errors; no failed production assertion. Final broad gate batch43466 completed EXIT0; final evidence and independent whole03.1
acknowledgment are recorded below.

## Final F and whole03.1 verification — ready for root acceptance

Independent reviewer whole03.1 integration PASS, no outstanding selected-scope code or
verification findings. Public import correction inspected; final logs independently read.
Reviewer01a0e7ae-ae7b-7673-891f-af13d150a7a1 fresh79700/258400=30.84%
11:16:23.984Z (usage/runtime), quiescent. Implementer latest99060/258400=38.34%
11:16:29.960Z (usage/runtime); no production edits since E and no source edits after
corrected18test pass. Root owns final queue acceptance.

All final logs have prefix/private/tmp/workflow-admit-f-:
- migrate.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx migrate run →PASS (empty; migrations through105000 already applied).
- clippy.log: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets
  -- -D warnings →PASS50.80s.
- prepare.log: DATABASE_URL=same cargo sqlx prepare -- --all-targets →PASS40.47s.
- check.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targets →PASS1.04s.
- full.log: TEST_DATABASE_URL=same DATABASE_URL=same SQLX_OFFLINE=true
  RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
  →1981PASS0FAIL22ignored60.56s. Explicit task-only DB, no skipped DB checks.
- fmt.log/diff.log: cargo fmt --all -- --check; git diff --check →PASSempty.
- graft.log: graft build →PASS623cards. Final sequential batch43466 EXIT0.

| Original/remaining criterion | Final coverage |
| --- | --- |
| Scoped durable identity | Accepted normalized schema and A–D composite FK, nullable exact job association, immutable fixed schedule, historical provenance and adversarial schema tests; all repeated in final full DB suite. Applied migrations unchanged. |
| Exact atomic admission snapshot | E snapshot roundtrip includes bundle/input/params/resources/binding revision/limits/fixed deadline/actor/trigger; first execution+ID-only first job; populated-history final and deferred-commit rollback; E/F18targeted and1981full pass. |
| Source identity vs command replay | E same-key and different-key real competitors, canonical source redelivery, input/trigger/association conflicts, saved replay after edit/removal/archive; F deactivation and saved-resource replacement; one run/job with canonical aliases. |
| Current authorization | F observed actor/member, channel grant, MCP resource revocation for new/replay/alias; all deny without changing full admission record counts. Read preparation remains non-authoritative; saved resources rechecked at commit. |
| Source authority | F missing/real foreign/inconsistent message, schedule occurrence and parent run/execution/step matrices, successful valid controls; observed membership and occurrence revocation; child action unsupported. |
| Committed history boundary/independent messages | E real older-created uncommitted membership stays absent after capture, later admission sees it; deterministic scoped pinned membership and10001overflow rollback; independent message runs/jobs and source aliases. |
| Sole job/attempt ownership | Accepted C/D nullable gate and exhaustive legacy exclusion inventory/tests; E uses existing background_tasks ID-only payload, no second queue/attempt owner; final full suite reruns isolation. |
| Combined acceptance | Independent actual-code whole03.1 PASS plus final migrations/SQLx/offlinecheck/Clippy/stock2MiBfullDB/fmt/diff/graft gates above. No03.2 execution/runtime claims. |

Resources: stopped task PostgreSQL with pg_ctl -D/private/tmp/workflow-admission-pg-e3aa
stop -m fast -w after all tests; status confirmed no server running. Data directory retained
per root instruction. No active build/test processes; reviewer quiescent. No stage/commit/
reset/deploy or unrelated cleanup. Counted graft savings≈2674586tokens over21reported
estimate lines (graph tool estimate, not measured billing savings). No remaining03.1 gaps
identified; root final acceptance/PROGRESS/RESUME remains the only coordination step.
