# CHS p01 — Contracts and response ownership

## Identity and authorization

- Request: implement all of `plan/03-channel-handles-send` using `implement-astra-reviewed-2-2`.
- Original requirements: `../../../01-contracts-and-response-ownership.md`; dependent contracts and final gate in original phases 02–07.
- Phase: p01; batch CHS-P01-B1 (whole phase).
- Root/author A: `/root`, user-confirmed `gpt-6.1-sol` / `xhigh`; runtime metadata otherwise unexposed.
- Reviewer B: `/root/channel_reply_reviewer`, requested `gpt-6-astra` / `high`, no inherited conversation; actual runtime settings unexposed. Reuse this handle; reviewer owns only assigned phase review records/logs.
- Worktree: `/Users/mac02/dev/openbot`; branch `main`.
- Starting commit / initial review base: `769a23aa` (resolve full identity in snapshot evidence).
- Pre-existing changes: none; clean at startup. Preserve all committed autonomous-agent and knowledge work.
- Record root: `/Users/mac02/dev/openbot/plan/03-channel-handles-send/phases/p01/docs`.
- Coordination authority: `/Users/mac02/dev/openbot/plan/03-channel-handles-send/COORDINATION.md`, writer `/root`, revision 2 checked. CHS-I001–I003/r1, I004/r2, I005–I006/r1 acknowledged. User explicitly changed verification to targeted tests only and queued plan04 after plan03 acceptance.
- Host context capacity, occupancy, and compaction threshold unavailable. Use host defaults and durable checkpoints; no global configuration edits.
- Commit/push/deployment not authorized. Implementation, disposable verification resources, and independent review authorized.

## Status

Implementation underway. Root verified unchanged initial source digest and accepted architecture CHS-P01-D1/r2 after explicit B PASS CHS-P01-AG1-R2. Architecture findings F1/F2 resolved in decision; runtime fixes still require batch review. No phase acceptance or integration claimed.

## Scope and acceptance

Implement bounded typed output projection, explicitly selected active message responder, immutable ownership at accepted-message admission/run creation, tenant-qualified persistence and concurrency control. Preserve observer workflows, reply-first correlation, quiet ingress, samples, and current authority. Missing/invalid content fails; suppression remains a future typed terminal disposition, never inferred from business output.

Trace binding configuration/lifecycle, ingress commit, message intent snapshots, admission/run creation, binding fixtures, output schema facilities and all affected callers before edits. Gate consequential schema/ownership/admission decisions with B before dependent implementation.

## Batches and checks

1. CHS-P01-B1: original batches 01A + 01B together. Focused unit prefix `channel_reply_contract_`; database prefix `channel_reply_binding_`; affected binding/schema/admission/quiet/duplicate/sample regressions. List filters and require nonzero intended tests before execution.
2. For SQL changes: fresh migrations, SQLx prepare/check, offline all-target compile; formatting and diff checks. Record exact commands, counts, fixture identity, output paths and failures.
3. Final combined gate after p01–p07: focused affected behavior/interface checks selected from original phase07/current CI inventory. User explicitly authorized targeted tests only: no full database suite, stock-stack full suite or full lease suite; do not claim these ran. Plan04 follows accepted plan03.

## Decisions

### CHS-P01-D1 / r1 — superseded; original preserved

