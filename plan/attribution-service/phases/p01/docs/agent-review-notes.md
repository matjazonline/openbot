# Attribution p01 review notes

## AS-P01-G1-N1 — architecture gate r1

- Date/workflow: 2026-11-15, `implement-astra-reviewed-2-3`, p01, round AS-P01-G1-R1, decision AS-P01-D1/r1. Records: [TASK](TASK.md), [REVIEW](REVIEW.md).
- Reviewer: `/root/attribution_reviewer`; requested Astra/high, no inherited context. Actual runtime model/effort and context occupancy telemetry unavailable. B maintained this durable checkpoint; no agents spawned, no source/design edits, no database/build resources allocated.
- Worktree/branch: `/Users/mac02/orca/workspaces/openbot/attribution-service`, `attribution-service`. Start/review-base/HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`; no pre-existing changes reported, no committed batch delta.
- Digest: `4608c3e7f0a0b36e2b5e480682135dfc1a3bad5d73b3680780fd25dc07a7e6ce`. Exact exclusions: `plan/attribution-service/phases/p01/docs`, `plan/attribution-service/COORDINATION.md`.
- Coordination: authoritative `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md` revision 2; notices AS-C001/r1 and AS-C002/r1, pinned in [AS-P01-G1.coordination.md](logs/AS-P01-G1.coordination.md). Read at start and rechecked before return; retained copy compared byte-for-byte. No changed applicable notice found.
- Decision evidence: [immutable r1](logs/AS-P01-D1-r1.md), compared byte-for-byte against `services/attribution/contract/README.md`.

### Coverage and evidence

Read the ten original numbered phase requirements, their API-first/Sign in with Openbot amendments, the root AGENTS.md, skill acceptance/reviewer/evidence references, and TASK decision, acceptance mapping and contract-impact inventory. Reviewed the full proposed README and plan amendments for ownership, identity and credential separation, PKCE/code replay, session revocation, company scoping, lifecycle ordering/freshness, durable acknowledgements, checkout selection/freezing, event/order/refund identity, exact money, reporting clocks, consent, and retention horizons. Deferred native providers and production deployment choices remain explicit; neither is a gate defect.

Trail-first independent source inspection verified existing session liveness at `src/adapters/persistence/session.rs:38–57`, company access at `src/application/use_cases/company.rs:190–246`, direct company deletion at `src/adapters/persistence/company.rs:104–138`, and account deletion at `src/adapters/persistence/user_lifecycle.rs:11–17,74–118`. `trail callers delete_company_with_cleanup`, `trail callers finish_account_deletion_on` in both directions, and exhaustive `trail grep 'DELETE FROM companies'` established two distinct production deletion owners. Source inspection established the impact-map gap in F1; no claim of an implemented attribution defect is made.

TASK AC1–AC5 broadly distinguish schema checks from runtime proof, and correctly retain unexercised p03–p10 obligations. Its lifecycle and commerce impact rows need the discriminating cases in F1/F2. The planned single partial-refund ledger and generic competing-refund checks cannot alone prove same-refund/different-event idempotency. The direct-company-delete checks cannot prove account-delete lifecycle publication. Contract validation/OpenAPI examples remain future p01 work and are not accepted by this gate.

Executed read-only checks: frozen snapshot at start; branch/HEAD; `git diff --check` (PASS); byte comparisons of immutable decision and coordination copies (PASS). Final snapshot/freshness check retained in [reviewer checks](logs/AS-P01-G1-R1-reviewer-checks.txt). No Rust/database/browser/contract behavioral test was executed: implementation does not exist and architecture review was explicitly read-only. No manifest is required for this stage. No scope was silently passed on test names or author claims.

### Outcome

**CHANGES REQUIRED** for AS-P01-D1/r1. Open actionable findings: [AS-P01-G1-F1 and AS-P01-G1-F2](agent-review-findings.md). No optional suggestions. No architecture or implementation PASS has been granted. A correction recheck should review revised decision/mappings for these issues and any consequential changes; unaffected inspection evidence may be reused if the artifact and coordination support it.

### Process and instruction feedback

Batch size was suitable and the cross-application identity/lifecycle/commerce gate is justified before dependent schema/code. Original plan checks were useful for detecting the refund invariant omission. The impact inventory named one deletion owner but omitted an equivalent SQL writer. The existing verification-contract instruction already requires exhaustive producers/consumers/equivalent surfaces; no new general rule is supported. Apply that rule with an exhaustive stored-data mutation search before treating a named lifecycle caller chain as complete. Evidence: F1, the two production `DELETE FROM companies` writers. Likewise distinguish event-id deduplication from business-fact deduplication when mapping financial retries (F2); this is already explicit in plan 07B.4.

## AS-P01-G2-N1 — architecture correction recheck r2

- Date/workflow: 2026-11-15, `implement-astra-reviewed-2-3`, p01, round AS-P01-G2-R1, decision AS-P01-D1/r2. Records: [TASK](TASK.md), [REVIEW](REVIEW.md).
- Reviewer: `/root/attribution_reviewer`, requested Astra/high, independent context retained from G1; actual runtime settings/context occupancy metadata unavailable. Checkpoint saved here; no source/design edits, agent spawning, or database/build resource allocation.
- Worktree/branch: `/Users/mac02/orca/workspaces/openbot/attribution-service`, `attribution-service`. Start/review-base/HEAD remain `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
- Digest: `8e9c4d5b9f3e5904144472410e2a7d338a02f5d2fe68167eaa7730952338f4ed`; exact exclusions `plan/attribution-service/phases/p01/docs`, `plan/attribution-service/COORDINATION.md`.
- Coordination: authoritative `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`, revision 2, AS-C001/r1 and AS-C002/r1. Read at recheck startup and compared again before return with [G2 immutable copy](logs/AS-P01-G2.coordination.md); unchanged.

