# 02WR-P03-C1-R1 bounded verification disposition

2026-11-12; B /root/workflow_roles_reviewer; scoped clarification, not partial B1 review. Sourceb5494650088d5ff4a9969baed95eba8930f59eb00c6aface813a626f492817db at unchanged HEAD103004a8ac2fd643cf14bde7e09022ae79f72053. Authority revision15, active I001–I003/r1,C001/r1,C002/r2,C003/r2,C004/r3.

## Evidence assessed

Read immutable C1-r1.md, current TASK clause and retained B1-principal-tests1.txt:both competing schedules failed specifically during account teardown on workflow_decisions_action_identity (lines14–23), not due to a lock timeout/deadlock accepted as refusal. Current reviewer_account_race_tests.rs:15–58 already uses admin run-as distinct from owner reviewer and principal IDs;91–161 observes real publication/retirement account blockers. Thus the failures are downstream of the affected lock mechanism, not a passing concurrency result.

Independently read the accepted p02 source tar member and compared the exact workflow_decisions_action_identity clause to current: identical, SHA2565d548fd5b5173256d43a65c920eaab6a45007d29e77cb731c350817d48d6efa0. Independently compared user_lifecycle's owned-company/account-deletion tail with git HEAD: identical. Current diff changes only the membership notification block, skipped for the failing owned-company case. Static comparison retained in02WR-P03-C1-R1-inherited-check.json. This supports the scoped unrelated-dependency disposition; no pristine-baseline behavioral reproduction was run by B, and this is not a blanket claim that all deletion paths are verified.

## F3 coverage disposition

PASS for the proposed verification adjustment. A reviewer who is an admin in the surviving workflow tenant and owner of a distinct empty company still exercises the production account UPDATE, account mapping, membership retirement, notices and actual final-publication body. Separate run actor/reviewer/principal identities ensure the policy-derived user lock is required. Both commit orders must retain exact observed user SHARE/UPDATE blockers, named current-authority refusal/no publication when retirement wins, coherent publication then actual retirement when publication wins, cancellation of retained notices and canonical replay/no duplicate external write. These remain proposed assertions, not results accepted by this clarification.

The complementary same-workflow-company-owner production lock_account_on prefix with admin run-as explicitly exercises the original user-before-company gate: publication must be observed waiting on the user query while the retirement prefix owns owner account/company locks. Rolling that prefix back and requiring successful publication is adequate for that ordering property only. It cannot establish owner-tenant teardown commit, and must remain labeled as a rollback-prefix check. The two forms together cover the affected F3 identity/ordering contract without pretending the inherited FK allows an owner deletion to commit.

Keep both failed outputs and original assertion history. Any changed/renamed test names need the skill's inventory/disposition, not silent deletion. Do not alter the unrelated FK or lifecycle cleanup merely to make this fixture green. Owning an approved final-response workflow tenant still has a known failing account-deletion commit; retain it as a separate unresolved inherited limitation for p05, not an accepted success or hidden test skip. Full plan acceptance must explicitly report its disposition. All F1/F2 and other F3 behavioral obligations remain for B1.

## Label bound clarification

SQLprincipals_display_label_check already enforces nonblank text<=255bytes. reviewer_principal_tests.rs:120–178 currently asserts that64 emoji (256bytes) fail that named producer constraint, while63emoji+abc (255bytes) survives park/read exactly, then rejects snapshot updates. This is valid early-bound and lossless snapshot evidence. It is not successful truncation of a valid oversized source: left(display_label,256) never truncates a valid current source. Keep the255byte producer constraint; do not raise it to exercise the dormant1024byte snapshot cap. D1/r3's upper snapshot bound remains safe, while its proposed oversized/truncation evidence is replaced by this precise existing-producer-bound disposition.

No source/design changes, tests, DB mutations or whole-B1 acceptance by B. Known unrelated teardown failure and prior label regression remain distinct; this clarification does not close their implementation status.
