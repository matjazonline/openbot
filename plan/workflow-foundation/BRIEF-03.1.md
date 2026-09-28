# 03.1 — Durable admission and independent contexts

Status: expansion reconciled; codec/actor, definition SQL and production binding lifecycle
fragments implemented and independently reviewed; admission/history/job SQL remains pending.
Whole03.1 is not accepted.
Accepted phase02 library work remains
uncommitted and must be preserved. Original03 and EXPANSION are authoritative; root owns queue
acceptance/PROGRESS/RESUME. No deployment, commit, or reset is authorized.

## Architecture reconciliation (2026-09-27)

PostgresPersistence will implement the existing draft-copy, definition, binding, admission and
inspection ports under persistence/workflow. Add normalized workflow/version, binding/revision,
run, execution, wait and audit records with scoped FKs. Reuse background_tasks/task_attempts as
the sole job/attempt owner. Workflow jobs reference executions and carry only IDs; existing
claim/recovery paths must exclude their discriminator until the new runtime owns them. Existing
background_tasks.channel_id is NOT NULL, so company-associated runs require carefully audited
nullable channel support and stronger conditional shape constraints; inspect all callers before
changing it. Existing source uniqueness on jobs cannot deduplicate workflow logical bindings.

Lifecycle transactions must close current actor management/channel visibility revocation races,
draft/archive and binding lifecycle CAS, immutable publication replay, and configuration/readiness
changes. PreparedAdmission currently drops actor; retain it for transaction reauthorization.
Keep both company command-key conflict equivalence and binding+canonical-source admission
uniqueness; source aliases must select saved configuration even when caller command keys differ.
No dedup by thread/content. Publication replay and admission replay reauthorize current access.

Capture a bounded committed thread-message membership snapshot within admission. Timestamp or
allocated sequence cutoff alone admits a transaction that commits later with an older timestamp/
sequence. A membership snapshot avoids changing all legacy message writers or taking a
conversation execution lock. Later context.load consumes only these scoped pinned IDs.

Published bundles currently lack a restore codec. Persist exact source, compiled representation,
manifest/hash, dependency facts and child closure in a versioned bounded form. Restoration must
validate identity and fail on compiler/default drift, never silently substitute new compilation.
Use the application's SourceDecoder port; no application-to-adapter dependency.

## Authoritative resource capability matrix

- Resource-free versions: all lifecycle/admission atomic guarantees must work now.
- Existing MCP connection UUID owner: company_mcp_connections and tool grants are real authorities;
  use current scoped enabled/deleted/grant facts under locks, never cached ResourceStatus grants.
- Model connections: existing owner keys by company/provider, not RuntimeResourceId; mapping and
  phase06 agent execution integration need explicit resolution, not invented generic grant rows.
- HTTP connections, frozen tool policies/profiles/skills: no matching durable authority currently
  exists. Fail closed where unsupported. Phase04/06 must supply their actual owners and adapters.
- No generic workflow resource authority table may duplicate existing providers. Production
  activation is claimed only for supported authoritative paths, not every library fixture kind.

Root accepted this architecture direction and capability limitation; unavailable kinds stay
explicitly deferred to their owners. This is not permission to omit SQL/race guarantees for
supported paths or to mark the whole03.1 point verified prematurely.

## Acceptance and remaining gates

Real PostgreSQL competing edits/publications/archive/activation/admissions and authority revocation;
failed final statement rolls back all records; same source rejoins after binding changes; two
messages in one conversation admit independently; history excludes later commits; snapshot
roundtrip/corruption/unknown format tests; cross-company FK rejection. Run isolated migration,
SQLx prepare, locked offline all-target check/Clippy, fmt/diff, workflow tests at2MiB and required
DB-backed/stock-stack gates; refresh graft. Nested independent Astra/medium review inspects actual
code and integration while implementer stops editing. Later03 execution/04 effects/05 decisions/
07 child semantics remain their owning points; do not claim their runtime acceptance now.

## Worker/resource record

Initial implementer /root/implement_persistence, verified session
01a0e496-1723-7322-9279-28df79d83881, Astra/medium. Startup23,901/258,400=9.25%
at20:37:27Z; discovery86,847=33.61% at20:39:19Z, token_usage_record.usage /
task_started.model_context_window. No database/service created. No SQL edits yet.
Discovery source pointers: workflow/contracts.rs:80, ports.rs:17, lifecycle/ports.rs:7;
publication/mod.rs:24, compiler/compile.rs:47; participant.rs:526; domain/entities/channel.rs:250;
baseline migration tables background_tasks:1954, task_attempts:3216, thread_messages:3569,
MCP connection:2180, model connection:2230. Existing task unique source constraints at3757–3776.

## Initial partial implementation and review

Changed publication/storage.rs + storage_tests.rs: bounded4MiB JSON envelope, flat deduplicated
closure up to256 versions, exact source/compiled representation/manifest/content hash restoration,
children restored bottom-up, reject malformed/foreign/cyclic/missing/unreachable versions. Frozen
facts gain Deserialize, but domain names and AgentKey deserialize through checked parsing.
Application uses SourceDecoder, never an adapter import. PreparedAdmission now retains actor for
future transaction reauthorization. Existing APIs and accepted uncommitted work remain intact.

Independent reviewer /root/implement_persistence/review_codec, UUID
01a0e49b-ee82-7f02-bc14-7f3d02d6ec39, Astra/medium, actual-code fragment review. One P2:
representation/hash comparison alone did not pin validator/default semantics absent from JSON.
Corrected compiler::SEMANTIC_REVISION in required envelope field, checked before rebuilding;
documented bump discipline for schema options/defaults/context budgets/registry/interpreter
semantics, added MustNotDecode missing/mismatch regression. Same reviewer correction PASS
at20:47:06Z,70,391/258,400=27.24% (usage/task_started sources). No edits/delegation.
This is fragment-only review, not SQL or complete03.1 acceptance. Whole-point review remains due.