### Coverage and checks

Independently compared [r1](logs/AS-P01-D1-r1.md) and [r2](logs/AS-P01-D1-r2.md). The only decision changes add the equivalent account-deletion producer and explicit refund business-fact uniqueness/conflict semantics. Reused unchanged G1 original-requirement and source-owner evidence, including the two distinct production company deletion paths. Checked TASK's revised decision, AC6/AC7 and lifecycle/commerce impact rows against both findings and the original 03C and 07B.4 requirements.

For F1, r2 requires every owned company's terminal version/notification to survive account deletion in the same transaction, rollback to publish nothing, and existing cleanup/revocation obligations to remain. AC6 distinguishes successful/rolled-back account deletion, remote purge and delayed provisioning. For F2, r2 scopes refund identity by company/store/external refund ID independently of delivery identity, binds one order, forbids a second debit and conflicting replacement, and extends those rules to competing deliveries and pre-payment reconciliation. AC7's sufficient-balance E1/E2 case distinguishes duplicate-refund application from over-refund prevention, and E3 distinguishes immutable conflict handling.

Executed: initial and final frozen snapshots; byte equality of current contract to immutable r2 and current coordination to G2 copy; branch/HEAD; `git diff --check`. All PASS; [retained final output](logs/AS-P01-G2-R1-reviewer-checks.txt). No behavioral tests were run or claimed; architecture-only review has no implementation evidence manifest. Runtime implementation/assertions remain explicitly Not exercised in TASK and must be verified by their owning phases.

### Outcome and process

**PASS for AS-P01-D1/r2**, digest `8e9c4d5b9f3e5904144472410e2a7d338a02f5d2fe68167eaa7730952338f4ed`. Findings AS-P01-G1-F1 and AS-P01-G1-F2 are **FIXED at the architecture/verification-plan level**. No new findings or optional suggestions. This does not accept the complete p01 implementation batch or any runtime claim.

Both actionable corrections arrived together in a narrow delta; reusing unaffected G1 evidence was appropriate. The revised cases now distinguish the failures identified in G1. No additional instruction change is supported by this correction round.

## AS-P01-B1-N1 — whole contract batch

