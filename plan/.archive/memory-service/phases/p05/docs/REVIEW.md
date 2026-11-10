# p05 review record

| Round | Stage | Batch | Reviewer | Artifact digest | Outcome | Notes / findings |
| --- | --- | --- | --- | --- | --- | --- |
| MEM-P05-B2-R1 | batch review | p05-B2 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | d20858a7e356ccd1cce9dbad6bff214a2bdf37dd60dd3ad00c7e8fab10c2f314 | CHANGES REQUIRED (10 Minor actionable, 3 optional; no Major). Return-time freshness: digest now f808b0a4… because of sibling edits (untracked plan/agent-workflow-call/, src/main.rs NO_BACKWARD_COMPATIBILITY_NOTES) — re-freeze before recheck | [notes](agent-review-notes.md) MEM-P05-B2-R1; [findings](agent-review-findings.md) |
| MEM-P05-B2-R2 | correction recheck | p05-B2 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | a369b44e6526a7e542e81d78108877c0874d3308185c17d9e3ffb62cff2cfc75 | CHANGES REQUIRED: F1–F9, F13 fixed; F11, F12 fixed (optional); F10 deferred (optional, accepted); new F14 (Minor, actionable) and F15 (optional) | [notes](agent-review-notes.md) MEM-P05-B2-R2; [findings](agent-review-findings.md) |
| MEM-P05-B2-R3 | correction recheck (F14, F15, F11 nit) | p05-B2 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 9c7929bf1e3b9fce84664264e0530133669cf078bc77ff45a8ad587dafd51abc | **PASS** for the whole batch p05-B2 (R1–R3): F14, F15, F11 nit fixed; new F16 optional only | [notes](agent-review-notes.md) MEM-P05-B2-R3; [findings](agent-review-findings.md) |
| MEM-P05-B3-R1 | batch review | p05-B3 (agent memory tools) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 0d0b4622ed5d78726877bfccc1604450e4a67eff42ddff189957a58090336c3d | CHANGES REQUIRED: 1 Major (F1), 3 Minor actionable (F2–F4), 1 optional (F5). Return-time freshness: digest now 834ac9d9…d0fe — only src/main.rs differs from freeze tree dfeeca67 (a sibling session removed 4 legacy plan entries from NO_BACKWARD_COMPATIBILITY_NOTES at 12:27); re-freeze before recheck | [notes](agent-review-notes.md) MEM-P05-B3-R1; [findings](agent-review-findings.md) |
| MEM-P05-B3-R2 | correction recheck (F1–F5; broadened to `search_scopes` and the eligibility refactor) | p05-B3 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 50d052b7858ba6aab6cdad37ff2e0022d73d4a7f465bbe3d235146a802b23457 | CHANGES REQUIRED: F1 resolved (ruling U4), F2 fixed, F3 code fixed but its test does not discriminate → new F6 (Minor, actionable), F4 resolved (ruling U5), F5 resolved; new F7 optional | [notes](agent-review-notes.md) MEM-P05-B3-R2; [findings](agent-review-findings.md) |
| MEM-P05-B3-R3 | correction recheck (F6; optional F7 taken) | p05-B3 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 8857f53867e70b2edb79a9b13a0d21d4b9f05f5729f129daec690f5744bea777 | **PASS** for the whole batch p05-B3 (R1–R3): F6 fixed, F7 taken; no open B3 findings. Freshness: current digest de7efd2c… differs from freeze tree a69f88c5 only in sibling plan files and src/main.rs NOTES (listed in the note) | [notes](agent-review-notes.md) MEM-P05-B3-R3; [findings](agent-review-findings.md) |
| MEM-P05-B4-R1 | batch review | p05-B4 (automatic recall, 05B.1–4, E7) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | a998bca3b5a3688a535b43b4cbd49d62e1cbe2d3601921a78679b049d21eece1 | CHANGES REQUIRED: 4 Minor actionable (F1–F4), 1 optional (F5); no Major | [notes](agent-review-notes.md) MEM-P05-B4-R1; [findings](agent-review-findings.md) |
| MEM-P05-B4-R2 | correction recheck (F1–F5) | p05-B4 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 507e893896000e551a847db69b4179ba7b399d7683e4feb2e82fd9a8401f8f1b | CHANGES REQUIRED: F1 fixed, F2 fixed but its deleted-item branch is untested → new F6 (Minor, actionable), F3 fixed, F4 resolved (ruling U6), F5 not taken (accepted); new F7 optional | [notes](agent-review-notes.md) MEM-P05-B4-R2; [findings](agent-review-findings.md) |
| MEM-P05-B4-R3 | correction recheck (F6; optional F7 taken) | p05-B4 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 8490e2e6c999eea62a7738e2e762b18c1fbda91cbec21f106612bc761d4f78ee | **PASS** for the whole batch p05-B4 (R1–R3): F6 fixed (reviewer mutation L now fails the test), F7 taken; no open B4 findings | [notes](agent-review-notes.md) MEM-P05-B4-R3; [findings](agent-review-findings.md) |
| MEM-P05-B5-R1 | batch review | p05-B5 (capture, generation intents, delivery, E9 races) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | be48350f22f124546d81d9d9abc175039d0c971b8580c817ac32fe103629a5c3 | CHANGES REQUIRED: 2 Minor actionable (F1, F2), 2 optional (F3, F4); no Major. B6 gate split accepted | [notes](agent-review-notes.md) MEM-P05-B5-R1; [findings](agent-review-findings.md) |
| MEM-P05-B5-R2 | correction recheck (F1–F4) | p05-B5 | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 6041e394d913523eb06c8aaac8348c675134643e8947fe1281a6a20eca9e4f1e | **PASS** for the whole batch p05-B5 (R1–R2): F1 fixed, F2 resolved (ruling U7, implemented), F3 fixed, F4 taken; new F5 optional (nit). Phase 05 still waits on the B6 gate | [notes](agent-review-notes.md) MEM-P05-B5-R2; [findings](agent-review-findings.md) |
| MEM-P05-B6-R1 | gate review (the phase-05 gate, E11; reviewed as code) | p05-B6 (live integration script, CI job, live and browser tests, 05 gate evidence) | claude-opus-5-5 / xhigh (B abdd866fb428eea05) | 88808f5ad06857e86e417fe9593fd6b72185bdc01b42ad52871054c77e81b0a1 | **PASS** for p05-B6; **phase 05 can be accepted**. Split condition met (all five prefixes pass on the final tree; reviewer rerun 148 passed, 0 failed, 2 ignored); B5 impact limited to the per-part source_ref; B5-F5 fixed. Optional only: F1–F3. Not exercised: CI execution, all-target suite (06C) | [notes](agent-review-notes.md) MEM-P05-B6-R1; [findings](agent-review-findings.md) |