1. **Contract.** Introduce a cohesive application-owned `channel_reply` module and persistence port. A versioned, bounded contract contains only subject/body value bindings: text literals or references into the workflow's final business result. Reuse domain `Binding`/`ContextReference` evaluation, canonical-content parsing and existing compiler schema facilities. Configuration supports explicit simple string projections through required object properties (and literals); reject unsupported/ambiguous schema paths, optional paths and non-string values. Runtime schema/content/budget validation is mandatory. Result fields cannot carry destinations, send authority or suppression.
2. **Selection.** Add one tenant-qualified `channel_reply_settings` row per channel with optimistic revision, optional designated binding, its immutable configuration revision, and contract. Selection is explicit and requires ManageBinding authority, a current active channel-scoped (not thread-scoped) message-triggered binding, selectable bundle, current readiness, compatible output schema, and exact expected selection/binding revisions. Retain a row when clearing selection, so ABA/stale forms cannot become initial writes again. Changing the binding head does not silently change the pinned responder revision/contract. Selection stores no credentials. Validate again inside the authorized write transaction using existing company/channel lock ordering; two competing updates with the same revision yield one success.
3. **Accepted-message snapshot.** In the ingress transaction, capture the selected binding/configuration revision and contract into `channel_reply_contexts`, keyed by the existing `(company_id,intent_id)` association. A database insertion trigger on message intents closes all ingress entrypoints, including in-app posting and direct persistence paths. Serialize ingress capture and settings changes on the channel row so an absent selection is also a stable observation. Quiet ingress captures no response context. Store source/channel/thread/actor/policy through existing tenant-qualified immutable intent/configuration references, not client-supplied result data. No extra workflow engine or parallel delivery system.
4. **Admission.** Extend the existing candidate and admission-binding selection to read the accepted responder's immutable revision (ordinary observer bindings retain existing semantics). Shared admission verification checks the exact frozen responder selection under its real Message claim, current binding activity, selectable workflow and existing authorization/readiness. In the same run-creation transaction, create a unique tenant-qualified `channel_reply_runs` link from context to the designated message-triggered root run. A consumed human reply, trigger mismatch, quiet ingress, observer/manual/scheduled/child/sample run gets no link. Deactivation or authority refusal remains visible in existing admission outcome records; never redirect to the newer responder. Replays reuse the same link/run.
5. **Schema/lifecycle.** Edit the sole init migration, use fresh databases, and enforce tenant/channel/configuration/intent/run association with composite FKs plus narrow immutable/validation triggers where FK keys cannot express all facts. Ownership and frozen contract/configuration never update. Use existing retention/lifecycle constraints rather than deleting live obligations; clearing settings changes future ingress only. No backfills, shims or legacy decoders.
6. **Later producers.** p02 adds typed run-local terminal disposition; p03 atomically creates one durable final decision at shared completion and parks exact protected final content through existing approvals; p04 publishes via current canonical message/delivery operations under an intent claim, not a fake workflow fence. Those are later gated phases, not implemented or accepted in p01.

**Invariants/failures:** one current selected responder and one run link per accepted context; ordinary observer admission preserved; frozen configuration/contract survive later selection edits; current revocation can refuse execution/publication; invalid projection fails before success; sample/quiet data cannot acquire production response authority. No context/link for reply-consumed or non-message roots. Transaction rollback leaves no partial association. Database failures propagate rather than becoming authorization decisions.

**Affected contracts:** binding revision reader, candidate query, admission binding resolver/current verifier, run creation, ingress intent insertion; new cohesive selection port and immutable response context tables. Existing binding request structs remain unchanged; p06 exposes selection through a purpose-specific endpoint/form rather than adding optional correctness defaults to broad traits.

**Verification:** `channel_reply_contract_` tests for literal/reference projection, schema/path support, escaped pointer segments, bounds/blank/sanitized content, optional/ambiguous schemas, forged result/destination fields; `channel_reply_binding_` DB tests for roundtrip/reconfiguration/clear CAS, competing assignments, tenant/thread/source rejection, snapshot before selection/head edit, trigger mismatch, quiet/sample/manual/child exclusion, reply-first correlation, replay and rollback. Run affected existing binding/admission/message-intent tests with isolated resources, fresh migration and SQLx/offline checks. Reviewer must explicitly pass this decision revision before consequential edits. No implementation yet.

### CHS-P01-D1 / r2 — correction of AG1-F1 and AG1-F2

Retain D1/r1 except for the following explicit replacements. Original r1 remains above and immutable in logs/CHS-P01-D1-r1.md.

**F1 / frozen facts and lifecycle:** existing message intents are not immutable. A context stores immutable copies of company, intent, original message/channel/thread, initiating principal, origin/admissibility, delivery policy, relay facts/correlation/acceptance time, selected binding/configuration revision, run-as actor and contract revision/content. Context INSERT validation derives/checks these facts against trusted intent/configuration rows; an UPDATE guard prohibits changing any snapshot fact. For an intent that has a snapshot, a narrow intent UPDATE guard also rejects changes to its source association, origin/admissibility, role/mention/reply source, relay/correlation/policy and acceptance time; ordinary lease/queue/outcome/reply-settled columns remain mutable. `responder_principal_id` on the intent may still become NULL through its existing principal-deletion behavior only when no retained snapshot references the principal; the snapshot principal is tenant-qualified with a deferred NO ACTION FK, so retained snapshots prevent principal deletion until their source retention cleanup. No copy follows a changed/current source principal. Context/source identity uses tenant-qualified associations and immutable configuration references. Contexts cascade with their source intent/message lifecycle; a run-linked context cannot be pruned independently while its run link exists. Company teardown cascades the scoped settings/contexts/links together using deferred association constraints. Ordinary source/channel/thread deletion while a linked obligation exists is blocked by those links, preserving live obligations/audit; cleanup must remove eligible run links/history through the existing retention owner before deleting the source. Passive snapshots may be pruned with their source, not on a new unbounded-history clock. Verify allowed full-company teardown and blocked partial deletion/mutation with rollback-scoped DB tests.

