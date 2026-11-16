# P02 review record

## 03PA-P02-D1-R1 — D1/r1 architecture gate

- Decision: [D1/r1](logs/D1-r1.md); source digest `263c1d573f0bdb4b75234d625406c1d1560dc7b8fe7c5004bdf21a73b17f2eb6`; base/HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`.
- Outcome: **CHANGES REQUIRED**. No dependent p02 implementation accepted.
- Durable note: [03PA-P02-D1-R1/N1](agent-review-notes.md).
- Open actionable findings: [03PA-P02-D1-R1/F1–F3](agent-review-findings.md): retained personal-history lifecycle (high); identity/principal lock inversion (medium); incoming personal-mail notification producer (medium).
- Coordination revision12 pinned and unchanged; exact artifact exclusions and review coverage in N1. Architecture review is not execution evidence.
- Next step: author resolves all three findings in one revised immutable decision, then requests one recheck. Retain D1/r1 and this original outcome.

## 03PA-P02-D1-R2 — D1/r2 architecture correction recheck

- Decision: [D1/r2](logs/D1-r2.md), SHA-256 `6755c841b5053518ebd03d60d7ce51f9e1858c8aef6acfbfdaf23ea2a9670228`.
- **PASS for D1/r2**, on unchanged source digest `263c1d573f0bdb4b75234d625406c1d1560dc7b8fe7c5004bdf21a73b17f2eb6`, HEAD/base `830561e2a7a4fb859f389fbaf6995a756badb561`.
- Note: [03PA-P02-D1-R2/N1](agent-review-notes.md). [R1/F1–F3 dispositions](agent-review-findings.md): all resolved at decision level, no actionable residual.
- Reviewed §§4–5,8–10 and AC10–AC12; reused unaffected R1 coverage. Live/pinned coordination revision12 and the exact five snapshot exclusions are unchanged.
- Architecture approval permits dependent implementation; it is not a p02 implementation PASS. AC1–AC12 runtime evidence and independent B1 review remain required.

### R2 final-outcome correction before return

The initial persisted R2 PASS draft above is **superseded**: final outcome **CHANGES REQUIRED**. `03PA-P02-D1-R1/F2` is reopened solely for §9/AC11's requirement that both race orders yield exactly one mail commit. A rekey-first stale old-email message must be permitted to return Superseded/refused with no residue; no stale access may be preserved to satisfy that assertion. See N1 addendum and F2 follow-up. F1/F3 stay resolved and the proposed identity lock prefix is sound. No dependent implementation gate was released.

## 03PA-P02-D1-R3 — Current architecture gate: PASS

- **PASS for [D1/r3](logs/D1-r3.md)**, SHA-256 `167adf1a44f4a6de9bc0fe119ff341692cb1bed2722929e047b3731d0a273db0`, on source digest `263c1d573f0bdb4b75234d625406c1d1560dc7b8fe7c5004bdf21a73b17f2eb6`; HEAD/base `830561e2a7a4fb859f389fbaf6995a756badb561`.
- Note [03PA-P02-D1-R3/N1](agent-review-notes.md); [F1–F3](agent-review-findings.md) all resolved at architecture level. R3 fixes the sole R2 residual: correct order-dependent stale-self-mail race assertions in §9/AC11.
- Unchanged lifecycle/notification/lock decisions reuse R2 evidence and unaffected R1 coverage. Coordination revision12 and all five exclusions unchanged; before/after source snapshots match.
- Dependent B1 implementation can proceed under D1/r3. Runtime correctness and p02 completion remain unverified pending implementation checks and independent batch review. Historical R1/R2 outcomes remain preserved.

## 03PA-P02-B1-R1 — Ownership, direct policy and public settings: CHANGES REQUIRED

- Artifact: `776169e238188e2347d093f0bd45fbca8ceae56ca5d1d4b65419929fd028b1b1`; phase start/review base/HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`. Accepted D1/r3 remains current; revision14 coordination pinned in [B1-R1-coordination](logs/B1-R1-coordination.md). Exact five exclusions and full coverage in [03PA-P02-B1-R1/N1](agent-review-notes.md).
- **CHANGES REQUIRED**, findings [03PA-P02-B1-R1/F1–F2](agent-review-findings.md): obsolete verified email regains member authority on rejoin (high); separate outsider's grants/thread places are stranded when current email moves to restored lifetime principal (medium). Both are implementation corrections under D1/r3.
- All36 B1 author-delta paths and affected callers reviewed; independent18 focused tests PASS. Production lifecycle probe reproduced stale-email authority and lost desk access; execution exit101 on the security assertion. Author136 focused tests/build/static evidence inspected; strict clippy and CSS failures independently confirmed inherited and retained, not passed. Reviewer fixture setup failure and corrected18-test run are preserved. No full phase/plan gate claimed.
- B1 remains unaccepted; B2/B3 remain unimplemented/out of scope. Author should fix both findings with focused tests before one correction recheck. No reviewer source mutation. Final frozen identity/coordination check recorded in N1.