- Date/workflow: 2026-11-15, `implement-astra-reviewed-2-3`, p01, round AS-P01-B1-R1; [TASK](TASK.md), [REVIEW](REVIEW.md).
- Reviewer: `/root/attribution_reviewer`, requested Astra/high, independent context retained; actual runtime model/effort and context occupancy telemetry unavailable. This note is the durable checkpoint. No agents, source/design edits, shared databases or builds.
- Worktree/branch: `/Users/mac02/orca/workspaces/openbot/attribution-service`, `attribution-service`; start/review-base/HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
- Digest: `21bacb27bc5ba0b0d0394310910cbc41e2fc8ba448934876a5a4e9d8196b9001`; exact exclusions `plan/attribution-service/phases/p01/docs`, `plan/attribution-service/COORDINATION.md`.
- Coordination authority `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`, revision 2, AS-C001/r1 and AS-C002/r1; read at startup and compared at return with [immutable B1 copy](logs/AS-P01-B1-R1.coordination.md). No changed requirements.

### Coverage and evidence

Reviewed the complete phase01 deliverable: API-first plan amendments; README compared with accepted AS-P01-D1/r2; all identity and attribution operation security requirements, parameters, request/response schemas, status/bounds declarations; all examples; pinned Python dependency file; standalone check wrapper; and all nine test bodies. Used compact JSON extraction to read every schema and operation without omitting fields. The canonical identity contract and portable copy are byte-equal. Root AGENTS applies; no more-specific guide exists in the new service/contracts paths. Trail-first discovery found the new validation module unindexed; direct reads then covered that module. Additional Trail inspection confirmed the existing Openbot cookie name is `session`.

Checked TASK AC1–AC7 and the actual evidence mapping against original 01A/01B and later consumers in phases03–09. The schemas preserve separate credential classes, live-authority obligations, store-scoped commerce, immutable lifecycle/refund identities and exact money. The new owner/admin status-control operation stays in Openbot's session/CSRF boundary; it does not authorize arbitrary-user grants or terminal deletion. Reused G1/G2 source-owner and architecture evidence where unchanged. Reporting and retry consumers revealed F1/F2 despite structurally valid schemas.

Executed `ATTRIBUTION_CONTRACT_PYTHON=/tmp/attribution-service-contract-env/bin/python services/attribution/scripts/check-contracts.sh`: **PASS 9/9**, 2.022 seconds in the tool result. Inspected the saved author inventory and in-place/standalone outputs (9 selected and 9 passed in each). Executed `check-evidence.py` against [the manifest](logs/AS-P01-B1-R1.evidence.json): all four entries retained and schema-valid. The manifest validator does not establish behavioral completeness. Host urllib3/LibreSSL warning was present; validation uses local schema references, and no tests skipped.

Executed independent schema probes retained in [AS-P01-B1-R1-reviewer-probes.json](logs/AS-P01-B1-R1-reviewer-probes.json): listed create/rotation parameters and request fields; confirmed a base ReportGroup validates while adding reason/refund-count metrics fails; checked FinancialTotals has no alternative fields for those metrics. These are observations about the wire artifact, not runtime defect reproductions. Snapshot/HEAD/canonical-copy/coordination/whitespace checks and manifest validation are retained in [reviewer checks](logs/AS-P01-B1-R1-reviewer-checks.txt).

No Rust, database, browser, live authorization/ledger, native-provider or deployment checks were applicable to this contract-only batch. Those remain required later. No tests were removed. Coverage was thorough for the wire artifact; no runtime implementation exists to inspect or accept. The pending AC6 runtime tests are correctly distinguished from accepted architecture.

### Outcome and process feedback

**CHANGES REQUIRED**, findings AS-P01-B1-F1 and AS-P01-B1-F2, both P2. No optional suggestions. The evidence supports schema validity and arithmetic/fixture consistency, but the generic bounds/security tests do not verify the missing retry identity or reporting metrics. Correction should include discriminating contract checks and update affected mappings.

The batch size was suitable. No new general instruction is needed: existing criterion-to-production-path/assertion guidance already requires response schemas and retry consumers to be checked, beyond OpenAPI meta-schema validity. F1 also clarifies a limitation of the earlier gate: its generic retry wording passed before concrete creation/rotation schemas existed; the completed wire contract now exposes that mismatch. This later discovery is linked below without rewriting the earlier PASS.

### Follow-up to AS-P01-G2-N1 — 2026-11-15