**F2 / cardinality and authority:** an ingress `channel_reply_contexts` row is only a passive eligibility/configuration snapshot, never an obligation and never claimable publication work. It may have ZERO or ONE designated root run link. Only creation of `channel_reply_runs` under the authentic message admission claim in the same transaction as run creation establishes the automatic-response obligation. A reply consumed by a wait retains at most its passive snapshot and has NO run link/final response decision. Trigger mismatch, refusal or deferred correlation likewise creates no run link (deferral may later resolve normally). Unmatched ordinary ingress may admit one designated run and observers; only the designated run has a link. Later finalization/publication enumerate run-linked committed final decisions, never passive snapshots. Replace r1's contradictory 'one run link per accepted context' with 'at most one link per snapshot; every admitted designated responder has exactly one link'; replace 'No context/link for reply-consumed' with 'no response authority/run link for reply-consumed'. Test persisted zero/one-link shapes for matched, deferred, unmatched, mismatch and refused cases.

No implementation yet. Request B's single correction recheck for both findings against D1/r2.

## Artifact and resources

- Current source base/HEAD: `769a23aa6ef423e964f8221a8ee6b0743325c6ba`; architecture snapshot digest `4868f6c61a7b3620e19ecddffc2d982d1f51f95c1d206e6fb7f39b8ac3b5b707` (no implementation changes).
- Exact nine snapshot exclusions: `plan/03-channel-handles-send/COORDINATION.md`, each `plan/03-channel-handles-send/phases/p01/docs` through `p07/docs`, and `plan/03-channel-handles-send/integration/final/docs`; recorded in `logs/CHS-P01-AG1-manifest.json`. Never exclude deliverable plan files, source or generated assets.
- Resources: owned temp root `/private/tmp/openbot-chs-xb34mvy9`, PostgreSQL18.6 data path `pgdata`, allocated loopback port65037, database names `openbot_chs` and `openbot_chs_test`; not started at allocation. Receipt `logs/resources.json`. System cargo/node and existing caches available. No `.env` at startup. Earlier failed sandbox port allocation left only an empty `/private/tmp/openbot-chs-1_qhqlxc` directory; no server/database receipt/process was created.
- Next action: inspect existing transaction and immutable binding/admission snapshot owners; write D1, establish isolated database, request B's architecture review.

## Evidence and results

- Read complete selected skill, coordination/context/review references, root/source/application/infra guides and all original plan requirements.
- `git status --short --branch`: clean `main...origin/main` at startup.
- Graft graph available; initial binding/admission/ingress discovery completed. No test run yet.
- B `/root/channel_reply_reviewer` completed architecture round CHS-P01-AG1-R1, changes required for F1/F2, durable records saved. Root corrected both together in D1/r2. No implementation started before gate pass. User interrupted the sandbox escalation; resumed with unrestricted tools, preserving scope/roles.
- Snapshot script initially failed because macOS realpath lacks `-m`; a phase-local verification-only realpath adapter in logs/tools lets the unchanged installed script run. Original script identity preserved; PATH adapter is shared with B.

### Implementation verification checkpoint

- p01 contract, selection CAS, frozen ingress contexts/source guards, admission pinning/linking and 23 focused tests written; not yet accepted. Source files remain in working tree; no commits.
- `author-fresh-migrate-01`: PASS, PostgreSQL18.6 fresh sole init migration 0.832s. Owned server settings checked: pg_stat_statements top/utility off, parameter logging0, checksums on.
- `author-compile-01`: failed 8 initial type-wiring errors (private import, non-serializable revision, missing SourceSpan defaults); corrected in source without changing shared domain serialization.
- `author-compile-02`: PASS, offline all-target compile108.714s, before the final link-replay and integrity-test additions; repeat affected compilation after tests.
- `author-channel-reply-list-01`: active targeted test binary build; list and execution counts pending. No runtime pass claimed.
- `git diff --check`: PASS at checkpoint. First fmt check reported expected formatting differences; cargo fmt applied afterward.
- Failed baseline-migrate runner cwd attempt retained as logs/baseline-migrate.{json,log}; corrected runner and baseline-migrate-02 PASS before schema changes.
- All exact commands/environments/status/times retained in logs/run-check receipts.

