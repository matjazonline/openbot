# Reviewer findings

All findings below share the identity and scope of [CL-P01-B1-R1](agent-review-notes.md#cl-p01-b1-r1).

## CL-P01-B1-F1

- Date: 2026-11-07; workflow: `implement-astra-reviewed-2-2`; phase: p01; batch: B1; stage: whole-phase batch review.
- Reviewer: `/root/cloudroom_reviewer`, independent context. Requested settings: `gpt-6-astra` / `high`; authoritative actual model/effort metadata is not exposed to this agent, so actual settings are not independently asserted.
- Worktree: `/Users/mac03/dev/openbot`; branch: `main`; phase start/review base/HEAD: `97e5d60dc538e26c7cb976a8d63772eed80baff2`.
- Frozen artifact: `79f5005f390df698935120b9dacdecabaa7ed8610eb03d2b0a563f8addd71cd4`.
- Records: [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); note: [CL-P01-B1-R1](agent-review-notes.md#cl-p01-b1-r1).
- No architecture gate applies to this code-only round; p02 persistent schema/concurrency remains a future gate.

- Classification: actionable verification/requirements gap; **P1**, blocks p01 acceptance.
- Required: p01 01A probe items 1–6 include restart identity, provider/model discovery and UI-visible native threads for all three providers, queued/active/completed and actual turn correlation, durable pagination/replay, two-client manipulation, crash after SDK acceptance before mapping persistence, stop semantics and permission visibility/recovery. The README requires compatibility proof before dependent commitments; coordination CL-I001 requires accepting p01 before p02 implementation.
- Observed: `integrations/cloudroom-plugin/compatibility/probe/server.ts:57–110` performs only scheduled Codex create/send in one plugin client. Lines 96–98 compare two empty event lists; retained `queued.txt` reports zero events. Stop is called only on an undispatched thread. There is no crash/restart/reconciliation path, provider-model query, second client, real UI observation or interaction exercise. The runner reloads the plugin but never reconstructs the server/profile. `before.txt`/`after.txt` do not even advertise `acp-opencode`. The manifest correctly marks many gaps NOT EXERCISED but names `atomicCreateMetadata` PASS when line 85 merely reads metadata (without comparing its operation identity), and no acceptance-to-mapping crash is tested.
- Evidence: [runtime result](logs/reproducible-probe-r1/result.json), [queued proof](logs/reproducible-probe-r1/queued.txt), [before](logs/reproducible-probe-r1/before.txt), [after](logs/reproducible-probe-r1/after.txt), [round manifest](logs/CL-P01-B1-R1.evidence.json). AC1 PASS only supports its exercised subset; AC2 remains NOT EXERCISED. Source inspection of the thread-only stop signature is valid evidence of API shape, not active cancellation behavior.
- Impact: phase exit would commit downstream correlation/recovery choices without the required compatibility evidence. Honest uncertainty is the right fallback for unsupported operations, but unexecuted probes do not establish which operations are unsupported.
- Required correction: exercise the required public-boundary probes with decisive assertions and retained source/runtime identity, including available non-inference restart/crash/two-client cases. Complete authorized provider/UI/permission proof or obtain an explicit scope disposition; until then keep p01 unaccepted. Narrow the metadata claim to what is proved until crash/recovery evidence exists. Do not infer permission to use existing credentials.
- Instruction lesson: existing plan and verification-contract guidance already require this distinction; no additional instruction proposed.
- Resolution history: 2026-11-07 OPEN against frozen digest above. Not a defect originating in an approved architecture gate.

## CL-P01-B1-F2

- Date: 2026-11-07; workflow: `implement-astra-reviewed-2-2`; phase: p01; batch: B1; stage: whole-phase batch review.
- Reviewer: `/root/cloudroom_reviewer`, independent context. Requested settings: `gpt-6-astra` / `high`; authoritative actual model/effort metadata is not exposed to this agent, so actual settings are not independently asserted.
- Worktree: `/Users/mac03/dev/openbot`; branch: `main`; phase start/review base/HEAD: `97e5d60dc538e26c7cb976a8d63772eed80baff2`.
- Frozen artifact: `79f5005f390df698935120b9dacdecabaa7ed8610eb03d2b0a563f8addd71cd4`.
- Records: [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); note: [CL-P01-B1-R1](agent-review-notes.md#cl-p01-b1-r1).
- No architecture gate applies to this code-only round; p02 persistent schema/concurrency remains a future gate.

- Classification: actionable requirements/contract gap; **P2**, blocks acceptance of p01 01B as complete.
- Evidence: `src/domain/local_agents/operations.rs:76–80` defines `LocalAgentOperation` with only `id`, `command`, `deadline`. It has no immutable payload hash, dispatch evidence or outcome. `DeliveryState`/`OperationOutcome` are standalone unused enums in `src/domain/local_agents/mod.rs:67–82`, not an operation contract. `LocalMachine` at lines 26–34 has no capabilities snapshot or reference; `LocalCatalog` is a separate application type with no relation to the machine.
- Required: the original p01 01B responsibility table explicitly assigns stable identity, immutable payload hash, deadline, dispatch evidence and outcome to the operation, and observed capabilities to the machine. p02 says to persist these phase-01 profiles/sessions/operations, and p03 consumes the digest/conflict and separate state dimensions.
- Impact: the advertised phase boundary cannot represent essential facts that dependent persistence/protocol work must preserve; tests currently exercise input syntax and bounds, so they cannot establish these missing responsibilities. This is not a request to implement p02 persistence early.
- Required correction: complete the application/domain boundary for the specified operation and machine responsibilities, with validated digest/evidence/state values and focused contract assertions, or explicitly agree a revised phase ownership before claiming p01 complete. Preserve inner-layer independence and the distinction between observations and validated outcomes.
- Instruction lesson: original requirement-to-contract mapping was insufficiently concrete; existing acceptance-mapping guidance applies. No new general rule proposed.
- Resolution history: 2026-11-07 OPEN against frozen digest above. No architecture gate previously accepted these omissions.

## CL-P01-B1-F3

- Date: 2026-11-07; workflow: `implement-astra-reviewed-2-2`; phase: p01; batch: B1; stage: whole-phase batch review.
- Reviewer: `/root/cloudroom_reviewer`, independent context. Requested settings: `gpt-6-astra` / `high`; authoritative actual model/effort metadata is not exposed to this agent, so actual settings are not independently asserted.
- Worktree: `/Users/mac03/dev/openbot`; branch: `main`; phase start/review base/HEAD: `97e5d60dc538e26c7cb976a8d63772eed80baff2`.
- Frozen artifact: `79f5005f390df698935120b9dacdecabaa7ed8610eb03d2b0a563f8addd71cd4`.
- Records: [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); note: [CL-P01-B1-R1](agent-review-notes.md#cl-p01-b1-r1).
- No architecture gate applies to this code-only round; p02 persistent schema/concurrency remains a future gate.

- Classification: actionable correctness/reproducibility defect; **P2**, blocks the claimed reproducible runner guarantee.
- Evidence: `integrations/cloudroom-plugin/compatibility/run-probe.mjs:17,28,91`. Every command runs with `cwd=<checkout>/gui`; the clean-source command is `git diff --exit-code HEAD -- gui`. Git interprets that pathspec relative to cwd, so it checks `<checkout>/gui/gui`, which has no tracked files. Modifying tracked `gui/package.json` leaves that guard exit code 0.
- Reproduction: [independent checks](logs/reviewer-r1-independent-checks.txt) records an isolated Git fixture: commit `gui/package.json`, modify it, run the exact guard from `gui` → 0; check the actual cwd-relative tree → 1. `git -C /tmp/openbot-cloudroom-d8b3cc3/gui ls-files -- gui` also returns no files. Current upstream HEAD/source hashes independently match, so this finding does not assert the retained run secretly used modified source.
- Impact: a later probe can build altered SDK/server/provider sources yet report the pinned commit and PASS, contradicting README's unchanged-source guarantee and invalidating reproducibility.
- Required correction: make the guard actually cover the intended checked-out source from its working directory and verify it refuses a modified tracked source before build/start. Preserve exact revision and locked install checks.
- Instruction lesson: pathspecs depend on command cwd. Existing reproducibility requirement applies; no broader instruction change proposed from this single failure.
- Resolution history: 2026-11-07 OPEN against frozen digest above. Not a gated-design defect.

## CL-P01-B1-F4

- Date: 2026-11-07; workflow: `implement-astra-reviewed-2-2`; phase: p01; batch: B1; stage: whole-phase batch review.
- Reviewer: `/root/cloudroom_reviewer`, independent context. Requested settings: `gpt-6-astra` / `high`; authoritative actual model/effort metadata is not exposed to this agent, so actual settings are not independently asserted.
- Worktree: `/Users/mac03/dev/openbot`; branch: `main`; phase start/review base/HEAD: `97e5d60dc538e26c7cb976a8d63772eed80baff2`.
- Frozen artifact: `79f5005f390df698935120b9dacdecabaa7ed8610eb03d2b0a563f8addd71cd4`.
- Records: [TASK.md](TASK.md), [REVIEW.md](REVIEW.md); note: [CL-P01-B1-R1](agent-review-notes.md#cl-p01-b1-r1).
- No architecture gate applies to this code-only round; p02 persistent schema/concurrency remains a future gate.

- Classification: **optional, nonblocking** evidence correction; **P3**.
- Evidence: [test-move-map.json](logs/test-move-map.json) claims the four original tests were under `application::local_agents::contract_tests`; [rust-test-list-retry.txt](logs/rust-test-list-retry.txt) actually discovers them under `application::local_agents::commands::tests`. Its replacement entries therefore map new names to themselves, not old names to discovered replacements.
- Impact: the declared audit is inaccurate, but independent review reconciled all four actual old names to the same suffix under `contract_tests` and confirmed all six current tests execute. No test disappearance was found, so this does not block acceptance by itself.
- Suggested correction: publish a new corrected movement map/evidence round using the raw pre-move inventory; preserve the relied-upon R1 evidence and manifest immutably.
- Instruction lesson: the skill already requires runner-discovered names; no new instruction proposed.
- Resolution history: 2026-11-07 OPEN OPTIONAL; replacements independently verified in [independent checks](logs/reviewer-r1-independent-checks.txt).

## CL-P01-D01-R1 disposition

2026-11-07: architecture decision CL-P01-D01/r1 PASS; no new findings. This gate does not close or recheck CL-P01-B1-F1–F4. See [gate note](agent-review-notes.md#cl-p01-d01-r1--architecture-gate).

## CL-P01-B1-R2 resolution updates

2026-11-07, artifact `30e57585311a0721cc56e3fa4f2220ffcee91886e30949f0534e6885e2fa2346`, independent [R2 note](agent-review-notes.md#cl-p01-b1-r2--correction-recheck):

- **CL-P01-B1-F1 — FIXED for p01 scope.** R7 public-SDK source runner exercises actual acceptance/mapping SIGKILL, positive metadata reconciliation without another spawn, independent CLI queue edits and restart identity. Native captures and real UI now establish all three providers' terminal correlation/replay, permission/question recovery and active thread-wide stop. The 1.19.5 OpenCode pin and failed earlier cases are explicit. Linux/actual desktop packaging and active human-turn interleaving remain later-phase obligations; no p07 acceptance implied.
- **CL-P01-B1-F2 — FIXED as originally reported.** Operation now has private frozen payload/deadline/digest, typed dispatch evidence and separate outcome; machine now references a scoped capability snapshot through the consuming port. New F5/F6 document remaining semantic defects rather than silently reopening different requirements under this ID.
- **CL-P01-B1-F3 — FIXED.** `check-source.mjs` uses explicit root and `diff HEAD -- gui/`; source preparation verifies exact approved patch and resulting file identity. Independent two-test regression run passed, including modified tracked source, wrong root/revision, unrelated/untracked edits and patch tampering. Current upstream has exactly the approved UI change.
- **CL-P01-B1-F4 — FIXED.** `logs/test-move-map-r2.json` maps actual pre-move `commands::tests` identities to discovered `contract_tests` replacements. R1 records unchanged. Nine current tests independently discovered/executed.

## CL-P01-B1-F5

- Date: 2026-11-07; workflow `implement-astra-reviewed-2-2`; phase p01/B1, correction recheck [CL-P01-B1-R2](agent-review-notes.md#cl-p01-b1-r2--correction-recheck); [TASK](TASK.md), [REVIEW](REVIEW.md).
- Reviewer `/root/cloudroom_reviewer`, requested GPT-6 Astra/high; actual metadata unavailable. Worktree `/Users/mac03/dev/openbot`, branch main; base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; digest `30e57585311a0721cc56e3fa4f2220ffcee91886e30949f0534e6885e2fa2346`.

- Classification: actionable correctness/contract defect, **P2**, acceptance-blocking. Introduced by the F2 correction, not by D01's gated UI design.
- Evidence: `src/domain/local_agents/operations.rs:189–215` validates all `Succeeded` outcomes through `NativeIdentity`; the final `else if` rejects success for `AcceptedLocally`, `SubmissionIntent` or `NotDispatched` regardless of command. `LocalCommand::RefreshCatalog {}` at line 69 has no thread/session target and succeeds by returning provider/project capabilities. A real successful refresh therefore cannot be decoded without inventing an unrelated `CloudroomThreadId`. Conversely the current validator accepts a refresh success with any fabricated native identity. The focused operation test uses RefreshCatalog but only checks queued/uncertain states, missing this production-relevant success variant.
- Required behavior: the closed command contract must represent valid success evidence for each command without treating native thread identity as universal. Preserve the stricter native turn identity requirement for successful create/send and correct thread matching for thread controls. Add focused assertions for successful catalog refresh and rejection of fabricated/mismatched evidence.
- Impact: downstream catalog result acceptance either rejects a valid operation or must manufacture identity evidence, undermining the typed boundary the phase is intended to establish.
- Verification: exact source branch inspected; isolated standalone harness retained but did not run due cached dependency-artifact mismatch, not a project compile failure. See [review note](agent-review-notes.md#cl-p01-b1-r2--correction-recheck).
- Instruction lesson: existing discriminating-variant verification guidance applies. No new instruction proposed.
- Resolution history: 2026-11-07 OPEN at R2 digest above.

## CL-P01-B1-F6

- Date: 2026-11-07; workflow `implement-astra-reviewed-2-2`; phase p01/B1, correction recheck [CL-P01-B1-R2](agent-review-notes.md#cl-p01-b1-r2--correction-recheck); [TASK](TASK.md), [REVIEW](REVIEW.md).
- Reviewer `/root/cloudroom_reviewer`, requested GPT-6 Astra/high; actual metadata unavailable. Worktree `/Users/mac03/dev/openbot`, branch main; base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; digest `30e57585311a0721cc56e3fa4f2220ffcee91886e30949f0534e6885e2fa2346`.

- Classification: actionable requirements/contract defect, **P2**, acceptance-blocking. Exposed by required real-provider compatibility proof; not a D01 design defect.
- Evidence: `src/domain/local_agents/selection.rs:17–31` freezes only machine/project/provider/model plus workspace/permissions; there is no reasoning-level input. The successful native probe in `integrations/cloudroom-plugin/compatibility/probe/native.ts:68–86` requires an independently supplied `reasoningLevel` argument and passes it to `bb.sdk.threads.spawn`. The deliverable manifest pins successful OpenCode 1.19.5 `openai/gpt-6-luna` execution with `reasoningLevel: low`. Retained `logs/native-r1/server-ui.txt:89` records the real default failure: HTTP 400, `Provider acp-opencode does not support none reasoning level. Supported reasoning levels: low, medium, high, xhigh, max.` The corrected native-start passes explicit low; that input cannot be encoded by the production domain contract.
- Required: original p01 01B freezes each accepted operation's effective execution inputs and keeps provider/model policy explicit. The proven supported execution policy must be representable in the frozen profile/session/create contract (and covered by its immutable digest), with validation against the supported capability where needed. Do not silently invent an adapter-only override or depend on mutable provider defaults to reproduce the accepted smoke.
- Impact: a valid typed OpenCode profile cannot express the only demonstrated working selection; translating it without the missing option reproduces the observed refusal. Choosing low later outside the frozen payload breaks the explicit effective-input boundary and makes the compatibility proof apply to different inputs than production.
- Required correction: carry the relevant typed execution option through the frozen contract and demonstrate serialization/digest preservation plus invalid/unsupported option handling. Reuse the existing actual successful smoke where the resulting contract expresses exactly its inputs; do not rerun unrelated providers merely to fix the type.
- Instruction lesson: actual SDK probe inputs must be reconciled into the production contract. Existing criterion/contract-impact guidance covers this; no new instruction proposed.
- Resolution history: 2026-11-07 OPEN at R2 digest above.

## CL-P01-B1-R3 resolution updates

2026-11-07, workflow `implement-astra-reviewed-2-2`, reviewer `/root/cloudroom_reviewer` (requested Astra/high; actual metadata unavailable), artifact `cfc3dd8052971597cce455f3c7013a5051c0abb74aed605b490017c547d93233`, base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`, worktree `/Users/mac03/dev/openbot`:

- **CL-P01-B1-F5 — FIXED.** CatalogSnapshot evidence is exclusive to RefreshCatalog, permits its successful result without a native thread and refuses invented native evidence. Thread commands reject catalog evidence, and native success retains required thread/turn checks. Independent current test `cloudroom_contract_catalog_success_requires_catalog_evidence` and existing effect-success test pass.
- **CL-P01-B1-F6 — FIXED.** Required typed reasoning is present in frozen ExecutionPolicy and its command hash. Per-model bounded capabilities plus pure selection validation reject unsupported reasoning. Exact successful OpenCode model/low input now serializes and preserves its digest; high tampering, unknown/missing reasoning and unsupported none are refused. Independent reasoning/digest and catalog-support tests pass. Reused R2 actual native proof remains applicable to those exact inputs.

[CL-P01-B1-R3 review note](agent-review-notes.md#cl-p01-b1-r3--f5f6-delta-recheck), [12-test results](logs/reviewer-r3-rust-tests.txt), [evidence](logs/CL-P01-B1-R3.evidence.json). F1–F4 remain resolved. No new findings; no unresolved acceptance-blocking p01 issues.
