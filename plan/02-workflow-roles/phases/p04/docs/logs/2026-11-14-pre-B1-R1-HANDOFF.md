# STOPPED — explicit user request, safe checkpoint117

The user requested a safe stop, a written handoff, and the working-directory/handoff paths. **Do not resume implementation until the user asks to continue.** This overrides earlier continue-until-complete instructions. No goal is active. No commit or push was made.

## Workspace and authority

- Working directory: `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`
- This handoff: `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot/plan/02-workflow-roles/phases/p04/docs/HANDOFF.md`
- Branch: `room/check-last-handoff-md-under-plan-02-work-qc8d`
- Current HEAD: `2bbe2649024aa6a4617b74b0c0c7868b14590b5f`
- Original p04 review base: `103004a8ac2fd643cf14bde7e09022ae79f72053`.
- Root/A: @thread:thr_c7pja4f7ef, user-confirmed Sol6.1/xhigh; workflow implement-astra-reviewed-2-3. Reusable independent Astra/high reviewer B: @thread:thr_9t82uwgsiu, idle, no review pending.
- Authoritative coordination: `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot/plan/02-workflow-roles/COORDINATION.md`, revision38, root sole writer. Acknowledged I001–4/r1,C001/r1,C002/r2,C003/r2,C004/r3,C005/r3,C006/r2,C007/r1,C008/r1,C009/r1,C010/r2.
- Phases1–3 accepted in phase. p04/B1 remains unaccepted. p05 remains queued; plan03 is later.
- User authorizes push **when the whole02 plan is complete**, after required review/integration/full gates. Do not push partial work. Before every eventual push, pull/rebase (or pull/no-rebase if unpushed merge commits); never force-push.

## Latest verified checkpoint

Build117 PASS3m26s. The final cleanup contention test PASS1/2.47s with the real notification worker, real message admission, real parked-child creation, observed token-FK blocking, actual cleanup skip/progress, release and awaited child commit. Exact source Rust/SQL+init digest before/after:

`ce403744604560f7dd2eb7a1fcbcdb9cf18f7aac06d1c085487ba467b698eeca`

Evidence: `logs/continue-origin-cleanup-native117.{json,txt}`; inventory1174469 unchanged. It corrects only the116 fixture's missing real notice admission; no production change after116. Do not repeat all19 absent new relevant changes: unchanged18PASS116 plus corrected1PASS117 covers that focused set. Full-suite and independent B1 acceptance are still outstanding.

Other retained current evidence:

- Fresh disposable resume18 and resume18_test migrations PASS: `logs/continue-migrations18.txt`.
- SQLx prepare116 alltargets locked/offline compilation PASS5m51s; generated query metadata unchanged: `logs/continue-sqlx-prepare116.txt`.
- Build116 PASS3m18s; focused11619selected=18PASS1FAIL64.79s. Sole failure occurred before contention because the test had not admitted its notice;117 fixes it through actual delivery/claim/process. All18 unaffected passes retained in `logs/continue-origin-cleanup-native116.{json,txt}`.
- Output1152PASS29.39s: genuine native MessageSend and actual automatic final publication preserve exact canonical producer/origin through actual source retention, actual history payload expiry and actual producing-run retention. Final-origin omission reaches the real published-decision fence (nontransactional sequence), rejects deferred commit atomically, then real publisher retry/replay succeeds. Ordinary canonical producer forgery negatives pass. Inventory1154467 unchanged.
- Native tool1133PASS: real saved native read/refusal/ack/reassign and capability/forgery cases; fixture now waits for exact handling-root jobs to settle before cancelling/awaiting workers.
- Runtime109 broader selected checks418PASS; fixture failures and every correction retained in TASK/logs, never relabeled as passes.
- Final `git diff --check` and `rustfmt --check --edition2024` on the7 changed source/test files PASS. Trail rebuilt2144files/30561nodes/45900edges. Current117 fixture-only change has not had a separate full rebuild of the graph; normal Trail queries refresh changed files.

## Implementation now present

D4/r2 architecture is independently accepted, **not implementation accepted**. See immutable `logs/D4-r2.md`, REVIEW and TASK for all original obligations.

D4.1 target/origin: authentic fenced receipt mints tenant token, canonical-message-owned immutable original target and message origin; admission seals run origin. Source receipt/case retention cannot turn private work into ordinary fanout. Lost source action context closes fresh original admission as typed notification_source_retired with zero new roots/providers. Exact original handling-root ownership is latched through former-root retention with a nullable live-run pointer. Original targeting, races, intent loss, omission/forgery, fence rollback and tenant teardown are verified in106/107 and current116.

D4.2 child/rerun: exact immediate parent/rerun and canonical source-message derivation, immutable links and deferred completeness. Actual recovery, authorized rerun and source-retention checks pass108/current116. Canonical workflow output producer seals reserve exact historical producing run before canonical insertion; ordinary insertion sealsNULL. Normalized message destinations and published final decisions seal exact origin. Source/producer retention does not erase canonical-message suppression. Output115/current116 pass.

**Important corrected premise:** direct in-app channel publication does not enqueue a workflow admission intent. Do not fabricate one or add automatic dispatch. The actual generated-message consumer/source-admission proof remains required through the real internal relay. Sending elsewhere also does not settle the original notice's automatic final reply; that genuine pending final must publish before history/producer retirement. Tests now do this.

D4.4 cleanup:

