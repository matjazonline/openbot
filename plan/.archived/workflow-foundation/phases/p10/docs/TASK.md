# p10 — verification, operations and cutover readiness

## Authority and scope

User requested `implement-astra-reviewed-2.2` for `plan/workflow-foundation/10-verification-and-cutover.md`.
Root `/root` is Author A, user-confirmed Astra/high; independent Reviewer B `/root/reviewer`
was originally requested Astra/medium without inherited context and will be reused. No new author.
Worktree `/Users/mac02/dev/openbot`, branch `workflows`, starting/current review base `5d4aab1d44e209fbb46bae275be77c86cd3fef22`.
Initial tree clean. Prior p08/p09 implementation and evidence committed/pushed at this base;
p09 accepted R020 at precommit digest76d100c63dd8735808ebd7b0c84ee0cbbb4e7932818e28575dc55f0e3030c3ea.
No source commit/push/deployment requested for p10. Only isolated phase test resources may be created.
Records: `/Users/mac02/dev/openbot/plan/workflow-foundation/phases/p10/docs`.
Root owns TASK/HANDOFF; B owns REVIEW/notes/findings. Earlier records remain immutable.
Coordination `/Users/mac02/dev/openbot/plan/workflow-foundation/COORDINATION.md`, root writer r15,
pinned logs/coordination-r15.md; WF-C004r2 acknowledged, inherited C001r2/C002r1/C003r1/C005r1 retained.
No pending cross-phase request. Snapshot exclusions: this p10 docs directory and COORDINATION.md only.
No artifact frozen yet. No owned running resource yet.

Graft command, graph and connector unavailable; scoped rg source/caller inventory substitutes.
Host context capacity/compaction threshold is not exposed; keep durable checkpoints, no global edits.
The three removed04.7 contracts referenced by plan10 are recovered verbatim from `2d608f2^` in logs/.
They are historical requirements, not evidence of current acceptance.

## Acceptance and batches

Original plan10 governs:17-scenario end-to-end matrix; fresh owner/caller inventory and mandatory
stale-evidence behavioral integration (restart, duplicate submission, stale revision, competing
claimants, effects/attempts/evidence/durable state); exact-public-trigger reassessment and independent
architecture review of any obstruction; preserve C adversarial SQL test; metrics and safe operational
procedures; deadline/maintenance/fairness/capacity; reproducible CI and fresh-schema cutover readiness.
Actual production cutover requires a specified deployment/database target and is not inferred here.

- B1: reconciliation safety and current topology reassessment. Inventory actual writers/callers;
  architecture gate for original exact-public diagnostic disposition if still obstructed; add
  supported-entry-point/provider/competing-claimant regression coverage, retain A/C SQL coverage.
  Focused checks: new integration tests plus existing claim SQL, reconciliation and recovery tests.
- B2: remaining acceptance gaps, operational visibility/runbook, obsolete path/docs cleanup and CI.
  Audit each matrix item against current tests; implement missing behaviors/verification. Hold gate
  only for any new persistent/concurrency/authorization contract. Focused relevant tests per behavior.
- Final: full formatting, diff, locked offline all-target build/Clippy, fresh migration and SQLx
  metadata, network-isolated database suite, stock-stack/lease gates, transport/security/frontend
  checks, final independent combined review. Repository full-suite instruction overrides skill's
  default request-only completion. No redundant full-suite permission request.

Known baseline: pre-existing generated CSS drift DV-P09-8; p10 CI reproducibility makes its resolution
part of this phase's audit. SQLx CLI absent at p09; prefer installing pinned CLI for prescribed gates.
No claims of passing checks yet. Next: finish source inventory and define concrete B1 architecture.

## B1 gate accepted

WF-P10-R001 PASS for P10-D1r1, SHA256
ce2ce8dafb6bee1e28b05255832097b62660ae20866637ef2114ccb895a973cd. Root verified unchanged
r15 coordination and decision. Exact historical public A diagnostic remains unexecuted; accepted
bounded owner obstruction is not substituted by layered SQL coverage. B1 behavioral tests proceed.
Use real http_test_support publication/binding/admission, HttpRuntime and production reconciliation
resource authority; the older action_dispatch fixture's direct bundle mutation is not used.
Owned PG18.6: /private/tmp/workflow10-ai7s7dqk port55500 roleworkflow10; workflow10/workflow10_test
freshly migrated. SQLx CLI0.8.6 installed; no production/sibling DB touched.

