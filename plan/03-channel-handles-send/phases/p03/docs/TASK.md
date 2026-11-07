# CHS p03 — Durable response intents and completion

## Identity and authorization

- Original requirements: [03-durable-response-intents.md](../../../03-durable-response-intents.md), including both03A/03B; original all-plan scope remains phases01–07, then all `plan/04-autonomous-agent-history`.
- Workflow: `/Users/mac02/dev/openbot/.agents/skills/implement-astra-reviewed-2-2/SKILL.md`. Phase p03, cohesive whole-phase batch CHS-P03-B1.
- Root/author A: `/root`, user-confirmed Sol6.1/xhigh. Independent B: `/root/channel_reply_reviewer`, requested Astra/high without inherited history. Native runtime/context/compaction telemetry unavailable; no model substitution or additional authors.
- Checkout `/Users/mac02/dev/openbot`, branch `main`; phase start/review base/current HEAD `769a23aa6ef423e964f8221a8ee6b0743325c6ba`. No commits, push, deployment or production resets authorized.
- Prerequisite p01 accepted0995151e; p02 accepted `aa7cd3e8ddd572c6dd7a551cc69a8119769b09b8ace32b567744f4d1fc11ed5e`, B PASS CHS-P02-B1-R2, exact source archive/manifest retained in p02 logs. p03 delta is reviewed against that accepted working-tree source, not HEAD alone.
- Preserve the unrelated external deletion `plan/local_machine_tunnel_plan.md`; root did not delete/restore it. Include its absence in the source identity and archive manifest, separate from authored phase changes. No other source exclusion may hide it.
- Authoritative coordination: `/Users/mac02/dev/openbot/plan/03-channel-handles-send/COORDINATION.md`, root writer, revision7; exact pin `logs/coordination-r7.md`. Acknowledged I001–I003/r1, I004/r2, I005–I006/r1, C001/r1, C002/r1, C003/r2; no pending cross-phase request.
- User explicitly requires **targeted tests only**. This overrides repository/plan default full-suite gates throughout both plans. No full-suite claim.

## Status and decisions

Whole-phase B review complete with one P2 finding CHS-P03-B1-F1. Author correction implemented; affected checks and independent recheck pending. p04–p07 and plan04 remain authorized but unstarted until p03 acceptance.

Costly decisions were gated before edits. Exact accepted architecture: [CHS-P03-D1/r2](logs/CHS-P03-D1-r2.md), B PASS CHS-P03-AG1-R2, root receipt [architecture acceptance](logs/CHS-P03-root-architecture-acceptance.json). Original r1/gate CHANGES REQUIRED/F1 (nonexistent root action ledger) and exact r2 amendment remain immutable in logs and B's review records. Implementation uses the actual128-per-execution message admission, not a new root resource.

C001 authentic admitted-root ownership and C002 execution-local typed disposition remain binding. Passive contexts confer no sending authority; only successful compiled noReply suppresses. Schema-valid empty business values remain valid, child suppression stays local, and explicit messages retain their own receipts.

## Implementation and invariants

