# 02WR-P04-D3-R1 independent architecture inspection

2026-11-12; independent B; complete D3/r1 supplement only. Partial p04 source43e2e0dfb959a236ffb0c2aeefa4dbe7d899685d21ddaf5a88d7cacd5a405721 is not implementation-accepted.

Premise independently confirmed against actual sources:
- department.rs142–158 calls agent_publication::lock_company_on50–61: real actor user SHARE, then workflow/authority.rs45–85 company NO KEY UPDATE and member/principal SHARE.
- department_desks.rs230–274 subsequently requests exclusive desk gates and channel rows while company/principal locks remain. channel_gate.rs58–86 sorts/deduplicates all keys and uses shared admission versus exclusive settings transaction advisory locks.
- inbound.rs93–157 acquires shared gates before settings/binding/provider/thread work. Therefore the new D1 blocking gate→company path could wait on the writer while that writer waits on the gate. An account-retirement third party adds the stated account→company edge. This is a static source proof, not an executed deadlock.

D3's sorted real-account SHARE NOWAIT then company NO KEY UPDATE NOWAIT removes blocking edges from a held agent gate to these authority owners. A contention error requires rollback of the whole transaction, awaited before separate deferral; a savepoint/inner retry would not discharge the held-gate obligation. Missing rows are not treated as busy. Exact account/route/purpose revalidation after acquiring the company preserves current authority, and a changed required set cannot introduce a late account or gate. Once company is owned, D1's company→source/fence ordering remains. Ordinary person/control/legacy projection is not silently moved under a gate.

Actual queue/worker integration inspected:
- notification.rs133–174 increments attempts in claim and commits the claim before returning; event row is not held during subsequent projection/gate wait.
- notification.rs250–276 failure uses execution+owner identity and processing state, clears lease fields, applies capped exponential backoff, and dead-letters at max attempts. It cannot update a replacement execution. D3 must honor the returned ownership result, not report Deferred unconditionally.
- notification.rs514–543 read/locked completion validates execution/owner and current wall-clock lease. Completion must preserve this actual expiry check even if gate acquisition consumed most of the lease. Reap278–294 is the recovery path; cap and queue status remain finite.
- worker103–168 currently handles Applied/Stale/LeaseLost and invokes fail on Err;216–235 fails the event through the port. A new explicit Deferred branch is necessary to avoid running failure twice after adapter-owned backoff. Failure of separate backoff persistence remains Err/operational failure; no applied receipt is claimed.
- schema4136–4172 bounds attempts/error detail and allowlists error classes; the proposed safe new classification must be admitted there and in the exhaustive consumers. No second queue or immediate gate-held retry is proposed.

Canonical commit integration inspected:
Inbound143–193 reuses sorted gates and checks settings revision;291–353 verifies real association binding;360–386 orders provider keys;480–576 locks threads then resolves participant identities;608–645 resolves author/participants and atomically stores canonical message;712–769 creates intents before delivery/final claim. Thread/mod.rs234–274 calls the identity resolver. These private-envelope/association producers must be included in the complete predeclared gate/account inventory, as D3 expressly requires. Re-entering hold_channel_settings must request only the already-owned gate set. The architecture does not approve an unexamined late identity/account acquisition or change generic ingress semantics.

Decision/verification assessment:
The actual both-order role-writer/notice cases and real three-claimant account-retirement schedule discriminate the reported cycles. Exact message/intent/origin/root assertions and ordinary-user multi-binding checks preserve accepted D1 targeting. Lease loss while waiting/deferring, stale versus missing hints, competing workers, poison iteration and single Deferred handling cover the new recovery surface. Tests are required before B1; none were run here. Permanent route loss stays original-case handling failure, and retry cannot switch the captured binding/version/account or remove origin suppression. D2 audit ownership/label decisions remain unchanged.

No remaining architecture finding. Keep source/caller inventory and actual lock-boundary evidence in B1 rather than relying on this static gate. Read-only inspection, source snapshots and coordination pin cmp only; no DB/Cargo/SQLx/browser/source mutation.