## B1 implemented and verification

Added supported HTTP lifecycle integration with actual publication/admission, loopback provider,
production resource authority and fixed historical verifier observation. Two claimants and two
actual dispatchers race; assertions cover provider calls/effects, durable attempts, one-use proof,
replay immutability, stale revision refusal, historical proof rejection, and fresh receipt-only
recovery through successful completion. Services/runtime are reconstructed, not a killed OS process.
Fixture::new owns an isolated database; test writes no protected production history directly.
Documented current obstruction and repaired plan10 links to recovered historical requirements.

Network-isolated focused stock2MiB test command:
`cargo test --locked --offline --lib -- workflow_cutover_http workflow_action_reconciliation_claim_sql workflow_http_ workflow_action_reconciliation_proof_`
PASS43/0failed/0ignored (17.99s), logs/b1-focused.log; new journey alone PASS1 (b1-integration.log).
Includes original A witness/public controls and C actual duplicate-candidate test. Formatting and
whitespace PASS. Pinned SQLxCLI0.8.6 migrate both freshDBs PASS; SQLx prepare with locked/offline
all-target compilation PASS, b1-sqlx-prepare.log; .sqlx unchanged. Final full gate remains owed.
B1 now frozen for independent batch review; no dependent implementation until acceptance.

### B2 operational design (independent inventory while B1 is frozen)

Existing /metrics JSON supports fixed-name gauges but workflow state is mostly in scoped UI/read
models. Add a read-only application metrics port, PostgreSQL snapshot and cancellable sampler owned
by WorkflowEngine::spawn's existing joined handles. No schema/authorization/concurrency state change:
metrics cannot mutate workflow records or authorize work. Only fixed aggregate names, no IDs or
payload labels. Snapshot each30s, total application budget4s and SQL statement2s/lock250ms, read-only
repeatable read; on failure mark collection unhealthy while retaining prior successful gauges and
last-success timestamp. Exact counts may fail on overload; never label truncated/failed data as0.
Durable sources cover live queue age/runs/waits/expired leases/deadlines/unknown actions, recent
step latency/retries/delivery acceptance/failure, outstanding model token reservation and current-price
estimated spend with missing-price counts. Rolling history24h, present-state measures all live rows;
explicitly estimate prices, do not invent a monetary spending limit. Existing scoped run/action UI
provides actionable links separately from labels. Query plan/runtime evidence and real-owner tests
must cover SQL and clearing gauges; failure/shutdown tests cover sampler behavior.

CSS automatic discovery includes archived source snapshots in tracked phase logs. Make Tailwind
source explicit (src Rust only) and regenerate committed asset; change check:css to compare temporary
output against working-tree asset so a planned asset change can be verified before commit. Verify
same-byte repeated generation. Update cutover/runbook and current acceptance map; preserve historical
records, mark outdated progress/overlapping plans as historical. No automatic live deployment.

### B1 R002 correction

F001 Low: a complete500 after effect was uncertainty, not the promised lost response. The actual
loopback provider now increments its effect then returns a200 response whose declared11-byte body
fails with ConnectionReset; the HTTP client cannot receive a complete result. Original first
rejection remains500 without effect. Repeated call/effect/evidence/receipt assertions unchanged.
Focused actual transport-loss journey PASS1/1 at stock2MiB under network guard, logs/b1-f001.log.
Formatting/diff PASS. Only the test provider changed; unaffected43-test and SQLx evidence reused.

## B1 accepted; B2 active

R003 PASS exact e8e20cf25f93466f1e09168fde58e2cc7d1c42dc6ab68d0fccefe5bc29077a43;
root recomputed same and compared coordinationr15, logs/b1-root-acceptance.txt. F001 fixed.
B2 starts with operational design above; no new durable or authority protocol. Add one complete
reviewed-email journey (classifier/context → agent → human edit → actual delivery worker/provider
acceptance) to join evidence previously split across tests. Preserve current model/profile exclusion.

### B2 implementation checkpoint