- Optional strict channel approval policy: static current privileged user reviewers1..16; timeout duration1..86400seconds, never an absolute deadline on settings. Configuration CAS and immutable ingress snapshots capture policy separately from response projection. First proposal materializes database-now+duration capped by run deadline; retries reuse it.
- Immutable terminal proposal per owned execution contains validated business output, canonical projected content, original source, exact route/sender/recipient facts, policy/deadline, integrity and real completion-owner witnesses. SHA256 covers normalized bounded evidence. FrozenAction uses an explicit typed channel-final origin; ordinary STEP/model-tool prepare/dispatch paths refuse it.
- Automatic and suppressed decisions are unique per actual owned root/association. Shared completion writes the decision atomically with business output/disposition, progression/events and job settlement. Deferred SQL proofs reject orphan success, uncompleted publication and mismatched proposal/action/source/approval links. Suppression skips projection, messaging permission and approval, while preserving business validation.
- Required approval parks the real Pure, Fenced or accepted source-wait owner using existing protected-action decisions, private notices, formal submissions and continuation accounting. No fake attempt or user AST step. A required saved-finalization port runs before worker handlers; direct pure/I/O/SQL claim paths cannot rerun the handler. Approved resume uses saved output only; rejection, timeout and refusal follow original-step failure routing. Saved child exhaustion cannot become reconciliation of the original handler.
- Common messaging authority and knowledge-release rules are reused. Current company/channel/thread authority, policy and frozen recipient clearance gate the final effect. Review-stage and formal-submit clearance prevent an admin role from bypassing payload restrictions; existing decision views redact private payloads. Immutable configured persistence is shared by worker and HTTP WorkflowServices completion ports, without an Arc/composer cycle.
- Account locks (actor, legitimate requester, frozen reviewers and acting reviewer) precede company, then run/execution/job/attempt. Source/channel/recipient/release checks stay after run. Revocation/cancellation/reviewer contention use actual competing transactions.
- First automatic proposal reserves one invocation in the existing128-per-execution admission. Counts union dispatch/reservation identities; replay does not double charge and rejected reservations remain spent. No bound raised.
- Publication work has page1..64,30second lease,8attempts, bounded retry backoff, seven-second overall storage operations, SQL lock2seconds/statement5seconds, worker/generation fencing and terminal diagnostics. Indexed retained claim timestamps schedule company turns even for page1. Claims confer publication authority, never WorkflowFence. p04 still owns canonical publication and supervised draining; transport state remains on the existing delivery queue.
- Tenant-qualified links/immutability and live-root retention guards preserve proposals/approvals/pending or claimed publications. Terminal failure preserves workflow success and permits normal closed-job-first run retention; company teardown remains possible. Samples/manual/nonowned child runs create no production obligations.

No backward compatibility: edit the sole init migration and reset only adopted disposable databases; no backfill, legacy decoder or dual path.

## Resources and checks

Adopted root-owned PostgreSQL18.6, PID85060, `/private/tmp/openbot-chs-xb34mvy9/pgdata`, loopback65037; main/test `openbot_chs`/`openbot_chs_test`. Preserve baseline pair/worktree `/private/tmp/openbot-chs-baseline-6ra2cluo`; no other resources touched. Phase-local runner/resources in logs; primary target serialized, CARGO_INCREMENTAL1, jobs4, stock RUST_MIN_STACK2097152. Runtime tests use `scripts/test-network-isolation.sh`; ALLOW_MISSING_DATABASE_URL unset.

Affected regression inventories/results: [summary](logs/affected-summary-01.json), [deduplicated result](logs/affected-result-01.json), complete individual list/test receipts/logs.526 distinct selected,525 passed,0failed; one pre-existing ignored browser-fixture exporter (no UI change in p03). Scopes: feature92, batch15, completion14, wait20, child-call9, repeat4, decision38, poison recovery12, protected action32, messaging64, message admission14, knowledge release49, sample15, autonomous-agent151 (150passed/1ignored). Inventories overlap; totals are deduplicated.

After that pass, typed the proposal integrity decoder and extracted fixture publication setup; affected final feature92 rerun passed. Other interface evidence remains reusable because those two edits did not change unrelated behavior.

Current final checks all PASS, with exact commands/environment/output in linked JSON/log pairs:

- `author-current-main-migrate-01`, `author-current-test-migrate-01`: fresh sole migration on the owned pair, latest schema.
- `final-sqlx-prepare-01`: `cargo sqlx prepare -- --locked --offline --all-targets`; actual regeneration after all SQL, including dynamic test queries. Cache bytes unchanged.
- `final-sqlx-check-01`: corresponding prepare `--check`, PASS.
- `final-clippy-01`: offline locked all-target Clippy with `-D warnings`, PASS.
- `final-feature-tests-01`: guarded feature92,0ignored, PASS after the last decoder/fixture edits.
- `final-fmt-01`, `final-diff-01`, `final-graft-01`: format, diff check, deterministic graph refresh PASS.
- Aggregate pointer: [author-final-checks.json](logs/author-final-checks.json).

All work-in-progress failures and corrections are retained in [author-retained-failures.json](logs/author-retained-failures.json) and their complete receipts/logs: early Tx signature, missing serializers/helper argument, shared-trigger NEW.id, replay assertion, explicit autoderef, fixture receipt/directory/workflow identity, retention ordering, import and public-ingress ceiling assumptions. They were fixed and covered by later checks. An unclosed test-helper delimiter during final extraction was caught by direct cargo fmt and corrected before final compilation/format verification. No assertions were weakened. The two unrelated inherited onboarding failures remain untouched with clean original-HEAD baseline evidence in p01; they were not part of this affected selection.

