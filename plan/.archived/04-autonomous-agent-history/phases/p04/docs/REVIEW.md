# AH-P04 architecture review

## AH-P04-AG1-R1 — CHANGES REQUIRED

Reviewed whole D1/r1 SHA `a18e672bdcbc51585ee2f4904f8dc72f42662d6acd99a540b06e99080b82f69f`, against unchanged accepted P03 source `fd507b5bb5be7ca2d7661a002528e10a1a04a33b999ff0303dcf493b70077473`, HEAD `4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d`, current coordination12.

- **AH-P04-AG1-F1 (high):** specify native first-freeze source/content/classification attestation; equality with a newly stored snapshot alone cannot establish eligible history.
- **AH-P04-AG1-F2 (high):** protect classified current input and later retained observations at the pre-provider audience check; seed level alone is insufficient.

[Complete finding and required evidence](agent-review-findings.md) · [Whole-gate coverage, checks and limitations](agent-review-notes.md#ah-p04-ag1-r1) · [Entry proof](logs/AH-P04-AG1-R1-reviewer-entry.json) · [Exit proof](logs/AH-P04-AG1-R1-reviewer-exit.json).

Architecture only. No P04 implementation/runtime or later-phase acceptance. Source and authoritative coordination verified unchanged; no Cargo/DB checks run by B.

## AH-P04-AG1-R2 — architecture PASS

**PASS** for D1/r2 SHA `642993aa5857d93778385b365a6205b689e08cf57de6a952d4b99d04b74b78ae`, unchanged accepted03 source `fd507b5bb5be7ca2d7661a002528e10a1a04a33b999ff0303dcf493b70077473`, HEAD `4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d`, authoritative coordination13/C005r2 proposed.

Both high findings are **FIXED at architecture level**: bounded native source/content/stamp attestation at first freeze, and full frozen/current content classification before provider entry. Unchanged whole-gate R1 coverage reused; no further actionable issue.

[Recheck coverage and limitations](agent-review-notes.md#ah-p04-ag1-r2) · [Finding dispositions](agent-review-findings.md#ah-p04-ag1-r2-dispositions--2026-11-07) · [Entry proof](logs/AH-P04-AG1-R2-reviewer-entry.json) · [Exit proof](logs/AH-P04-AG1-R2-reviewer-exit.json) · [Decision delta audit](logs/AH-P04-AG1-R2-reviewer-decision-audit.json).

Architecture only: all P04 implementation and targeted runtime verification remain pending. No Cargo/DB checks run by B and no later-phase acceptance.

## AH-P04-AG1-R3 — trusted-audience architecture PASS

**PASS** for D1/r3 SHA `d2aebd321d11d0ca7b1ac17c25b2dfc1cf73b06068b77d862b8f6962a0c41f95`, reviewed against frozen current source `6381269bc650db9a6f991ef159c102d90f7079fe88a783cd022f7c3e4ab7c9d6`, HEAD `4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d`, authoritative coordination15/C005r3 proposed.

The revised audience rule follows immutable trusted conversation scope, preserves private/member and child inheritance boundaries, and retains current authority, full content comparison and post-provider settlement ownership. AH-P04-A-F3 / [AH-P04-AG1-F3](agent-review-findings.md#ah-p04-ag1-f3--trusted-private-scope-was-confused-with-its-thread-anchor) is **FIXED at architecture level only**. Its current code reproduction still fails; no implementation acceptance.

[Coverage, prior-review follow-up and limitations](agent-review-notes.md#ah-p04-ag1-r3) · [Entry proof](logs/AH-P04-AG1-R3-reviewer-entry.json) · [Exit proof](logs/AH-P04-AG1-R3-reviewer-exit.json) · [Decision audit](logs/AH-P04-AG1-R3-reviewer-decision-audit.json).

Only affected architecture reviewed; unchanged R2 architecture coverage reused. Whole P04 source/evidence review and targeted correction checks remain required. No Cargo/DB checks run by B.

## AH-P04-B1-R1 — whole phase04 CODE PASS

**PASS** for source `12de390114bb49391c9b20a091795450ee4aac280ff195fe3848032af17b7a4d`, HEAD `4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d`, current coordination17. Entire42-file phase delta reviewed against accepted P03, all128 owned/external36 inputs verified. AG1-F1/F2/F3 are FIXED in reviewed code; no new actionable findings.

Original04A/B coverage includes bounded selection/budgets, native source attestation, immutable snapshot/recovery/taint, current trusted audience, separate provider seed/live protocol, checkpoint equality and model-only behavior. Author evidence independently reconciled:597distinct targetedPASS/99history/0ignored; all required family prefixes nonzero; fresh migrations, actualSQLxprepare/check, offline all-targets, Clippy/fmt/diff/Graft PASS. B inspected receipts without rerunning Cargo/DB.

[Complete coverage and limitations](agent-review-notes.md#ah-p04-b1-r1) · [Finding dispositions](agent-review-findings.md#ah-p04-b1-r1--whole-code-dispositions-2026-11-07) · [All42 paths](logs/AH-P04-B1-R1-reviewer-coverage.json) · [Evidence audit](logs/AH-P04-B1-R1-reviewer-evidence-audit.json) · [Entry](logs/AH-P04-B1-R1-reviewer-entry.json) / [exit proof](logs/AH-P04-B1-R1-reviewer-exit.json).

No full-suite/live-provider or P05–P07 acceptance implied. Root exact verification remains required.
