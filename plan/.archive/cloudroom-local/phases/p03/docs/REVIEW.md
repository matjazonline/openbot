# p03 independent review

## CL-P03-D01-R1 — combined architecture gate

**CHANGES REQUIRED for CL-P03-D01/r1.** [Decision](logs/CL-P03-D01-r1.md), [review note](agent-review-notes.md#cl-p03-d01-r1--combined-architecture-gate), [CL-P03-D01-F1](agent-review-findings.md#cl-p03-d01-f1--distinguish-immutable-journal-bytes-from-replaceable-connection-identity) (medium): define immutable journal-dedup bytes separately from changing connection identity so lost-ack replay after generation replacement remains valid while stale connections are rejected.

- Decision SHA-256 `142b5d376cc93112fba8cb7186ac468b8c9274d15f5f7b770b5d1626bc875305`; source unchanged from accepted p02: `ee02338b474bb7b234a8a8978669f952df0b36aabf125d191ae1bb2fc547aad7`.
- Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`, main. Coordination r9 and active C002/r2 pinned/fresh. [Input checks](logs/reviewer-d01-r1-input-checks.json), [snapshot](logs/reviewer-d01-r1-final-snapshot.txt).
- Gate-only inspection, no runtime/DB/native operations. p03 not implemented or accepted; p05 workflow producer must stay refused until atomic approval/parking/wake integration exists.

## CL-P03-D01-R2 — replay/fence correction recheck

**PASS for CL-P03-D01/r2 only.** [Decision](logs/CL-P03-D01-r2.md), [review note](agent-review-notes.md#cl-p03-d01-r2--replay-fence-correction-recheck). **CL-P03-D01-F1 resolved at architecture gate; no remaining findings.** Current transport integrity is separate from canonical journal-payload dedup, and current socket/DB fencing precedes replay acknowledgment.

- Decision SHA-256 `6674b2420de4589f1311c7b556ee26b0407e0feb62198f1aa9074452f74e9367`; unchanged source digest `ee02338b474bb7b234a8a8978669f952df0b36aabf125d191ae1bb2fc547aad7`.
- Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; coordination r9 and active C002/r2 pinned/fresh. [Input checks](logs/reviewer-d01-r2-input-checks.json), [snapshot](logs/reviewer-d01-r2-final-snapshot.txt).
- Inspection-only gate; no implementation or execution proof claimed. Root may implement the combined p03 design. Actual focused checks and batch acceptance remain outstanding; p05 workflow producer stays disabled until its atomic integration is complete.


## CL-P03-B1-R1 — whole phase implementation review

**CHANGES REQUIRED** for artifact **`1033301ab60d3c96e0e86148137a8255dc1b3156887ff7cdc3c7b30632e86916`**. [Review note](agent-review-notes.md#cl-p03-b1-r1--whole-phase-implementation-review); [all findings](agent-review-findings.md#cl-p03-b1-f1--reconcile-physically-unsettled-uncertain-operations-on-reconnect).

- **CL-P03-B1-F1/high:** unsettled uncertain effects are excluded from reconnect delivery and Ready barriers.
- **CL-P03-B1-F2/high:** Refused after SubmissionIntent creates definitive failure/expiry and frees capacity without proof of nonexecution.
- **CL-P03-B1-F3/medium:** gated actual Rust/Node command-digest preservation evidence is missing.

Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`, main. Entry/final source identities match; pinned coordination r11 remains fresh. Manifest validated and 33→69 inventory preserved. Author's focused tests and compilation evidence inspected; B independently reran uncertain-capacity and real WS lost-ACK tests (2 PASS), which do not cover the identified failure combinations. [Input checks](logs/reviewer-b1-r1-input-checks.json), [final snapshot](logs/reviewer-b1-r1-final-snapshot.txt). No source edits or new full suite/native acceptance claim. Approved p05 workflow deferral and user-dispositioned Linux gap remain explicit. Root should correct all findings together and request one focused recheck; p03 is not accepted.


## CL-P03-B1-R2 — unified correction recheck

**PASS** for artifact **`1e4f5b2148970677a7f1c388fa5a38cc52b1e7685061a4fac15857ea6b1325d9`**. **CL-P03-B1-F1, F2 and F3 resolved; no remaining actionable findings.** [Review note](agent-review-notes.md#cl-p03-b1-r2--unified-correction-recheck), [saved resolutions](agent-review-findings.md#cl-p03-b1-f1-resolution--2026-11-08-cl-p03-b1-r2).

- Reviewed complete 11-file correction and affected consumers, reusing unchanged R1 coverage. Uncertain physical work now participates consistently in recovery; pre-dispatch refusal cannot follow submission intent; actual Rust→Node command-digest proof is registered and passing.
- B independently executed **9 focused tests PASS**, including six PG recovery/refusal tests, real WS recovery, 216-case capacity/derived-state matrix and actual Node wire proof. Author's current77 PASS, sequential offline all-target Clippy/SQLx, format/whitespace and retained unaffected checks inspected; evidence manifest validated. 69→77 test inventory, no removals.
- Base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; entry/final digest match, r11 authority matches pin. [Input checks](logs/reviewer-b1-r2-input-checks.json), [final snapshot](logs/reviewer-b1-r2-final-snapshot.txt).

p03 focused review passes. Root acceptance still requires its identity check; p05 workflow integration, later native/plugin work, user-dispositioned Linux gap and p07 final full verification remain explicit.