Read-only metrics port/snapshot/sampler implemented with bounded reads, failure retention and
shutdown cancellation. Sampler occupies one connection, so startup reserve increases from8to9
(default20 remains adequate); worker_slots boundary tests and .env/deploy docs updated together.
No ownership/concurrency protocol or schema change. SQL first execution caught wrong view column
`state`; corrected to effect_state before tests. Unknown includes applied_without_result.
New reviewed_delivery_tests joins actual ingress/classifier/history/model/human/worker delivery;
scripted transport acceptance is explicit, not a live SMTP test. Added metrics state assertions
through owner-created runs, human waits and HTTP uncertainty; bounded-lock and sampler tests.
CSS source(none) + explicit Rust sources, generatedasset, temporary-output comparison; two check:css
runs PASS. Operations, cutover, scenario evidence map and stale overlapping plan notices updated.
B1 source changed only to add metrics assertions, requiring affected review/check at B2.

B2 focused6: five PASS, reviewed journey failed solely on expected RFC Message-ID brackets;
corrected expectation to saved `<reviewed@partner.test>`, rerun PASS1/1 at stock2MiB
(b2-focused.log, b2-reviewed-correction.log). Added current-price monetary conversion coverage via
ModelUsageStore::save_model_price afterward, included in final full gate. Metrics EXPLAIN against
empty fresh testDB PASS, planning5.450ms/execution0.858ms (b2-metrics-explain.log); this is not a
large-production-database throughput claim. Browser protocols16/16 PASS; deploy/credential scripts,
transport boundaries and sample static gates PASS (final-browser-protocols/final-script-checks.log).
Final full gated script /private/tmp/p10-final-gates.sh running, status final-gate-status.log;
contains fmt/diff/CSS, offline alltargetscheck/Clippy, migrate/SQLxprepare+check, alltargettests,
stockstack, leasebudget, generateddecisionbrowserfixtures+browser. No concurrent phase DB checks.

## B2 frozen for whole batch and combined final review

All-target offline check, Clippy(-Dwarnings), migration and SQLx prepare PASS; .sqlx unchanged.
Snapshot b2-snapshot.txt pins implementation/deliverables; exact exclusions unchanged (phase docs,
coordination only). Coordinationr15 unchanged versus pinned copy. Final suite continues read-only
against the frozen source; independent B may inspect now but must defer DB/build checks until root
reports the gate process finished. SQLxcheck/tests/stack/lease/decisionbrowser evidence still pending.
No source mutations during review; final verification failures will be communicated before correction.

R004 interim notice (review not complete): existing production_workers_stop_on_the_shutdown_token
asserts3 handles but sampler makes4 for one workflow slot. Root will correct with all consolidated
findings; not an unrelated failure. Reviewer also identified a superseded notice splitting the
post-approval plan's existing no-backward-compatibility sentence. Source remains frozen while full
suite records its results. No claim of full verification yet.

## R004 consolidated corrections and final gate continuation

R004 found F002Medium stale lifecycle taskcount and F003Low split compatibility paragraph; no
other actionable B2 defects. Full library gate completed2310PASS/1FAIL/3ignored317.18s, solefailure
F002; binary target was not reached because Cargo stops after failed library target. New metrics,
current-price conversion and reviewed-email tests all passed in that run. Three ignores are the
existing developer JavaScript dump and two external-DNS smokes (intentionally incompatible with
network denial), not silently skipped DB checks. logs/final-tests.log retains failure.

Corrected F002 expects4 owned tasks and checks count after cancellation/join so a stale expectation
cannot bypass shutdown coverage again. F003 restores the whole compatibility paragraph, followed
by historical notice. No production behavior changed after full-suite snapshot. Affected focused
shutdown check + binary suite, then the already-required complete stock2MiB library suite, lease
budget and decision browser gates run in final-corrections.sh/final-correction-status.log. This
avoids a redundant third full library run for test-only/text corrections while verifying every
library test again at the stricter stock stack. Original alltarget check/Clippy/migration/SQLx
results remain applicable; fmt/diff repeated. Recheck review after these required gates pass.

Before final recheck, made README status point to the authoritative phase10 record rather than
hard-code "in progress", which would become stale at acceptance. Documentation-only delta beyond
F002/F003, included in logs/b2-final-snapshot.txt for R005; source code unchanged and diff checkPASS.

Final corrected shutdown PASS1, binary PASS5, complete stock2MiB library suite PASS2311/0failed/
3existingignores (323.83s), under networkguard. logs/final-corrected-shutdown.log,
final-binary-tests.log, final-stack.log. This closes original library failure and verifies every
library/binary test on final code. Lease budget now running alone; decisionbrowser follows.