## 03PA-P02-B1-R2 — Correction recheck: PASS for B1

- **PASS**, artifact `30f6b67395ab9b9ec3235a5ca1d80bc78e2b596cebd8fc3ea19bec49a5edc7bb`; phase start/review base/HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`; accepted D1/r3 unchanged. Coordination14 pinned in [B1-R2-coordination](logs/B1-R2-coordination.md); exact five exclusions and final identity check in [03PA-P02-B1-R2/N1](agent-review-notes.md).
- [R1/F1 and F2](agent-review-findings.md) **RESOLVED**: obsolete account-email identities are detached before lifetime restoration; current identity adoption preserves outsider grants/thread places under existing reachability rules. Reviewed all five correction paths, affected production callers, lock order and decisive regression assertions; reused unchanged R1 B1 coverage. No new actionable findings.
- Independent current identity/direct regression run: **16 PASS**, including confirmed changed-email rejoin, replacement mailbox account, real grants/thread places, reachable outsider and competing restoration. Author139 focused tests/build/SQLx/source/static evidence inspected. Manifest validated15 PASS/4 retained FAIL declarations; historical regression/fixture failures and inherited clippy/CSS failures remain honestly retained. No whole-suite PASS claimed.
- This permits root's B1 acceptance checks and subsequent B2 work. B2/B3 and overall p02/plan acceptance remain outstanding. Reviewer sessions complete; no source edits or pending builds.

## 03PA-P02-D2-R1 — Personal notification membership pin: architecture PASS

- **PASS for [D2/r1](logs/D2-r1.md)**, SHA256 `e7e6f369f9c7b4554e5674d85c2a9eb0984f9e23bdf7770904d84b38486fa581`; frozen source context `fe26fdf734fecfa8a95ead071597b34362755361505c0f688f08662396adc2f0`, phase start/review base/HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`. Exact exclusions unchanged; coordination14 pinned in [D2-R1-coordination](logs/D2-R1-coordination.md).
- [Note03PA-P02-D2-R1/N1](agent-review-notes.md), [findings record](agent-review-findings.md): no actionable architecture findings. Reviewed immutable membership pin across unprocessed retirement/rejoin, current content/recipient authority, source/event guards and account→company→source→event ordering against existing production paths and D1/r3 AC10/AC12.
- Runtime notification, native guard and both-order race tests remain NOT EXERCISED by this architecture review. Current B2 UI/transport code is not accepted here. Dependent notification implementation may proceed after root identity acceptance, followed by whole-B2 review and required phase/plan gates.

### D2-R1 later correction

Actual projected-retirement test exposed the membership FK deleting notification state. R1's source-pin reasoning did not satisfy projection/read-state retention; see retrospective [03PA-P02-D2-R1/F1](agent-review-findings.md) and D2-R1/N1 follow-up. Original R1 review remains preserved; D2/r2 below is the current architecture.

## 03PA-P02-D2-R2 — Retention-key correction: architecture PASS

- **PASS for [D2/r2](logs/D2-r2.md)**, SHA256 `1a19b9a8d5a17325ff4a41ac06082d8b8519417ec55ddf3d47e4e1a314f7fde2`; source context `9523bf98825e86deec23e90b798d640841a2e715514494c9c23310dfb77f4b22`, base/HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`. Coordination14 pinned, exact five exclusions unchanged.
- [03PA-P02-D2-R2/N1](agent-review-notes.md); [D2-R1/F1](agent-review-findings.md) resolved at decision level. Reviewed generated child membership key, account cleanup, recipient/target principal FK actions, native source/recipient guards and account/company/source lock implications. Unchanged D2/r1 architecture evidence reused.
- Reduced PostgreSQL FK-semantics probe PASS; actual corrected application migration/retention/read-state/deletion/race tests remain outstanding. Original15 PASS/2 FAIL evidence retained; separate mixed-recipient ingress failure unresolved. No B2 source/runtime acceptance implied.


## 03PA-P02-B2-R1 — Direct transport, mailbox and personal notifications: CHANGES REQUIRED

- Artifact `411abe73337fe93ccde3a39560722bca1125df70dcc0c32e106b9a233e59e52e`; base/HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`; coordination14, accepted D1/r3+D2/r2 and exact five exclusions unchanged. Start/final snapshot and live/pinned coordination match.
- **CHANGES REQUIRED**, [03PA-P02-B2-R1/F1 (MEDIUM)](agent-review-findings.md): direct diagnostics return private identifiers after membership removal commits during the payload query. Equivalent exact-message/full-mailbox/post-write responses await activity after their final inbox check. Fix the final read/return boundaries together and add deterministic revocation-during-read checks.
- [03PA-P02-B2-R1/N1](agent-review-notes.md) records all136 delta paths, criterion/consumer/test coverage and limits. Independent selected run61 PASS; standalone real-reader/removal race reproducesF1 and exits101. Author current19+30, broader1023/3/4, migration/offline/SQLx/static evidence inspected; inherited CSP and seven clippy diagnostics remain explicit failures, not B2 defects or green gates.
- No other actionable findings. B2 remains unaccepted pending correction recheck; B3 workflow surfaces, p03 production agent changes and p04 integrated/full-suite gate remain outstanding. Reviewer source untouched; sessions/build complete, reviewer-owned database retained.


