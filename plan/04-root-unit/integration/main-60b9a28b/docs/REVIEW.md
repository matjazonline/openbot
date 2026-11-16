# Independent review: main integration

## 04RU-MAIN-B1-R1 — PASS

[04RU-MAIN-B1-R1/N1](agent-review-notes.md), retained independent B, **PASS** for frozen artifact **09f2972f563e400b3553eb64d1a834d9cf942ab944d130180463f2410d22c4a8**, HEAD/start/review base **60b9a28b9f5b39981d804d0fdaeab5c75916c65d**, coordination2/04RU-C001/r1 and exact exclusions current integration docs plus Plan04 COORDINATION.md.

All five current delta files and affected authority/teardown contracts reviewed. Attribution uses canonical root membership with unchanged locks/permissions; actual competing status requests have one winner. Populated company/account teardown preserves exact old outbox records and one matching terminal event, retains installation reservations and removes all other scoped rows. No findings.

Evidence validates:55PASS/0FAIL/1ignored, all21HTTP checks PASS, fresh combined migration/all-target SQLx and8reachability tests PASS. Default Trail failure is an inspected lexical false positive against an unchanged SQL comment; scoped integration-base check passes, and the rationale must remain in the commit message. Actual browser SSO remains NOT EXERCISED and required at the final gate. No reviewer runtime/source edits. Root may accept this integration after pin checks; p02/p03/final full-suite and explicit Doop deferral remain outstanding.