- `src/application/workflow/storage.rs`: explicit NotificationOriginCursor/NotificationOriginSweep port, existing worker cursor and bounded sweep call, actual token-deletion metric.
- `src/adapters/persistence/workflow/storage.rs`: indexed tuple pagination; materialized bounded **scanned** page before indexed dependency checks and row locks; sorted SKIP LOCKED within page; exact target/message/run rechecks at DELETE. Cursor advances through retained/locked prefixes even with zero deletes. Existing limits/timeouts/guards/FKs remain unchanged.
- Init migration adds retired-token partial index `(company_id,id) WHERE notice_id IS NULL`.
- Explicit application Port test implementation and cursor-progress assertions. New `escalation_origin_cleanup_tests.rs` is registered under inheritance tests. Actual3-source/fenced-notice orphan fixture, real source retention and canonical-owner loss, bounded/locked/retained prefixes, competing collectors exactly-once deletes, in-use deletion/invalid-limit refusal, and actual child propagation versus cleanup now pass.

This turn also changed `escalation_origin_message_tests.rs`, native tool test fixture settlement, inheritance module registration, and phase records. No relay implementation was started.

## Remaining work and next action after resume

1. Implement D4.3 authenticated internal relay and its real causal journeys. Required: typed non-wire provenance from actual DeliveryRecord/RenderedPart through InternalRelayMail and receiving commit/redelivery; exact persisted company/source channel/delivery/part/recipient/RFC/canonical-content checks; immutable source-message token inheritance. Missing/forged/foreign/mismatched proof must refuse without canonical effects or SMTP fallback washing. Preserve existing denied cross-company transport, gates/accounts/leases/hop budgets and current authority. Update all real sender paths/fixtures/mocks explicitly. Actual relay consumer after producer retention, self/two-agent/cross-desk cycles, external header/RFC spoofing and ordinary user independence remain required. No public origin flag or cross-company transport.
2. Finish the remaining D4 completeness/forgery/source-deleted-in-creation-transaction/exact replay matrix and original AC1–7/D1/D2/D3/F1 obligations. Do not infer waiver or completion from focused passes. Source retention removes visibility/action authority while suppression persists.
3. Refresh SQLx/fresh schema as needed for future SQL changes; focused checks, source connectivity, fmt/diff and Trail. Freeze whole cohesive p04/B1 with the ORIGINAL fixed6 exclusions from retained snapshot records, original requirements/coordination revisions and exact artifact identity; request independent Astra/high whole B1 review. Correct actionable findings, verify freshness and record root acceptance. No intermediate implementation review per small step.
4. Only then p05: docs and plan cross-references, complete scripts/check.sh plus missing CI gates: formatting/whitespace, offline alltargets, clippy, migrations/SQLx, source_modules, full database suite, network isolation, unchanged stack/lease budgets. Record unavailable checks honestly. Then completed-plan commit/pull-rebase/push. Preserve unrelated work and never commit secrets.

Relay source mapping already inspected (read-only): DeliveryRecord in application/transport/queue.rs197 has mandatory company UUID, canonical attribution/bindings and attempt_count. RenderedPart has index/key/payload/body digest; TransportPayload persists transport/version/body. OutboundEmailV1 is the existing public email-adapter frozen DTO at egress.rs66; as_relay_mail158 has no proof; wire_relay181 adds hop1/source once. EmailSender has actual delivery+part at send521 but relay_internally488 currently loses part. InboundMessage156, CommitPlan/build/request and InboundCommitRequest605 lack proof. Private agent notice commits use InboundOrigin::Application, not Internal. Receiving commit_as_on115 owns gates before row owners and checks redelivery before canonical INSERT. CanonicalContent sanitizes invisible characters; quiet prefix handling uses Unicode trim and reserved suffixes. Reuse existing representations/helpers rather than inventing wire proof or trusting RFC alone. Read current code/Trail before deciding the exact relay schema/proof implementation.

## Resources, continuity and safety

- No own build/test/browser/server/reviewer work is running at this stop. All exec sessions closed. Owned disposable PostgreSQL127.0.0.1:52471 remains available for resume; leave unrelated threads/processes alone.
- Cache: `/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d`; env.sh selects resume18/_test;17 backup is outsideGit. Do not commit env/credentials/databases/cache.
- Source env.sh, then use CARGO_BUILD_JOBS=1 JOBS=1 CARGO_INCREMENTAL=0 SQLX_OFFLINE=true. Native build: `cargo test --locked --offline --lib --no-run --jobs 1 --config 'profile.test.package.open_bots.debug=0' --message-format=json`. Native tests use stock RUST_MIN_STACK2097152 and focused exact names; serialize heavy work. Keep existing resource bounds. SQLx prepare116 used CARGO_PROFILE_DEV_DEBUG0 and alltargets locked/offline.
- Owned `check_native.py` in that cache discovers the exact successful build artifact, saves actual test inventory and selected command/results plus before/after source hash. Args: `--build N --previous N --label LABEL --select SUBSTRING --expected COUNT`. It is a local tool, not a project dependency or deliverable.
- Preserve the inherited dirty source and unrelated user AGENTS/Trail/Claude/MCP changes. Never stash/reset/discard or accidentally stage them. Use Trail before source search; rg is unavailable, use graph then git grep/Python for uncovered files. Cloudroom CLI only for agents.
- TASK/REVIEW/immutable decisions and logs are authoritative for original scope/evidence. Prior handoff archived at `logs/2026-11-13-pre-stop117-HANDOFF.md`. This current STOP instruction supersedes old continuation wording in archived checkpoints.