B's retained sandbox cannot nest the network guard; preserve independent source/evidence review and run any exact requested diagnostics mechanically on root with the same guard/stock-stack fixture. No bypass or reviewer replacement.

## Artifact and next action

Use the exact same9 exclusions for every snapshot: COORDINATION.md; phases/p01 throughp07/docs; integration/final/docs. Full array comes unchanged from accepted p02 manifest. Use p01 logs/tools realpath adapter for the installed snapshot script. Exclude workflow records only.

Freeze with `logs/freeze-review.py CHS-P03-B1-R1` after this checkpoint. It retains a cumulative archive, deletion-as-null hashes, complete p03 delta against accepted p02, pinned coordination and exact source digest. Do not overwrite identities. While B reviews, freeze all implementation files. Accept only B's explicit PASS for the recomputed digest with saved notes/findings and no unresolved defects. Then publish the accepted producer in coordination and start p04.

Frozen whole-phase artifact CHS-P03-B1-R1: digest19960fb95b5d0a4fcb1547e317cb46fe91161392f76967d2802364eb1843f808,103cumulative paths/66authoredphase changes plus preserved external deletion. Exact manifest/archive/p03 delta in logs. Independent B whole-phase review assigned to /root/channel_reply_reviewer; implementation frozen.

B entry identity mismatch reconciled: root-authored302-byte frozen-artifact note was accidentally written to p03/HANDOFF.md after the snapshot. Exact text preserved under logs/CHS-P03-B1-R1-unintended-handoff.md. Root restored/recomputed the original19960fb9 identity without source changes or changed exclusions; see root-identity-reconciliation receipt.

## CHS-P03-B1-R1 corrections

B completed whole-phase review: CHANGES REQUIRED, one actionable P2 CHS-P03-B1-F1; all66 authored deltas covered. Exact guarded isolated diagnostics reproduced both suppressed/failed decision DELETEs committing with a surviving succeeded run (0passed/2expected-rejection failures). Full source remained19960fb9 during review.

Correction scope: preserve each final decision for the lifetime of its run, across all states, while allowing existing whole-run retention and company teardown. Change the existing DELETE guard and focused boundary/lifecycle tests only; restore the accepted D1/r2 invariant, no architecture contract change/gate needed. Rerun the affected intent/feature tests, owned fresh migrations, actual SQLx prepare/check, offline lint/format/diff/graph; no full suite. Freeze a new R2 artifact with exact correction bridge and unchanged9 exclusions, then request one F1 recheck.

F1 correction complete: sole init DELETE guard retains every final decision while its company/run survives, with existing full-owner teardown allowed. The same suppressed/failed cases now reject SQLSTATE23514 and preserve complete state; both positive run-retention controls pass. Current checks: fresh pair migrations PASS;10intent-schema tests PASS;21existing retention tests PASS (including real competing collectors/large native tree); actual current SQLx prepare/check,offline all-target Clippy-Dwarnings,fmt/diff/graft PASS. Exact receipts in author-correction-checks.json plus correction-intent-tests-01.json and migration receipts. Current distinct evidence union546PASS/1inheritedignored/0failed, with unchanged R1 behavior evidence reused explicitly; no full suite. Independent R2 correction recheck pending; source will be frozen under new R2 identity.

Frozen correction CHS-P03-B1-R2:849e88b7cea90a4c3bf103fc84e45b2c1cf62db8e576b9a2af6e5ca00dfd5efc; same HEAD/103paths/66cumulative phase deltas and exact9exclusions. R1-to-R2 bridge changes only init-schema DELETE guard and intent-schema tests. Coordinationr7 unchanged. Independent F1 correction recheck assigned; implementation frozen. Current union547selected/546PASS/1ignored/0failed; parser note retained in current-affected-result-02.json.

ROOT ACCEPTED p03 at849e88b7cea90a4c3bf103fc84e45b2c1cf62db8e576b9a2af6e5ca00dfd5efc after B PASS CHS-P03-B1-R2, saved F1 FIXED disposition, matching root snapshot/all103hashes and current coordinationr7. See logs/CHS-P03-root-acceptance.json. Combined integration remains pending. Next authorized phase p04, thenp05–p07 and allplan04.