## MEM-P05-B6-R1

- Gate: **PASS**; phase 05 can be accepted. Residuals: the new CI job's first run is the only proof of its wiring,
  and the all-target full suite belongs to 06C.
- Optional:
  - [MEM-P05-B6-F1](agent-review-findings.md#mem-p05-b6-f1--the-ci-job-builds-the-memory-service-with-an-implicitly-installed-toolchain) — the job installs stable, but the service build relies on rustup implicitly installing services/memory's pinned 1.95.0.
  - [MEM-P05-B6-F2](agent-review-findings.md#mem-p05-b6-f2--the-service-url-keeps-only-a-tcp-host-and-port-from-the-admin-url) — a socket-style admin URL points the service at the driver's default server, and query parameters are dropped (fails closed).
  - [MEM-P05-B6-F3](agent-review-findings.md#mem-p05-b6-f3--the-operator-token-is-passed-on-curls-command-line) — the operator token is on curl's argv while the call runs.
- Resolved: [MEM-P05-B5-F5](agent-review-findings.md#mem-p05-b5-f5--unconfigureds-doc-comment-now-prefixes-configured) (fixed).

## MEM-P05-B5-R2

- Resolved: [MEM-P05-B5-F1](agent-review-findings.md#mem-p05-b5-f1--the-operator-docs-say-message-triggered-and-scheduled-runs-are-never-captured), [MEM-P05-B5-F2](agent-review-findings.md#mem-p05-b5-f2--with-the-url-removed-capture-keeps-freezing-new-intents-that-nothing-sends-or-settles) (U7), [MEM-P05-B5-F3](agent-review-findings.md#mem-p05-b5-f3--validates-doc-comment-now-documents-provision), [MEM-P05-B5-F4](agent-review-findings.md#mem-p05-b5-f4--the-mapping-does-not-cite-e11s-delete-vs-delivery-for-capture-intents) (taken).
- Optional (new): [MEM-P05-B5-F5](agent-review-findings.md#mem-p05-b5-f5--unconfigureds-doc-comment-now-prefixes-configured) — `unconfigured`'s doc comment now sits on `configured()`.

## MEM-P05-B5-R1

Actionable (Minor):
- [MEM-P05-B5-F1](agent-review-findings.md#mem-p05-b5-f1--the-operator-docs-say-message-triggered-and-scheduled-runs-are-never-captured) — docs/workflow-operations.md says message-triggered and scheduled runs are never captured, yet thread capture captures them, customer email threads included.
- [MEM-P05-B5-F2](agent-review-findings.md#mem-p05-b5-f2--with-the-url-removed-capture-keeps-freezing-new-intents-that-nothing-sends-or-settles) — with `MEMORY_SERVICE_URL` removed, capture keeps queuing new intents that hold the exchange text, and nothing sends or settles them. docs/deploy.md says only already-queued ones are affected and that they expire.

Optional:
- [MEM-P05-B5-F3](agent-review-findings.md#mem-p05-b5-f3--validates-doc-comment-now-documents-provision) — `validate`'s doc comment now sits on `provision`.
- [MEM-P05-B5-F4](agent-review-findings.md#mem-p05-b5-f4--the-mapping-does-not-cite-e11s-delete-vs-delivery-for-capture-intents) — the B5 mapping does not cite E11's "delete vs delivery" (covered by B2's kind-agnostic tests).

## MEM-P05-B4-R3

- Resolved: [MEM-P05-B4-F6](agent-review-findings.md#mem-p05-b4-f6--a-deleted-tool-read-memory-is-never-shown-to-be-revoked-the-absent-id-path-is-untested), [MEM-P05-B4-F7](agent-review-findings.md#mem-p05-b4-f7--the-docs-do-not-say-that-disabling-memory-revokes-in-flight-runs-that-were-shown-memory) (taken).

## MEM-P05-B4-R2

Actionable (new):
- Minor [MEM-P05-B4-F6](agent-review-findings.md#mem-p05-b4-f6--a-deleted-tool-read-memory-is-never-shown-to-be-revoked-the-absent-id-path-is-untested) — the F2 test covers only a corrected revision. Reviewer mutation L (an id absent from the fetch passes) survives all 32 recall and tool tests.

Optional (new):
- [MEM-P05-B4-F7](agent-review-findings.md#mem-p05-b4-f7--the-docs-do-not-say-that-disabling-memory-revokes-in-flight-runs-that-were-shown-memory) — the docs omit that a disable makes resumed runs that were shown memory end `memory.context_revoked`.

Resolved in R2 (entries in the findings): F1, F2 (code), F3, F4 (U6); F5 not taken (accepted).

## MEM-P05-B4-R1

Actionable (Minor):
- [MEM-P05-B4-F1](agent-review-findings.md#mem-p05-b4-f1--a-resumed-attempt-replays-its-frozen-pack-unvalidated-when-no-recall-is-composed) — with no recall composed (URL removed after a freeze), a resumed attempt replays its frozen pack with no revalidation (fail open).
- [MEM-P05-B4-F2](agent-review-findings.md#mem-p05-b4-f2--recorded-memory-tool-items-are-never-revalidated-on-resume-though-a-fetch-based-check-is-possible) — E7 requires revalidating recorded tool items. TASK drops this, though a fetch by recorded id and revision could check them.
- [MEM-P05-B4-F3](agent-review-findings.md#mem-p05-b4-f3--e11s-recall-deadline-and-two-concurrent-recalls-in-one-company-cases-are-missing) — E11's "recall deadline" and "two concurrent recalls in one company" cases are untested, so one permit per pack request would pass.
- [MEM-P05-B4-F4](agent-review-findings.md#mem-p05-b4-f4--the-recall-cap-and-byte-term-deviate-from-e7-only-in-task) — D1 (8,192) is justified, but it and the byte term deviate from E7 only in TASK; record them in the decision record.

Optional:
- [MEM-P05-B4-F5](agent-review-findings.md#mem-p05-b4-f5--revalidation-runs-per-scope-sequentially-outside-the-recall-deadline) — revalidation runs scope by scope with full request timeouts, outside the recall deadline.

## MEM-P05-B3-R3

- Resolved: [MEM-P05-B3-F6](agent-review-findings.md#mem-p05-b3-f6--the-one-operation-search-test-passes-with-one-permit-per-request) (reviewer mutation D now fails the test 3/3), [MEM-P05-B3-F7](agent-review-findings.md#mem-p05-b3-f7--the-attached-session-and-thread-run-refusals-are-proven-only-in-the-pure-decision) (taken; reviewer mutation E fails the new test).

## MEM-P05-B3-R2

Actionable (new):
- Minor [MEM-P05-B3-F6](agent-review-findings.md#mem-p05-b3-f6--the-one-operation-search-test-passes-with-one-permit-per-request) — the F3 test passes with one permit per request (reviewer mutation D, 5/5): it never requires the three requests to be in flight together.

Optional (new):
- [MEM-P05-B3-F7](agent-review-findings.md#mem-p05-b3-f7--the-attached-session-and-thread-run-refusals-are-proven-only-in-the-pure-decision) — the attached-session and thread-run refusals are tested only on the pure decision, never through the SQL facts.

Resolved in R2 (entries in the findings): F1 (U4), F2, F4 (U5), F5; F3's code (its verification continues as F6).

## MEM-P05-B3-R1

Actionable:
- Major [MEM-P05-B3-F1](agent-review-findings.md#mem-p05-b3-f1--the-user-scope-is-reachable-from-a-manual-run-of-a-workflow-another-person-published) — a manual run of a workflow someone else published can read (and forward) the actor's private user memory: arch-review M2 scenario 3 and the E11 "foreign-authored manual workflow" case. D4 reinterprets E2 only in TASK.md, and the E11 refusal and inheritance cases are not exercised.
- Minor [MEM-P05-B3-F2](agent-review-findings.md#mem-p05-b3-f2--the-memory-tool-checkpoints-authorization-and-fencing-have-no-adversarial-tests) — `require_memory_invocation` and the tool fencing have no tampered-argument, ungranted-tool, lost-ownership, duplicate-save or revoked-credential tests (E11); reviewer mutation C survives.
- Minor [MEM-P05-B3-F3](agent-review-findings.md#mem-p05-b3-f3--one-multi-scope-search-takes-several-interactive-slots-at-once-e1-a-tool-call-is-one-operation) — a multi-scope search holds one interactive permit per scope concurrently, which is the company's whole share of 2 (E1: a tool call is one operation).
- Minor [MEM-P05-B3-F4](agent-review-findings.md#mem-p05-b3-f4--memory-tools-are-offered-and-publishable-while-the-memory-service-is-unconfigured) — the catalog and publication gate only on tenant enabled, not on the service being configured (E1/E5); D10 covers dispatch only.

Optional:
- [MEM-P05-B3-F5](agent-review-findings.md#mem-p05-b3-f5--the-workflow-bundle-publication-gate-and-the-catalogs-enabled-read-are-untested) — the workflow-bundle Publish gate and the catalog's DB read are untested.

## MEM-P05-B2-R3

- Resolved: [MEM-P05-B2-F14](agent-review-findings.md#mem-p05-b2-f14--delivery-lanes-take-more-pool-connections-than-the-reservation-and-docs-state), [MEM-P05-B2-F15](agent-review-findings.md#mem-p05-b2-f15--marker-owned-invariant-docs-no-longer-describe-memorysave), F11 nit.
- Optional (new): [MEM-P05-B2-F16](agent-review-findings.md#mem-p05-b2-f16--one-row-lock-wait-in-a-completion-now-pauses-every-delivery-lane) — completion has no lock_timeout, so a long-held intent row lock now pauses all lanes.

## MEM-P05-B2-R2 findings

Remaining actionable (Minor):
- [MEM-P05-B2-F14](agent-review-findings.md#mem-p05-b2-f14--delivery-lanes-take-more-pool-connections-than-the-reservation-and-docs-state) — delivery lanes take up to `MEMORY_SERVICE_BACKGROUND_IN_FLIGHT` pool connections; the reservation, `worker_slots` check and docs/deploy.md:765 still count one.

Optional:
- [MEM-P05-B2-F15](agent-review-findings.md#mem-p05-b2-f15--marker-owned-invariant-docs-no-longer-describe-memorysave) — the marker-owned invariant docs don't mention memory.save's intent protocol.

Resolved in R2 (resolution entries are in the findings): F1, F2, F3, F4, F5, F6, F7, F8, F9, F13; optional F11, F12; F10 deferred.

## MEM-P05-B2-R1 findings

Actionable (Minor):
- [MEM-P05-B2-F1](agent-review-findings.md#mem-p05-b2-f1--the-terminal-intent-wake-path-disableexpiryrefusal--step-failure-has-no-test) — terminal-intent wake path untested (behavior confirmed by reviewer probe).
- [MEM-P05-B2-F2](agent-review-findings.md#mem-p05-b2-f2--memorysave-not-made-marker-owned-e4-an-interrupted-save-attempt-becomes-reconcile) — memory.save not marker-owned (E4); interrupted attempt → Reconcile.
- [MEM-P05-B2-F3](agent-review-findings.md#mem-p05-b2-f3--contexthistory-exactness-test-cannot-distinguish-exact-from-inexact-agent-messages) — context.history exactness test non-discriminating.
- [MEM-P05-B2-F4](agent-review-findings.md#mem-p05-b2-f4--e6-race-and-mutation-evidence-use-undeclared-stand-ins) — E6 race/mutations use undeclared stand-ins; patches not retained.
- [MEM-P05-B2-F5](agent-review-findings.md#mem-p05-b2-f5--missing-step-level-05ae3-cases-and-competing-tests-for-new-settlement-protocols) — missing 6/8/9, drop-to-3, missing-ceiling, owner-removal/claimed-delivery competing and workflow-tool child cases.
- [MEM-P05-B2-F6](agent-review-findings.md#mem-p05-b2-f6--an-unopenable-credential-envelope-makes-one-intent-poison-the-whole-delivery-queue) — unopenable envelope poisons the queue.
- [MEM-P05-B2-F7](agent-review-findings.md#mem-p05-b2-f7--delivery-worker-is-single-flight-fifo-the-delivery-permits-and-tenant-fairness-are-inert) — single-flight FIFO delivery; no tenant fairness.
- [MEM-P05-B2-F8](agent-review-findings.md#mem-p05-b2-f8--settled-intents-keep-the-frozen-memory-text-forever-unbounded-table-scans) — settled intents keep memory text forever; unindexed scans; no 06A carry.
- [MEM-P05-B2-F9](agent-review-findings.md#mem-p05-b2-f9--the-editor-catalogue-never-lists-memory-steps-even-when-they-are-handled) — editor catalogue hides memory steps when configured.
- [MEM-P05-B2-F13](agent-review-findings.md#mem-p05-b2-f13--the-competing-claimant-delivery-test-assumes-which-claimant-wins-flaky-under-load) — competing-claimant delivery test flakes under load.

Optional:
- [MEM-P05-B2-F10](agent-review-findings.md#mem-p05-b2-f10--the-channel-reply-exactness-assertion-does-not-isolate-the-b2a-5-lowering) — channel-reply test does not isolate B2a-5.
- [MEM-P05-B2-F11](agent-review-findings.md#mem-p05-b2-f11--raise-to-maxexposed_level-item-levels-as-defense-in-depth) — raise to max(exposed, item levels).
- [MEM-P05-B2-F12](agent-review-findings.md#mem-p05-b2-f12--consumption-item-refs-store-content_sha256-not-e6s-sources_sha256) — consumption refs `sources_sha256` (B3/B4).

Confirmed unrelated failures (left unchanged; see notes): `rendered_markup_carries_no_inline_javascript`
(pre-existing offender in an unchanged file), two `security_auth_limits_*` tests (private cluster `--locale=C`).

Earlier phase-05 reviews (old workflow, read-only): tmp/memory-service-run/reviews/p7a-arch-r1..r4.md (gate PASS r4),
p7b*-code-r*.md (P7b accepted at r4, tree 113801e4).
