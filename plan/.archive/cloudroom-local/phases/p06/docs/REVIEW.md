
## CL-P06-D01-R1 — combined architecture gate

**CHANGES REQUIRED** for **CL-P06-D01/r1**, SHA-256 **`137e653cbf2e4691855dab793d2a4298ac31e1683ada0b79cc92012371c7d1d2`**.

- **F1/P2:** plugin mapping expiry cannot see prepared/queued server session references; a still-authorized continuation can lose its required frozen local mapping at90days.
- **F2/P2:** each tree's blanket external-call blocker prevents both collectors from ever retiring two terminal old roots that continued one another's sessions.

[Findings and required corrections](agent-review-findings.md#cl-p06-d01-r1--combined-architecture-findings), [complete gate note](agent-review-notes.md#cl-p06-d01-r1--combined-retention-replay-and-supervised-packaging-gate). Reviewed physical settlement/current-fence replay, SQL/workflow ownership/locks, UUIDv7 admission horizon, plugin retention/pressure, pinned package and supervised wrapper/platform proof against original06/README/07 and actual consumers. No other actionable findings in the reviewed design.

Source remains accepted p05 **`c7dfb6034c5f3f5001e107ac94a008da7fc55b88054af8208cfa1b79a6685664`**, base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`. Coordination r27 pin and exact-eight-exclusion snapshots match; [input checks](logs/reviewer-d01-r1-input-checks.json), [final checks](logs/reviewer-d01-r1-final-checks.json).

Revise both ownership gaps together before dependent source changes or C007 activation. This was inspection-only architecture review, with no runtime/platform acceptance. Linux native remains NOT EXERCISED; p06 implementation and p07 final checks remain required.

## CL-P06-D01-R3 — combined architecture correction gate

**PASS for CL-P06-D01/r3 only**, SHA-256 **`89345a33f64e528eefbf70c3877902f6e09921314fbe1bab262f7cb4a10c1e41`**. **F1/F2 resolved at design; no remaining/new actionable architecture findings.**

Reviewed bounded server-absence confirmation before plugin mapping deletion, non-resurrection at actual session creation owners, the explicit coordinated-restore/re-pair boundary, and guarded terminal call archival that breaks closed cross-tree reference cycles without execution authority. Reused unchanged R1 coverage; r2 was superseded by r3 before disposition.

[Complete review note](agent-review-notes.md#cl-p06-d01-r3--combined-correction-gate-f1f2-and-restore-boundary), [finding resolutions](agent-review-findings.md#cl-p06-d01-r3--combined-resolution-history). Exact-eight-exclusion entry/final source identity remains accepted p05 **`c7dfb6034c5f3f5001e107ac94a008da7fc55b88054af8208cfa1b79a6685664`**; authority r29 pin and final decision hash match [checks](logs/reviewer-d01-r3-final-checks.json).

Root may activate C007/r3 and implement p06. Inspection-only gate: combined server/plugin races, cyclic collection progress, proof/restore negatives and all original package/lifecycle/runtime checks remain required. No p06 code/platform acceptance; final repository/macOS/native gate remains p07. Linux native remains NOT EXERCISED under I005.


## CL-P06-B1-R1 — whole-batch implementation review

**CHANGES REQUIRED** for artifact **`2fdbad30cd3ba21436662b82dfd114ad519e75f52736b322a7b5d51db4d393fb`**.

- **CL-P06-B1-F1 / P2:** mapping retirement repeatedly selects the same oldest 100 IDs when the server retains them. Later absent mappings are never queried; at 4096 mappings new sessions remain paused despite reclaimable space. Production Journal reproduction confirms 3996 absent mappings stay hidden across ten pages. Add advancing bounded traversal and its regression.
- **CL-P06-B1-C1:** plugin typecheck/tests are not executed in CI. This remains required p07 integration work, not another p06 blocker.

[All findings and correction scope](agent-review-findings.md#cl-p06-b1-r1--whole-batch-implementation-review-2026-11-08); [complete review and limits](agent-review-notes.md#cl-p06-b1-r1--whole-batch-implementation-review-2026-11-08). Independent checks: 124 local-agent tests, 83 plugin tests, 21 workflow-retention tests and plugin typechecking pass. All 59 delta paths/blobs, final package inputs, authority r31, D01/r3 and final frozen digest verified. Full repository/paired native acceptance remains p07; Linux native NOT EXERCISED. Source stayed frozen; no p06 acceptance.


## CL-P06-B1-R2 — correction recheck

**PASS** for artifact **`c52a03aec4aaa4fbc55afcf807c77e8814aa4eedffa05f3d727b3217723d3def`**. **F1/P2 resolved; no remaining p06 acceptance-blocking findings.**

Bounded mapping traversal now advances past retained/lost pages, survives socket reconnects and wraps safely. Independent checks pass: **86 plugin tests**, plugin typechecking, and the actual **PG/WSS/Node combined lifecycle test**. Three-file delta, corrected package inputs, exact frozen digest, authority r32, unchanged D01/r3 and all 32 retained evidence hashes verified.

[Complete recheck](agent-review-notes.md#cl-p06-b1-r2--f1-correction-recheck-2026-11-08); [finding resolution](agent-review-findings.md#cl-p06-b1-r2--f1-correction-resolution-2026-11-08). Unchanged R1 coverage reused. C1 remains required p07 CI work; full repository/paired production-plugin/provider acceptance remains p07. Linux native NOT EXERCISED under I005. No source changes during review.