AS-P01-B1-F1 identifies a gap exposed by the later complete API: AS-P01-D1/r2's general retry advice cannot be followed by several new mutation contracts because they define no operation identity. The G2 architecture PASS did not verify those absent schemas. Retain the earlier outcome; correct the retry contract and review its affected semantics before p01 acceptance.

## AS-P01-B1-N2 — correction recheck and D2/r1 gate

- Date/workflow: 2026-11-15, `implement-astra-reviewed-2-3`, p01, round AS-P01-B1-R2; [TASK](TASK.md), [REVIEW](REVIEW.md).
- Reviewer: `/root/attribution_reviewer`, requested Astra/high, independent context retained from earlier rounds; actual runtime model/effort and context telemetry unavailable. Durable checkpoint saved; no workers, project source/design edits, shared database or build allocation.
- Worktree/branch: `/Users/mac02/orca/workspaces/openbot/attribution-service`, `attribution-service`. Start/review-base/HEAD `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
- Digest: `f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02`. Exact exclusions: `plan/attribution-service/phases/p01/docs`, `plan/attribution-service/COORDINATION.md`.
- Coordination authority `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`, revision 2, AS-C001/r1 and AS-C002/r1; read at startup and rechecked against [immutable R2 copy](logs/AS-P01-B1-R2.coordination.md) before return, unchanged.
- Architecture decision: [immutable AS-P01-D2/r1](logs/AS-P01-D2-r1.md), compared with current README. Accepted D1/r2 and prior source/requirement coverage reused for unaffected invariants.

### Coverage and evidence

Reviewed the correction in README, every affected management operation and response, new reason/refund schemas, Openbot status CAS request, both new fixtures, and all three added test bodies. Inspected TASK D2/r1, AC8/AC9 and their later-phase runtime obligations. The correction does not rely on schema validation as proof of implemented authorization or concurrent durability.

For B1-F1, every `/v1/` management mutation accepting a session now requires original Idempotency-Key/Created-At, with declared receipt identity, current authority checks, conflicting payload/time refusal, and atomic receipt/mutation. The 24-hour acceptance horizon, 30-second future tolerance and minimum 48-hour receipt retention avoid reapplying an unchanged expired request after pruning. UUID reuse with a changed original time is forbidden. Credential creation/rotation distinguishes first 201 secret delivery from 200 metadata-only replay, rejects secret-bearing replay payloads, and defines revoke/reissue recovery without repeated rotation or overlap extension. OAuth code ambiguity restarts login; status CAS/reread avoids repeating stale state changes. Examined these persistent decisions as the explicit D2/r1 gate, not routine documentation edits.

For B1-F2, ReportGroup requires a closed reason-count object covering all seven unattributed reasons, and FinancialTotals requires distinct refunds per currency. README defines distinctness, clocks, filters and overlapping product groups. The report fixture and test distinguish reason changes at equal order totals and one/two monetary-only refunds at equal money/returned-unit totals; missing fields are rejected. Current schemas can represent the required metrics. Actual query aggregation and replay behavior remain pending runtime proof in p04/p05/p08 and integration phases.

Executed the contract suite once against this revision: **12/12 PASS**, retained in [reviewer test output](logs/AS-P01-B1-R2-reviewer-tests.txt). Inspected author listing/standalone audit: nine old names preserved, three added, standalone 12/12 PASS. Validated [R2 manifest](logs/AS-P01-B1-R2.evidence.json): two retained PASS entries, no schema/identity failures. Snapshot, branch/HEAD, whitespace, canonical identity-copy equality, immutable decision equality, coordination freshness and manifest validation are retained in [final checks](logs/AS-P01-B1-R2-reviewer-checks.txt). The existing LibreSSL warning did not skip local assertions or imply HTTP tests.

### Outcome and feedback

**PASS for architecture amendment AS-P01-D2/r1** and **PASS for p01 contract batch digest `f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02`**. B1-F1 and B1-F2 are **FIXED** for this wire-contract scope. No new findings or optional suggestions. No Rust/database/browser/live feature acceptance is implied; those checks remain required for subsequent implementation.

The corrections were cohesive and sufficient for a delta recheck. Explicitly reopening a gate for the new persistent receipt/retention contract was appropriate. The new assertions distinguish the previously missing wire behavior; no additional instruction change is supported.
