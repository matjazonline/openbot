# VG-P07-B1-R1 — CHANGES REQUIRED

Reviewed whole phase07 against its original requirements, accepted D1/r1 and affected native integration contracts. Frozen artifact **`8ce396f8ef9b731cb1f33846c82a30de0e8d9852623618e8fec9218375267cc4`**, HEAD **`ace526fd8259ec53c5199ffa7555a1a99dcba58e`**. Entry/exit snapshots and coordination revision23 match.

Two acceptance-blocking findings are saved with evidence:

- **VG-P07-B1-F1 — medium:** control conformance transport does not bind request/cancellation IDs to trusted expected local context before gateway calls.
- **VG-P07-B1-F2 — medium:** the credential/result serialization checker misses imported aliases; an independently compiled leaking DTO mutation produces no violations.

[Full findings and required verification](agent-review-findings.md) · [Coverage, limits and checks](agent-review-notes.md#vg-p07-b1-r1) · [Evidence inventory](logs/reviewer-B1-R1-evidence.json)

Independent focused checks: **42 Rust tests passed** (22 service contract,9 portability,11 e2e), plus15 existing checker mutations/current-tree gate/docs checks. Those passes do not close the findings. Author final build/lint/migration/SQLx/browser receipts were inspected; browser cases were not independently rerun. The unchanged inherited transport-scanner failure remains separate. No full/runtime/stack/lease suites or live-provider calls ran; no full-suite-green claim.

Original architecture acceptance remains retained in [A1 review](logs/reviewer-A1-R1-REVIEW-retained.md). Phase07 implementation is **not accepted** pending the combined correction and recheck.