### Author audit / test build continuation

- Added actual competing ingress claimants test in addition to competing settings assignments; new focused inventory now expected24 (7 contract +17 DB). Added explicit manual-root, thread-scoped selection, snapshot rollback and observer-link forgery cases. Counts remain pending executable inventory.
- First targeted list build failed solely on missing CorrelationId fixture import; fixed. Second build was deliberately stopped with SIGTERM only for its verified owned cargo/rustc descendants to batch a claim-page fixture signature correction, preserving caches and original receipts. Third list build is active (session39866). No other process was stopped.
- Shared binding reader trace includes lifecycle writes, schedules, provisioning and onboarding; include focused regressions for these interfaces after new tests. Same existing-head read semantics retained when no frozen revision is selected.
- Pending audit check: confirm company cascade order against new immutable-intent guard. If principal SET NULL runs before snapshot cascade, guard must allow teardown only after owning company row has gone, while continuing to reject mutation for live companies. Do not weaken live-source immutability.

### Focused runtime results and final author corrections

- Inventory author-channel-reply-list-03:24 intended tests. First run author-channel-reply-tests-01:17PASS/7FAIL/0ignored. Six failures caused by new guard using nonexistent workflow_run_admissions instead of actual workflow_admissions; one fixture used invalid ordinary membership instead of member. Both corrected; server evidence retained in author-postgres-failure-01.log.
- author-channel-reply-tests-02:24PASS/0FAIL/0ignored8.59s under network guard, including linked-run company teardown, real competing selection/ingress claimants, immutable source facts, pinned old config and reply correlation.
- Final author audit added explicit numeric format_version=1 to the new bounded contract (DB checks and unsupported/missing-version test; no legacy reads), yielding25 targeted tests. Also enforced D1/r2 deletion semantics: snapshots only leave with source retention; run ownership only leaves with run retention or full-company teardown. Updated retention and forged-owner tests preserve assertions and check exact rejection23514. New settings reads/writes have5s lock/20s statement timeout.
- author-channel-reply-list-05 deliberately stopped only its owned cargo/rustc processes to combine final audit changes; no pass claimed for this interrupted build. Next list/run must verify the final25-test artifact. No more optional source changes before verification/review.

### Final artifact runtime checks and baseline audit

- Candidate source digest eec077d537f975767e3d1d0deed360b88a76b40730a910c85caad126bc6fc4a7,16 implementation files; exact nine exclusions unchanged. Architecture remains D1/r2; deletion guards implement its previously specified retention semantics, not a new contract.
- Final new prefixes: channel_reply_contract_8 selected/8PASS0FAIL0ignored; channel_reply_binding_17selected/17PASS0FAIL0ignored, network guarded,2MiB test threads. Earlier24PASS not counted as final artifact coverage for changed version/deletion delta.
- Existing focused regressions: message_intent_14PASS; message_reply_13PASS; workflow_binding_sql_7PASS; workflow_admission_binding_4PASS; schedule binding1PASS; schedule reconfiguration1PASS; builder provisioning1PASS; binding trigger2PASS; binding actor1PASS. Commands/inventories/outputs in regression-*-list/tests-01 receipts.
- Onboarding module13selected/11PASS2FAIL. Failure1 at onboarding/tests.rs343 attempts deletion of a published agent (existing protected-lifetime conflict). Failure2 at787 is existing Workflow resource refusal after ownership transfer. No source changes made there. Relationship unconfirmed until clean HEAD comparison. Pipeline stopped on this failure; remaining two binding filters run separately.
- Isolated detached clean baseline worktree /private/tmp/openbot-chs-baseline-6ra2cluo/worktree at original HEAD; COW target /private/tmp/openbot-chs-baseline-6ra2cluo/target; separate local DB pair openbot_chs_baseline(_test), same owned18.6 cluster. Active runner99490 tests exactly both observed failing cases under identical network/stack settings. Resource receipt baseline-resources.json. Never stash/reset primary changes.
- Fresh sole init migrations for main and test DB PASS after final version/deletion guards (author-fresh-migrate-04/fresh-test-migrate-04). SQLx prepare active83038, graph refresh02 active59557; SQLx check/offlinecompile/Clippy/fmt/diff and code review still pending.

