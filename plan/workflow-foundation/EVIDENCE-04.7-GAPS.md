# 04.7 criterion-to-test map — resumed provider assignment

This is a focused evidence inventory, not acceptance. Original execution item6 in
`04-actions-http-and-delivery.md`, BRIEF-04.7 sections1–5 and discriminatory checks
at lines261–326, and accepted attribution/witness/observation refinements remain
authoritative. Existing assertions were read through graph spans. “Partial” means
the listed test catches the stated behavior but leaves the exact gaps below.
No04.8 implementation; root alone updates queue acceptance.

Current runtime after independently checked audit expansion and additive184000:
the original seven receipt/provider tests give7PASS/0FAIL in
`/private/tmp/workflow-04.7-audit-provider.log`. The earlier5PASS/2FAIL evidence is
preserved in `/private/tmp/workflow-04.7-resume-provider-r6.log`; repeated actor-audit
uniqueness no longer blocks those two full scenarios. Combined with three focused
audit tests, the same filter gives10PASS/0FAIL at stock2MiB in
`/private/tmp/workflow-04.7-audit-focused.log`. These results do not close the gaps
below or substitute for independent implementation/full04.7 review.

Focused `action_reconciliation_audit_tests.rs` verifies real same-key/different-key
contenders, distinct subsequent command audits, exact immutable replay, orphan and
NULL-execution audit rejection, actor/kind scope mismatch, duplicate audit reference,
UPDATE/DELETE/kind laundering, and retained non-reconciliation uniqueness/NULL
semantics. Populated historical upgrade/malformed-history rejection, wrong
company/run/execution/refusal linkage, exact deferred reopen and whole-transaction
rollback remain pending under CONTRACT-04.7-AUDIT.md. See EVIDENCE-04.7-AUDIT.md.

Paths below are relative to `src/`. Function names in each row retain the
`workflow_action_reconciliation_` prefix unless another full prefix is written.

| Alias | File / reviewed spans |
|---|---|
| P | `adapters/persistence/workflow/action_reconciliation_provider_tests.rs` (all four tests and request/effect/barrier fixture) |
| R | `adapters/persistence/workflow/action_reconciliation_receipt_tests.rs:218–441` |
| S | `adapters/persistence/workflow/action_reconciliation_snapshot_tests.rs:56–141` |
| B | `adapters/persistence/workflow/action_evidence_binding_tests.rs:4–83` |
| U | `application/workflow/actions/reconciliation/tests.rs:78–393` |
| V | `application/workflow/actions/reconciliation/service_tests.rs:90–159` |
| H | `adapters/persistence/workflow/action_history_tests.rs:101–145,225–252` |
| Q | `adapters/persistence/workflow/action_uncertainty_tests.rs:192–245,268–361` |
| A | `adapters/persistence/workflow/action_receipt_acceptance_tests.rs:166–238` |

