# 04.7 SQL A/C evidence

**Verified against the authorized amended criteria, 2026-10-02. Full 04.7 remains
incomplete; no 04.8 work.** Direct implementation, without the adaptive Sol/Astra
skill workflow. Independent scoped source/combined review PASS, no findings:
[`review-ac.md`](/private/tmp/workflow-sql-ac-20261002/review-ac.md).
Base `9cff5164fa0694a4e7664f6ff8c69e473d4c2a97`; inherited PROGRESS/RESUME changes
and topology proposal preserved. No production API/type/schema/migration,
compatibility, dependency or resource-limit changes.

## Coverage and limits

| Evidence layer | Verified evidence | Limit |
| --- | --- | --- |
| Production-function A | Explicit six-column reference relation calls the unchanged production guard. Exact-fixture checkpoint accepts genuine current witnesses while the full schedule commits. A later transaction proves both witnesses historical, no current matches, and every other guard prerequisite; forcing only its named constraint raises exact `23514: invalid current workflow reconciliation claim episode`. | Tests the witness pair collectively, not independent isolation of each xid predicate. Does not exercise historical-witness rejection through an otherwise-valid public episode INSERT. |
| Public-boundary A | Catalog OID/flags prove the reference and public triggers use the same production guard. Genuine scheduling commits, result/final revision and public episode match; existing public wrong-command-scope and wrong-retired-ordinal negatives pass. | These public malformed-binding controls are distinct from the reference-trigger historical test. |
| Public-boundary C | Separate committed P1/P2 adversarial preparation; two full owner scheduling commits with authentic independent execution/job/fence/attempt/marker/entry histories. Each production pending helper returns its exact command key before the actual public run UPDATE raises `23514: multiple current workflow reconciliation claim episodes`. | Preparation is explicitly adversarial committed topology, not exclusively normal application history. |
| Controls/rollback | C checks single-candidate public transition and separate normal scheduling/claim. A/C compare every public table after target rollback. A checks reference-row rollback, instrumentation absence and unchanged production definitions. C checks trigger attachment/order and unchanged definitions. | Disposable database removal cleans committed preparation; only the target attacks claim full transactional rollback. |
| Future integration | Mandatory [Phase 10 item](10-verification-and-cutover.md#reconciliation-stale-evidence-and-duplicate-candidate-integration) records behavioral safety, owner/caller refresh, exact-public-INSERT reassessment and independent review if still obstructed, plus later owner-created multi-execution/competing-worker coverage. | Not executed or satisfied by these SQL tests. Later functionality does not guarantee exact-public-path reachability. |

Implementation:
[`A witness test`](../../src/adapters/persistence/workflow/action_reconciliation_claim_sql_witness_tests.rs),
[`C candidate test`](../../src/adapters/persistence/workflow/action_reconciliation_claim_sql_candidate_tests.rs),
[`C assertions`](../../src/adapters/persistence/workflow/action_reconciliation_claim_sql_candidate_support.rs).
The existing isolated admission, provider, proof, scheduling, snapshot and panic-safe
OwnDatabase helpers are reused. Deep fixture seams remain boxed.

## Verification

Exact serialized commands: [`verify.sh`](/private/tmp/workflow-sql-ac-20261002/verify.sh).
Complete output: [`verification.log`](/private/tmp/workflow-sql-ac-20261002/verification.log).
Both database URLs explicitly select the disposable verification database on port
55439. `RUST_MIN_STACK=2097152`; default libtest parallelism, no limit changes.

- Focused A PASS 1 and C PASS 1 (including normal control).
- `cargo test --locked --offline --lib workflow_action_ -- --nocapture`: **157 PASS**,
  zero failures/ignored; 185.18 seconds. Includes new A/C, existing public-binding,
  episode/lifecycle, competing-claimant, cancellation/recovery and upgrade tests.
- Formatting and whitespace PASS.
- SQLx prepare and prepare `--check`, both `--locked --offline --all-targets`, PASS.
- Locked offline all-target compilation and strict Clippy (`-D warnings`) PASS.
- Fresh migrations PASS; all 53 applied SHA384 checksums in both retained and fresh
  databases match repository bytes. Normalized schema equivalence PASS, SHA256
  `8bc51eb58988ce21505dea9f8e6cbca73c05f02c862f5690670a49c343c9b784`.
- All 101 migration/SQLx/dependency/config files preserved; 45 SQLx entries unchanged.
  Runtime test SQL produces no new macro metadata. `graft build` PASS.

Schema/checksum records: [`audit.log`](/private/tmp/workflow-sql-ac-20261002/audit.log),
`audit.py`, `*-schema.sql`, `*-migration-checksums.tsv` in the same directory.
Reviewed source hashes: `reviewed-source-sha256.json`. Focused fixture identities,
exact commands and initial failures: `a-test*.log`, `c-test*.log`, `initial-checks.md`.
Initial C preparation incorrectly reused E1's resource target for E2: authorization
refused before provider invocation. E2 now receives its own scoped resource target
and verifier registration; no failed preparation was counted as the target negative.

Retained cluster identity verified: system `7691265791172745923`, mac03,
`workflow_admission`, port 55439, data `/private/tmp/workflow-admission-pg-e3aa`.
All isolated fixture pools closed/databases removed; disposable verification database
`workflow_sql_ac_20261002_test` removed. Final [`cleanup.log`](/private/tmp/workflow-sql-ac-20261002/cleanup.log)
contains only `postgres` and `workflow_admission`. Retained cluster left running.
No staging, commit, deployment or retained-data reset.