### CHS-P01-B1 / R1 — frozen whole-phase code review

- Entire16-file implementation frozen at digest eec077d537f975767e3d1d0deed360b88a76b40730a910c85caad126bc6fc4a7, base/start/review-base/HEAD769a23aa6ef423e964f8221a8ee6b0743325c6ba. Manifest/snapshot CHS-P01-B1-R1-* and immutable CHS-P01-B1-coordination-r2.md. Exact nine exclusions unchanged. No preserved pre-existing dirty changes.
- Confirmed baseline comparison: both onboarding failures reproduce individually at clean detached original HEAD, same errors at lines343 and787, same ownedPG18.6/networkguard/2MiB stack. Receipts baseline-onboarding-01/02; baseline source/worktree unchanged. These are confirmed inherited failures, not fixed or hidden. Current primary selections82 tests:80PASS/2inheritedFAIL; no ignored. No full-suite claim.
- SQLx prepare PASS74.981s; .sqlx has no source/cache diff. Graft build02 PASS. SQLx check72558 active; final offlinecompile/Clippy/fmt/diff pending. Only read-only checks and excluded workflow-record writes permitted while B reviews.
- Request independent B whole-phase review against original phase01 requirements, applicable coordination and D1/r2. Review all16 files including7 untracked implementation files and full original-HEAD delta; architecture pass does not substitute for code review. Root acceptance waits for explicit current-digest PASS and required affected checks.

### Author final check receipt

- Source still frozen at eec077d537f975767e3d1d0deed360b88a76b40730a910c85caad126bc6fc4a7 after all checks; author-final snapshot and CHS-P01-B1-author-checks.json retained.
- SQLx check PASS70.457s; offline all-target Clippy-Dwarnings PASS80.391s with SQLX_OFFLINE=true and --locked --offline. This current-artifact all-target compiler invocation covers the offline compile gate; do not invent a separate final cargo-check run. Earlier standalone check02 was an intermediate source.
- Final cargo fmt --all -- --check PASS2.338s; git diff --check PASS. Graft build02 PASS. No .sqlx byte changes. Both original-HEAD onboarding failures confirmed inherited with exact matching errors and clean baseline status; leave their source/tests unchanged.
- Current primary scope82 selected:80PASS/2confirmed inheritedFAIL/0ignored; no full suite run. B whole-phase review active; no author source edits during review. Acceptance pending explicit reviewer decision/durable records and root freshness check.

### Reviewer sandbox health checkpoint

- B had completed inspection of all16 files/callers and baseline evidence but its guarded inventory tool was blocked on an escalation approval in its original spawned-agent sandbox. Root current host is unrestricted/approval-never; the worker retained the earlier sandbox, so the permission change did not propagate.
- After ~17min without a status response, root interrupted only the current turn and resumed the SAME B handle/context/model/role to obtain status; no replacement or self-review substituted. No source changes. B immediately disclosed the pending call; retry without escalation also failed at nested sandbox-exec (reviewer-channel-reply-list-03 receipt).
- No further escalation or guard bypass. B continues independent actual-source/constraint and author-evidence review, explicitly recording its unavailable independent runtime invocation. Required affected author checks did execute. Root can mechanically execute an exact reviewer-chosen guarded diagnostic if a concrete concern requires it, without changing roles or authoring review findings. Review still pending actual decision.
- Additional author sample capability boundary scan PASS and its4 negative/control tests PASS; receipts author-sample-boundary-01 and author-sample-boundary-tests-01. Rust scope remains82 selected80PASS/2confirmed inheritedFAIL.

### CHS-P01-D1 / r3 — proposed corrective amendment (not active)

Preserve r1/r2 and original reviewed artifact. B is finishing B1/R1 and has identified two supported gaps; no correction code has been edited.