| Original criterion / accepted refinement | Existing meaningful assertions | Exact remaining gap |
|---|---|---|
| Current owner/admin and true association before scope reads; same authority under locks; original actor differs from command actor | V `denial_precedes_any_scope_read` catches preflight denial before reads; V `resource_error_preserves_operational_failure` catches directory error; S restores company association | DB-backed owner/admin/member/outsider, wrong company/association, forged actor/subject/digest/marker; command-vs-original actor, principal/resource revocation races, operational errors at settlement and I/O |
| Immutable request identity; exact replay reauthorizes, key conflict; no reverify/reschedule | U `request_identity_binds_every_command_component` mutates11 fields; S `snapshot_revision_refusal_replay_reauthorizes_resource` checks refused-command exact replay, changed-note conflict and revoked resource | Successful evidence/scheduled command replay, owner revocation, duplicate after proof expiry, different actor/input/reference/revision with same key, concurrent same/different keys; verify zero provider I/O and no extra facts/revision |
| Caller cannot supply verdict/quiescence/grant; explicit host verifier registration; bounds/malformed persisted data | U `input_cannot_carry_authority_and_bounds_are_bytes`, `verifier_binding_and_times_fail_closed`; V `uninstalled_matching_source_never_settles` | Persisted malformed command/evidence decode; registration/source mismatch in real service+SQL; exact envelope boundary, stored diagnostic/output bounds and secrets; adversarial SQL check/FK isolation |
| Unknown stays unknown; ordinary retry/safe labels/timeout/cancel/error metadata cannot mint proof | U `unknown_claim_is_never_final_absence`; P `provider_active_is_unknown_until_actual_effect_or_barrier` observes a request still capable of applying | Persisted UnknownNote/verifiedUnknown restart+duplicate+ordinaryRetryCommand; claimed safe labels and error metadata through actual recovery; assert no extra attempt/debit/entry |
| Genuine remote request/effect/quiescence, not momentary absence; zero-entry reservation crash | P active/barrier test: delayed effect can apply after local timeout; barrier prevents covered request, new authorized entry can apply; P live-clock test uses zero-entry marker | Concurrent barrier vs delayed effect, old provider polling after barrier, authentic marker closure before entry, cancellation during verifier, no detached work; fixture actual-code review still required |
| Snapshot-verifier-settlement binding and no I/O while DB locks held | U `coverage_binds_marker_and_all_request_provenance`; S verifies full worker/generation/entry provenance; U verifier time/binding unit matrix | Pause verifier and compete actual revocation, coverage insertion, revision change, operation/marker change, expiry, late receipt, cancellation; verify settlement refuses stale token and concurrent lock holder progresses |
| DB-clock issue/commit validity,5s/24h bounds, expired-proof refusal | U verifier time matrix; P `live_db_observation_and_future_timestamp_settlement` tests trusted future observation rejected independently by settlement and live timestamp accepted | Real verifier timeout/operation budget; proof expiry between verification/commit/reservation/enter; expired final proof across restart/ordinary retry; applied truth never expires into replay permission |
| Applied valid output => durable linked receipt before continuation, restart zero I/O | P `provider_lost_applied_restart_without_io`: actual durable effect, lost response, recovered receipt count before reclaim, fresh provider receives0 calls, shared completion succeeds | Explicit receipt evidence composite provenance assertion; command exact replay after restart; deferred receipt/command failure; wrong company/digest/marker/source XOR direct SQL |
| Applied missing/invalid result remains positive, no output/edge; supported replay veto, later valid receipt | U `applied_result_validation_preserves_positive_truth`; P `provider_applied_without_usable_output_never_downgrades` runs missing/schema-invalid/oversize recovery,0receipt and retry_safe=false, contradicting final proof conflicts | SupportedReplay policy plus these cases through real recovery and command paths; later distinct valid applied receipt; duplicate cannot reinterpret; SQL/Rust full truth-table equivalence; explicit output/edge/successor/attempt assertions |
| Applied attribution old/new/marker/unattributed | U `applied_request_must_belong_to_exact_snapshot` unit matrix; R old actual vs new/recovered canonical receipt establishes old-entry conflict | Legitimate consumed new-entry recovered evidence creates no finality breach; unattributed positive evidence after proof records unattributed_applied; zero-entry marker attribution breach; full scope composite FKs |
| Receipt truth precedence and append-only conflicts; late actual truth never erased | R `old_actual_after_new_receipt_keeps_same_and_different_truth` loops same/different outputs and completed/live execution; R `old_actual_after_recovered_receipt_retains_first_result` retains first output; P priorApplied vs finalAbsence | Different verifiedApplied outputs, applied evidence vs final proof both orders, unknown cannot overwrite positive/final truth; late receipt before consumption, terminal cancellation/expiry vs reconciliation commands; pending reopened job parked |
| Shared conflict veto at receipt-return/reserve/heartbeat/completion; historical completion preserved | R old actual/new receipt test conflicts before complete_io, expects exact action.evidence_conflict retirement and no result; completed history equality/replay preserved | Handler actually receives valid result before conflict, then completes before heartbeat; reserve replay/return_authorized/heartbeat after conflict; assert0committed output/route/successor for live case; competing completion-first/run-lock order |
| One final proof per exact coverage; consumption unique, commits with entry before actual call | R final-a schedule/claim/new-entry sequence projects not_applied then needs_reconciliation; P barrier permits one later actual call; shared new_claim uses two competing claimants | Two concurrent proof commands plus two dispatch claimants; assert1consumption/entry; same-attempt competitors, no processing old attempt; consumption invalidation/expiry/crash before call; final proof uniqueness and every coverage FK |
| Consumed proof cannot be reused after lost retry; fresh proof covers all entries | Existing R projection after consumption catches consumed-state change only | Retry after final proof loses response, restart/ordinary retry refuse reuse, fresh authentic barrier/proof covers both old and new entry; no refund after error/crash/ack loss |
| All invocation siblings safe; receipt-only/all-proof/mixed vs unknown distinguishable;128/129 bounds | H `workflow_action_history_unsafe_sibling_and_129_fact_overflow` tests supported-replay+unsafe sibling and128/129 SQL predicate; U coverage unit matrix tests128/129 entries | Real reconciliation siblings: all receipts, all final proofs, mixed receipt/proof/supported replay, each with unknown/appliedNoResult/conflict;128/129 entries and siblings at snapshot/settlement/schedule/reserve/enter/shared recovery gates; never truncate |
| Existing job only; waiting/reconciliation, non-child, activated+uncompleted, failed lease-free; preserve attempts/debits/frozen operation | P/R positive schedules reclaim existing job through existing claim owner; provider asserts stable key | Every negative: running/human/child/failed/cancelled/succeeded/deadline, runnable sibling/output/route/successor, activation/max_steps/attempt caps, invalid-result/deadline poison/root-budget exhaustion; verify no replacement job, untouched old attempt/debit/lineage/counters; actual claim rechecks budget/current dispatch authority |
| Evidence/conflict generated revision; command receipt stores final revision; refusal/replay no bump | S refused command returns same stored revision; no explicit generated revision assertions | Waiting+terminal Unknown/Applied audit-only revision bumps; stale distinct request refuses; conflict bump, scheduled exact final revision, duplicate/refusal0bump, overflow atomic rollback |
| Terminal truth retained; owning-run serialization cancel/expiry/receipt/command, no reopen or invented edge | S restores terminal snapshot; Q `workflow_action_uncertainty_parked_deadline_cancel_and_late_receipt_truth` retains actual receipt with terminal state and null output/successor (prior04.6) | Authorized reconciliation on terminal runs; competing cancel/expire vs applied/final/unknown commands; terminal conflicting receipt; no success/error edge/automatic reopen; deadline owner remains poll_work/expire_run |
| Full reconciliation transaction rollback | R `sql_actual_receipt_conflict_and_audit_guards` rejects orphan actual observation at deferred commit and retains0facts; Q retirement rollback checks prior04.6 only | Inject deferred command/evidence/receipt/reopen failure; assert evidence,coverage,receipt,command,actor audit,run/job changes and generated revision all rollback while remote effect remains |
| Composite provenance, source XOR, command/evidence linkage both directions, append-only facts | R SQL test checks canonical-receipt observation linkage, nullable-evidence conflict shape, observation UPDATE/DELETE rejection; B current-xid spoof/witness tests | Isolated reconciliation receipt source XOR/foreign evidence/digest/marker/entry/nonApplied, orphan command/evidence exact scope both directions, evidence/coverage/consumption mutation/deletion and proof source constraints; SQLSTATE+constraint identify intended failure, no earlier guard masking |
| Allfive original schema findings corrected behavior | B first OLD state/reason retained; direct witness insertion rejected; explicit historical entry xid8 rejected independently and valid entry accepted; R completed history retained | Full counterexamples: command/evidence scope in both directions; terminal→wait→running and human waiting_reason-only laundering cannot reopen; historical entry consumption retrofit plus explicit consumption/current xid spoof; old/new/unattributed applied attribution; completed execution conflict cannot park/rewrite committed history. Existing narrow B tests do not exercise actual invalid reopen/retrofit |
| Required gates and independent whole criteria/integration review | Current provider/receipt targeted commands recorded below and BRIEF resume evidence; prior04.6 acceptance retained | Stock2MiB allaction+affected admission/control/recovery/budget/lease/completion/pending/maintenance, fmt/whitespace, lockedofflinealltargets, strictClippy, freshschema/allmigrations inspection, SQLxprepare/check, graft build, independent FULL04.7 originalcriteria+actualcode+integration PASS and root acceptance. No fullphase04 claim |

