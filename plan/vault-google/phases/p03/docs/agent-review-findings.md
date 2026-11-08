# Phase 03 reviewer findings

## VG-P03-AG1-R1 — No actionable architecture findings

2026-11-07; implement-astra-reviewed-2-2; p03 architecture stage; independent B `/root/reviewer` requested Astra/high (actual runtime settings not independently exposed). Worktree `/Users/mac02/.codex/worktrees/1133/openbot`, base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`, baseline digest `34d6a2e7933a26523842f46e10a29afe5b97bfac9ab334c7eb06accf63cb3558`.

D1/r2 SHA256 `a33eb50f9ae21e674f8bd20edb989032400ade440a79df70635c42b8db21e15b` passes the architecture gate. No actionable defects or optional design changes identified. Implementation, race tests, and C002 implementation satisfaction are still pending; this is not a code acceptance.

[Review note](agent-review-notes.md#vg-p03-ag1-r1--architecture-pass-for-d1r2), [review](REVIEW.md), [decision](logs/VG-P03-D1-r2.md).

## VG-P03-B1-F1 — Final gate accepts authority that expires during module wait

- Date/workflow/round: 2026-11-07, implement-astra-reviewed-2-2, VG-P03-B1-R1 whole-phase code review; [TASK](TASK.md), [REVIEW](REVIEW.md), [note](agent-review-notes.md#vg-p03-b1-r1--whole-phase-implementation-changes-required).
- Reviewer: independent `/root/reviewer`, requested gpt-6-astra/high; actual runtime settings not independently exposed. Phase p03, worktree `/Users/mac02/.codex/worktrees/1133/openbot`; review-base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; artifact `a8326ab3d24c3f3283ceb91015b8590c4a978c898b8a42ce8c9ea440102882ba`; governing D1/r2 SHA `a33eb50f9ae21e674f8bd20edb989032400ade440a79df70635c42b8db21e15b`.
- Classification: actionable defect, **high**, correctness/security (time-sensitive dispatch authority).

Evidence: `src/adapters/persistence/workflow/integration_authority.rs:214–256` calls `current()` before `module.check()` and then only rolls back the host transaction. `current()` delegates to `action_dispatch.rs:96–169`, where `lease::live_window` is checked before the later module call. `src/adapters/persistence/integrations/grants.rs:302–346` checks invocation deadline before acquiring module locks and does not recheck temporal authority after waiting. A row lock prevents replacement/revocation transactions from changing locked rows, but does not prevent `lock_expires_at`, run deadline or invocation deadline from passing. The existing ordinary remote entry path (`action_dispatch.rs:577–627`) obtains a final live window and carries its bounded deadline; this new gate drops that temporal protection.

Observed reproduction: copy of the exact frozen `src/` tree under reviewer-owned logs, with only one appended test (all other source bytes verified identical). The test creates a real persisted HTTP action/grant/context; holds the account's module-owner row lock; sets the real job lease to expire in one second; starts `final_dispatch_guard`; waits for its backend to block; releases the module lock after1.3seconds. The database then confirms the job lease expired, yet the gate returned **Ok(())**. One test ran and failed its denial assertion, exit101. [Test source](logs/reviewer-lease-regression.rs), [output](logs/reviewer-lease-repro.log), [exact-copy/command receipt](logs/reviewer-lease-repro-evidence.json), [copied test-bearing source](logs/reviewer-lease-repro-source.tar.gz). The temporary full copy was removed; implementation source was never edited. Network guard rejected external destinations; no provider calls were made.

Required behavior/correction: success must remain bounded by the current workflow fence/lease, run deadline and invocation deadline after every potentially blocking final-gate wait. Preserve host-before-module lock ordering, module revocation linearization, and cancellation; do not authorize using a lease/deadline that expired while waiting. Add a real contested-lock regression for lease expiry and corresponding deadline/cancellation cases, including a valid unexpired control. A later06 transport gate must not be relied on to make03's required live-authority port truthful.

Origin and instruction lesson: this is an implementation failure to preserve original03's immediately-before-dispatch/current-fence contract. D1/r2 already requires a live fence and a successful combined current decision; its architecture review did not explicitly call out time passing under locks. Candidate scoped lesson for cross-module authority callbacks: lock-based freshness protects mutations, not wall-clock expiry; inspect and test temporal authority at the end of blocking composition. No instruction edits are authorized by this record.

Resolution history: **OPEN** in VG-P03-B1-R1. Correction and independent recheck required before phase acceptance.

### VG-P03-B1-F1 resolution — FIXED in VG-P03-B1-R2

2026-11-07; exact corrected source `ef36f90e6587c12bf9d38541d612a31945101a7d7fde5c782ef3c8df0a7175b9`, base/HEAD97e5d60dc538e26c7cb976a8d63772eed80baff2, same worktree/roles and D1/r2. Independent recheck confirms the host checks the live lease/run window after the module callback; the complete gate additionally enforces invocation deadline and cancellation. Host-first ordering and revocation linearization remain intact.

B independently ran five real contested-lock tests covering expired lease/run/invocation, cancellation, and an unexpired successful control:5 passed,0 failed,0 ignored. [Command receipt](logs/reviewer-temporal-R2.json), [output](logs/reviewer-temporal-R2.log), [correction](logs/correction-R2.diff), [note](agent-review-notes.md#vg-p03-b1-r2--correction-recheck-pass). R1's failing regression and original finding are preserved. **Disposition: FIXED.**