1. Selection must recheck BOTH the selected run-as current company authority and its current channel association visibility inside the settings transaction, using the same existing association authorization owner as binding configuration/admission. Selecting through an owner cannot bless an invisible run-as actor. Existing CAS/readiness/pinning remain unchanged; refusal writes no selection/context. Add a private-channel regression for a current admin that has lost visibility, plus the permitted case.
2. An explicitly configured responder must retain a PASSIVE ingress snapshot even if its binding has since been deactivated. Remove current activity as a capture prerequisite; retain explicit non-null selection, pinned message trigger, tenant/source checks and admissible ingress. Active/current authority remains mandatory for admission and the owned-run link. The existing candidate path already includes a frozen responder regardless of current activity, so a matching inactive responder becomes a visible refusal and creates no run/link/effect. No selected responder, intentional clear, quiet ingress, trigger mismatch and reply-consumed/deferred cases retain their distinct zero-authority behavior; no fallback/redirect is introduced. Add inactive-BEFORE-ingress and explicit-clear tests (existing deactivation-AFTER-ingress test remains).
3. This changes no ownership/claim/transaction/retention model or payload schema; it corrects the r1 active-capture premise against the original requirement for visible expected-response admission refusal. If B confirms this is a consequential gate correction, obtain explicit D1/r3 PASS before dependent source edits. Code correction/recheck must cover both saved findings together, retain all prior evidence, and rerun only affected tests/checks.

### D1/r3 activation and correction authorization

B explicitly passed D1/r3 in CHS-P01-AG2-R1; source/coordination freshness checked unchanged against B1/R1 manifest. D1/r3 is now active as a corrective clarification of original requirements; retained proposed text/previous r1/r2 remain immutable. B1-F1/F2 remain open until code recheck. Root resumes authorship to correct both together, add two DB cases and run affected checks. No user permission, role or model change is needed.

### Combined B1-F1/F2 correction implemented

After explicit CHS-P01-AG2-R1 PASS for D1/r3 and root freshness check, added current run-as channel-association authorization inside settings selection and removed current binding activity from passive ingress capture. Admission retains current-activity/current-authority checks. Added two targeted DB tests: rejected/private visibility selection has no partial settings mutation and works only with valid view grant; deactivated-before-ingress selection captures passive context, records one refusal/no run, and explicit clear distinguishes no responder. Existing after-ingress revocation/HITL/quiet/mismatch coverage retained. Three implementation files changed relative to B1/R1.

Fresh owned main/test migrations PASS correction-fresh-migrate-01/test-01. Combined focused correction driver active session96504; logs/correction-checks-01.log and per-command receipts are authoritative. Expected19binding+14message-intent+1binding-actor tests, all listed before guarded execution; SQLx prepare/check, offline alltarget Clippy, fmt/diff follow. No unrelated changes/full suite; both B1 findings remain open pending reviewer recheck.

- Correction first runtime:19binding selected18PASS/1FAIL. The positive private-channel fixture had an unverified admin account; the existing association owner correctly requires verified email as well as a view grant. Fixed only that new local fixture by verifying its account BEFORE the denied/no-grant assertion, so the test isolates missing/revoked visibility; production authorization unchanged. Failure receipt correction-binding-tests-01 retained. Driver stopped; later checks not yet run.

- Correction attempt02 active session84753:19reply-binding PASS,14ingress-intent PASS,1run-as authority PASS, all nonzero inventories/guarded2MiB execution. First failed fixture attempt retained and corrected locally; no production rule weakened. SQLx prepare/check, offline Clippy and fmt/diff continue.
- Retained R1 exact16-file source archive CHS-P01-B1-R1-source.tar.gz, reconstructed only by inverse of the three known correction edits and verified against EVERY frozen R1 byte hash; archive receipt documents method/hash. R1-to-correction.diff retains the actual three-file delta for narrow recheck. No original reviewer evidence rewritten.

## Root acceptance — p01 complete

Accepted in phase at0995151ed93e67f2e5feee8092cdc08687580b79f1d988d40d5d072d5e39decf after explicit B PASS CHS-P01-B1-R2, both B1 findings fixed, D1/r3 architecture PASS AG2-R1, complete affected author checks and root recomputed source/coordination freshness. Receipt CHS-P01-root-acceptance.json. All code remains uncommitted in primary checkout. This accepts p01 only; p02–p07/final combined integration and plan04 remain required. The84 unique Rust selections comprise82PASS/2confirmed inheritedFAIL (no ignored), plus4sample guard tests; no full suite/deployment claim.