Original3focused tests at2MiB PASS (/private/tmp/workflow-03-1-codec-tests.log). Initial check
ran before test file creation and failed missing file; corrected. Later pre-correction alltarget
check passed, superseded by corrected gates. Current exact commands/logs:
- SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow
  → /private/tmp/workflow-03-1-codec-tests-corrected.log
- SQLX_OFFLINE=true cargo check --locked --offline --all-targets
  → /private/tmp/workflow-03-1-codec-check-corrected.log
- SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
  → /private/tmp/workflow-03-1-codec-clippy-corrected.log
- cargo fmt --all -- --check; git diff --check: PASS,
  /private/tmp/workflow-03-1-codec-{fmt,diff}-corrected.log
- graft build → /private/tmp/workflow-03-1-codec-graft.log

Remaining03.1: all schema/lifecycle/CAS/admission/source-alias/history/job integration described
above, actual authoritative resource capability implementation/matrix, real DB races and rollback,
migrations/SQLx/full DB and2MiB gates, independent combined review. No durable guarantee is
claimed by this partial codec fragment. No task databases/services/cleanup obligations.

## Partial handoff checkpoint

Corrected current-tree gates PASS:167workflow tests at2MiB, locked offline alltarget check,
Clippy-Dwarnings, fmt/diff and graft (logs above). All build processes finished. No required
fragment checks outstanding, independent fragment review PASS after one correction. Full03.1
still IMPLEMENTING, not accepted; next worker starts normalized SQL ownership and real
transactions using the architecture packet above. Do not rerun accepted phase02 discovery or
mistake this codec checkpoint for durable admission. No database was created/modified.

Natural implementer rotation planned below50% before substantially larger SQL work. Latest
44.84%,115,865/258,400 at20:49:34Z; reviewer parent-confirmed71,065=27.50% at20:47:14Z
(token_count.info fields). Reviewer completed; retire subtree. No pending edits/tests/processes.

## Definition SQL fragment (second worker)

Implementer /root/implement_sql, verified UUID01a0e4a2-53a5-7e12-9c4f-e9fbc04638ac,
Astra/medium. Startup26,148/258,400=10.12%20:50:48Z; correction boundary113,522=43.93%
21:08:51Z; latest117,573=45.50%21:11:06Z. Sources token_usage_record.usage /
task_started.model_context_window. Preserve all earlier uncommitted work.

Added migration20260927210000_workflow_definitions.sql (do not edit after this checkpoint),
persistence/workflow/{mod,authority,rows,definitions,tests}.rs and module registration;
application templates/contracts.rs gains checked TemplateOrigin::restore.
PostgresPersistence now implements WorkflowDraftCopies and WorkflowDefinitions. Scoped normalized
definitions/versions/definition-events, draft copy/save/archive CAS, immutable publication command
replay, exact stored codec restoration, preserved template provenance, selectable child version
hash checks. Actual company/member/principal locks authorize every mutation and replay. Missing
agent/skill/profile/tool-policy authority fails closed; no duplicate authority store was invented.
PublicationDirectory production capture and binding ResourceDirectory still need owning adapters.

Nested reviewer /root/implement_sql/review_lifecycle, verifiedUUID
01a0e4a7-132f-71d3-a17a-dd7646421ca0, Astra/medium, no edits/delegation. Found missing exact
source/bundle equality and incomplete fragment DB tests. Corrected source equality, corruption
lookup/selectable tests, observed PostgreSQL lock contention for publish/archive and revocation,
final-audit publication rollback, cross-company FK, copied provenance, revoked replay. Correction
review PASS21:10:39Z,76,894/258,400=29.76%; parent sample79,416=30.73%21:10:50Z.
This review is fragment-only; whole03.1 independent integration review remains mandatory.

Evidence logs prefix /private/tmp/workflow-03-1-:
- lifecycle-check-corrected.log: locked offline all-target check PASS.
- lifecycle-prepare.log: SQLx prepare against task schema PASS; .sqlx unchanged (new SQL is runtime SQL).
- lifecycle-clippy.log: locked offline all-target Clippy-Dwarnings PASS.
- lifecycle-fmt.log / lifecycle-diff.log: fmt-check / diff-check PASS.
- lifecycle-graft.log: graft build PASS.
- migrate.log: all4migrations PASS on isolated schema (new migration8.5ms).
- lifecycle-tests-unrestricted.log: original2SQL tests PASS2MiB; sandbox attempt failed networking,
  correctly retried with escalation. Superseded by expanded tests.
- lifecycle-workflow-corrected.log:171PASS/1test-fixture failure; template catalogue omitted its
  required dependency root. Fixed root1->empty map; this failure is not a production-code issue.
- lifecycle-stock-full.log: full DB-backed stock-stack gate launched with corrected fixture;
  final result/resource cleanup recorded below when finished.

Local PostgreSQL5432 was unavailable. Task-only UTF8 cluster
/private/tmp/workflow-03-1-pg-53a5 on127.0.0.1:55439; log workflow-03-1-postgres.log;
schema DB workflow_03_1_schema, shared-test DB workflow_03_1_test. Own-database test fixtures
create/drop only their generated databases. Cluster cleanup is this worker's responsibility.
No existing database was changed, no commit/reset/deploy.

Remaining03.1 is substantial: binding/revision/run/execution/wait/audit normalized records;
binding lifecycle/readiness and admission transactions; canonical source replay independent of
command key, history membership, source-authority checks; job integration and exclusion audit;
actual MCP supported authority path tests; full original races/rollback and whole-point review.
Do not treat these partial checks as point acceptance or advance03.2.

Immediate seam: WorkflowBindings::admission_binding(company,binding,key) cannot select saved
configuration for canonical-source aliases with new keys; add source/trigger selection input in
port/service/mocks before SQL implementation. Legacy background_tasks.channel_id is non-null;
explicit discriminator filters and nullable DB decoding need audited reads/claims/controls/reap,
not merely claim filtering. Root agreed explicit queries over a new legacy view; sole physical
background_tasks/task_attempts owners remain. Full exhaustive job references saved in
/private/tmp/workflow-03-1-job-references.txt. No legacy job/schema changes in this fragment.