## 03PA-P02-B2-R2 — Final read-authority correction recheck: PASS for B2

- **PASS**, artifact `b02007609c01e28674ee6640fe07f6fc115e95879c5916ff01356bcaca8fa7f6`; phase start/review base/HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`. Accepted D1/r3+D2/r2, coordination14 and exact five exclusions unchanged; final source/coordination identity verified.
- [03PA-P02-B2-R1/F1](agent-review-findings.md) **RESOLVED**. Audit, exact-message, full-mailbox and post-write response paths repeat current authority after the final independent payload/activity read. Reviewed all five correction files and decisive race assertions; reused132 unchanged source hashes and R1 coverage. [03PA-P02-B2-R2/N1](agent-review-notes.md) records evidence and limits.
- Independent current direct-UI selection **10 PASS**, including all four new revocation races; actual diagnostics payload lock and controlled HTML activity-reader barriers. Collection4574 =4570+4, no removals; author manifest hashes verified. Author94 affected checks/build/SQLx/source/static evidence inspected. Strict clippy remains FAIL on seven inherited diagnostics; historical CSP/CSS failures retained. No full-suite PASS claimed.
- No actionable residual findings. Root may accept B2 and begin B3 after its identity/persistence checks. B3, p03 integration and p04 final gates remain outstanding; no whole-phase/plan acceptance. No source edits or pending reviewer processes.


## 03PA-P02-B3-R1 — Principal workflows, roles and final delivery: CHANGES REQUIRED

- Artifact `45d7f5be8d17a9fbad70acc3a001084498ecebd7b1d2556ac3eb5de81a69cee5`, phase start/review-base/HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`; accepted D1/r3+D2/r2, coordination15 and exact five exclusions unchanged. Start/final source and live/pinned coordination match.
- **CHANGES REQUIRED**, [03PA-P02-B3-R1/F1 HIGH and F2 LOW](agent-review-findings.md): canonical direct deliveries fail the desk-only decoder and roll back the global claim batch; direct delivery attention events are suppressed by a leftover desk-only trigger gate. Both independently reproduced against current production persistence; standalone positive control proves the batch impact.
- [03PA-P02-B3-R1/N1](agent-review-notes.md) records all 186 delta paths and affected unchanged consumers, criterion coverage, 18 independent PASS, 4588 collected/14 added/0 removed, native schema and baseline-clippy checks, and explicit fixture/negative-test limits. Reviewer production-persistence probe intentionally exits101 on reproduced failure; no source edits.
- Author SQLx/offline/source/static checks inspected; seven inherited clippy errors remain failures. Broader2273-case affected gate is **pending**, final R1 evidence manifest absent at return; neither is claimed green. Root must retain/disposition its final results before correction acceptance. No B3/p02/plan PASS; p03 integration and p04 final gates remain outstanding. Reviewer processes complete and shared compiler cache released.


## 03PA-P02-B3-R2 — Combined delivery decoder and native wake correction: PASS for B3

- **PASS**, exact artifact `1f4af6f63e8f51cd33c0c777ffd62fbeb05d218830a9dbc5b1c6f5a8451398aa`; phase start/review-base/HEAD `830561e2a7a4fb859f389fbaf6995a756badb561`; accepted D1/r3+D2/r2, coordination15 and exact five exclusions unchanged. Final source and live/pinned coordination identities verified.
- [03PA-P02-B3-R1/F1 and F2](agent-review-findings.md) **RESOLVED**. Exact typed targets survive queue decoding and delivery presentation; direct reads enforce current policy before paging and after final payload reads; native direct attention events now emit. Reviewed all 17 correction files and affected consumers, reusing verified unaffected R1 coverage. [03PA-P02-B3-R2/N1](agent-review-notes.md) records proof and limitations.
- Independent current **19 PASS**, including actual direct publication → mixed queue → email worker/recording provider → fenced terminal delivery, native event controls and real reader/removal race. Independent fresh migration PASS; collection 4589 (+1/no removals). Author 235 affected PASS, offline/SQLx/source/static checks and manifest validated. R1's previously pending broader gate is now 2266 PASS/7 inherited ignores. Strict clippy remains failed on seven diagnostics independently matched to immutable HEAD; no full-verification claim.
- No residual actionable findings. Root acceptance checks remain; p03 production integration and p04 full gates are outstanding, so no whole-plan PASS. Reviewer source untouched, processes complete and compiler cache available.
