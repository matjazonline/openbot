# 03PA-P03 — Agent default binding, sender identity and notices

> No backward compatibility. Reset disposable databases; no backfill, dual path or legacy decoder.

## Scope and authority

Original requirements: `plan/03-principal-addresses/README.md`,
`03-agent-default-binding-and-relay.md`, accepted p02 D1/r3 §§1–10 and D2/r2,
and active C001–C007/r1 plus revision10 notes in this worktree's authoritative
`plan/03-principal-addresses/COORDINATION.md` (root sole writer, revision16).
Acknowledged I001–I004/r1 and all those notices at startup. C006 keeps production agent
registration, automatic-desk removal, own-agent relay authority and notice rerouting together here.

Workflow `implement-astra-reviewed-2-3`; root/A `/root`, Sol6.1/xhigh user-confirmed.
B `/root/p02_reviewer`, requested Astra/high, independently started without inherited author
history; idle after saved p02 PASS. Reuse only in review role with this phase's record paths.
Actual worker runtime settings and host context thresholds are not independently exposed.

Worktree `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`;
branch `room/check-last-handoff-md-under-plan-02-work-qc8d`.
Phase start/review-base/HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`.
Prerequisites accepted in phase: p01 `dc9329df…ca23`; p02 B1 `30f6b673…c7bb`,
B2 `b0200760…fa7f6`, final B3/p02 `1f4af6f63e8f51cd33c0c777ffd62fbeb05d218830a9dbc5b1c6f5a8451398aa`.
Accepted source export: p02 `docs/logs/p02-accepted-source.json`/`.tar.gz`, archive SHA256
`60222466760ff53f1ea99b6f478a828bedc347ae049dad8ac4302ca57400a860`;
cache `/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa02-complete-w28o3_jt`.
Sparse omissions retain HEAD bytes. p03 review covers its delta against that export plus affected
consumers, not unrelated user/configuration changes or a repeat of accepted unchanged p01/p02.

Preserve unrelated `.claude/`, `.mcp.json`, `AGENTS.md`, `.trail/` changes and concurrent remote
main work. No blanket staging, stash, reset, force push or deployment. Finish/review/full-gate
Plan03 then integrate and normal-push main; then Plan04/review/full gate/push; then Plan05.
User already authorized that sequence; no renewed push confirmation.

## Batch and gates

One cohesive B1 implements all p03 production provisioning, deletion, directory/onboarding/builder,
relay and agent-notice changes. D1/r1 records the combined costly decisions (persistent ownership,
publication deletion, authenticated sender and notice lock protocol); obtain independent architecture
PASS before dependent source edits. One whole-B1 review follows focused checks; fix all actionable
findings together. p04 owns combined-main integration, deliverable docs and the final full suite.

Snapshot exclusions, exact for this phase: `plan/03-principal-addresses/phases/p03/docs`,
`plan/03-principal-addresses/phases/p02/docs`, `plan/03-principal-addresses/phases/p01/docs`,
`plan/03-principal-addresses/COORDINATION.md`, `plan/02-workflow-roles/COORDINATION.md`,
`plan/02-workflow-roles/phases/p05/docs`. Never exclude implementation or deliverable docs.

## Contract trace before implementation

- `company_agent.rs:71–125`: one production creation transaction used by regular/library-copy
  (`:211–250`), onboarding (`onboarding.rs:198–221`) and builder
  (`workflow/provisioning_builder.rs:58–141`). It still creates a default desk through
  `assistant_channel_on`, returns desk JSON and writes `agent:{id}` commands.
- `workflow/provisioning_native.rs:110–236`: published workflow composition and active binding
  already have responsible ownership here. Generalize the target; retain shared-desk selection.
- `agent.rs:480–574` and init schema agent_revision_immutable: delete holds company/agent,
  retires namespaces/principal, but every published revision currently blocks deletion.
  Owned default workflow cleanup must preserve all external publication/history references.
- `department_roster.rs:31–113,283–353`, `agent_authoring/directory.rs:26–60`,
  provisioning listing `:367–466`, onboarding `:249–259` and its HTTP completion lookup still
  depend on default desk records. Builder capability names, schemas, prompt and snapshots
  expose `create_agent_channel`; both tool and UI outputs must follow the new invariant.
- `thread/ingest/relay.rs:7–205`, `policy.rs:121–187`, `routing.rs:531–585` and
  `authorize_channel:355–404`: source sender authentication is desk-only; internal desk
  ingress bypasses participant ACLs. Frozen delivery proof (`thread/inbound.rs:317–381`,
  native `message_internal_relays`) currently pins only a source channel.
- Agent tool calls have trusted `AgentToolOrigin` (`application/agent/tool_origin.rs:18–31`).
  Final proposals identify the exact terminal execution (`channel_reply_proposals.execution_id`);
  agent history exchanges record which actual agent ran it. Pure workflow finals confer no
  arbitrary agent identity. Keep this distinction explicit in relay authority.
- `notification/agent_notice_route.rs:10–131`, `agent_notice_message.rs:15–109`,
  native `workflow_escalation_agent_route`/receipts/targets still name desk routing. Preserve
  current captured binding/revision, zero-or-one handling root, source authority, notification
  origin seals, payload classification, membership pins and NOWAIT/backoff protocols.

Applicable guides read: root AGENTS, src/AGENTS, persistence/AGENTS, application/AGENTS,
HTTP/AGENTS and pages/AGENTS. Trail used before source and exhaustive literal/caller traces.

## Original acceptance and verification mapping (historical plan)

Current execution status and evidence are recorded in the continuation checkpoints below; this
original table preserves all acceptance obligations and does not claim present check status.

| Criterion/source | Production path/case | Decisive assertion | Focused proof | Status |
| --- | --- | --- | --- | --- |
| AC1 creation / 03A1,4 | Real regular, copy, onboarding and builder provisioning | one agent address, canonical email binding, published agent revision/workflow, active principal binding, zero automatic channels/agent commands; taken slug rolls back all | `agent_address_create_`, real scripted Deployment + queue/provider terminal path; API/onboarding/builder suites | NOT EXERCISED |
| AC2 deletion / 03A2 | Real company-agent delete | unused deletes own binding/workflow/revisions/address atomically; direct threads/runs or external retained publication references refuse without partial cleanup | `agent_address_delete_`, native cross-tenant/ownership negatives, both-order admission/delete race; retained-history/publication/memory tests | NOT EXERCISED |
| AC3 tools/directory / 03A3,4 | Real builtins/publications, People/agent directory, settings/onboarding | `create_agent` returns direct identity; `create_desk_for_agent` creates explicit shared desk; department/role placement intact; prompts/samples use principal address | provisioning/builder/department/authoring/API/HTML focused tests | NOT EXERCISED |
| AC4 sender / 03B1–3 | Real frozen publication→DeliveryWorker→internal relay→receiving commit | agent's own address or authorized source desk only; exact sending principal author/context; member-agent granted restricted desk admitted, unrelated agent refused; forged sender/proof/tenant/retired authority refuses with no writes | `relay_identity_`, actual production contender/proof tests, existing relay/trace/auth/spam/direct policy suites | NOT EXERCISED |
| AC5 private notices / C001–C005,C007 | Real notification projector/admission worker on production-created agent | one private direct message/intent at exact captured principal binding/revision; origin suppression across real agent relay and producer retention; route retirement is original-case failure, no fallback | converted escalation agent/origin/role/account/NOWAIT/lease/race suites; real direct notices | NOT EXERCISED |
| AC6 regression/native | Fresh migration, SQLx, runtime queries, all changed consumers | tenant/target/ownership native guards hold; no test silently lost; thresholds unchanged | focused affected DB suites, offline all-target, source_modules, formatting/whitespace, boundaries+fixtures/docs, clippy with baseline disposition | NOT EXERCISED |

Before any moved/deleted tests, retain the accepted runner inventory (4589 lib cases), then map
every removed name to a collected replacement or intentionally removed old default-desk behavior.
No assertion weakening or resource limit changes. Recording provider/test verification stamps are
named stand-ins; production-created agent address/binding is required here, replacing p02 stand-ins.
Run focused checks for each changed behavior only. p04 final gate runs scripts/check.sh and every
CI-only gate, including fresh migrations/SQLx, full database suite, network isolation, stack and
lease budgets, CSS/docs and graph refresh. Seven confirmed baseline clippy diagnostics and earlier
CSP/CSS failures remain failed until their integrated dispositions; no whole-suite green claim.

## Resources and status

Root owns serialized compiler cache `/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/target`.
Postgres `127.0.0.1:52471`, role mac02; allocate new `pa03_codex_` disposable databases.
B uses `pa03_review_` fixtures. Never reset the cluster or remove other sessions' resources.
No p03 source edits or checks executed. Current next action: complete D1/r1, freeze source context,
pin coordination16 and assign independent combined architecture review to B.

## D1/r1 review assigned

Combined architecture D1/r1 SHA256 `4ea0517a6110bb82727a600f61b420c44dddf6d6ba87d137f74a25b269351abb` assigned to reused independent B `/root/p02_reviewer`, now owning only p03 review records. Exact source context `1f4af6f63e8f51cd33c0c777ffd62fbeb05d218830a9dbc5b1c6f5a8451398aa` independently recomputed with the six recorded exclusions, start/base/HEAD830561e2. Coordination16 pinned at logs/D1-R1-coordination.md. Decisions include native workflow ownership/deletion, agent sender proof and principal notice lock/receipt changes together. No implementation or runtime check yet; no dependent edit until explicit D1 PASS. Source frozen and compiler released to B.

## D1/r1 review in progress — preliminary gaps, not final findings

B has verified exact decision/source/coordination and traced creation, deletion, notice locking and relay sender provenance; full round remains pending. Author independently discovered D1 incorrectly attributes canonical binding creation to an address trigger: existing triggers only guard kind/advance revision/retire; responsible producer is `integration/email_binding.rs:158–190 sync_principal_email_binding_on`. Endpoint keys are local parts (`protocols/email/types.rs:43–45`), so private notice setup must use transport-owned mailbox construction rather than treating endpoint.external_endpoint_key as a complete email. These facts were sent to B without editing the frozen decision.

B reports a concrete additional private-notice gap for the combined outcome: `commit_platform_on` reaches `hold_principal_settings` and checks a placeholder observed identity; `admission_source::message` later checks canonical system author. Both ordinary policies refuse private agent mail. D1/r2 must define purpose-specific current notice/event-fence/receipt-based permission at BOTH boundaries; ordinary system/forged mail must remain refused. Root also asked B to check the analogous proven-agent vs observed-wire-identity seam for internal relays. No final finding IDs/outcome yet, no dependent implementation edits, and source remains accepted p02 digest. Await complete combined result, then correct all actionable findings in ONE revised decision/recheck.

## D1/r2 combined correction recheck

B returned D1-R1 CHANGES REQUIRED, N1/F1 and F2 MEDIUM; all records saved, no implementation authorized. Author corrected both together in immutable logs/D1-r2.md SHA256 `dc3a26cd5572086749483223267eb1c73f6f77380a8fca981b2dadb91a74d144`. F1: explicit existing binding sync helper and principal-before-binding creation/rename transaction, new deferred slug equality guards (incoherent raw rename refuses), transport-rendered actual mailbox from local endpoint/slugs. F2: purpose-specific live projector/event/fence/current-route authority before any canonical direct effect, plus exact sealed receipt/current targeted-claim/binding/revision/source authority at admission; no general system/Platform/observed-identity bypass. Added real projector→private agent→actual admission/handling-root positive and native/app/race refusal proofs. Authenticated-agent principal propagation remains separate.

Source stays exactly accepted p02 digest `1f4af6f63e8f51cd33c0c777ffd62fbeb05d218830a9dbc5b1c6f5a8451398aa`, start/base/HEAD830561e2, six exclusions unchanged, coordination16 pinned D1-R2-coordination.md. Request one combined F1/F2 recheck from existing B; findings remain B-owned OPEN until reviewed. No p03 source/check yet, compiler free, all user authorizations unchanged.

## D1/r2 accepted; B1 provisioning implementation started

Independent B D1-R2/N1 explicit PASS for decision SHA256 `dc3a26cd5572086749483223267eb1c73f6f77380a8fca981b2dadb91a74d144`; F1/F2 RESOLVED at decision level saved. Root independently matched decision, source digest and coordination16 before publishing coordination17 and releasing B1 source edits. All prior notices unchanged/acknowledged. No p03 implementation acceptance or runtime claim.

Initial source work: native shared workflow provisioning now accepts typed new-desk/existing-desk/agent targets and shares workflow composition/binding/selection; explicit optional owner on definition insertion, agent default workflow ids and principal-only bootstrap added. Company-agent creation registers canonical principal address then invokes existing binding sync, publishes and provisions principal workflow instead of a desk. `CreatedAgent` now returns principal_id. Init schema adds immutable tenant owner_agent_id on definitions, agent-revision owner cascade preserving incoming references, deferred agent/address slug equality guards and removes default-desk command comment. These are PARTIAL/unverified production edits; builder/onboarding/directory/deletion/rename/relay/notice consumers and tests are not done.

`cargo fmt --all` PASS. Initial `SQLX_OFFLINE=true CARGO_TARGET_DIR=<owned target> cargo check --locked --offline --lib` is running in session37610, retained logs/B1-provisioning-check1.txt. No source edits while compiler reads current input; expected stale desk consumers are to be resolved, never bypassed. No DB migration/runtime check yet. Need fresh pa03 author schema before DB tests; p02 schemas now obsolete for new binary migration checksums. Next poll check1, inspect errors, then continue whole B1. No batch review before all p03 behavior and focused proof complete.

## B1 provisioning continuation checkpoint — current source5

Current intermediate source digest `612aed13bbb8d39b55af86dd5c8c07b4cdff2f3a76112abfbc26940699fd5a78`, base/HEAD830561e2, exact six exclusions unchanged; logs/B1-provisioning-source5.txt. This is NOT a batch review freeze/acceptance. D1/r2 remains accepted and coordination17 current/acknowledged. Independent B remains idle; do not request layer reviews before whole B1 completion.

Additional production edits since check1: agent rename updates address then existing canonical-binding helper under its principal lock; unused deletion clears response selection while retaining its revision, removes direct bindings, then uses explicit owner cascades with incoming-reference guards. Onboarding now returns typed principal_id rather than default-desk JSON, and completion/reload use its scoped existing onboarding port via new completed_agents method. UI links direct inboxes. Builder name/enum/constants mechanically renamed in24 indexed source paths (including tests), native builtin enums changed; create_agent has no desk configuration fields. New create_desk_for_agent takes an existing agent plus explicit shared-desk configuration, using existing guarded provisioning. Schemas extracted into registered provisioning_schemas.rs; prompt updated. AgentDirectoryEntry now has optional AgentAddress(principal_id,address); bounded company listing/department rosters read addresses through principals and display the direct address. Shared department desks keep their own desk model. No backwards alias/decoder/default desk path added.

Checks: lib check1 FAIL on3 expected removed desk consumers; format/check2 FAIL on dangling old comment and misnamed AppConfig field, retained and corrected; lib check3 PASS44.58s before tool/directory changes; check4 FAIL on one stale CreateAgent.channel_slug race-refusal branch, corrected. **Current locked offline lib check5 PASS41.41s** at source5; cargo fmt PASS. Real rg is installed; transport boundary and fixtures passes1/2/3 all PASS. Fresh runtime1 migration PASS599.84ms before native builtin rename; current **runtime2 migration PASS** and exact SHA384 schema proof2 matches current init. Runtime1 and all p02 runtime/gate DBs now have obsolete checksum for the newest binaries. No p03 runtime test/SQLx prepare/all-target/test-module/source_modules/full-suite/independent implementation review yet. Accepted old p02 test executable is NOT current p03 proof.

Current fixture: pa03_codex_runtime2_20261115, PostgreSQL127.0.0.1:52471 rolemac02. No active root compiler/test/graph/boundary process; cache free. No reviewer process. Record paths and unrelated changes preserved.

NEXT: Continue whole B1 implementation under accepted D1/r2. Authenticated relay sender/proof/current ACL and principal private-notice routing/commit+admission proof are still UNIMPLEMENTED. Finish remaining directory/settings/dispatch-sample consumers, schemas/capability exactness and stale test fixtures. The newly added OnboardingSession.completed_agents and AgentAddress/CreatedAgent/tool input contracts have NOT had their test doubles/cfg tests converted; all-target checks will expose these required semantic updates. Original before inventory4589 is retained; map every renamed/moved test. Rewrite old default-desk tests to meaningful principal binding/no-channel assertions and preserve incoming-publication/history/delete guards; do not simply weaken assertions or skip tests. Production-created-agent direct runtime/SMTP/webhook/queue/relay/private-notice tests replace p02 stand-ins. Keep D1/r2 §6.3 real projector→private direct message→targeted admission/one root proof and native/app/claim/route/source race negatives mandatory. Re-inventory role refusals per C004 when routing changes. Then focused behavioral tests, all-target/offline/SQLx/source/static proof, whole-B1 immutable evidence/review; no intermediate layer acceptance. p04 docs/main integration/full CI-equivalent gate still required before normal main push; then Plan04, then Plan05 per authorization.

## B1 continuation — relay compiles; principal notice implementation in progress

Resume reconciled coordination17 and accepted D1/r2; root/A remains user-confirmed Sol6.1/xhigh,
B /root/p02_reviewer remains idle. No replacement or intermediate layer review. Context/compaction
settings remain unexposed. No source acceptance or push. All six exclusions and authorizations unchanged.

The prior relay-check1 errors were already corrected on disk. Current relay-check2 locked/offline
lib PASS42.26s. New notice source work uses the selected principal reply binding and canonical
transport endpoint; shared principal gate replaces desk admission gate, preserving accounts/company
NOWAIT suffix. TransportRenderer.application_address owns deployment-qualified recipient rendering;
NotificationDisposition pins optional route slug and worker passes the rendered identity. Canonical
private notice writer now requires exact projector event/execution/worker/route authority before
provider/thread/message effects. Ordinary application commits retain observed author semantics.
Receipts and target seals pin the non-null agent principal, not desk, with native event fence proof,
deferred private-system-message receipt completeness and shared exact targeted-admission predicate.
Native root ownership and source authorization use that predicate. These changes are PARTIAL and
UNACCEPTED; behavioral proof, old desk tests/fixtures and full native review remain outstanding.

B1-notice-check1 lib PASS24.61s with old Platform dead-code warning (subsequently removed).
B1-notice-migrate1 fresh database migration PASS642.91ms on pa03_codex_runtime4_20261115,
Postgres127.0.0.1:52471 rolemac02; schema still mutable so recheck SHA384 before using a binary.
B1-all-targets-check1 currently running in session12075. Initial boundary invocation used wrong
script path (FAIL127; retained B1-notice-transport-boundary1.txt), corrected invocation uses
scripts/transport-boundary-check.sh and scripts/tests/transport-boundary.sh, session97836 pending.
No implementation runtime test, SQLx cache regeneration or whole-B1 review has passed yet.

Next: collect all-target/boundary results, fix production errors and semantically convert default-desk
fixtures/tests to production-created principal-addressed agents. Correct manual recipient test helper
to the existing example.test fixture renderer (currently mailagents.com). Rebuild fresh database after
any schema edit. Old platform identity test name changed to
application_ingress_keeps_identity_locks_and_cannot_fabricate_system_authorship; map the removed
old name against collected tests, and add the real authenticated notice projector/admission identity
lock positive. Native receipt key/lease tests need the two new projector fence fields and principal
instead of desk. Preserve affected claim/role/race/origin tests, no skips or weak assertions.
Finish entire B1 and focused proof before independent review. Plan03 p04 integration/full CI gate
and authorized main push remain; Plans04/05 remain unstarted.

## B1 continuation checkpoint — current notice source2 (unaccepted)

Intermediate source `3f49dff07554a3243f36074b804b38cae6927fd6ebda1e19fb56d13175c0129d`,
base/HEAD830561e2, exact six exclusions unchanged (logs/B1-notice-source2.txt). Source1 snapshot
attempt failed on macOS realpath -m; corrected with Homebrew coreutils PATH, no skill edits.
Coordination17/D1-r2 and all authorizations remain unchanged. No batch review freeze/acceptance.

Manual projector calls now pass transport-rendered recipient; ordinary application-origin ingress
keeps observed identity locks/authorship. Test transports explicitly implement new renderer and relay
ports. Converted initial agent/department/directory/roster/onboarding/notice fixtures to typed principal
outputs and selected response bindings, replacing automatic-desk counts/command lookups. Renaming
frees the old canonical address atomically; onboarding tests now expect a new principal. Unused-agent
deletion test expects owned workflow cleanup. Several stale test bodies still need semantic correction
and runtime proof; initial compilation fixes alone are NOT behavioral acceptance.

B1-all-targets-check1 failed78 test/test-double errors (normal exit101). Check2 reduced to3 setup
errors (OnboardingPersistence is implemented by PostgresOnboarding, and helper shadowed existing
agent_principal fixture); stopped own compiler with SIGINT130 after diagnostics to correct them.
Both processes confirmed stopped. Fixed all three, plus current source direct association authorization
through existing association_from_columns and principal gate waiter identification. Current
cargo test --locked --offline --lib --no-run is running session91020, log B1-test-build1.txt.
Compiler cache remains serialized author-owned; no reviewer processes.

Current runtime4 migration SHA384 matches source and native principal receipt FK/route/guards are
present: logs/B1-notice-schema-proof1.json (schema inspection ONLY). Transport boundary+fixture
runs2/3/4 PASS. Latest graph refresh session77665 pending, log B1-notice-trail-refresh1.txt.

Next collect build, save complete runner inventory and map removed names from B1-before-lib-tests.
Run small actual agent creation/deletion and actual private notice projector→admission tests against
runtime4 (set TEST_DATABASE_URL explicitly; never allow missing DB). Then fix all runtime regressions.
Known stale notice guard tests: selected responder removal now makes route ineligible, so preserve
no-context private-policy proof by disabling only context capture on its isolated fixture, keeping
valid selected route. Receipt replay native test must include principal and projector fence columns;
projected event is no longer a live fence, so nonce forgery needs a real pre-receipt mutation probe,
not a post-completion replay whose unrelated stale lease masks the key assertion. Sender spoofing
fixtures into private agents must deliberately open agent or use authenticated real team sender,
without bypassing sender-proof/ordinary direct policy. No thresholds or assertions may be weakened.

No p03 runtime pass, SQLx refresh, all-target pass, evidence acceptance or independent implementation
review yet. Finish whole B1 and required focused proof, then independent Astra B; p04 still owns main
integration/docs/full CI gate and authorized Plan03 push. Plans04/05 unstarted.

## D1/r3 ownership gate reopened (no dependent FK edit yet)

Current lib test executable built (B1-test-build1 PASS3m46s; macOS linker unwind warning retained),
collects4589 tests. Focused actual agent run B1-agent-direct-tests1:8 PASS/3 FAIL. Two fixture defects
(onboarding low-level agent already has principal, and test query uses missing binding.workflow_id)
remain author corrections. Real unused-delete FAIL proves own agent_publication_requests owner FK
and immutable guard omitted by D1/r2, preventing every production-published unused deletion. Native
incoming FK inventory saved B1-delete-owned-fks.txt. Reopen combined D1 for narrow own-receipt
lifetime correction, immutable logs/D1-r3.md SHA25629d8b74a6d14d3a990a4e52ef2a0a3dfc78b30a6a216932f152a86e4ec3d0259.
No owner-FK/guard source change made. Unchanged decisions retain D1/r2 coverage; runtime source
identity remains3f49dff07554a3243f36074b804b38cae6927fd6ebda1e19fb56d13175c0129d.
Coordination17 pinned D1-R3-coordination.md, requirements unchanged. Assign independent B gate recheck.

B1-notice-runtime-tests1 actual projector PASS; actual targeted admission test FAIL before selected
positive while its unrelated-binding refusal helper expected Stale but now receives NotFound from
private source authorization. No targeted-root positive claimed. Correct exact refusal assertion or
responsible contract only with trace/evidence, then run real selected path. All source frozen during
D1/r3 review; compiler/test resource currently free. No whole-B1 review or Plan03 push.

## D1/r3 accepted at decision level

Independent03PA-P03-D1-R3/N1 PASS for29d8b74a6d14d3a990a4e52ef2a0a3dfc78b30a6a216932f152a86e4ec3d0259;
retrospective03PA-P03-D1-R2/F1 resolved at decision level. Root independently rechecked decision,
source3f49dff0…129d and unchanged coordination17 before acceptance (D1-R3-root-acceptance-snapshot).
Narrow own agent-publication-request cascade/parent teardown guard may now be implemented.
No runtime/whole-B1 pass. Company direct-thread teardown lifecycle issue is a separate newly observed
costly-decision gap and needs its own recorded extension gate before affected schema edits.

## D2/r1 extension gate assigned

D1/r3 narrow source correction implemented (owner FK and parent-teardown guard) after PASS. Routine
B1 corrections include principal propagation in batch_commit::successor (actual worker had native
job/run association mismatch), direct fixture SQL, receipt/result schemas, real pre-receipt nonce
mutation test and no-context private-policy fixture retaining the live selected route. These changes
are not yet compiled/exercised; new DB required after accepted D1/r3 schema edit.

D2/r1 proposes direct thread company lifetime and deferred exact direct target pins after rollback-only
native reproduction; new immutable decision315d018f5d4a587c5ad590c2cc3da8d6366cf0581d7a7886555b76974c694b24.
Source frozen26c71787f442752d2de436e197dcb0da8b36c5eaeca73103d925508b65c31a62 at same HEAD/base,
six exclusions unchanged, coordination18 pinned D2-R1-coordination.md. No D2-dependent lifecycle
schema change exists yet. Whole-B1 remains unaccepted. No source edits during this gate review.
Root may compile this frozen source, reviewer must request compiler-cache ownership before building.

## Latest continuation — D1/r3 accepted and runtime proof; D2/r1 pending

Root/A Sol6.1/xhigh confirmed. B /root/p02_reviewer Astra/high is CURRENTLY RUNNING independent
D2/r1 architecture extension review (not whole-B1). No replacement worker, no source edits until it
returns. Root sole coordination writer, revision18; all active notices I001–I004/C001–C007/r1 and
revision10 unchanged. Current source frozen26c71787f442752d2de436e197dcb0da8b36c5eaeca73103d925508b65c31a62,
HEAD/base830561e2, exact six exclusions unchanged. D2 decision SHA256315d018f5d4a587c5ad590c2cc3da8d6366cf0581d7a7886555b76974c694b24;
pin logs/D2-R1-coordination.md. NO proposed D2 lifecycle schema edit yet.

D1/r3 PASS03PA-P03-D1-R3/N1 accepted by matching root snapshot; retrospective D1-R2/F1 RESOLVED
at decision level. Implemented only owned publication-request→agent CASCADE plus immutable DELETE
allowance after actual agent/company parent teardown. Retained both request→revision and all external
history/publication FK guards. B1-agent-direct-tests2 **11 PASS**, including real production creation,
unused deletion, onboarding readback, placement/grants, collision/failed-publish rollback and competing
creators. Query fixture errors fixed. No claim yet for new retained-consumer deletion races.

Production batch_commit::successor had omitted principal_id when creating the next job, explaining
real handler native association errors. Added exact run.principal_id to that INSERT SELECT, preserving
shared native job/run guard. Unrelated-binding private notice refusal now asserts exact NotFound
rather than old Stale; selected actual targeted admission now PASS. No-context fixture disables only
context capture in its isolated database and retains selected route. Native nonce guard now tested
by tampering immediately before receipt under real live event fence, with durable sequence proving
boundary reached; zero canonical effect after rollback. Old post-completion replay was invalid proof.

Current lib test build2 PASS1m24s (unused ChannelReplies/ConfigureChannelReply test import warning
still to remove after freeze; macOS compact-unwind warning retained). Binary remains
/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/target/debug/deps/open_bots-ecc73d9eac497eda.
Build1 complete inventory4589; 16 old names mapped to16 collected replacements in
B1-test-removal-map1.json, retaining semantic obligations. Regenerate after additional tests.
Compiler/cache free. Last boundary job session79457 pending; previous transport boundary+fixtures4
PASS; graph refresh1 PASS; whitespace/format1 PASS before last source corrections.

Current runtime fixture **pa03_codex_runtime5_20261115**, Postgres127.0.0.1:52471 rolemac02,
fresh migration B1-owner-migrate1 PASS after D1/r3 schema. Earlier runtime4 checksum is obsolete for
new binary. Set TEST_DATABASE_URL explicitly when invoking binary plus DATABASE_URL and existing
RUST_MIN_STACK=16777216 (unchanged repo setting). Do not allow missing database or reset cluster.
B1-provisioning-unit-tests2=5PASS/1FAIL (stale required-field loop still names desk instead of address).
B1-notice-runtime-tests3=**18 PASS/2 FAIL**. Real projector→exact targeted handling root/one-of-eight,
ordinary direct fanout, no-context private policy, pre-receipt nonce refusal, expiry/competing
projectors, account/company NOWAIT/backoff/lease rollback, real tool read/refusal/ack/reassign PASS.
All logs retained, no full acceptance.

Remaining failures:
1. Actual private-notice company teardown fails external_threads_target_principal_fk. Rollback-only
   B1-teardown-probe1..4 (SQL/logs/summary) proves threads has no company owner FK: deferring pins
   alone leaves direct threads orphaned; adding company owner CASCADE and deferring thread/external
   target pins makes same teardown + SET CONSTRAINTS ALL IMMEDIATE pass before rollback. D2/r1
   proposes company ownership plus deferrable NO ACTION direct target pins; B currently reviewing.
   No exploratory DDL/data persisted; runtime4 schema unchanged by probes.
2. Three-party role test fixture waiter ambiguity: after notice waits at blocker principal, role
   writer starts, but waiter(blocker,None) selects the earlier projector as writer. The account
   update schedule then waits on wrong blocker until production role writer times out. Proposed
   routine fixture fix after freeze: waiter optional excluded PID instead of obsolete advisory mode;
   capture projector PID first, then require a DIFFERENT waiting PID after starting real role writer,
   then require account writer blocked by that writer before releasing principal. Keep assertions,
   timeouts and real three-party work unchanged. Actual idle-role contention test already PASS.

Next: await B D2 decision, verify identities/notes/findings, correct together if required, and only
then implement accepted lifecycle source delta. Fix stale required-field loop and waiter/remove
unused imports. Add real authenticated notice identity-lock positive (ordinary application identity
lock test already converted), native wrong/missing fence/claim/route/source negatives, retained
external deletion/race proof and relay_identity_ real production tests. Convert remaining old
provisioning/runtime/direct-agent stand-ins and consumer fixtures; run focused affected suites.
SQLx regeneration, all-target/source_modules/static gates, whole-B1 manifest/review and p04 full
integration/CI gate still outstanding. No Plan03 main push. Then authorized Plan04/review/gate/push,
then Plan05; both remain unstarted. Never stage unrelated .claude/.mcp/AGENTS/.trail work.

## D2/r1 accepted at decision level

Independent03PA-P03-D2-R1/N1 PASS; root decision/source/coordination identities match recorded
D2-R1-root-acceptance-snapshot. Retrospective lifecycle F1 resolved at decision level only.
The initial record-publication assertion searched the REVIEW link label for the full note ID;
actual note ID was saved in agent-review-notes. Source/decision identity assertions had passed.
Verified actual saved note and REVIEW link, completed coordination19 publication; no gate substitution.
Implemented only accepted explicit thread company owner and15 deferred NO ACTION principal target pins.
Runtime/native/concurrency proof remains outstanding. B idle. Whole-B1 still unaccepted.

## Latest checkpoint — D2 accepted; all21 notice checks green; production fixture conversion

D2/r1 independent03PA-P03-D2-R1/N1 PASS accepted by root matching decision/source/coordination18;
coordination19 now authoritative. B /root/p02_reviewer idle; no pending reviewer work. Historical
D2-R1/F1 lifecycle finding resolved at decision level. Implemented only accepted explicit threads
company owner CASCADE and15 deferred NO ACTION direct target pins. Live principal retention and
all other incoming references unchanged. No source freeze/review active; continue whole B1.

Current focused results at lifecycle source1 d2d8334426b947b95e7b22fe53233cb5c02d11c0f139b74594c9f4a40d8db685:
- B1-test-build3 PASS1m28s (only macOS unwind linker warning).4593 tests now collected implicitly
  from filter totals: prior4589 +3 lifecycle tests +1 real notice identity-lock/admission positive.
- B1-native-lifecycle-tests1 **2 PASS**: retained native principal physical deletion cannot commit;
  foreign target insert cannot commit; both assert exact threads_target_principal_fk and full row
  inventories restored after rollback.
- B1-retained-direct-delete-tests1 **1 PASS**: actual production-created agent with direct history
  refuses deletion and all13 resource table snapshots remain equal.
- B1-notice-runtime-tests4 **21 PASS/0 FAIL**,20.08s: actual projector→exact targeted root, including
  real principal company teardown, corrected three-party waiter excluding projector PID, no wire
  identity observation under held identity advisory key, all prior lease/nonce/competing/tool cases.
- B1-provisioning-unit-tests3 **6 PASS** (required direct address field, create_agent vs shared-desk
  custom-step schemas). Earlier failed logs retained; no weakening or limit changes.
- Transport boundary/fixtures lifecycle1 PASS. Fresh runtime6 migration604.65ms PASS; schema SHA384
  and15 deferred pins/thread company FK/owned publication cascade match in B1-lifecycle-schema-proof1.json.

Current fixture **pa03_codex_runtime6_20261115** on Postgres127.0.0.1:52471 rolemac02. Runtime4/5
have obsolete checksums. Current lib binary path stable same ecc73d9 hash. TEST_DATABASE_URL explicit
plus DATABASE_URL to runtime6; existing RUST_MIN_STACK16777216 unchanged. New source build4 is RUNNING
session36778, logs/B1-test-build4.txt. Do not edit source while it compiles. Native/source-static checks
for all additional D2 table cases and company/ingress competing claimants remain mandatory.

New source since green source1: Deployment::build in message_ingress_tests.rs now registers keyed
model connection BEFORE calling actual CompanyAgentService::create; uses shipped native templates
at APP_DOMAIN, then creates its explicit support desk. Removed duplicate low-level agent publication.
This turns shared runtime/SMTP/webhook fixtures into production-created canonical agent principals.
public_desk_private_agent_tests::private_endpoint now reads/asserts production canonical assistant
address+endpoint rather than inserting assistant-direct-standin. Source edit not yet compiled or
runtime verified. No production behavior change in this last delta, only fixture contract conversion.

NEXT: collect build4, run public_desk_private_agent_ runtime/SMTP/webhook cohort and message_ingress
smoke to prove fixture. Continue direct-workflow/ingress stand-in conversions: DirectIntents still
registers noncanonical workflow-direct-standin and must be updated meaningfully (production-created
agent if required, with explicit pure-binding fixture replacing default selection). Remaining builder
runtime/dispatch tests still send old create_agent desk fields or expect channel receipts; fix per
actual create_agent/create_desk_for_agent contracts, no aliases. Real relay_identity_ proof and all
origin/retention/source-authority/route/fence negatives, retained outside publication/delete race and
company/ingress concurrency remain. SQLx/all-target/source_modules/static proof, whole-B1 manifest
and independent implementation review still pending. p04 integrated full CI/docs/main push then
Plan04 and Plan05 remain unstarted/authorized. Preserve unrelated user/config changes.

## Resume 2026-11-15 — recovered later on-disk work; builder conversion

Root/A remains user-confirmed Sol6.1/xhigh. Coordination19 checked and acknowledged;
I001–I004/C001–C007/r1 and revision10 unchanged. B /root/p02_reviewer remains idle after D2 PASS.
Context/compaction controls unexposed; durable checkpoints used. No new phase acceptance/push.
Recovered logs after the previous handoff: build6 PASS (4596 collected tests), actual default direct
reply PASS, both company-delete/direct-ingress race orders PASS, native agent publication negatives
PASS; real escalation-origin relay FAIL because its delivery becomes retryable. Current runtime6
used with explicit TEST_DATABASE_URL/DATABASE_URL and unchanged RUST_MIN_STACK16777216.
Reran public_desk_private_agent_ against current compiled production fixture:8PASS, including real
SMTP and signed-webhook auth/spam/reply/privacy and knowledge audience checks, log
B1-production-public-desk-tests1.txt. DirectIntents was already converted on disk to canonical slug
and an explicit low-level pure-binding fixture; default runtime is tested through actual Deployment.

Current source edits (unaccepted): split obsolete builder cases into provisioning_builder_tests.rs;
replace automatic-desk cases with principal target/active binding/no desk/no agent-command assertions,
explicit separate reviewed-desk staffing, invalid implicit desk fields, placement/refusal and real
competing-claimant receipt checks. Shared creation counts now include principal workflow bindings;
listing expects direct address instead of desk. Added retained-error diagnostic to actual relay test.
Build7 RUNNING session32491, B1-test-build7.txt. No source edits while compiler is active.
Next run builder dispatch cohort and relay diagnostic, correct all failures, convert builder runtime
and prompt expectations plus remaining ingress stand-ins, implement/prove relay_identity_ and retained
consumer/race/native proof; then SQLx/static/all-target/source_modules, whole-B1 Astra review and
p04 integrated full CI/docs/main push. Plan04/root-unit and05/scopes remain queued and authorized.

## Resume checkpoint — real relay reaches commit; direct fixtures and tool authority corrected

Build9 PASS1m25s at fresh pa03_codex_resume1_20261115 (same server127.0.0.1:52471/mac02).
The native deferred relay-target trigger used unparenthesized IF NOT CASE; PostgreSQL parsed its
expression incorrectly only at actual COMMIT. Corrected syntax in init schema, fresh migration
PASS598.93ms and SHA384/15 deferred target pins/company owner proof retained in
B1-resume-migrate1.txt/B1-resume-schema-proof1.json. Schema currently stable; runtime6 checksum
obsolete for subsequent builds. Real two-agent cross-desk origin exchange now PASS; first relay
journey proceeds past delivery/forgery/producer retention and failed later on a stale desk fixture.

Broader focused checks (all failures retained): dispatch23PASS/2FAIL; origin10PASS/5FAIL;
builder runtime6PASS/4FAIL; prompt2PASS/1FAIL; address46PASS/6FAIL. Logs B1-resume-checks2.json.
Followed these failures to one production omission: provisioning_status supported contracts lacked
create_desk_for_agent, refusing explicit staffing despite its declaration/publication/grant. Added
that exact declared capability (no broader bypass). Remaining failures identify obsolete fixtures:
null channel decoded as UUID for direct roots, old desk_id target seal SQL, department endpoint
queried as principal, old agent command lookup, and preproduction mismatched agent/address slugs.
Corrected typed principal source/negative probes and actual desk fixture selectors; converted the
five ingress policy/identity/race/directory/notification users of stand_in_agent to real production
CompanyAgentService creation with canonical address, published default and endpoint. Namespace
native demotion test now starts from canonical slug. Builder prompt preserves explicit confirmation
and exact skill-slug requirements; published tools and all schema validation remain enforced.

Build10 RUNNING session50355; B1-test-build10.txt. Do not edit source while it compiles. No review
freeze or p03 acceptance. B remains idle; coordination19/notices unchanged/acknowledged. Collection
4596;22 removed names all mapped to actual collected replacements in B1-test-removal-map2.json.
Next rerun failed focused cohorts, finish relay_identity real grant/spoof proof and remaining
retention/origin/native negatives, broad affected consumers, SQLx/static/all-target/source_modules;
then whole-B1 evidence/Astra review. p04 integration/full gate/main push and Plans04/05 still queued.

## Resume checkpoint — production relay identity and builder runtime proof

Build12 PASS1m19s. Fresh author fixture remains pa03_codex_resume1_20261115; explicit DB URLs
and unchanged16MiB test stack. No active reviewer/freeze/acceptance; coordination19 unchanged.
B1-resume-affected-tests3 exercised105:100PASS/5FAIL. Its corrected successors are retained in
B1-relay-and-builder-focused-tests1:9 exercised,7PASS/2FAIL. **All three relay_identity_ tests PASS**:
real production-created agent→frozen message__reply→DeliveryWorker→EmailSender→canonical commit
uses department member grant and retains exact agent author; unrelated department agent refuses
without SMTP/receiver effects; changing From to another production-created agent refuses exact
source proof without a canonical hop. Shared reviewed/custom-step builder runtimes and real native
approve/edit/reject PASS after using the shipped built-in Builder definition's three published
capabilities through real CompanyAgentService (no manually incomplete tool list).

The remaining two failures were fixture-only: recording relay proof used random tenant, masking
its loop assertion; native internal-intent forgery still decoded nullable channel as UUID on a direct
source. Corrected recording-helper claim tenant from its configured source desk (it still does not
prove frozen parts; native tests own that proof) and direct native forgery to principal target.
Old relay tests now expect typed Refused outcomes and preserve exact no-effect assertions.
Split long relay test file into793-line parent/388-line runtime sibling; one moved test needs map.
Moved old automatic-desk builder tests into registered builder sibling;22 removals already mapped,
plus new module move and renamed unproved-agent ACL unit test need current inventory update.

Added (uncompiled/unexercised at this checkpoint): production agent deletion with retained external
shared-desk publication must refuse and restore13 resource snapshots; real native projector receipt
matrix tampers missing/wrong execution/worker/event/binding/revision/agent/principal immediately
before receipt under a valid live claim, with nontransactional sequence proving boundary reached
and zero messages/intents/receipts/targets after rollback. No policy/assertion/limit weakening.
Build13 RUNNING session45943, B1-test-build13.txt. Wait for completion before source edits.
Next focused tests for two corrections/new native/retained-publication cases; add both-order agent
retirement vs real direct-ingress race proof using existing direct_company_lifecycle fixture/lock
observer (principal must lock before retained check, already traced agent.rs513–625). Then broad
p03 affected suites, SQLx/offline/all-target/source_modules/boundaries; freeze evidence and whole-B1
Astra review. p04 integration/full CI/docs/main push, then Plan04/review/gate/push and Plan05 remain.

## Native origin and relay correction proof; external-reference fixture qualification

Build13 PASS1m14s; B1-native-and-relay-focused-tests2:3PASS/1FAIL. All9 native receipt mutation
cases and the complete real escalation email-relay origin/retention/forgery/native journey PASS.
Recording relay tenant/loop unit correction PASS. The failed added deletion fixture created an
independently published shared-desk workflow, not an external reference to the agent-owned workflow
version; no existing native incoming version pin was established. This was NOT evidence of a
production regression or permission to broaden retention semantics. Corrected the fixture to
configure an external company binding to the exact agent-owned published version through the real
BindingService/LifecycleAuthorizer. Preserve all13 resource snapshots on refusal. Existing deletion
error mapping now names that exact incoming binding-version FK as Conflict, with no guard/cascade
change or architecture revision. The old failed log is retained; new test name explicitly says binding.

Added two real agent-deletion/direct-ingress races sharing the production-created direct_request
fixture and actual pg_blocking_pids observer: ingress first must commit one thread/intent and force
full deletion refusal; retirement first must remove its address/agent and refuse all ingress effects.
Both wait for the actual native lock queue before releasing the blocker. No timeout/limit changes.
Build14 emitted only test-import path errors and was intentionally stopped130 after diagnostics;
corrected to existing lifecycle/domain public exports. Build15 RUNNING session25293,
B1-test-build15.txt; source stable during compilation. Transport boundary+fixtures2, format and
whitespace1 PASS. Graph refresh RUNNING separately. Coordination19/all notices unchanged; B idle.
Next collect build15, run exact external-binding and two agent races, then broad affected cohort,
SQLx/all-target/source_modules/static proof and current test inventory/removal map. No whole-B1
acceptance or push yet; p04/full gate and authorized Plan04/05 sequence remain mandatory.

## Broad affected diagnostics and native fixture correction batch

The current1407-test affected run (build15, input snapshot5b512db14a4240952a6d084741eb3db8a1de42e1d9d56c804415437dd1dac6c4)
finished **1323PASS/76FAIL/8 inherited ignores**,476.98s; log B1-resume-broad-affected-tests1.txt,
all failures extracted to B1-resume-broad-failures1.json. This is failed affected verification, not
unrelated baseline. SQLx prepare --all-targets PASS2m07s; no .sqlx diff. Build16 PASS2m35s.

56 failures directly reported Company agent has no direct address, with related UI saves returning
500/unchanged drafts. Traced exact producer sync_agent_address_on, which intentionally requires the
canonical address; no production fallback/compatibility path was added. Native authoring/storage
fixtures intentionally begin unpublished, so production CompanyAgentService would destroy their
first-publication/replay assertions. Added one clearly named register_native_agent_address test
helper: exact principal lock, canonical namespace insert, actual email-binding synchronization;
it does NOT publish a workflow or claim production creation/default behavior. Applied at shared
native agent-publication/UI authoring/workflow picker/runtime HTTP/MCP/storage fixture creation,
plus two direct persistence tests. Real production-agent direct/relay/projector checks remain real
CompanyAgentService creation, unchanged; these native fixtures do not replace them.

Other corrections from broad failures: department lead tests now compare typed own principal inbox
(binding/configuration/selection/address), not removed agent command/default desk; onboarding fault
is injected on SECOND owned principal-workflow insertion instead of absent automatic channel, with
same all-effect rollback assertions; rename/new-owner resubmission proves fresh principal/agent
rather than relying on old automatic-desk collision. Bare unpublished native agent's owned revision
can now be deleted, so frozen-result/revoke/deactivate assertions run before unused deletion, then
assert owned cleanup. Real admitted role capture still blocks deletion through existing manifest FK;
classified that exact FK as Conflict. Delete-first role race now requires successful retirement,
exact NoHolder admission refusal and zero partial capture. No guard or cascade was weakened.
Updated two department address wording assertions, and converted a pure-system relay positive to
an explicitly public desk (system origin confers no team/department authority); actual agent grant
positive/nonmember/spoof are separately exercised by the three real production relay tests.

RUNNING now:
- session27385 B1-native-fixture-correction-tests1.txt runs build16's corrected UI/publication/
  workflow authoring/department/onboarding/role/persistence/skill cohort with explicit author DB.
  It exercises the earlier fixture correction subset; later runtime fixture additions are build17.
- session61947 B1-test-build17.txt compiles the completed known correction batch. No source edits
  while compiler reads. Schema unchanged since fresh author resume1 DB; old test process was a
  compiled immutable build15 artifact, and its complete failing output is retained.

Next collect both, resolve any remaining exact failures, run all76 original failed names against
current runner with explicit replacements for renamed tests, plus affected native fixture cohorts.
Then offline/source_modules/last SQLx/static proof, updated collection map, immutable whole-B1
manifest and independent Astra review. Coordination19/notices remain checked and unchanged; B idle.
No p03 implementation acceptance, main integration/push or Plans04/05 yet.

## Latest checkpoint — 2026-11-15, build19 pending; rerun76 original failures next

Broad run76 failures:56 explicit missing canonical addresses, related UI write failures and old
automatic-desk/retirement/text assumptions. One register_native_agent_address helper now supplies
canonical namespaces/endpoints only for native **unpublished** authoring/storage fixtures. This
preserves their initial publication/replay/immutable-binding assertions without adding a production
fallback. Applied shared publication/UI/workflow picker/HTTP runtime/MCP runtime/storage fixtures,
plus persistence/skill tests. Production direct/relay/notice fixtures still use real company creation.

143 of145 corrected fixture tests passed in build16. The sole failure was important: the old
publication test revokes a capability grant, whose retained tombstone's existing FK must block
deletion even after deactivation. Restored original delete-refusal/frozen-result assertions and
original test name; DO NOT replace this with unused-deletion success. Existing native grant FK
unchanged; agent_delete_error now maps actual agent_binding_grants and agent_skill_grants FK names
(from current pg_constraint) to precise Conflict. This is error classification, not a guard change.
The admitted role manifest FK likewise maps to Conflict; role delete-first legitimately succeeds
before capture and next admission must return exact NoHolder with zero partial capture. Both-order
new direct-ingress/agent-deletion race tests3PASS already retained from build15.

The pending remainder of known fixes is compiled by19: three shared HTTP/MCP/native-storage raw
agent factories register their canonical namespaces; two department wording expectations follow
direct addresses; the pure-system relay positive explicitly targets a public desk, with team-only
unproved sender refusal and real member/nonmember/forgery journeys retained. Extracted recording
InternalHop/relay_hop/refusal helpers to a registered small test_support/relay.rs module; test names
stay unchanged. Build18 failed only because refusal helper was private after extraction; corrected
visibility/import in19. No root compiler interruption remains active.

Next collect19, save current --list inventory, and run **all76 exact original failures** read from
B1-resume-broad-failures1.json. Map only onboarding_second_channel_error→second_principal_workflow
and a_hop_to_a_plain_channel→a_pure_workflow_hop_to_a_public_channel to collected replacements;
original publication revoke/deactivate/delete test is restored. Do not omit tests to obtain green.
Then exercise affected shared native fixture cohorts (HTTP runtime/MCP/storage/UI/authoring), new
production relay/notice/agent deletion/creation checks, offline all-target/source_modules and final
SQLx/static proof. Update runner-removal mapping (existing24 plus2 renamed tests, original pub name
restored), AC evidence table, whole-B1 manifest/digest; request independent Astra B whole-B1 review.
Coordination19/notices unchanged. Plan03 p04 integrated full CI/docs/main push and Plans04/05 remain.

## Latest verification checkpoint — all-target/SQLx/source modules PASS; build20 then original76 round2

Build19 PASS1m41s. Round1 of exact76 original failed cases exercised all76, **73PASS/3FAIL**,23.37s
(B1-original76-correction-tests1.txt / results1.json, inventory8). Remaining3 all failed at native
fixture updates with missing address: MCP isolation retarget fixture, HistoryThread private-history
republish fixture, and ToolSetup workflow binding repoint fixture. Traced their actual raw-agent
constructors and added the existing explicit native namespace helper at creation, not a fallback in
mutation/production code. These are3 test setup calls only; no new SQL, schema, trust rule or public
contract. No old failures are omitted. Original publication grant-retention test name/assertions are
restored. Actual grant FK names queried from pg_constraint and precise Conflict mapping retained.

SQLx prepare2 PASS1m09s, offline --all-targets1 PASS0.24s, source_modules1 **8PASS**,7.54s (its
non-test production lib compilation finished before the next lib test build; final3 edits are cfg(test)
fixture calls and add no module path). Current schema/metadata unchanged. C004 re-inventory via
trail grep AppError::WorkflowRole in application/workflow finds only existing sample-role producer,
message-admission classifier and role test; no new pre-PreparedAdmission role producer introduced.

CURRENT RUNNING: session9889, B1-test-build20.txt, locked/offline --lib --no-run for final3 native
fixture additions. Source stable. Static boundary/fixture/format/whitespace3/2 checks running in a
separate serialized shell job (record its session from tools if needed). No reviewer/test/SQLx worker.
When20 finishes, run `python3 .../logs/B1-resume-rerun-failures.py 2` (the runner now accepts a
round number; retains round1 unchanged, records inventory9 and tests/results2). It requires all76
names collected and maps only the two intentional onboarding/public-system relay renames. Then
exercise helper-dependent native/runtime and production behavior cohorts, final current offline/
static/collection evidence, whole-B1 manifest/digest and independent Astra review. Coordination19,
notices, resources, six exclusions, user authorizations and downstream p04/Plans04/05 obligations
unchanged. No accepted-in-phase or main-push claim.

## B1 whole-phase review preparation — recovered build26 and completed corrections

2026-11-15: source was already beyond the prior HANDOFF (build26). Recovered retained later
checks before making any project edits; no production/source edits were needed on this resume.
Build26 includes the explicit pure-workflow/system relay denial regression (1PASS); build20's
original76 rerun now executes current4604-case binary and passes all76 in23.38s. Prior failures
remain retained. The later571-case affected cohort was566PASS/2FAIL/3 existing ignores; its two
raw-agent rename fixtures are corrected through the actual authoring writer, preserving saved
capabilities. The subsequent78-case rename/ingress/notice/default-reply cohort passed all78.
No skipped, removed or weakened failing assertions. B1-R1-test-removal-map.json accounts for all
26 removals from4589 accepted p02 names and all41 new collected names (current4604).

Final format, whitespace, locked/offline all-target compile, transport boundary and boundary
fixtures PASS (B1-R1-final-checks.json). SQLx prepare3 PASS; source_modules1 8PASS. Current schema
SHA384 matches retained native ownership/pin proof and the actual migrated author DB exactly
(B1-R1-schema-identity.json). Strict clippy retains7 demonstrated unchanged inherited diagnostics,
0 introduced (B1-resume-clippy-baseline-disposition1.json). Their correction/disposition and the
previous CSS/CSP failures remain integrated p04 obligations, never a full-green assertion.

Whole-B1 delta: B1-R1-author-delta.json/.patch,134 source/deliverable paths against the immutable
accepted p02 sparse export plus HEAD bytes for omissions. Exact exclusions stay unchanged. Active
coordination19 pinned as B1-R1-coordination.md; I001–I004/C001–C007/r1/revision10 unchanged and
acknowledged. B remains independent Astra/high /root/p02_reviewer. No phase acceptance or push.

### Current criterion-to-assertion and consumer disposition

| Criterion | Exercised production path and decisive assertion | Retained evidence | Status |
| --- | --- | --- | --- |
| AC1 creation | actual regular/library-copy/onboarding/Builder creation provisions canonical principal address + published self workflow + active principal binding, with zero automatic desks/commands; exact default direct ingress/worker produces a real answer; conflict/competing creation stays atomic | final78 and B1-R1-production-tests; original76 round2 covers onboarding/API regressions | PASS when final production cohort completes |
| AC2 deletion | unused agent removes owned workflow/binding/address; exact external owned-version reference retains all13 inventories; direct-history ingress-first and retirement-first competing races preserve/refuse atomically; native principal pins prevent history erasure and company teardown completes | B1-R1-production-tests, B1-agent-delete-reference-and-race-tests1, B1-native-lifecycle-tests1 and retained lifecycle/native schema proofs | PASS when final production cohort completes |
| AC3 tools/directory | native Builder actual dispatch/reviewed desk approval/edit/reject retains department placement, direct binding and idempotency; real complete builtin tool grants; directory/settings/onboarding show direct principal identity; no implicit desk fields | final affected571/final78, original76 round2, B1-R1-production-tests | PASS when final production cohort completes |
| AC4 sender | actual frozen message__reply→DeliveryWorker→EmailSender→canonical commit uses exact authenticated agent/grant; member positive, nonmember and other-agent From negatives leave no canonical/SMTP effects; pure workflow/system negative reaches restricted desk refusal | B1-R1-production-tests includes all relay_identity_ journeys and pure-system denial; final78 retains actual origin/native checks | PASS when final production cohort completes |
| AC5 private notices | actual live projector/current captured principal route creates exact private message/intent and one handling root; native9-field claim mutations rollback; account/company/role NOWAIT both-order/three-party/lease/origin/retention/poison cases exercised | B1-R1-production-tests, final78 and original76 round2 | PASS when final production cohort completes |
| AC6 native/regression | fresh migration/schema pins; SQLx macros and real runtime assertions; source module reachability; no silently lost tests; format/offline/boundary checks | B1-R1-final-checks.json, schema-identity.json, test-removal-map.json; SQLx prepare3, source_modules1 | PASS in affected scope; inherited clippy FAIL retained for p04 |

Native unpublished authoring/storage fixtures intentionally use register_native_agent_address;
that helper proves namespace/email-binding readiness only, not production creation/default behavior.
Production agent-address/default/relay/notice/public-private checks continue through real
CompanyAgentService. Role-refusal C004 inventory adds no production pre-PreparedAdmission producer.
No account fallback, principal-retirement compatibility path, cascade or guard bypass added.

## B1-R1 frozen and ready for independent review

Final current92 production/relay/notice/public-private/creation/deletion/builder checks **92PASS**,
72.15s. All six mapped criteria now PASS within affected scope, with inherited strict clippy
FAIL separately retained as the integrated-p04 obligation; historical failed logs do not become
passes. Frozen artifact `63dc835d92424c643cafcea862aa1afd127e3d121f3f5c9e2de2a14c9a48f866`; base/start/HEAD830561e2; exact six exclusions above unchanged.
Review evidence `logs/03PA-P03-B1-R1.evidence.json`;134-path delta and removed26/replacement map
retained. Coordination19 and active notices checked unchanged against immutable B1-R1 pin.
Entire implementation worktree is frozen for whole-B1 review. No source edits until B returns;
workflow-only records may continue. No phase acceptance, final integration or main push yet.

## B1-R1 review status — D2 proof gaps identified, full outcome pending

B's independent production cohort80PASS plus13PASS corrected setup; its earlier12 failures were
reviewer schema-without-SQLx-ledger setup failures before behavior, corrected using actual migration.
B asked for D2-required populated direct notes/history company teardown and foreign-target UPDATE
commit refusal. Author checked native lifecycle tests/probes and caller inventory and found no
retained proof of those exact obligations. Existing INSERT and retained-principal DELETE negatives,
company ingress races and separate direct notes/history tests do not establish them. Therefore
that part of AC2/AC6 is **NOT EXERCISED**, superseding any broader mapping claim above; no phase
acceptance. Round1 manifest remains immutable and records checks actually run, not comprehensive
coverage. B is continuing whole134-path review; wait for its complete finding set before source
corrections. Current frozen source/digest and coordination19 unchanged; no source edits.

## Pending whole-round correction queue (source still frozen)

B additionally requested the already-gated D1 §§4/7 proof of a forged workflow message author
reaching native workflow_message_author_guard, post-observation relay source/target authority
change with receiving-commit/redelivery reauthorization, and actual frozen agent delivery into a
private agent principal followed by ordinary admission. Existing frozen sender trio, static pure
system refusal, origin immutability tests and desk-receiving two-agent exchange do not prove these
exact cases. Author found no retained coverage and explicitly marked them unexercised; B will
include them in the complete round outcome rather than authoring implementation or opening a
separate review round. No source edits while whole-B1 review continues.

After full result releases the freeze, correct the complete finding set in one batch. Expected
proof can reuse actual production default-agent runtime for populated history+notes teardown,
native direct thread FK for foreign-target UPDATE commit refusal, and real queued source replies
for native-author fault/refusal and deterministic target/source revocation barriers plus ordinary
private-agent delivery/admission. Tests must reach the named native/commit boundaries and assert
rollback/no recipient effects; mocks, direct predicate calls and separate fixture existence do not
substitute for the promised paths. Any newly demonstrated contract defect requires appropriate
source correction and affected checks; no existing assertion/guard or bound may be weakened.

## B1-R1 complete — F1/F2 correction batch started

Independent B returned CHANGES REQUIRED, note03PA-P03-B1-R1/N1, two MEDIUM findings F1/F2:
required populated lifecycle/foreign UPDATE evidence and native forgery/post-observation/ordinary
private-agent relay proof.99 distinct independent tests PASS; no additional runtime defect
established. Full notes/findings and original artifact identity saved; no acceptance. Freeze released
only after complete round. Author is correcting both in one batch under unchanged accepted gates
and coordination19. Current edits are test-only: real default direct runtime helper+populated
notes/history teardown with exact surviving-tenant inventory, native FK UPDATE commit refusal,
actual pure-workflow publisher fault injection reaching named native author guard, typed relay
fixture recipient reuse, ordinary private-agent admission and advisory-gated source/target real
writer races across both first delivery and redelivery. No guard/schema/bound changes planned.
Locked offline lib no-run build1 pending; preserve failed diagnostic evidence and correct all issues
before meaningful focused runtime checks, SQLx/source/format/boundary proof and one combined F1/F2
recheck. No p03/p04/main integration acceptance or push yet.

## Latest correction checkpoint — build4 PASS; final100 production cohort running

R1 CHANGES REQUIRED03PA-P03-B1-R1/N1, F1/F2 still B-owned OPEN; no production defect established.
Correction delta is8 test/helper/module-registration paths vs verified reconstructed R1 baseline
at /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-b1-r1-ftki0yks. R1 reconstruction identity
in logs/B1-R1-reconstructed-baseline.json (HEAD+p02 export+retained R1 patch,134 hashes checked).
Draft R2 correction json/patch need regeneration after final code changes; do not review stale hash.

Build1 FAIL fixture API/type calls, retained/corrected. Build2 PASS3m19s; initial13 cases10PASS/3FAIL:
UPDATE reached earlier membership FK; private-agent proof query misspelled handling-roots table;
final-publisher forgery reached earlier linkage guard. Fixed fixtures, not guards. Bare valid native
thread now isolates deferred target UPDATE; private-agent actual delivery/admission passes.
Build3 PASS1m15s with an unused import (now removed); focus2 **12PASS/1FAIL22.31s**. Comparable second
company now runs its own actual production-created default-private agent, transport/history/notes/
delivered outbox on the SAME DB; normal delete leaves its full16-table snapshots unchanged.
Both source-rename and target-settings writer races PASS for first delivery and accepted redelivery.

Last failure was fixture classification: generic raw SQL publication clone did not reserve a
workflow producer, and the direction-specific fault was not applicable to the in-app payload.
Native test now explicitly reserves the exact live workflow producer before INSERT (matching real
writer), unconditionally corrupts only the actual new author, and asserts forged agent+role+seal
inside TX before commit. All native guards remain enabled. Native explicit sender-forgery test3
**1PASS1.74s**, exact SQLx constraint workflow_message_author_guard, full12-table effect counts
rollback, same classified producer positive control commits after fault removal. No runtime/source
schema guard change. Original failed fixtures/logs retained, not production bypass evidence.

Build4 PASS1m20s (only existing macOS unwind linker warning);4610 current collected cases. Source
rename/target races now also snapshot participant identities, participants and email metadata.
RUNNING session8691 logs/B1-R2-production-tests.txt, current100-case cohort covering all previous92
plus new sender/native/lifecycle proofs and original native target insert/delete negatives. No
compiler/SQLx/reviewer active. Source stable, current database/schema unchanged. Next collect100,
resolve real failures, final format/whitespace/transport boundaries and fixtures; diagnostic clippy
all-targets without -D must enumerate cfg(test) lints beyond the7 inherited strict failures; do not
claim strict green. SQLx prepare, offline all-target/source_modules, graph refresh; map intermediate
native test move against retained B1-R2-before-native-move-tests and current inventory, retain all26
original removals/replacements. Regenerate8-path delta, immutable R2 manifest/digest/coordination19
pin; one combined F1/F2 Astra correction recheck. Then accept p03 only explicit B PASS, p04 docs/
main merge/full CI+temporary branch Docker verification/main push, then Plan04 then Plan05.

## B1-R2 correction freeze — ready for one combined independent recheck

Artifact **3247c1f394f32bb86dea1a31f3f1856c0f805ed0c68329149217f9c72fa9230d**, start/base/HEAD830561e2; exact six exclusions unchanged.
Coordination19 live bytes match both immutable pins; all active notices unchanged and acknowledged.
Complete correction delta B1-R2-author-correction.json/.patch:8 test/helper/registration files,781
lines, no production/schema/guard/bound changes. R1 source reconstruction verifies134 hashes
and provides exact before bytes; source frozen for B's combined F1/F2 recheck.

| Finding/criterion | Actual decisive proof | Evidence | Current author status |
| --- | --- | --- | --- |
| F1 / AC2,AC6 | Two production-created private agents in the SAME DB each perform actual direct email→run→reply/delivered outbox;16 descendant classes explicitly nonempty, direct sessions/conversations/notes pin exact principal; normal company delete removes all selected rows and comparable surviving tenant full rows unchanged | direct_company_teardown_removes_populated_agent_history_notes_and_transport, production100 and full-native-guard3 | PASS; B disposition pending |
| F1 / native UPDATE | Valid bare native thread avoids unrelated participant-child FK; foreign-principal UPDATE succeeds as a statement, COMMIT fails exact threads_target_principal_fk; complete both-tenant state unchanged | direct_target_native_foreign_principal_update_cannot_commit, production100 | PASS; B pending |
| F2 / native author | Actual MessageRuntime creates source; complete SQL-only clone reserves explicit same-run producer before INSERT; forged actual agent author/role+seal asserted in TX; COMMIT fails exact workflow_message_author_guard,12 effect counts rollback; same classified producer succeeds without corruption | relay_identity_native_forged_workflow_agent_author_rolls_back_publication, full-native-guard3 | PASS; B pending |
| F2 / revocation | Real source rename writer and target settings writer cross a proven receive-commit advisory gate after proof/ACL observation; both new delivery and accepted redelivery refuse changed authority/current replay; canonical/provider/thread/intent/identity/metadata full rows unchanged | two relay_identity_*after_observation* cases, production100 | PASS; B pending |
| F2 / ordinary principal | Actual frozen source→DeliveryWorker→EmailSender→production private agent principal; exact agent author/sender, zero platform receipt, actual intent claim/admission at selected receiver workflow with receiver agent manifest, no handling root | relay_identity_actual_agent_to_private_agent_admits_ordinary_work, production100 | PASS; B pending |
| AC6 / collection+static |4610 collected; original26 removals all mapped,47 additions; intermediate new native test moved with exact before/after inventory; source_modules8PASS; offline/SQLx/format/whitespace/boundary/fixtures/graph PASS | final-checks, maps, manifest | PASS affected scope |

Current production100 **100PASS80.09s**. A final audit noticed the shared legacy fixture disables
run immutability for corruption tests; new direct-runtime/native-author cases explicitly restore
it, then those exact3 cases **3PASS14.67s** (build5 PASS1m39s). No sender/producer/native guard was
disabled in the forgery probe. Earlier canonical-linkage/native-clone classification failures were
fixture failures, retained honestly; they are not proven production bypasses.

Final SQLx prepare1m45s PASS, unchanged.sqlx; offline/source_modules8PASS/format/whitespace/
transport boundary+fixtures/trail build allPASS. Diagnostic clippy fullcfg(test) enumerates only7
unchanged inherited warnings; strict lint remains FAIL and p04 obligation. Exact commands/results
retained in03PA-P03-B1-R2.evidence.json; no full-plan/full-CI claim. No tests/workers/compiler active
after final check job; B may own independent resources next. Review must resolve both saved F1/F2
against this exact digest before p03 acceptance. Plan03 p04/main integration/full CI/push and
Plans04/05 remain pending, already authorized.

## p03 accepted in phase — 2026-11-15

Root independently matched3247c1f3…a9230d with exact six exclusions, validated R2 manifest and
checked saved03PA-P03-B1-R2/N1 PASS/both F1/F2 RESOLVED. No unresolved acceptance-blocking
p03 defect. Coordination19 unchanged at acceptance; writer published revision20 acceptance with
unchanged contract notices. Complete material producer export p03-accepted-source.json/.tar.gz
is retained for integration comparison. Root may commit the scoped Plan03 implementation/evidence
under existing authorization. B completed all review work and cleaned its fixtures; no outstanding
phase worker/process. This is accepted-in-phase only, not integrated/full-plan completion. p04 now
owns docs/main merge/full suite and all inherited lint/CSS/CSP gates before normal main push.

## Reviewed producer commit preparation

Scoped source/evidence staging includes no unrelated .claude/.mcp/AGENTS/.trail or Plan02 record
changes. All77 available manifests validate --require-tracked. Raw retained terminal logs/unified
patches intentionally contain original whitespace (185 record paths); full staged diagnostic is
retained, not rewritten to invalidate hashes. Staged source and record Markdown whitespace PASS
with only raw docs/logs excluded. No source check/CI configuration/attribute changed. Material
export413 files, archive6891f662b5c05529e4e569b2a35d4be3870b82497640870ae99f523e5a08cacb,
cache pa03-accepted-u6pffgve. Known staging/commit representation changes are checked by exact
export-to-index/source hashes; historical accepted digest remains the pre-staging reviewed identity.