### Final second-worker checkpoint

Full stock-stack/database gate completed:1896passed,1failed,22ignored; all5new SQL tests PASS
at2MiB including corrected provenance fixture. No stack overflow. Broad gate remains FAILED:
application::transport::dependency_tests::the_application_layer_imports_no_adapter_framework_or_provider_type
detects application test imports of adapters in workflow/compiler/compile_test_support.rs:2,
workflow/tests/snapshot_cases.rs:16, workflow/tests/authorization_cases/resources.rs:12. These
were already in the inherited uncommitted workflow work, but are not waived: next worker must
correct dependency direction/test fixture boundaries and rerun the broad gate. See
/private/tmp/workflow-03-1-lifecycle-stock-full.log. Do not describe full03.1 or all gates as passed.

Final substantive-boundary measurement126,801/258,400=49.07%21:14:32Z (usage/runtime sources).
Stopped substantive work below50%; reviewer idle/completed. All build/test processes finished.
Task PostgreSQL stopped successfully and /private/tmp/workflow-03-1-pg-53a5 removed, including
only task-owned databases. Logs retained. No database/service cleanup obligation remains.

## Third-worker partial checkpoint — UNREVIEWED

Implementer /root/implement_binding_sql, verified UUID01a0e4bd-bc41-7982-977f-2d0cbd9945ab,
Astra/medium. Startup27,228/258,400=10.54%21:20:48Z; checkpoint105,521=40.84%21:30:30Z,
token_usage_record.usage / task_started.model_context_window. Stopping because nested reviewer
spawn failed `agent thread limit reached`; no reviewer handle was created, no self-review
substitution, no deeper delegation. Root instructed save/cleanup/stop. This entire third-worker
fragment is implemented-but-UNREVIEWED;03.1 stays IMPLEMENTING and03.2 must not start.

Changed fragments:
- Test dependency boundary: cfg(test) src/test_support.rs is a crate-level composition root
  reexporting workflow source decoding. Registered in src/lib.rs; compiler/compile_test_support.rs,
  tests/snapshot_cases.rs and tests/authorization_cases/resources.rs use that outer test wiring.
  Dependency guard unchanged; no new exception or test filename rename. Applicable rule:
  src/AGENTS.md permits real adapters in test fixtures while production application code consumes
  ports; composition is outside application and absent from production. Reviewer must explicitly
  assess this boundary. No SourceDecoder production port change.
- WorkflowBindings::admission_binding now takes &TriggerRef; service passes it. Port contract
  documents canonical-source selection for new command aliases. MemoryStore selects old binding
  by company/logical binding/source before current head and atomically retains alias command
  equivalence separately from original run causality. Exact command-key changed-trigger remains
  conflict; new key+same canonical source/input rejoins old run, changed input conflicts. Manual
  events use stable trigger ID; message source ID, schedule occurrence and immediate child cause
  identify other events. Previous causality fixture incorrectly expected a second same-message
  run for a changed key/input; corrected to conflict. New source alias regression proves original
  config survives reconfiguration/removal with exactly one run/job. These are application/mock
  tests, NOT durable SQL admission acceptance. SQL owner must implement this contract.
- New migration20260927213000_workflow_bindings.sql adds scoped heads/revisions/audit events.
  Logical head state revision/activity is separate from immutable configuration revision.
  Deferred selected-revision FK permits initial atomic head+revision creation but rejects dangling
  heads at commit; company/channel/thread and version ownership use scoped FKs. No generic
  resource authority or second job owner. New persistence/workflow/binding_schema_tests.rs
  proves missing revision rollback, cross-company version rejection and nonexistent revision
  update rejection. Migration applied to task databases; it is now immutable. Production
  BindingLifecycle SQL/readiness/association authorization is still NOT implemented.

Evidence (logs under /private/tmp):
- workflow-03-1-boundary-source-stock-full.log: full DB-backed library suite at2MiB,
  1898passed/0failed/22ignored. Includes dependency guard fix/source alias change; predates new
  binding migration/test. Command TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_03_1_test
  DATABASE_URL=same SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib.
- workflow-03-1-binding-schema-workflow.log: post-schema174passed/0failed, including6real SQL
  tests, same env and cargo test filter workflow. No DB tests skipped.
- workflow-03-1-binding-schema-migrate.log: new migration applied14.793ms; all5migrations
  also exercised by isolated own_database SQL test fixtures.
- workflow-03-1-boundary-source-clippy.log: pre-schema locked offline all-target ClippyPASS.
- workflow-03-1-binding-schema-{fmt,diff,graft}.log: post-schema formatting/diff/graphPASS.
- Post-schema check/Clippy/SQLx evidence and resource cleanup finalized below.

Remaining: independent review of this fragment and eventual combined03.1integration; production
binding lifecycle/resource readiness including real MCP locks/grants; normalized run/execution/wait/
admission/source-alias/history schemas and SQL; sole existing background_tasks/task_attempts job
integration with explicit legacy query exclusion/fallible nullable decoding; admission/source
authority/races/rollback/history membership tests; final combined full DB/stock-stack gate after
all03.1changes. Schema-only additions do not prove binding lifecycle atomicity or production
readiness, and current broad suite result does not cover the later binding schema addition.

Task resources: own UTF8 cluster /private/tmp/workflow-03-1-pg-bc41 on127.0.0.1:55439,
test DB workflow_03_1_test; log workflow-03-1-bc41-postgres.log. Initialization initially failed
sandbox shared memory then retried correctly with escalation. No existing DB/reset/commit/deploy.
Cleanup state is recorded below before worker stops.

### Final third-worker verification/cleanup

