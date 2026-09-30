# 04.7 prior-entry bounds — exact refusal receipt correction

Status: focused correction implemented and verified; independent Astra recheck and
root acceptance pending. This does not accept full 04.7 or start 04.8. Original
Execution 6, BRIEF-04.7, CONTRACT-04.7-AUDIT, CONTRACT-04.7-BOUNDS and RESUME remain
authoritative. Accepted production correction remains unchanged.

Finding: `/private/tmp/workflow-04.7-prior-bounds-code-review.md` P2 observed that
masking the whole command table could hide missing, incorrect or extra refusal
receipts, or mutations to the 128 historical successful commands. The frozen prior
four-path artifact is `/private/tmp/workflow-04.7-prior-bounds-01a0f664/HANDOFF.md`.

The correction changes only `action_reconciliation_prior_bounds_tests.rs` plus
this separate evidence. Its pure `only_refusal` helper validates exactly one new
company/key receipt with a fresh nonnil ID, exact run/execution/invocation/digest/
dispatch/actor/request digest, expected and result revisions, decoded BoundExceeded
result, and NULL evidence/audit linkage. It removes only that validated row, then
compares the entire remaining command history and every other table to before.
Both fresh snapshot refusal and paused 128-to-129 settlement call it. The returned
result revision must equal the captured before-state run revision; the command's
expected revision remains separately checked on its receipt. A genuine competing
consume/retire cycle changes the current revision after the paused command began.

All genuine 128 owner-created cycles, positive 128-prior/129-total dispatch, actual
two claimants, ledger/barrier/paused verifier and recovery assertions remain intact.
No production, owner, shared helper, SQL query, SQLx cache or migration changed.
The earlier combined `workflow_action_` 116 PASS evidence in the prior artifact is
reused because those helpers/owners/dependencies are hash-identical. No redundant
SQLx preparation was run for this assertion-only change.

Artifact/log directory:
`/private/tmp/workflow-04.7-prior-assertion-correction-01a0f676/`.

Verification commands (both retained URLs were explicit):

```sh
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow_action_reconciliation_bounds_128_prior_consuming_129_total_then_lost_refuses
env DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission SQLX_OFFLINE=true cargo check --locked --offline --all-targets
cargo fmt --check
git diff --check
graft build
```

Focused stock-2-MiB final run: 1 passed, 0 failed/ignored, 28.99s following 39.21s
compilation (`focused-r2.log`). The first run (`focused.log`) preserves an incorrect
test expectation that current result revision equalled the paused command's old
expected revision: actual 778 vs 775. Corrected by comparing current result revision
to the real before-state, without changing production or weakening receipt identity.
Offline all-target compilation, formatting, tracked/untracked source whitespace and
graft refresh passed; complete logs and frozen hashes/delta are in the artifact.

Retained PG18 cluster system ID 7691265791172745923 was verified cleanly stopped,
restarted with port 55439/socket `/private/tmp`/max_connections 200, then cleanly
stopped with data preserved. Database `workflow_admission`, role `mac03`, exact data
directory `/private/tmp/workflow-admission-pg-e3aa`; all 50 applied migration file
hashes/checksums unchanged. No retained reset/drop, stage/commit/deploy or new matrix
implementation. Other pending bounds/schema/eligibility/RESUME groups and full gates
remain pending as recorded by the previous handoff.
