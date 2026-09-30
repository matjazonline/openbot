# 04.7 bounded supported-provider / mixed-sibling / conflict runtime evidence

Implementation evidence only; root alone accepts scope. Original Execution6 and
BRIEF-04.7:157–326 plus accepted provenance/time refinements remain authoritative.
No04.8. Preserve HEAD655aaaba3132c561809f97fd25c89f600bca1fa9 plus all existing work.
No production, port or migration edit; all applied migrations through184000 unchanged.

Five isolated real-DB tests under sibling_tests::runtime_tests use the accepted
all-public-tables repeatable-read snapshot and genuine durable provider operations,
requests, effects and finality barriers. Tests are in three files under500lines;
every function remains under80lines. Only parent change is three-line child wiring.

| Test (workflow_action_reconciliation_runtime_ prefix) | Discriminatory behavior |
|---|---|
| supported_positive_veto_then_genuine_valid_receipt | SafeRepeat frozen policy plus explicit matching host registration creates a genuine SupportedReplay marker. Missing usable output and schema-invalid recovered output each record positive Applied truth. Before that fact SQL and actual Rust expiry retirement classify supported retry safe; afterward both classify unknown, actual release parks, real recovery cannot claim or mutate any table. No fabricated receipt/output/route/successor, one effect/entry and zero consumption. A later genuinely recovered valid effect creates linked receipt, schedules only existing job with unchanged historical attempt/debit/activation/lineage/operation, stores exact generated final revision, claims normally, agrees again with real Rust/SQL safe classification, replays with zero provider polls, then normal completion succeeds. InvalidRecovery is a trusted-adapter fault: it returns schema-invalid output for a genuinely valid durable effect; later the real ledger recovers that same actual effect. It changes no effect/entry/marker history. |
| receipt_final_supported_siblings_continue_exactly | Three genuinely prepared distinct invocations in one execution: Reconcile receipt, Reconcile final proof, and SafeRepeat supported sibling. A validated frozen bundle includes both distinct tool contracts before remote dispatch; fixture_current_contracts is the explicit current-policy registry. MixedAuthority retains real current resource/policy locks and reads the exact registered tool contract. No stored action/marker policy is rewritten. Receipt alone cannot schedule while proof sibling unresolved. Genuine final proof schedules existing job, historical rows preserved. Two actual claimants yield one fence; receipt polls0, proof polls1 and consumes1, supported sibling polls1 without proof consumption. Exact5entries/1consumption/3effects, then normal completion. |
| conflicted_sibling_vetoes_all_receipt_continuation | Two real Applied effects; first recovered receipt then intentionally contradictory trusted final assertion commits conflicts. Second recovered receipt retains both valid receipts but cannot schedule, and existing conflicts remain byte-for-byte. SQL unsafe; real claimNone; whole-table equality around refused claim; exact2entries/0consumptions. Positive usable receipt cannot mask conflict in another sibling. |
| conflict_after_return_before_completion_retires_exact_attempt | Handler actually receives a valid accepted result, then contradictory final truth commits. Receipt unchanged and generated revision strictly increases from immediate before and matches persisted final. Reservation/replay refuses with full-table equality. complete_io runs before heartbeat and retires exact attempt with action.evidence_conflict; no committed output/route/successor. Subsequent heartbeatNone, whole-table equality. |
| completion_before_conflict_preserves_committed_history | Actual normal completion commits before contradictory truth. Conflict attaches and revision advances, but entire job/execution/attempt/debit rows and terminal state/terminal execution remain unchanged. Receipt truth retained; actual claimNone without any table mutation. This is actual sequential owner ordering, not a simultaneous run-lock race. |

Rust/SQL equivalence uses the real recovery::retire_on expiry path inside a rollback
transaction on a live claimed job, inspecting persisted attempt safety, then asserts
all-public-table equality after rollback. No copied classification substitutes for
recovery. Existing matching provider registration and database constraints authorize
actual entries; no disabled triggers or fabricated grant/receipt facts.

Final combined regression:34PASS0FAIL0ignored,2290filtered,13.18s (prior29+new5),
stock2MiB, /private/tmp/workflow-04.7-runtime-combined-r3.log.
Both explicitURLs postgres://mac03@127.0.0.1:55439/workflow_admission;
SQLX_OFFLINE=true RUST_MIN_STACK=2097152 and --locked --offline. Retained PG identity
verified workflow_admission/mac03/data/private/tmp/workflow-admission-pg-e3aa/
port55439/max_connections200. Every test owns its isolated migrated DB, drained normally.

Exact commands:

```
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib action_reconciliation_receipt_tests
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=false RUST_MIN_STACK=2097152 cargo sqlx prepare -- --all-targets --locked --offline
cargo fmt --all -- --check
git diff --check
graft build
```

SQLx prepare exit0,14.78s, -sqlx-prepare.log, no.sqlxdelta. Formatting/whitespace/
graft completion status and frozen exact delta/hashes recorded in
/private/tmp/workflow-04.7-runtime-frozen/HANDOFF.md. Logs preserve initial compiler
corrections (registered-provider helper signature and disjoint fence borrowing), then
4PASS/1 test-fixture count error: contradictory truth legitimately appends two conflict
facts. Final assertion requires nonempty conflicts and complete conflict-table equality
around second receipt, avoiding an invented single-fact invariant. No production fix.

Exact remaining runtime matrix:128/129 genuine remote-entry history and sibling
settlement/scheduling/reservation/enter/shared recovery gates, paused overflow after
128snapshot; all remaining child/runnable/unactivated/completed/output/route/successor/
activation-max_steps/lease-bearing failed job/missing-retired-attempt/poison/root-budget
eligibility and current claim budget recheck. Original seven gate tests are accepted
separately, including corrected immediate generated revision comparison.

Waiting/terminal Unknown and nonconflicting Applied/final truth with exact replay/
refusal/generated revisions; real competing cancel/expire ordering remain. Explicit
paused coverage/marker/operation/late-receipt races, valid foreign tenant/association/
resource-error negatives, receipt-before/after proof consumption late-pending parking,
return_authorized and heartbeat before completion, simultaneous competing completion
versus conflict run-lock ordering remain. Mixed SupportedReplay with AppliedNoResult
or invalid-output sibling and later valid receipt is now covered for supported-only
one invocation and Reconcile siblings separately; the exact combined mixed-policy
blocking composition still remains. SupportedReplay coverage here uses actual SafeRepeat;
provider-idempotency-specific integration remains under existing04.5 tests, not this
new positive-output matrix.

All schema provenance/sourceXOR/orphan/immutability/witness/retrofit/rollback/populated
upgrade/malformed-history work and broader stock2MiB affected suites, strictClippy,
locked offlinealltargets/freshschema/allmigrations/SQLxcheck, FULL originalcriteria+
actualcode+integration review/root acceptance remain required. No full04.7 completion.