Post-schema locked offline all-target checkPASS (workflow-03-1-binding-schema-check.log).
SQLx prepare -- --all-targetsPASS against migrated task schema
(workflow-03-1-binding-schema-prepare.log); .sqlx unchanged. Initial post-schema Clippy overlapped
SQLx prepare cache regeneration and failed missing cached query data; this was verification
sequencing error. After prepare completed, sequential corrected locked offline all-target
Clippy-DwarningsPASS54.74s (workflow-03-1-binding-schema-clippy-corrected.log).
Final git diff --checkPASS (workflow-03-1-binding-schema-diff-final.log).
No compile/test/prepare processes remain. Task cluster successfully stopped and task-only
/private/tmp/workflow-03-1-pg-bc41 removed; logs retained. No cleanup obligations remain.
Latest context110,919/258,400=42.93%21:32:53Z, usage/runtime sources. Worker stopped at root
instruction due reviewer capacity; all third-fragment independent review remains pending.

## Third-fragment independent review (fourth worker)

Implementer /root/resume_admission, verified UUID01a0e4ce-17da-79b2-a7f1-a5a01a95aa3b,
Astra/medium; startup24,334/258,400=9.42%21:38:38Z. Nested reviewer
/root/resume_admission/review_fragment, UUID01a0e4ce-6fab-7f71-b19f-a1fa13a8b502,
Astra/medium, actual-code fragment PASS with no findings. Reviewed test-only composition root
and unchanged dependency guard, TriggerRef canonical-source selection/alias equivalence/original
causality and actual service callers, scoped binding schema and baseline FK targets. Reviewer
independently ran SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
application::workflow::tests -- --nocapture:29PASS, exit0. Inspected prior174workflow/schema,
alltarget/Clippy/SQLx logs. Fresh reviewer78,168/258,400=30.25%21:41:29Z, usage/runtime sources.
No edits or DB resources from reviewer. This resolves third-fragment UNREVIEWED status only;
whole03.1 and later full integration/DB/stock-stack gates remain pending, root owns acceptance.

Narrow next seams discovered: lifecycle/ports.rs:49–65 BindingLifecycle; lifecycle/bindings.rs:
6–162 service semantics; binding/resources.rs:44–81 readiness validation; workflow/authority.rs:
6–52 existing company-first transaction authority/fail-closed publication. Existing MCP update,
delete and grant writers at persistence/mcp.rs:94–129,221–276 take company lock first. Channel
visibility domain rule at domain/entities/channel.rs:250–265: owner bypass; team/public visible;
allowlist requires principal view grant. LifecycleAuthorizer at workflow/authorization.rs:146–188
also requires management membership and scoped thread/channel association. Production SQL must
lock these owners and preserve identical rules, including revoked-access deactivation semantics.

Additional next-slice details (read-only discovery, no implementation): binding schema has
workflow_bindings state_revision/configuration_revision/active/channel_id/thread_id;
workflow_binding_revisions stores version_id/params/resources/actor_id; events keyed by lifecycle
revision. Existing workflow/rows.rs:82–139 VersionRow and VERSION_COLUMNS restore the bounded
bundle. workflow/definitions.rs:110–129 selectable lookup; next transaction must additionally lock
archival owner and compare exact stored content. lifecycle/contracts.rs:152–166 exposes
PreparedBinding actor/expected/state; binding/mod.rs:35–78 validates/reconstructs ConfiguredBinding.
Use service-produced state but recheck all authorization/readiness/CAS under transaction; no new
public preparation constructor needed. Existing workflow/tests.rs:41–108 Fixture and permissive
Preflight prove transaction authority independently of earlier service checks; add sibling binding
SQL tests rather than duplicating fixture. PostgreSQL tools /opt/homebrew/bin/{initdb,pg_ctl}.

MCP inspection must not invent support for arbitrary ResourceRequirement.contract labels:
ResourceStatus supported_contracts require actual provider facts. Existing MCP stores discovery,
grants, enabled/deleted, auth and credential presence; token writes (mcp.rs:336–380) also take
company-first lock. Unsupported policy/tool snapshots still fail closed via supported_publication.
Resolve supported MCP resource readiness and contract scope explicitly against these owners;
do not claim mcp.call execution or policy authority merely because a connection exists.

