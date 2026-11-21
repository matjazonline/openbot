# Plan04 p02 independent review

## 04RU-P02-D1-R1 — PASS (decision only)

[04RU-P02-D1-R1/N1](agent-review-notes.md), retained independent B, **PASS** immutable [04RU-P02-D1/r1](logs/D1-r1-decision.md), SHA256 **158e38af4621c3addba3d98a07a4f1aca0920a66d2da55af4050ac5ded9bc0d4**, source **54bfc2c64c4b501ee0539b514f0e0e26432a2ac823b0de55d9ef6a14fd588cdc**, HEAD/start/review base334e79c74b383dffd8e0d565c33a77c6ac24c94d. Exact exclusions p02/docs and Plan04 COORDINATION.md; coordination7/C001r1/C002r1/C003r2 and inherited one-step-both-ways01DEP-C002r1 pinned.

Required ownership, Members/Public, every-Members-desk receiving-agent grant enforcement, centralized creation and set-wise derived grant reconciliation are coherent with inspected callers/fences. Actual unchanged-production1000×200baseline supports replacing full rewrites; optimized mutation counts, timings, exact revisions, provenance and affected concurrency/security proofs remain mandatory. No findings or reviewer runtime. Root may implement after pin checks; whole-B1 review and later full Plan04 gates remain outstanding. This is not implementation or phase acceptance.


## 04RU-P02-D1-R2 — PASS (amended decision only)

[04RU-P02-D1-R2/N1](agent-review-notes.md), retained B, **PASS** immutable [D1/r2](logs/D1-r2-decision.md), SHA256 **c3cccfe77156a647c5f9515a491bbcc3ede1c1145d9b2b6fcd6157248c9bbe11**, source **a349e3ad58309dc0fa9e219d554f205ccba29712519dae558185e3edbd6dd6b6**, HEAD/start/review base334e79c74b383dffd8e0d565c33a77c6ac24c94d; coordination8 and exact two exclusions pinned.

The actual unverified-team regression contradicts r1's global email-grant assumption. A matching department_membership grant may carry authenticated UUID placement authority without current-email proof; every other provenance retains the proof requirement. Domain and locked association/follow/notification equivalents must agree without relaxing current membership, account/grant locks, identity/incarnation or direct/relay boundaries. No findings in the proposed amendment; root may implement and activate proposedC004r1 after pin checks.

Current291PASS/88FAIL/1ignored is retained failure evidence, not acceptance. Optimized join mutation counts pass but slower desk setup still requires the proposed equivalent-set pruning investigation and remeasurement. All affected security/approval/notification/race and whole-B1/final gates remain required. No reviewer runtime or source mutation.


## 04RU-P02-B1-R1 — CHANGES REQUIRED (whole B1)

[04RU-P02-B1-R1/N1](agent-review-notes.md), retained independent B, **CHANGES REQUIRED** for artifact **7ce85fba312130879a9e00c49febd3e48f54e71f42483284637524f51ed72539**, start/review-base/HEAD334e79c74b383dffd8e0d565c33a77c6ac24c94d; exact two exclusions unchanged, coordination9/C001r1/C002r1/C003r2/C004r1 and inherited01DEP-C002r1 pinned; D1/r1+r2 immutable hashes match.

**One actionable finding: [04RU-P02-B1-F1](agent-review-findings.md), medium/P2, acceptance-blocking.** The live classic create/edit forms still submit `@public` in the email field and have no Public checkbox or owner selector. Their changed handlers remove that marker unless `public` is checked: saving a Public desk silently makes it Members. Modern `/ui` synthetic form tests do not cover this actual producer mismatch. Complete both classic form contracts and add decisive form-derived create/edit/Public-off/owner coverage before re-freezing.

Whole production/schema/authorization/reconciliation/creation/consumer/fixture delta reviewed; actual retained logs, all37 removed-name replacements and historical failure corrections independently checked. Detailed positive coverage and limits are in N1; they are not partial acceptance. No reviewer runtime or source mutation. [Evidence audit](logs/B1-R1-reviewer-log-audit.json), [final source/authority pins](logs/B1-R1-reviewer-final-checks.json). Full p03/external gates, explicit Doop deferral and Trail false-positive disposition remain outstanding as recorded.


## 04RU-P02-B1-R2 — PASS (F1 correction; unchanged B1-R1 coverage reused)

[04RU-P02-B1-R2/N1](agent-review-notes.md), retained independent B, **PASS** artifact **6391c0ba46d382417a618bdc9238b32c1b8e1b976a457ee42f60b2382971e660**, start/review-base/HEAD334e79c74b383dffd8e0d565c33a77c6ac24c94d. Exact exclusions unchanged; coordination10 pinned, including **C002/r2: finish04/checks/pushmain, then STOP; no05/06**. Other policy notices and D1/r1+r2 remain unchanged.

**[04RU-P02-B1-F1](agent-review-findings.md) FIXED.** Both classic forms now reuse canonical required-owner/Public/additive-address rendering; all callers and the shared spam/reset contract agree. Inspected real rendered-handler PostgreSQL regressions and170PASS focused/HTTP21 evidence. Independently reran the native browser protocol:2PASS, covering four producer variants, native FormData, Public on/off/reset, owner selection, spam interlock and actual both-theme styles. No new findings. Dependencies, generated CSS, log-retention attributes and inventory4747→4749 were reviewed.

[Final pins](logs/B1-R2-reviewer-final-checks.json), [reviewer browser run](logs/B1-R2-reviewer-browser-tests.txt), [unchanged production/test audit](logs/B1-R2-reviewer-reuse-and-test-audit.json). Root may accept p02 using unchanged whole-batch coverage from R1 plus this correction recheck. Full p03/Plan04 gates and publication remain outstanding; Doop/Trail dispositions remain explicit. No reviewer source/DB mutation or remaining runtime resources.
