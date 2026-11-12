# p05 reviewer findings

## 01DEP-P05-B1-F1

- Date: 2026-11-11. Workflow `implement-astra-reviewed-2-2`; phase p05, batch B1,
  whole-batch/combined-integration review [01DEP-P05-B1-R1](agent-review-notes.md#01dep-p05-b1-r1).
  Reviewer `/root/p05_reviewer`, requested Astra/high; effective runtime settings not independently
  exposed. Worktree `/Users/mac02/.codex/worktrees/2050/openbot`.
- Artifact: base `c42fc15dd9eb05c9f463703b26a809d42929ab16`, HEAD
  `e7bc780ec7699051b99893d4f497bf2299c77738`, digest
  `b26116788fe4639df6d7a2a3dd8789a12a34736ca8450bcfb9e977f14bbaab50`.
- Classification: **actionable defect, Medium, verification/requirements**. Not an architecture
  decision defect.
- Evidence: `.github/workflows/ci.yml:147–176` configures query statistics and then explicitly
  verifies `pg_stat_statements.track=top`, `track_utility=off`, both parameter-log limits at zero,
  and a numeric `queryid` for the probe. `scripts/check.sh:20–22` excludes the server
  configuration. Plan 05A.3 requires comparison with CI and disposition of omitted required
  gates. Neither the 66-entry R1 manifest nor TASK's current CI mapping includes this check or
  a NOT EXERCISED disposition. The declared three external gaps do not cover it.
  Retained independent inventory: [B1-R1-reviewer-audit.txt](logs/B1-R1-reviewer-audit.txt).
- Impact: the completion record's missing-check inventory is incomplete. Successful migrations
  and database tests do not establish the statistics/privacy configuration or actual query
  recording required by CI; the local check script explicitly says tests do not depend on it.
- Requested correction: record this CI gate in the acceptance mapping and a new immutable
  review/recheck manifest. Exercise the actual assertions on a suitably configured owned fixture,
  or explicitly retain NOT EXERCISED with the prerequisite and follow-up needed. Update the
  completion/handoff gap inventory accordingly. Do not reconfigure the shared PostgreSQL server
  to manufacture a pass and do not rewrite the R1 manifest.
- Instruction lesson: existing plan 05A.3 and skill verification-contract guidance already
  require this. Candidate improvement for future final-gate inventories: map every named CI
  verification step to evidence or an explicit disposition, including environment assertions
  omitted by local scripts. Observation: enumerating the 28 protocol checks missed this distinct
  activation assertion; the underlying cause is unknown.
- Resolution history: **Open** at R1. Correcting the inventory can close this reporting defect;
  a required NOT EXERCISED gate would still block full phase acceptance.

### F1 resolution — 2026-11-11, 01DEP-P05-B1-R2

**Fixed**, independently reviewed at digest
`19a7c2b6b0a8349712e5180147b487590acd194bc32d95846d82030598519b3b` (same base/HEAD).
The [R2 manifest](logs/01DEP-P05-B1-R2.evidence.json), TASK correction mapping and plan completion
table now include this CI gate. Inspected `B1-R2-query-statistics.py`, execution output, resource
identity and server log: a fresh p05-owned PostgreSQL 18.6 cluster receives the actual app
migration and exercises all six settings plus numeric queryid and normalized `SELECT $1` text.
The four CI SHOW assertions and probe are present with real SQL results; the two additional
settings assert preload/query-id configuration. Configuration is isolated to the private process;
shutdown succeeds and reviewer confirms no `postmaster.pid` remains. Shared-server prerequisite
failures are preserved as context, not mislabeled as the successful fixture. This closes the
missing evidence, without claiming Docker or the other external jobs ran.

## 01DEP-P05-B1-F2

- Date: 2026-11-11. Workflow `implement-astra-reviewed-2-2`; phase p05, batch B1,
  round [01DEP-P05-B1-R1](agent-review-notes.md#01dep-p05-b1-r1), whole-batch verification.
  Reviewer `/root/p05_reviewer`, requested Astra/high, effective settings not independently
  exposed; worktree `/Users/mac02/.codex/worktrees/2050/openbot`. Same R1 base/HEAD/digest as F1.
- Classification: **actionable defect, Medium, verification/CI**; author-reported during review,
  independently reproduced. No architecture defect.
- Evidence: `git diff --cached --check` exits 2 for raw output under p05 `docs/logs/`, including
  `B1-author-artifact-audit.diff:8`, `B1-ci-agent-picker-rust-1.txt:9`, and
  `B1-graft-build-final.txt:2`. The earlier manifest whitespace PASS predates staging these
  files and does not establish the final retained patch passes plan 05A.4. See
  [bounded reviewer output](logs/B1-R1-reviewer-whitespace.txt) and the original
  [author audit](logs/B1-staged-whitespace-audit.txt).
- Impact: required patch-whitespace verification is red at the retained/staged artifact, despite
  no product-source whitespace defect. Evidence must retain its original bytes/hashes.
- Requested correction: resolve archival representation of raw evidence without modifying
  manifest-pinned bytes or weakening source/document/JSON checks, and rerun final patch whitespace
  plus retention validation. The author's proposed narrow Git attributes for raw `.txt`/`.diff`
  outputs under p05 logs are a reasonable approach to review as the correction delta; no blanket
  repository whitespace suppression. A deliverable `.gitattributes` remains outside the workflow
  record exclusions and therefore requires a new artifact identity.
- Instruction lesson: final whitespace checks must include staged/new retained evidence, not
  only the pre-retention worktree. Preserve immutable raw output; classify archive bytes explicitly
  rather than normalizing the historical evidence in place.
- Resolution history: **Open** at R1.

### F2 resolution — 2026-11-11, 01DEP-P05-B1-R2

**Fixed**, independently reviewed at the R2 digest above. The deliverable
`plan/01-departments/phases/p05/.gitattributes` is included in the snapshot and scopes `binary`
only to `docs/logs/*.txt` and `docs/logs/*.diff`. Reviewer checked effective attributes: raw
archives disable text conversion/diff/merge; representative source, Markdown, Python and JSON
remain unaffected. Independently executed staged/unstaged whitespace checks (both exit 0),
verified all R1 referenced hashes and identical reused R1 entries, and ran skill R2 plus
repository tracked-evidence validators (47 manifests). The original failure output remains
retained. Evidence: [B1-R2-reviewer-checks.txt](logs/B1-R2-reviewer-checks.txt).

## 01DEP-P05-B1-F3

- Date: 2026-11-11. Workflow `implement-astra-reviewed-2-2`; phase p05, B1 final integration
  round [01DEP-P05-B1-R3](agent-review-notes.md#01dep-p05-b1-r3). Reviewer
  `/root/p05_reviewer`, requested Astra/high, effective runtime settings not independently exposed.
  Worktree `/Users/mac02/.codex/worktrees/2050/openbot`; phase/review base
  `c42fc15dd9eb05c9f463703b26a809d42929ab16`, HEAD
  `e7bc780ec7699051b99893d4f497bf2299c77738`, artifact
  `41b58b0262aabcca57dfe1f0ce401f65c99ddafdfcb03304bb54653ae7b56056`.
- Classification: **actionable defect, Medium, verification/requirements**. No architecture or
  product-code defect identified.
- Evidence: CI's `doop-integration` job runs the complete service test command on `ubuntu-latest`.
  The retained managed source `doop/source/tests/canvasClipboardBrowser.test.ts:40–45` explicitly
  disables its 17 clipboard tests on Darwin unless `DOOP_CLIPBOARD_TESTS` is set. This is an OS
  difference, not an unconditional ignored suite: Ubuntu with its required browser exercises
  those tests. `logs/B1-R3-doop-service-test-rerun.txt` instead reports 2,711 passed / 17 skipped.
  The R3 manifest calls the service gate PASS with zero current NOT EXERCISED entries, and the
  completion record says required integration gates have successful evidence. Independent source
  excerpt/hash: [B1-R3-reviewer-identities.txt](logs/B1-R3-reviewer-identities.txt).
- Impact: the full CI service behavior has not been exercised. The 104 successful private-image
  checks cover document generation, access, lifecycle, exports and private assets, but do not
  substitute for clipboard assertions such as replacing stale element copies with external
  images/text, copy/paste focus, or grouped undo. Reporting the skip count is transparent but does
  not fulfill plan 05A.3 or the prohibition on completing with required unexercised checks.
- Required correction: exercise all 17 unchanged clipboard cases on a suitable owned fixture
  (or record an explicit authorized scope disposition). Keep skipped R3 evidence immutable and
  provide focused correction evidence with the actual discovered/executed count and assertions.
  Until that exists, retain this coverage gap in the current mapping/completion record. An
  isolated Linux/browser fixture avoids the shared macOS pasteboard concern documented by the
  upstream tests. No need to repeat unaffected private profiles, production image, memory or
  full Rust suites for an evidence-only correction.
- Instruction lesson: check platform-specific skip guards when using a different host OS from
  CI; a successful command with skips cannot automatically establish equivalent assertion
  coverage. Existing verification-contract rules already require this comparison; no new
  instruction edit is authorized.
- Resolution history: **Open** at R3. Root confirmed no equivalent clipboard-specific execution
  is currently retained and will address this in a separate correction round.

### F3 resolution — 2026-11-11, 01DEP-P05-B1-R4

**Fixed** at artifact `b715976039bbe891c5771d5e7946621afe528baf95b4d57bfd6fd41f59b89e35`
(same base/HEAD). Inspected actual isolated Linux execution: all 17 original clipboard cases
pass, zero skips/failures, in non-root sandboxed Chromium with no host pasteboard mount. The
previous failure exposed a missing synthetic materialization provider in the HTML harness;
the correction uses the existing provider pattern already present in sibling rendering fixtures.
The adapter explicitly does not prove server authorization, media admission or budgets. Those
claims remain covered by the separate actual private-image acceptance checks.

Independently compared 1,159 old/new managed source files, excluding dependency/build/Git output:
only `tests/fixtures/canvasClipboard.html` differs. Clipboard test/assertion/timeout bytes and
production sources are unchanged. The canonical patch hash/pins agree, and the new source
fingerprint, immutable helper and image are consistent across the refreshed 104-check official
acceptance. The full macOS service evidence is reused only for the 2,711 unchanged cases; the
17 Linux cases supplement it, as the completion record now explicitly says. R3's skipped run
and all failed R4 attempts remain immutable.

Evidence: [R4 manifest](logs/01DEP-P05-B1-R4.evidence.json),
[clipboard execution](logs/B1-R4-clipboard-tests-3.txt),
[independent identities](logs/B1-R4-reviewer-identities.txt), and
[R4 review note](agent-review-notes.md#01dep-p05-b1-r4). No remaining F3 coverage gap.
