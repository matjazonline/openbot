# VG-P06-A3-R1 — architecture PASS

Accepted **VG-P06-D1/r3**, SHA256 `58e5a9056c77498d967a61ef2c931076aba1dc62ec70dbdfea358254af3af13e`. Proposed C005/r3 is approved for activation. **A2-F1 is resolved at architecture level**; no new design findings.

The revised design drops the cancelled SDK transport to release handshake channels, awaits private HTTP resource completion, and performs bounded known-session fallback cleanup. Completion must cover returned HTTP/SSE streams as well as clients; root explicitly confirmed stream-held witness and cancellable body reads. Exact implementation/recheck obligations are recorded in [A3 notes](agent-review-notes.md#vg-p06-a3-r1).

This is not code acceptance. **B1-F1/F2/F3 remain open** pending one cohesive correction and code recheck.07 remains blocked.

Base/HEAD `ace526fd8259ec53c5199ffa7555a1a99dcba58e`; source unchanged `a781a861b6d39b7abccfdc0706619a7cfcc5573a440f5026ae0dbb492218c658`; coordination20. Exact exclusions: `plan/vault-google/COORDINATION.md`, `plan/vault-google/phases/p06/docs`.

[Decision](logs/decision-r3.md), [findings history](agent-review-findings.md), [entry](logs/reviewer-A3-R1-entry.txt), [exit](logs/reviewer-A3-R1-exit.txt), [coordination pin](logs/coordination-A3-R1-r20.md), [prior A2 review](logs/reviewer-A2-R1-REVIEW-retained.md). Read-only API/source recheck; no Cargo/DB/runtime checks or source edits.
