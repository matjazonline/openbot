# Independent current-main integration review

## VG-INT-MAIN-R1 — PASS with inherited check failures

2026-11-07. Independent B **PASS** for exact combined digest **`f2bd7d430aa1f4eff0289a6006fa64deeb9fa189def2e9ae74fa427f232b7926`**, detached HEAD **`4ab45555f8316ec7a8b1b64e8c05b77ff7d9a29d`**.

All146 accepted vault source hashes compared; exactly5expected merge paths differ. Both migration blocks/HTTP guard preserved byte-for-byte, final-action/source boundaries and supervised workers retained. No introduced integration defect found; unaffected accepted vault/channel evidence reused explicitly. Independent listed channel-completion run: **34/34PASS**. Author131distinct targeted Rust tests and1browser pass;21successful receipt hashes verified. Source and live vault12/channel28 coordination pins match at exit.

**Two inherited checks remain failed:** VG-INT-MAIN-F1 markup scanner and VG-INT-MAIN-F2 transport scanner, each reproduced on canonical pulled main and left unchanged. No all-checks-green/full-suite claim. Known unrelated overflow test was neither changed nor rerun. No full suite, vault06/07, history implementation, commit/push/deploy.

[Coverage and limitations](agent-review-notes.md#vg-int-main-r1), [findings/dispositions](agent-review-findings.md), [identity/freshness](logs/reviewer-identity-R1.json), [independent tests](logs/reviewer-completion-run.json). Root acceptance must retain these limitations and the pull/check stop boundary.
