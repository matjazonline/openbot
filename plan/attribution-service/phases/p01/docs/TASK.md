# Attribution service p01 — boundaries and contracts

## Identity, authority and current status

- Original request: implement all ten numbered phases of `plan/attribution-service`
  using `implement-astra-reviewed-2-3`, in dependency order.
- Subsequent user scope: API-first store-neutral commerce, native store adapters
  and real-store pilot later; Sign in with Openbot; confirmed implementation policy
  in AS-C002/r1. Root explicitly confirmed as Sol 6.1/xhigh.
- Workflow/skill directory: `/Users/mac02/orca/workspaces/openbot/attribution-service/.agents/skills/implement-astra-reviewed-2-3`.
- Phase: p01. Root/A: `/root`, Sol 6.1/xhigh (user-confirmed). B: `/root/attribution_reviewer`, requested Astra/high, no inherited conversation.
  No separate author. Gate AS-P01-G1 assigned; frozen digest
  `4608c3e7f0a0b36e2b5e480682135dfc1a3bad5d73b3680780fd25dc07a7e6ce`.
- Worktree: `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
  `attribution-service`. Start/review-base/HEAD all
  `2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`. No pre-existing changes at task entry.
- Record root: `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/phases/p01/docs`.
- Exact snapshot exclusions: `plan/attribution-service/phases/p01/docs`,
  `plan/attribution-service/COORDINATION.md`. Deliverable contracts/plans are included.
- Coordination authority: `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`.
  Revision 2 checked at batch startup; AS-C001/r1 and AS-C002/r1 acknowledged.
  No pending cross-phase requests. No earlier phase prerequisite.
- Implementation: phase-01 contract batch complete. Review: architecture AS-P01-D1/r2
  passed in AS-P01-G2-N1; whole-batch review next. Verification: 9 contract checks
  passed in place and in standalone copy. Accepted/integrated: no, pending batch pass.
- Effective context capacity/occupancy/compaction controls unavailable; host defaults
  with durable checkpoints. No global configuration changed.
- Owned paths: plan scope amendments, service `contract/`, canonical Openbot wire
  contract under `docs/contracts/`, phase records. No product source/schema change yet.
- Owned resources: no test database/port yet; contract tooling uses `/tmp/attribution-service-contract-env` (phase-owned). Only 13 GiB free disk at startup; use modest
  isolated build profiles/caches, do not delete unrelated caches or raise resource bounds.
- Baseline discovery/evidence: [initial record](logs/AS-P01-initial-discovery.md).
  Prior documentation checkpoint digest
  `0a3f44130cada7b6467926621da786841d0af591c1d52fd94ffdfb946080305f` is historical.

## Original sources and acceptance

The ten numbered files are the original requirements, amended solely for the
user's recorded scope/policy choices. The coherent p01 batch covers 01A and 01B:
network APIs, authentication sequence, credential/tenant types, error/retry/bounds,
and a concrete two-ad/multi-product/refund ledger. Native provider readiness is
explicitly deferred, not passed. Do not begin p02 until this batch is accepted.

## AS-P01-D1 / r2 — accepted architecture gate

Authoritative decision text is `services/attribution/contract/README.md` at the
frozen gate digest. Retain that exact revision in logs before requesting review.
This gate covers new cross-application identity/authorization, lifecycle and
commerce contracts before dependent code/schema exists:

1. Dedicated OAuth authorization code + S256 PKCE using the existing Openbot
   session owner; confidential fixed client/callback, one-use 60-second codes,
   opaque grants tied to original session/auth revision. Not a general OIDC issuer.
   Attribution stores grants encrypted, owns separate revocable sessions, and checks
   live account/company access on every protected user operation.
2. Reuse current company use-case authority. Confirmed owner/admin/member mapping;
   each operation binds active company and each child store. Scoped merchant key
   cannot impersonate a user; public IDs cannot authorize commerce.
3. Openbot monotonic lifecycle/outbox joins both direct company deletion and
   account deletion's owned-company transaction, preserving integration/memory
   cleanup and surviving account/company cascades. Rollback publishes nothing;
   retained tombstones prevent delayed recreation. Five-minute local freshness
   starts at authoritative request initiation; stale deliveries cannot extend it.
   Local lifecycle and tenant writes serialize through the tenant key.
4. Store-controlled checkout freeze chooses last eligible consented candidate.
   Event/order/refund identities are separate; envelopes and immediate changes
   commit together. Per-order serialization and immutable paid attribution prevent
   duplicate sales/reassignment; out-of-order facts remain durable and reconcilable.
   Refund business identity is company/store/refund ID independently of delivery
   event ID; identical facts replay once, changed facts/order conflict, including
   concurrent and before-payment delivery.
5. Exact integer money and independent aggregation preserve currency, product,
   event-time and refund semantics. Retention config must cover accepted delivery,
   retry and refund horizons; old inputs cannot resurrect pruned facts.

Failure cases and exact limits are in the decision text. Potentially expensive
schema/locking details are revisited in the owning implementation phase; this gate
approves the wire invariants, not unimplemented concurrency or security claims.

## Acceptance mapping

| Criterion | Production path and discriminating case | Decisive assertion | Planned evidence | State |
| --- | --- | --- | --- | --- |
| AC1 / 01A.1–3 | Every credential and cross-app path | Separate OpenAPI security schemes; no arbitrary actor ID grants authority; bounded errors/retries | OpenAPI validator and `attribution_contract_` schema/security cases | Not exercised |
| AC2 / 01A.4 | Two companies/two stores; multi-company user | Schema/examples distinguish active company and nested store; foreign combinations have explicit denial contract | Contract examples; runtime negative tests in p03–p08 | Not exercised |
| AC3 / revised 01B | Openbot code bridge and generic merchant API | Fixed authority/client/callback; replay and current revocation obligations; no native provider claims | B architecture review then actual two-application tests in p03/p10 | Awaiting gate |
| AC4 / 01B | Ad A then B, two products and partial refund | One paid order attributed to B; exact line/tax/shipping/refund totals without currency addition | Independent ledger fixture assertions | Not exercised |
| AC5 / confirmed policies | Consent, status expiry and storage uncertainty | Explicit consent, 30-day eligibility, 5-minute freshness and 503 recorded | Contract assertions; runtime boundary/race tests in owning phases | Requirements resolved; runtime not exercised |
| AC6 / 03C, G1-F1 | Actual account deletion deletes owned companies | Committed terminal lifecycle/outbox survives cascades; rollback publishes nothing; remote purge/tombstone rejects delayed provisioning | Explicit contract case, actual account-deletion/remote lifecycle tests in p03/p10 | Not exercised |
| AC7 / 07B.4, G1-F2 | Same refund R delivered by E1/E2, enough balance for two; E3 changes facts | One refund fact and one decrement; E3 conflicts; holds concurrently and before payment | Contract fixture; real ledger/reconciliation races in p07 | Not exercised |

Contract tests establish wire/schema/fixture consistency only. They do not prove
live session security, PostgreSQL serialization, platform checkout behavior or
production SLOs. Those retain explicit later-phase checks.

## Contract impact and traced ownership

| Contract | Producers/consumers | Inherited obligations and disposition | Focused runtime checks |
| --- | --- | --- | --- |
| Identity grant | Existing session/user use cases → bridge → attribution session → authority API | Existing `auth_revision` and originating session liveness fence grants. No shared cookie/secrets. | Code replay competitors, session/security-change regressions in p03 |
| Company access | `readable_company`/`managed_company` → authority API → scoped service resources | Existing read/owner/admin semantics retained; no switcher-cap authorization. | Multi-company foreign IDs and role removal/downgrade |
| Lifecycle | Both `delete_company_with_cleanup` and `delete_user_account` → `finish_account_deletion_on` transactions → durable outbox → local status/tombstone | Existing integrations/memory cleanup remains atomic; all owned-company deletion evidence survives account/company cascades; no callback-only delivery. | Direct and account deletion commit/rollback, remote purge, delete/provision races, stale delivery, crash/reconciliation |
| Commerce | Merchant-controlled checkout → key-authenticated events → immutable ledger → reports | No public sales; company/store from key; exact money, order/refund/event identity; refund ID uniqueness independent of delivery ID. | Competing events/refunds, E1/E2 same R with enough balance for both and changed-fact E3, before-payment reconciliation, commit ambiguity |

Repository graph and exact source references are retained in initial discovery.
Additional live trace: `src/adapters/persistence/session.rs:38–57` ties user session
revision to current account revision and revocation/expiry; reuse this owner.
Correction trace: exhaustive `trail grep 'DELETE FROM companies'` identifies two
production deletion owners; `trail callers finish_account_deletion_on --depth 4`
and exact `src/adapters/persistence/user_lifecycle.rs:74–118` inspection confirm
account deletion's separate transaction. Other direct SQL matches are fixtures.
All applicable root, src, application, HTTP and infra instructions have been read;
read persistence/pages instructions fully before touching those owners.

## Batches, checks and final gate

- p01 one contract batch. Architecture gate AS-P01-D1/r1 first, then complete
  OpenAPI artifacts, fixtures, validation and one whole-batch review.
- List nonzero `attribution_contract_` selection, run contract validation and
  fixture assertions, `git diff --check`, and plan-link/policy registration audit.
  Retain exact commands/counts in logs and an immutable evidence manifest.
- Each later phase runs focused behavior/interface checks with real isolated
  databases when required. No zero-test filter or missing-database skip counts.
- Final p10 executes both applications' full current CI, formatting, offline
  compilation, migrations/SQLx metadata, database, browser, boundary/extraction,
  network isolation, container, restore and integration/load gates. Native-store
  pilot is deferred by user scope; unavailable checks remain reported gaps.
- No commits, deployment, external messages or production resets implied.

## Next action

G1 returned changes required: AS-P01-G1-F1 (P1, missing account-deletion surface)
and AS-P01-G1-F2 (P2, refund business identity). Root corrected both in r2. B closed both in AS-P01-G2-N1 and explicitly passed
AS-P01-D1/r2 at digest `8e9c4d5b9f3e5904144472410e2a7d338a02f5d2fe68167eaa7730952338f4ed`.
The original r1 and r2 decisions remain in logs.

Freeze AS-P01-D1/r2 and request independent Astra/high architecture recheck. After
pass, finish the contract batch, run focused checks, obtain its artifact pass,
then proceed to p02. Preserve original decisions and all review evidence.

## B1 implemented artifact and verification

- Portable service wire contract: `services/attribution/contract/attribution.openapi.json`.
  Identity authority: `docs/contracts/attribution-identity.openapi.json`; byte-equal
  portable service copy `contract/openbot-identity.openapi.json`.
- Both are OpenAPI 3.1; schemas have bounded fields/collections, separate credential
  schemes, scoped selectors, response errors/retries and distinct event/business IDs.
  Cookie/CSRF/authority scope obligations are explicit; schema validation is not
  claimed as runtime authorization proof.
- `contract/README.md` contains the accepted protocol decisions and validation usage.
  Concrete examples cover two companies/four stores, all seven credential classes,
  expected permission denials, one checkout/two ads/two products, exact partial
  refund and distinct event IDs for one refund, plus a conflicting refund payload.
- Developer-only contract tooling uses `contract/requirements.lock` (fully pinned
  installed Python packages) and standalone `scripts/check-contracts.sh`.
  The canonical identity-copy equality check must enter integration CI in p02/p03;
  standalone validation intentionally has no dependency on parent repository files.
- Listed and executed 9 named `test_attribution_contract_` checks. Nonzero inventory:
  `logs/AS-P01-B1-tests-list.txt`; final result `logs/AS-P01-B1-tests-final.txt`.
  Command: `ATTRIBUTION_CONTRACT_PYTHON=/tmp/attribution-service-contract-env/bin/python services/attribution/scripts/check-contracts.sh`.
  PASS, 9/9; earlier pass retained in `logs/AS-P01-B1-tests.txt`.
- Copied only `services/attribution/` into a disposable `/tmp/as-p01-contract-copy-*`
  directory, listed the same 9 tests, and ran them: PASS 9/9. Evidence:
  `logs/AS-P01-B1-portable-contracts.txt`. Copy removed after success.
- Plan/reset/startup registration/link and identity-copy equality audit plus
  `git diff --check`: PASS (`logs/AS-P01-B1-doc-audit.txt`).
- `trail check`: exit 0 (`logs/AS-P01-B1-trail-check.txt`). Advisory matches native
  account/company-lock wording but no existing lock implementation changed.
- The macOS system Python 3.9 emits urllib3's LibreSSL warning. These tests resolve
  only local schemas and perform no HTTP requests; all assertions executed. CI
  will use a current OpenSSL-backed Python. No assertion was skipped or weakened.
- Actual acceptance mapping: AC1/AC2/AC5 wire declarations validated by schema,
  credential-class and actor/tenant-injection cases; AC4 concrete ledger independently
  reconciled; AC7 fixture distinguishes duplicate business facts despite sufficient
  balance. AC3 architecture passed; AC6 account-deletion surface/mapping independently
  reviewed. All runtime obligations remain scheduled in p03–p10, not claimed here.
- No Rust tests moved/deleted, no application source or migrations changed. Rust,
  database, browser and production/platform checks are not applicable to p01's
  contract artifact and remain mandatory for their implementation phases.

Next: freeze this complete contract batch, validate retained evidence manifest,
request independent B whole-batch review, correct all actionable findings, and
accept only the exact passed digest before beginning p02.

B1-R1 frozen digest: `21bacb27bc5ba0b0d0394310910cbc41e2fc8ba448934876a5a4e9d8196b9001`; exact exclusions unchanged.
Evidence: `logs/AS-P01-B1-R1.evidence.json`. Coordination revision 2 rechecked;
AS-C001/r1 and AS-C002/r1 remain applicable and acknowledged.


## AS-P01-D2 / r1 — retry contract amendment for B1-F1

B1-R1 identified absent management retry identity and ambiguous key issuance.
Before dependent persistence implementation, review this amendment with the
correction batch: all session-authenticated management mutations use a tenant/
principal/method/resource-path scoped Idempotency-Key and original timestamp,
bound to normalized payload. Receipt and mutation commit together; retries have
one effect, conflicting content/time returns 409, and live authority is rechecked.
Original requests older than 24 hours are refused even after pruning; receipt
retention is at least 48 hours. Requests may be at most 30 seconds in the future.

Credential issuance/rotation receipts store metadata only, preserving verifier-only
secret storage. First response 201 contains the secret; replay 200 identifies the
credential and instructs revoke/reissue with a new key/time if the secret was lost.
No retry rotates again, extends overlap or restores the old key. Other creates
replay original metadata. Code exchange ambiguity restarts login. Existing checkout/
event/lifecycle business identities remain unchanged. Openbot status mutations use
expected_version compare-and-set and reread after ambiguous response; they never
silently reapply an old command against a newer state.

Verification: B1-R2 checks every management mutation declares key/time and 409/410;
checks creation and rotation's distinct first/replay schemas, proves secret-bearing
or secret_available=true replay payloads are invalid, and checks status CAS input.
Runtime p04/p05 must exercise competing exact/conflicting requests and lost commit
responses for ordinary creation and credential issuance/rotation, once-only secret
handling, replay permission/key revocation, and pruning/expired-request boundaries.
No runtime guarantee follows from a schema test.

## B1-R2 correction scope and mapping

- AS-P01-B1-F1: D2/r1 above plus OpenAPI, protocol and three new discriminating
  schema tests; runtime obligations retained for owning phases.
- AS-P01-B1-F2: bounded unattributed-reason counts in each report group; distinct
  refund counts in each currency row, with explicit paid/refund clocks, filters,
  currency and multi-product overlap semantics. A valid report fixture distinguishes
  missing vs no-consent and one vs two monetary-only refunds with zero returned units.
  p08 query/UI tests must assert those actual returned aggregates, not reconstruct
  them from paginated detail rows.
- AC8: all management create/rotate/update/revoke consumers of retry advice have
  defined replay identity/secret recovery and declared schema assertions.
- AC9: analytics response schema expresses every required reason/refund metric;
  valid/invalid fixtures and differing same-total report cases distinguish omissions.
- All original nine tests remain discoverable; three new tests bring selection to
  twelve. No tests removed or behavioral criteria weakened. The D1/r2 gate remains
  accepted for unaffected contracts; D2/r1 needs explicit B acceptance.

B1-R2 correction artifact `f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02` is frozen for recheck. Both B1 findings
corrected by A, not yet closed by B. 12/12 contract checks pass in place and standalone;
9 original names preserved. Evidence manifest `logs/AS-P01-B1-R2.evidence.json`.
D2/r1 retry amendment requires explicit gate acceptance with correction recheck.
Coordination revision 2 / AS-C001,r1 / AS-C002,r1 rechecked and unchanged.


## p01 acceptance — 2026-11-15

Root recomputed digest `f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02`,
checked unchanged coordination revision 2 and notices, and validated retained R2
manifest. B explicitly passed D2/r1 and the full p01 artifact in AS-P01-B1-N2;
all four findings across architecture/batch rounds are closed by B. 12/12 contract
checks passed in place and standalone. **p01 is accepted in phase and present in
the current sequential worktree**. This is contract acceptance only; runtime and
p02–p10 implementation/full-suite gates remain unfinished. Next: p02 foundation.