Fourth-worker final checkpoint: root accepted reviewed third fragment. Root approved early
rotation before the substantially larger cohesive production binding/authority/DB-race slice.
Implementer88,279/258,400=34.16%21:43:22.852Z (usage/runtime sources); reviewer parent-confirmed
78,713=30.46%21:41:44.194Z (token_count.info fields), completed/idle per collaboration status.
Only BRIEF changed in this worker; git diff --check PASS. No source edits, migrations, database
clusters, or outstanding processes; reviewer focused test process finished. No cleanup obligations.
Precise next-slice packet above; do not redo accepted third-fragment review or full01/02 discovery.
Whole03.1 remains implementing. Approximate graft-reported savings this implementer ~2.62M tokens
(includes map's whole-repository baseline, not measured model-token reduction).

## Production binding lifecycle slice (fifth worker)

Implementer /root/binding_lifecycle, verifiedUUID01a0e4d3-5645-70a0-8b52-9c26f6585d6c,
Astra/medium. Startup24,319/258,400=9.41%21:44:20Z; current104,039=40.26%21:56:37Z,
usage/runtime sources. Reviewer /root/binding_lifecycle/review_binding,
UUID01a0e4d3-9f7b-7b01-a9d0-15a16680a2aa, Astra/medium. All prior work preserved.

Added production BindingLifecycle with company-first exact state CAS, fixed association,
immutable configuration revisions/events, bounded checked row/bundle restoration, current
management/member/principal/channel/thread/allowlist authorization, archival locking and exact
stored bundle comparison. Deactivation retains actor/association authorization but skips version
selection/resource readiness; inactive reconfiguration still checks resources. Applied migrations
unchanged. MCP connection_on reuses the existing validated credential-free owner projection.
Supported readiness is contract-free kind=mcp, current enabled/nondeleted connection, nonempty
validated discovered tool grants, and bearer credential presence. No arbitrary contract labels,
policy support, provider execution, or agent/model/HTTP authority is inferred. Existing MCP
company-first config/grant/token writers serialize these observations; use-time authority remains
phase04. Production admission/history/jobs still pending; no03.2 work started.

Independent actual-code review found one P2 missing changed-configuration DB roundtrip/history
assertion; added deterministic active reconfiguration with changed params+MCP selection, reread,
prior immutable DB revision and retained snapshot checks. Same reviewer correction PASS with no
further findings at21:56:41Z,87,255/258,400=33.77%, usage/runtime sources. Review inspected actual
CAS/authorization/MCP callers and tests. Runtime/static evidence finalization pending below.
Six DB tests passed at2MiB (before seventh roundtrip test added to compiled binary), log
/private/tmp/workflow-binding-tests.log. Isolated UTF8 migrations5PASS:
/private/tmp/workflow-binding-migrate.log. Task cluster/private/tmp/workflow-binding-pg-d356,
port55439, databaseworkflow_binding currently owned by worker; cleanup pending.

Current-source verification:181workflow tests PASS at2MiB, including7new DB tests;
/private/tmp/workflow-binding-workflow-tests.log. SQLx prepare against isolated schema PASS,
/private/tmp/workflow-binding-prepare.log; .sqlx unchanged as expected for runtime queries.
Current fmt/diff/graft PASS, /private/tmp/workflow-binding-{fmt,diff,graft}.log.
Final locked offline alltarget check/Clippy and affected MCP tests still in progress.

### Fifth-worker final checkpoint

Bounded slice independent PASS after actual-code review, P2 correction review and final log
inspection. Reviewer final89,382/258,400=34.59%22:03:48Z, usage/runtime sources; stopped/idle,
no resources. Implementer latest112,925/258,400=43.70%22:03:26Z before cleanup/final recording.
Production bindings now implemented/verified for the precise support above; root owns acceptance.
No admission/history/job implementation, no03.2 work, no full03.1 or phase acceptance claimed.

Exact final commands (task database URLs pointed to127.0.0.1:55439/workflow_binding):
- TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_binding SQLX_OFFLINE=true
  RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow -- --nocapture
  →181PASS, /private/tmp/workflow-binding-workflow-tests.log.
- Same environment: cargo test --locked --offline --lib adapters::persistence::mcp -- --nocapture
  →11PASS, /private/tmp/workflow-binding-mcp-tests.log.
- SQLX_OFFLINE=true cargo check --locked --offline --all-targets →PASS,
  /private/tmp/workflow-binding-check-final.log.
- SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings →PASS,
  /private/tmp/workflow-binding-clippy.log.
- DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_binding cargo sqlx migrate run →5PASS,
  /private/tmp/workflow-binding-migrate.log; same URL cargo sqlx prepare -- --all-targets →PASS,
  /private/tmp/workflow-binding-prepare.log, no.sqlx diff.
- cargo fmt --all -- --check; git diff --check; graft build →PASS,
  /private/tmp/workflow-binding-{fmt,diff,graft}.log.
Earlier check/test failures were test fixture type/naming errors, corrected before final gates.
No source changes after independent correction PASS. Only this evidence record updated afterward.
Task PostgreSQL stopped; /private/tmp/workflow-binding-pg-d356 and all contained task databases
removed. Logs retained; no cleanup obligations or pending build processes. Approximate graft
reported savings379k tokens (tool estimate, not measured model-token savings).

Next cohesive03.1 work remains production admission/source-alias/history/job atomic ownership
using the architecture above; new production binding helpers define current authority and selected
configuration. Do not redo accepted codec/definitions/binding/schema fragments. Whole03.1 still
requires admission races/rollback/snapshot gates and combined independent integration review.
Final implementer sample115,105/258,400=44.55%22:04:44Z (usage/runtime sources);
parent-confirmed reviewer90,170=34.90%22:03:55Z (token_count.info fields), completed/idle.
Both workers quiescent at this checkpoint; no further chunk started.

## Remaining admission scope reconciliation (2026-09-28)

Capacity verified before code: implementer `/root/admission`, session
`01a0e6a2-e3aa-7d91-af2a-14d0588b1352`, Astra/medium; nested read-only reviewer
`/root/admission/reviewer`, session `01a0e6a3-2c4b-75a2-9967-e9c5ceaadc34`, Astra/medium,
ready and idle. Accepted fragments and both applied workflow migrations are preserved.
Root owns acceptance/PROGRESS. The remaining selected scope is all of03.1, never03.2.

| Remaining criterion | Deliverable / seam | Required evidence |
| --- | --- | --- |
| Scoped durable identity | Additive migration for runs, logical executions, waits, admissions/aliases, immutable history membership and run audit. Scoped FKs tie execution/wait/history/source ownership to company/run; logical activation and source/binding uniqueness. Snapshot binding identity survives head removal without cascading away admitted runs. | Isolated migrations; cross-company/dangling relationship failures; rollback at deferred/final constraint; no edits to applied migrations. |
| Exact admission snapshot | Existing PreparedAdmission/WorkflowAdmission owns one transaction: saved bundle codec, input, binding revision/params/resources, limits/deadline, first execution and ID-only first job. Separate normalized snapshots from queue payload. | Stored/read snapshot roundtrip and malformed stored data rejection; changed binding/version cannot alter prior runs; final-write failure leaves no partial run/execution/job/history/alias/audit. |
| Two identities for replay | WorkflowBindings first resolves company+command key or company+logical binding+canonical source to saved run configuration. Transaction compares command equivalence separately: stable trigger/source, input, association, binding; random proposed IDs/correlation excluded. New alias key checks canonical source/input/association, stores its own original command equivalence, and returns original run/state. | Same-key races, different-key same-source races, changed-input/key conflicts, repeated alias with changed trigger conflicts, replay after binding reconfiguration/deactivation/removal/archive, distinct messages in one thread yield distinct runs/jobs. |
| Current authorization | Reuse company-first actor/principal lock and association helpers; new admission checks active exact selected configuration and selectable exact bundle. Replay retains saved configuration but rechecks current actor, source association, and saved resource authority; it does not require a live binding head or selectable version. | Membership/channel/source/resource revocation races observed through database lock contention; unauthorized new/replay/alias leaves no writes; unavailable owner/query errors propagate. |
| Source authority | Scoped canonical messages and thread membership; existing schedule occurrence owner and schedule linkage; child execution must resolve scoped execution/run/step. Child-action source remains unsupported/fail-closed until phase04 supplies the actual action owner; no invented action authority. Manual source identity is stable trigger UUID after actor authorization. | Missing/foreign/inconsistent message, occurrence, parent identities rejected; child-action unsupported regression; no authority inferred from correlation or IDs. |
| Committed history boundary | A bounded single-statement membership snapshot from thread_messages, with deterministic order and scoped pinned canonical IDs, stored in the admission transaction. No timestamp/allocated-sequence cutoff, conversation execution lock, or later implicit history scan. | Two transactions: older-created uncommitted membership commits after capture and is absent; committed membership present; later admissions see it; bound overflow rejects/rolls back rather than silently widening history. |
| Sole job/attempt ownership | Reuse background_tasks/task_attempts. Add workflow discriminator and scoped execution link/conditional payload shape; nullable channel only for workflow company association. Explicit exclusion at every legacy read, claim, status/control, lease/recovery, derived projection and mutation. Keep legacy entity channel required with fallible DB conversion. | Exhaustive inventory narrowed until graft reports no cap; real workflow company/channel jobs survive all legacy claims/controls/recovery, mixed legacy jobs still work; scoped FK and ID-only payload checks; nullable decode regression. |
| Combined acceptance | Formatting/diff, locked offline all-target check/Clippy, SQLx prepare, migrated DB tests, real admission races/rollback, workflow and full DB-backed library suite at stock2MiB, graft refresh; independent whole03.1 actual-code review after edits stop. | Exact commands/logs and criterion coverage recorded here; earlier unaffected fragment evidence reused, no full03.1 claim before root acceptance. |

Lock order follows existing authority: company owner first, membership/principals, association,
binding/definition/resource/source owners, then newly created run children. This serializes short
admission commits per company using the established lifecycle protocol; it never owns a
conversation lock while executing. Run-first runtime transitions remain later03 scope.
Snapshot replay must not reacquire mutable binding state as its authority or source of content.
Resource readiness remains the exact accepted MCP capability matrix; unsupported kinds fail closed.
Execution input activation, worker claiming/lease handling, waiting/resuming and handlers are
excluded from this point even though their normalized identity records are created here.

Initial discovery pointers: application/workflow/ports.rs:10–61, contracts.rs:92–184,
service.rs:44–74; persistence/workflow/binding_rows.rs:20–96, bindings.rs:30–55/96–118,
association.rs:6–61. Fresh initial job inventory is
`/private/tmp/workflow-admission-job-references.txt`; its300hit cap is explicitly NOT exhaustive
and must be narrowed before implementation audit. No production code edited at reconciliation.

## Sixth-worker admission schema checkpoint (2026-09-28)

Added immutable applied migration20260928070000_workflow_runs.sql and
persistence/workflow/run_schema_tests.rs (registered in existing tests.rs). No existing migration
changed. Runs retain exact bounded bundle/input/params/resources/binding identity and limits/deadline
independently of removable binding heads. Executions/waits introduce only logical identity and
scoped relationships; runtime state transitions/activation/results remain later owned changes.
Canonical source admission and command alias uniqueness, scoped pinned history membership, and
run audit records added. A composite UNIQUE on thread_messages extends existing stronger
channel+message uniqueness without rewriting data or weakening constraints. Source key encoding
and transactional authority are still the upcoming adapter's responsibility.

First job link is explicitly NOT implemented; background_tasks/task_attempts unchanged. Production
WorkflowAdmission/WorkflowBindings/inspection adapters, transaction authorization, canonical source
validation/encoding, run state, history capture/read and full legacy exclusion/nullable channel
integration remain pending. These schema tests do not establish those service guarantees.
Whole03.1 remains IMPLEMENTING and03.2 must not start.

Independent reviewer `/root/admission/reviewer`, UUID01a0e6a3-2c4b-75a2-9967-e9c5ceaadc34,
actual-code schema review found one P2: missing history integrity coverage. Added valid roundtrip,
foreign company/wrong run/wrong thread/missing membership rejection, ordinal uniqueness and bounds,
and membership rollback on failed final audit. Also strengthened competing-source inserts with a
barrier after both uncommitted run inserts and a5second timeout. Correction review PASS no findings,
71,081/258,400=27.51%06:21:54.824Z (usage/runtime sources). No migration changed after apply.
Initial test compilation failed two private fixture.version accesses; corrected using existing
public request(None).version, without changing accepted fixture code.

Evidence prefix `/private/tmp/workflow-admission-schema-`:
- migrate.log: all6migrationsPASS on fresh isolated database.
- tests-corrected.log:4new real SQL testsPASS at2MiB (barrier uniqueness/losing rollback,
  exact snapshot survives binding removal, execution/wait FKs, scoped membership and final rollback).
- workflow.log:185workflow testsPASS/0ignored at2MiB, including all existing workflow SQL fragments.
- check.log: SQLX_OFFLINE=true cargo check --locked --offline --all-targetsPASS.
- prepare.log: DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
  cargo sqlx prepare -- --all-targetsPASS; .sqlx unchanged (new queries runtime SQL).
- fmt.log/diff.log/graft.log: cargo fmt --all -- --check, git diff --check, graft buildPASS.
- clippy.log: final locked offline all-target Clippy-Dwarnings outcome recorded below.
Test commands use TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib
with workflow_run_schema and workflow filters respectively, followed by -- --nocapture.
Final whole-point full DB/stock-stack and independent integration gate remain due after actual
admission/job implementation; no premature combined acceptance.

Task-resource ownership transfer requested by root: PostgreSQL is RUNNING on127.0.0.1:55439,
data/private/tmp/workflow-admission-pg-e3aa, logfile/private/tmp/workflow-admission-postgres.log,
databaseworkflow_admission. Started with max_connections200 and socket/private/tmp; task-owned
cluster only, no existing DB touched. Next implementer/root owns eventual pg_ctl stop/data removal.
Reuse this cluster rather than recreate it. Root must explicitly preserve this cleanup obligation
at further handoff. No commit/reset/deploy or other resource changes.

Implementer latest114,190/258,400=44.19%06:23:30.459Z, usage/runtime sources. Natural early
rotation after fragment verification planned to leave the substantial adapter/job audit to a fresh
worker; not a50% threshold claim. Graft reported savings at least3,468,725tokens (one search output
was truncated; this is a lower bound, not actual measured token/cost savings). Initial capped job
inventory remains incomplete; narrower exhaustive scans mandatory next. No further code edits
since correction review.

Final ClippyPASS: SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings,
/private/tmp/workflow-admission-schema-clippy.log (26.67s). Post-evidence git diff --checkPASS.
All test/build/graph processes have finished. PostgreSQL intentionally remains running for the
explicit root/next-worker resource transfer above.

Root accepted schema-only fragment; whole03.1 remains IMPLEMENTING. Final independent reviewer
actual-code correction + evidence PASS:75,870/258,400=29.36%06:24:42.775Z, usage/runtime sources.
Reviewer independently reran fmt/diff and confirmed.sqlx unchanged; now quiescent with no resources.
Implementer final substantive sample116,546/258,400=45.10%06:24:40.434Z, usage/runtime sources;
root approved natural rotation. Root explicitly accepted ownership of the running task PG cluster.
Both workers stop here; preserve fresh-schema migration immutability and resume actual admission/job
integration with a new implementation subtree. No further substantive work.

## Seventh-worker admission selection slice (2026-09-28)

Scope: complete production WorkflowBindings read port, a prerequisite of transactional admission.
Root accepted this bounded slice; no write admission, history capture, run state or job ownership
completion is implied. Existing WorkflowService invokes it after preflight authorization; the
future WorkflowAdmission writer must recheck current actor/source/resources and exact selection
under locks. The read transaction grants no authority and does not create aliases.

New admission_binding.rs reads command and canonical-source identity, selecting a saved run's
exact bundle/params/resources/revision independently of removed/deactivated binding heads and
archived definitions. Same command requires its original trigger UUID/source/binding; a fresh
command for a repeated external source loads the same run configuration despite trigger changes.
Manual sources use the stable trigger UUID. SourceKey v1 is a JSON-array encoding of typed source
identities (including immediate child execution/action) for unambiguous bounded equality; action
encoding is not action authority. Unknown child-action admission remains unsupported until its
actual owner exists. Fallback requires an active binding and nonarchived definition. All reads
share one read-only repeatable-read transaction; stale reads are still subject to write-time CAS.
Exact restore uses existing bounded bundle codec and ConfiguredBinding validation plus explicit
company/workflow/version crosschecks. Input snapshot is not read by this narrow port.

New admission_binding_tests.rs seeds already-committed schema records directly to verify this
read port; it intentionally does not establish admission-write/concurrency guarantees. Tests cover
active/archive selection, saved replay after revision/head changes and archive, alias vs command
trigger semantics, different messages/tenant boundaries, malformed/unknown saved bundle and bad
resource data fail closed without current-config fallback, and versioned source identity bounds.
No migrations altered or added; previous applied schema remains immutable. Existing staged,
unstaged and untracked work preserved. Initial fixture compilation used an incorrect
CanonicalMessageId import; corrected to entities::message before rerun.

Implementer /root/admission_runtime UUID01a0e6b0-a00b-7622-9c63-3400a913f1d2 Astra/medium;
latest30.37%78487/25840006:30:23.270Z usage/runtime sources. Nested reviewer
/root/admission_runtime/reviewer UUID01a0e6b0-eec4-7931-ba5f-94cc4b96c810 Astra/medium,
capacity verified before edits9.11%23537/25840006:25:58.700Z. Actual-code review/checks pending.
Reusing task PostgreSQL/private/tmp/workflow-admission-pg-e3aa port55439 DBworkflow_admission;
worker owns cleanup or explicit transfer. No existing DB touched; no commit/reset/deployment.

### Seventh-worker final reader evidence

Independent actual-code review PASS without findings/corrections. Reviewer73,665/258,400=28.51%
at06:33:03.359Z, usage/runtime sources; inspected service/port integration, causal identity,
configuration/codec and schema constraints. Final gate evidence:
- TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true
  RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow -- --nocapture
  →190PASS/0ignored, including4new real DB readers + source identity test;
  /private/tmp/workflow-admission-binding-workflow.log.
- SQLX_OFFLINE=true cargo check --locked --offline --all-targets →PASS,
  /private/tmp/workflow-admission-binding-check-final.log.
- SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings →PASS,
  /private/tmp/workflow-admission-binding-clippy.log.
- DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission cargo sqlx prepare -- --all-targets
  →PASS, /private/tmp/workflow-admission-binding-prepare.log; .sqlx unchanged.
- cargo fmt --all -- --check; git diff --check; graft build →PASS,
  /private/tmp/workflow-admission-binding-{fmt,diff,graft}.log.
Six applied migrations unchanged; reuse prior isolated migration evidence. No concurrency mutation
protocol introduced by this read slice. Final complete03.1 DB/stock-stack/integration gates remain
due after production writer and first-job implementation.

Initial targeted/workflow runs failed sandbox PostgreSQL PermissionDenied; escalated localhost
rerun used the same task cluster. First escalated workflow build overlapped SQLx prepare clearing
.sqlx, causing offline macro misses; sequential rerun after successful prepare passed190tests.
Keep prepare sequential with offline builds. These failed logs are not acceptance evidence.
No code edits after independent review began; no change to accepted prior fragments.

Natural rotation agreed by root before larger job sweep; not a50% threshold stop. New inventory
/private/tmp/workflow-admission-job-scope.json has316background_tasks matches/53files, capped300
with16truncated, so NOT exhaustive. /private/tmp/workflow-admission-job-paths.txt lists returned
files; next worker must narrow per subsystem/file and include task_attempts and nullable-channel
consumers. No new job audit acceptance inferred from this sizing inventory.

Resource ownership explicitly transfers back to root: taskPG remains RUNNING,
/private/tmp/workflow-admission-pg-e3aa port55439 DBworkflow_admission, log
/private/tmp/workflow-admission-postgres.log, max_connections200/socket/private/tmp.
Do not clean up while reused; eventual next owner must stop/remove this task cluster only.
All worker build/test processes finished. No commit/reset/deploy; whole03.1 still IMPLEMENTING.

Final independent evidence PASS: reviewer77,267/258,400=29.90%06:35:13.552Z,
usage/runtime sources; stopped/quiescent, no resources. Implementer35.89%92,749/258,400
06:34:48.196Z before final recording; stopped after handoff. Graft reported2,608,595tokens
saved across counted discovery calls (tool estimate, not measured model-token/cost savings).
Root owns bounded reader acceptance and next-worker delegation; no further substantive work.

## Eighth-worker dormant job scaffold checkpoint (2026-09-28)

See [JOB-SLICE-03.1.md](JOB-SLICE-03.1.md) for root-approved job subdivision, uncapped inventory,
implementation contracts, reviewed corrections and exact final evidence. Additive applied migration
20260928073000_workflow_jobs_dormant.sql adds disabled workflow job shape/scoped execution link
and gates eight direct legacy triggers. CHECK keeps workflow jobs impossible in production and
channel remains NOT NULL until complete legacy query/routine and nullable audit. This is NOT
full job integration or whole03.1 acceptance.4new DBtests/194workflow/1919full-lib PASS at2MiB
(22existing full-suite ignored); static/SQLx/fmt/graft PASS; independent code correction PASS.
Task PG running, ownership accepted by root for transfer. Natural rotation near48%, no next slice.

## Legacy external projection/index checkpoint (2026-09-28)

Bounded external projection slice now excludes workflow jobs across channel removal, member
work-at-stake, dashboard task/shared-attempt aggregates, schedules and thread projections.
Full-suite plan regression was corrected with additive applied90000index migration; prior applied
migrations unchanged. Original plan bound stays intact, with additional workflow-heavy custom/
generic plan coverage. Independent actual-code PASS; full stock2MiB1939PASS/22existingignored,
focused5projection+4lookupPASS, offline all-target check/Clippy, SQLx prepare, fmt/diff/graftPASS.
Exact scope, evidence, resource transfer and final reviewer acknowledgment: JOB-SLICE-03.1.md.
This does not accept auxiliary/application/HTTP/nullable integration or enable workflow jobs;
workflow_disabled and channel NOTNULL remain intact. Whole03.1 still IMPLEMENTING; root accepts.

## Scoped auxiliary checkpoint A1 (2026-09-28)

Eleven remaining compound-FK auxiliary owners now enforce visible legacy task/company association
on INSERT/reassociation via additive immutable applied93000/94000. Shared attempts, prior source
guards, independent ownership-journal immutability and task deletion cascades preserved. Raw
all-owner rejection/legacy acceptance/invisible-parent/real deletion contention tests PASS;
old impossible workflow fixtures adapted without weakening guards. Independent correction
actual-code review PASS; full DB1942PASS/22existingignored at2MiB and static/SQLx/fmt/graftPASS.
Exact scope/evidence/correction history/resource transfer: JOB-SLICE-03.1.md A1 checkpoint.
Provisioning/schedule tenant guards(A2), historical handoff events(B), later nullable/exact-run
association gate(C/D) remain pending. Workflow disabled+channel NOTNULL retained, no production
admission writer/03.2, whole03.1 still IMPLEMENTING. Root owns bounded A1 acceptance.

### A2 tenant association checkpoint

Provisioning and schedule legacy association closure implemented, correction independently
reviewed PASS; detailed contract/tests/logs in JOB-SLICE-03.1.md tenant association checkpoint.
Schedule occurrences retain frozen captured channel scope when their schedule moves; current
schedule company and composite task/thread scopes remain enforced. Workflow creation remains
disabled/channel NOTNULL, no admission writer. Fresh-schema cutover only; no compatibility work.
B historical handoff provenance and later nullable/admission gates remain incomplete.

User steering09:44Z: schedule channel is fixed; no movement support is required. A2 remains
UNACCEPTED despite pre-steering1948testPASS and reviewerPASS. Fresh worker must enforce this
across relevant update APIs/UI/DB and replace movement tests with rejection/unchanged checks.
See JOB-SLICE-03.1.md A2 unaccepted handoff. No new-contract correction yet; B remains pending.

### B historical handoff checkpoint

Insertion-only event provenance implemented in additive20260928103000 with a scoped legacy task
key lock; saved immutable history survives task deletion and taskless events remain valid.
Independent handoff consumers audited through the existing legacy-only run association invariant.
Six new real DB tests and full stock2MiB1958PASS/22ignored, offline check/Clippy/SQLx/migrations/
fmt/diff/graft PASS; actual-code independent review PASS without findings. Full evidence and
resource handback: JOB-SLICE-03.1.md B checkpoint. Root owns B acceptance. C/D nullable job gate,
atomic admission and whole03.1 integration remain pending; no03.2 work.