The inventory deliberately leaves the missing discriminatory matrix unimplemented in
this bounded assignment. Existing prior04.6 acceptance does not certify its behavior
against new reconciliation facts; rerun affected suites after the complete04.7 scope.

## Bounded authority / replay / verifier races — resumed next group

`EVIDENCE-04.7-AUTHORITY.md` records seven new isolated real-DB tests and the final
combined17PASS/0FAIL/0ignored stock2MiB run in
`/private/tmp/workflow-04.7-authority-combined-r2.log`. New evidence covers current
owner/admin versus member/outsider, forged identities, UnknownNote/verifiedUnknown
fresh-service duplicates and ordinary retry without new attempts/debits, distinct
command actor versus original receipt-dispatch actor, successful immutable replay
and same-key conflicts, resource/role revocation on replay, paused verification
resource/principal/revision races with lock-release proof, and actual cancel/expire
owners during verification with terminal preservation. No production/schema changes.

The table above remains an original inventory; this scoped evidence narrows but does
not close its authority/verification rows. Authentic valid foreign tenant/association,
operational errors, changed coverage/late receipt, timeout/cancellation ownership,
proof clock/expiry/bounds, actual new-call actor branch and the rest of the full04.7
matrix remain pending. Fresh focused Astra review and whole04.7 review/gates/root
acceptance are required. Detailed exact remaining gaps are in the new evidence file.

## Bounded authoritative proof / one-use / clock group

`EVIDENCE-04.7-PROOF.md` maps seven isolated real-DB tests to genuine barrier
contention, concurrent commands/claims/entries, consumed-proof loss/restart/refusal/
fresh two-entry coverage, legitimate new Applied attribution, original dispatch
principal/resource revocation, expiry at settlement/reservation/enter with immutable
successful replay, and owned bounded/cancellable verification. Final combined24PASS,
0FAIL/0ignored at stock2MiB. Full-table snapshots include all provider/reconciliation
facts. No production/schema edit. This narrows the inventory without closing04.7;
focused independent review, all remaining matrix/broader gates and FULL review/root
acceptance remain required. Siblings/128–129, eligibility and provenance follow.