## Required final gates complete; R005 ready

Lease suite130/130PASS185.61s and measuredwork below50% attemptbounds, no raisedlimits
(final-lease.log). Generateddecisionfixtures2/2PASS and realChrome decisionprotocol1/1PASS
(final-decision-fixtures.log/final-decision-browser.log). OtherChrome protocols16/16PASS.
Full library2311PASS/3documentedexistingignores atstock2MiB and binary5/5PASS. fmt/diff,
lockedofflinealltargetcheck, Clippy(-Dwarnings), migration, SQLxprepare+check(nochangedmetadata),
CSS repeatgeneration, deploy/credential/transport/sample gates allPASS. Original F002 failure and
focusedcorrection retained. No remaining root DB/build processes; reviewer may run isolatedchecks.

R005 artifact37915f1301f66f04c5fff3f2a2ea28bc1fb98649eeb7997f39e00b61b4661ecb,
base/start/HEAD5d4aab1d44e209fbb46bae275be77c86cd3fef22. Snapshot b2-r005-assignment-snapshot.txt;
exact exclusions phase10docs and COORDINATION only. Coordinationr15 unchanged/pinned.
Recheck F002/F003 plus README neutral status wording and final evidence/combined acceptance.
No source edits until verdict. No deployment/commit/push authorized; graph remains unavailable.

## Phase10 complete — accepted locally, not deployed or committed

Independent WF-P10-R005 PASS exact37915f1301f66f04c5fff3f2a2ea28bc1fb98649eeb7997f39e00b61b4661ecb.
Root recomputed same digest and cmp'd fullsnapshot + pinnedcoordinationr15 before acceptance
(logs/final-root-acceptance.txt). F001/F002/F003 allclosed; R001 architectural disposition retained,
R002/R003 B1 and R004/R005 B2/final review records retained. Implementation and combined verification
complete in this workingtree; commit/push/production deployment not performed for p10.

Delivered read-only bounded durable metrics, actual stale-proof/lost-response reconciliation
journey, reviewed-email delivery integration, deterministicCSS/check, current acceptance map,
operations and fresh-target cutoverrunbook, stale plan notices. Full library2311+binary5 PASS,
stock2MiB, lease130+timing PASS, Chrome16+1 PASS, decisionfixtures2 PASS; fmt/diff/offlinealltarget
check/Clippy/migration/SQLxprepare+check/CSS/scripts/securityboundaries PASS. Original failures and
corrections retained; three intentional ignores explicitly identified above.

Remaining limits: exact public historical episode INSERT diagnostic remains unexecuted under the
reviewed obstruction, not inferred from behavioral success; same-run duplicate topology remains
adversarial, since supported owners remain sequential. Graft build unavailable (binary/index/
connector absent); source inventory retained. No production verifier installed; reconciliationnotes
cannot authorize replay. No production target or cutover authorized/tested; runbook gates deployment.
Memory/profile exclusions unchanged. No unverified required implementation work remains.

Root appended coordinationr16 status closure only, pinned logs/coordination-r16.md; inherited
contracts and notice revisions unchanged. Reviewed source digest unaffected. Owned PG18 cluster
/private/tmp/workflow10-ai7s7dqk port55500 stopped and removed after B completion; generated browser
fixtures removed. resources.json/resource-cleanup.log record cleanup. No sibling resources touched;
review/build/test evidence retained, reviewer idle/completed. No requested global context settings
changed; host compaction threshold remained unavailable. Next action, if requested: commit/push the
accepted workingtree or execute target-authorized cutover with its controlled pilot.

## Integration authorization — 2026-10-25

User requested "commit, merge main, push to main" after phase acceptance. Fetched origin and
confirmed local/remote main agree and are ancestors of workflows. Pre-commit source snapshot
still matches accepted37915f1301f66f04c5fff3f2a2ea28bc1fb98649eeb7997f39e00b61b4661ecb.
Proceed with the phase10 commit and fast-forward main, then push origin/main. No implementation
changes or conflict resolution are needed; accepted verification remains applicable. Git history
and remote refs record the resulting commit/integration identity; earlier uncommitted status above
is the historical state at acceptance, superseded by this authorized integration.
