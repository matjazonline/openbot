# Cloudroom local / p01

Workflow: implement-astra-reviewed-2-2. Status: investigating; not accepted or integrated.

## Assignment and resources

- Original request: implement all of `plan/cloudroom-local` with the selected workflow. Current group: `01-contracts-and-cloudroom-compatibility.md`; overview and p02–p07 remain requirements.
- Root/A: `/root`; GPT-6.1 Sol/xhigh confirmed by user in this session. B: not started; request GPT-6 Astra/high, independent context, at first gate/review. Workers may not spawn agents.
- Worktree: `/Users/mac03/dev/openbot`, branch `main`. Phase start/review base/HEAD: `97e5d60dc538e26c7cb976a8d63772eed80baff2`. No commits authorized.
- Preserved pre-existing work: nine deleted files under `.agents/skills/implement-astra-reviewed-2-2/`; no source changes at start.
- Record root: `/Users/mac03/dev/openbot/plan/cloudroom-local/phases/p01/docs`.
- Authority: `/Users/mac03/dev/openbot/plan/cloudroom-local/COORDINATION.md`, revision 1 checked at startup; CL-I001–004/r1 acknowledged. No notices or requests.
- Prerequisites: current application/workflow contracts; no prior Cloudroom implementation.
- Owned deliverables: pinned compatibility proof/probe under `integrations/cloudroom-plugin/compatibility/`; focused domain types and application local-agent contracts. No database/network admission until p02/p03.
- Disposable upstream checkout: `/tmp/openbot-cloudroom-d8b3cc3`, source `d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815`. Never use an existing Cloudroom profile. Probe storage/ports will be isolated before server startup.
- Context controls/runtime occupancy unavailable; use host defaults and durable checkpoints. Do not change global configuration.
- Snapshot exclusions (exact): `plan/cloudroom-local/COORDINATION.md`, `plan/cloudroom-local/phases/p01/docs`, `plan/cloudroom-local/phases/p02/docs`, `plan/cloudroom-local/phases/p03/docs`, `plan/cloudroom-local/phases/p04/docs`, `plan/cloudroom-local/phases/p05/docs`, `plan/cloudroom-local/phases/p06/docs`, `plan/cloudroom-local/phases/p07/docs`. Exclude only workflow records. Current digest: not frozen.

## Batches

- B1: Entire p01 compatibility proof plus typed contracts and bounds. SDK proof precedes dependent protocol choices. Review the cohesive artifact once; gate any costly decisions before dependent implementation.
- Focused checks: pinned public-SDK probe with explicit fixture/provider distinction; Rust discovered `cloudroom_contract_` / `cloudroom_bounds_` tests with nonzero selection, format and offline compilation.
- Final plan gate: p07 original full-suite list reconciled against CI; fresh migrations, SQLx cache verification, full database-backed suite, network/stack/lease/sample/browser/production checks plus plugin build/typecheck/tests. Real provider/macOS/Linux matrix separately reported. No full-suite claim on unavailable checks.

## Acceptance mapping

| Criterion/source | Production path and case | Decisive assertion | Check/evidence | Status |
| --- | --- | --- | --- | --- |
| AC1 / p01 01A | Pinned public SDK lifecycle and mutation/replay | Actual package/build identities; counted effects and durable identities across retries/crash | Disposable probe, manifest | Pending |
| AC2 / p01 01A | Provider/UI/permission/stop semantics | Actual native tasks or explicit unproved capability; thread stop never targeted stop | Fixtures and separate real-provider evidence | Pending |
| AC3 / p01 01B | Validated identities and closed commands | Unknown fields/commands, wrong identity, invalid values rejected | Rust focused contract tests | Pending |
| AC4 / p01 fixed bounds | Admission, frame/prompt/result/catalog/deadline boundaries | Exact max accepted; max+1 rejected including UTF-8 bytes | Rust boundary tests; peer enforcement in later groups | Pending |

## Contract impact

| Contract | Producers/consumers | Obligations/disposition | Focused checks |
| --- | --- | --- | --- |
| New local-agent identities/entities | Later SQL, HTTP, WebSocket and plugin | Transport/SQL excluded from inner types; frozen execution selections; company and association scoped | Validated decoding tests and p02/p03 checks |
| Public Cloudroom SDK | Plugin, native providers and local UI | Exact revision/SDK/provider pins; honest uncertain dispatch; no private imports/runtime | Actual SDK probe; counted fixtures do not prove provider inference |

## Decisions and check results

- 2026-11-07: Original plan and repository/skill instructions read. Source fetched at exact pinned revision; locked dependency installation in progress. No product code edited yet.
- Next: execute public SDK proof, record proven/uncertain semantics; trace Rust module consumers and implement contracts.

## B1 frozen artifact / 2026-11-07

- Current HEAD/start/review base remain `97e5d60dc538e26c7cb976a8d63772eed80baff2`; digest `79f5005f390df698935120b9dacdecabaa7ed8610eb03d2b0a563f8addd71cd4`, exact original exclusions unchanged. Entire source checkout is frozen for independent review. Snapshot uses installed GNU coreutils through `PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:$PATH`; BSD realpath rejected the skill script's GNU options on the initial attempt.
- Coordination revision 1 rechecked; instructions acknowledged; immutable copy `logs/coordination-r1.md`. No notices or pending cross-phase requests.
- Actual upstream source built successfully with the pinned lockfile. Registry SDK 0.6.50 is unavailable (npm E404); use the source build, not another BB package. Manifest hashes source contracts/package versions.
- Live repeatable public-SDK proof PASS: fresh profile/server/daemon; install/reload; secret settings and plugin SQLite durability; service abort before storage closes; metadata supplied on create; queued sends. Two equal create request IDs produced two threads, and two equal queued send IDs produced two queue entries. No upstream mutation idempotency is assumed. Terminal turn correlation, permission recovery, nonempty event replay, native inference, actual UI rendering and Linux remain NOT EXERCISED. Existing-logins authorization question is pending; no permission inferred from elapsed time.
- Implemented validated UUID/external/model/generation/sequence identities; company-owned entities; frozen execution selections; closed typed commands and observation evidence; scoped consuming read port. Domain owns prompt/output and operation types; application owns catalog/admission/protocol limits. No HTTP, database or SDK types in inner contracts.
- `cargo test --locked --offline --lib cloudroom_ -- --list`: 6 tests selected (nonzero). `cargo test --locked --offline --lib cloudroom_`: 6 passed, 0 failed. Decisive assertions: invalid and noncanonical identities; unknown fields and commands; exact max/max+1 UTF-8 prompt/output/catalog sizes; elapsed and above-24h deadlines; effect saturation reserves control queue slots. First run found serde unit-variant permissiveness; fixed with an empty struct variant. Original failure retained in `logs/rust-contract-tests.txt`; corrected evidence in final log.
- Four earlier test names moved from `commands::tests` to `contract_tests` when domain operation ownership was corrected; all four map to discovered replacements and two new tests, recorded in `logs/test-move-map.json`.
- Offline test compilation initially lacked locked fastembed 7.1.1; `cargo fetch --locked` provisioned cache without changing Cargo.lock; final offline compilation passed. `cargo check --locked --offline --all-targets`, formatting, whitespace, and public SDK declaration typechecking passed. Typecheck's earlier missing `mentions`/string-pagination failures were corrected; historical logs retained.
- Peer frame/chunk/reassembly/outbox and persisted admission enforcement are requirements of p02–p04; constants do not prove their future enforcement. No final/full-suite claim at this phase. Criterion mapping: AC1 live lifecycle/queued mutation proof PASS with explicitly limited coverage; AC2 NOT EXERCISED; AC3 typed decoding PASS; AC4 pure prompt/output/catalog/admission/deadline contracts PASS, peer/persistence integration pending p02–p04. Manifest: `logs/CL-P01-B1-R1.evidence.json`.
- Compatibility decisions: ambiguous SDK mutation is uncertain until positive matching evidence, never automatic resubmission; no completion from idle/latest assistant; manual thread-wide stop only; explicit exact provider/model with no fallback. These observations constrain p02–p05; persistent schema/concurrency gate belongs to p02 before dependent edits.
- Manual source-server/host listeners at ports 46986/46987 stopped; runner-owned services also terminated. Disposable profiles/source retained for evidence and possible authorized native proof. No project-source work or provider credentials changed.
- Independent review requested for whole p01 artifact, including whether outstanding proof blocks phase acceptance. Implementation exists; acceptance/integration remain pending. No p02 product edits.

- Independent B handle: `/root/cloudroom_reviewer`, requested GPT-6 Astra/high with no inherited history. First review CL-P01-B1-R1 in progress; digest confirmed by reviewer. No source changes during review.

## B1 correction work after CL-P01-B1-R1

- Full review returned CHANGES REQUIRED: F1/P1 public-SDK proof gap; F2/P2 operation/capability contract omissions; F3/P2 cwd-sensitive source guard; optional F4/P3 incorrect movement-map input. Original notes/findings and R1 manifest are immutable.
- F2 author corrections: operations now privately freeze command/deadline/digest; decoding recomputes the lowercase SHA-256 hash and rejects tampering. Typed dispatch evidence, separate outcome and delivery derivation are represented. A machine references its last accepted installation-scoped capabilities sequence; the read port requires company/machine/sequence and returns the bounded snapshot. Two new contract assertions bring discovered/executed focused tests to 8; all pass in `logs/rust-contract-tests-r2-final.txt`. This represents the contract; p02/p03 still own SQL scoping and authoritative transitions.
- F3 author correction: new `verifySource` uses explicit git `-C` root and `diff --quiet HEAD -- gui/`, avoiding both incorrect cwd pathspec and logging private diffs. A temporary-Git-fixture regression demonstrates unchanged PASS, wrong revision/root rejection, the old false pass, and modified tracked GUI rejection. `node --test integrations/cloudroom-plugin/compatibility/check-source.test.mjs` PASS, retained `logs/source-guard-tests-r2.txt`.
- F4 optional correction supplied in a new `logs/test-move-map-r2.json`, using the actual old `rust-test-list-retry.txt` inventory. It maps four old names to discoverable replacements; original R1 map remains unchanged.
- F1 available proof expanded: a real SIGKILL after public spawn acceptance and before any plugin thread mapping; server/profile reconstruction; positive metadata reconciliation with one saved attempt and zero new spawn calls; independent Cloudroom CLI and SDK messages preserving one local queue; graceful second server restart retaining secret/database/host identity. Full run PASS in `logs/reproducible-probe-r2/`; no native inference. Metadata now compares the saved operation identity explicitly. A plugin hook always parks probe-origin dispatch to avoid accidental inference even if scheduled work survives a failed run.
- Additional model discovery succeeded for Codex/Claude; ACP availability starts asynchronously. Actual UI build passed (`logs/cloudroom-ui-build.txt`). UI runtime attempt r3 failed because Vite was launched from gui rather than apps/app, so its React compiler dependency could not resolve; launch cwd corrected. No upstream source changed. UI attempt r4 was interrupted during local disk exhaustion; r5 is in progress.
- Local disk exhaustion interrupted Clippy (`logs/clippy-r2.txt`). Verified no rustc remained; removed only task-created incremental directories open_bots-2pz6km9wjzna5 (birth 11:34:41), open_bots-3drb81yxz4un7 and open_bots-2jrvmthkfwlee (birth 12:10:15), all on 2026-11-07 after this task began. Older caches preserved. Recovered 4.2 GiB; rerun uses `CARGO_INCREMENTAL=0` to avoid recreating the large task cache. This changes no resource limit or CI bound.
- `cargo test --locked --offline --test source_modules` unavailable: this checkout has no such test target, despite the installed skill reference. Log retained `logs/source-modules-r2.txt`; all new modules compile and 8 focused tests are directly discovered. Do not invent a passing target.
- Native account/project authorization remains pending. Native/UI/active-turn/permission/nonempty replay proof is still not accepted; F1 cannot close without actual evidence or an explicit user scope disposition. No p02 edits or phase acceptance.

## Current checkpoint: awaiting required user input

- Corrected source identity: base/HEAD/start `97e5d60dc538e26c7cb976a8d63772eed80baff2`; digest `bb7ece5de7bde5aa35d2311949099cba0988522511590945bead4152f7b5ba3f`; unchanged exact exclusions. Snapshot `logs/snapshot-b1-corrections.txt`; retained evidence `logs/CL-P01-B1-CORRECTIONS.evidence.json`. No commits, staging, deployments or production resets.
- Current focused verification: 9 discovered/executed Rust tests PASS, including immutable command/deadline hash, mismatched hash refusal, separate dispatch/outcome, wrong-thread evidence rejection and mandatory turn identity for successful prompt effects. Scope-reference and all previous bound tests pass. Clippy offline/all-targets with warnings denied PASS after the last Rust edit (`logs/clippy-r4.txt`). Formatting and whitespace PASS. Public SDK declaration typecheck PASS. Source-guard regression PASS. `graft build` refreshed graph; cache is git-ignored.
- Definitive live public-SDK run `logs/reproducible-probe-r6/` PASS: install/reload, secret/database durability, command non-idempotency, explicit metadata comparison, SIGKILL at accepted-create/mapping gap, positive recovery without resubmission, independent CLI/SDK queue ordering, graceful server restart and host identity. Actual source UI production build and Chromium render PASS for the same queued Codex thread (`ui-thread.json/png`), with real API response identity and visible prompt; no API stand-ins. Dev UI launch r5 failed on an upstream React-refresh development-module issue; using the successfully built UI via Vite preview resolved it without editing upstream. Earlier failures retained. Native inference/turn/permission/nonempty event replay and other providers' active/UI behavior remain NOT EXERCISED.
- F2/F3 corrections and optional F4 movement audit are author-verified, not reviewer-closed. The one correction recheck is deliberately not requested while actionable F1 still needs required native proof or an explicit scope disposition. Preserve reviewer R1 records unchanged. Do not count author verification as an independent pass.
- Coordination authority revision 1 rechecked, no notices, CL-I001–004/r1 remain acknowledged. No phase accepted or integrated. p02–p07 remain wholly unimplemented. Repository final full-suite gate has not run because implementation has not reached it.
- Required next input: answer pending question whether existing local Claude Code/Codex/OpenCode logins may run small smoke tasks in a disposable project, or explicitly limit work to fixtures and leave native acceptance unverified. No authorization inferred from elapsed time. User confirmation of root Sol 6.1/xhigh remains valid.
- Then complete/disposition F1, update evidence/artifact identity, and ask the same independent B for one combined recheck of F1–F4 before phase acceptance. Only after p01 acceptance establish p02 records and schema/ownership/concurrency architecture gate. No p02 implementation is authorized by a partial phase pass under this selected workflow.
- All probe/server/daemon/UI processes stopped. Task-created source checkout and isolated profiles retained, never development profiles. No further source edits while awaiting input. Model/effort and compaction settings as recorded above; actual reviewer metadata unknown beyond requested tool settings.

## Authorized native smoke continuation

- User replied “yes you can use for smoke test” to the pending question about existing local Claude Code/Codex/OpenCode logins and small smoke tasks in a disposable project. Authorization now persists for these smoke checks. No commits/deployments or production resets authorized. Root Sol 6.1/xhigh confirmation still valid. Coordination r1 rechecked, unchanged. Continue F1 proof, then one combined independent recheck of F1–F4.

## CL-P01-D01/r1 architecture gate


Original requirements: plan/cloudroom-local/README.md (no Cloudroom hosted account, local SDK/UI ownership) and 01-contracts-and-cloudroom-compatibility.md (failed proof may require a small reviewed upstream extension; no private agent runtime).

## Trigger and observed failure

Pinned source d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815 at gui/apps/app/src/components/Onboarding.tsx:87–100 forces an undismissable setup whenever the Cloudroom hosted account is absent and its Rust-Core self-hosted token mode is absent. Native local Codex and Claude execution works, and the real UI displays their history, but the global sign-in overlay blocks the native permission controls. Source lines 220–226 omit Skip setup while locked. This contradicts the local-only product requirement.

Evidence: native-r1/{codex,claude}-post-restart-state.txt prove three correlated completed native turns each and replay/pagination; codex-interaction-state.txt and claude-interaction-state.txt prove actual approval/user_question waits. approval-ui-diagnostic.json and approval-ui-controls.json show real UI control data behind the forced account modal. No native permission was approved or bypassed.

## Decision

For this integration's pinned source build, apply only the checked-in patches/local-onboarding.patch (identity in local-onboarding-patch-identity.json). Make setup optional: retain manual opening and desktop first-run setup, always offer Skip setup, and do not force the hosted-account dialog over existing local work. No server authentication, cloud API entitlement, native tool permission, credential, or database contract is changed. Desktop/local caller proof remains mandatory for protected API calls. Browser tests exercise the public loopback caller-proof challenge in a phase-owned profile to reproduce the desktop wrapper's authenticated context; they do not stand in API responses and never retain its token.

Base revision remains fixed plus patch SHA-256 c201acedcbe1003a3867507e7fb4138c4171d7f5fa0c2971c4e182800cd58ca4. Source file before SHA 26edcb0d980159dbf88fd28979d7d87dcd1935c8e429546e90228dbe5c22e489; after SHA d2aebaadf687bc3863f645acd2dff5add1fb943219afe0dec4f5a9ac2836496d. The patch has only been checked with git apply --check; NOT applied yet.

Build helper must verify the checkout root and exact base HEAD, accept only the approved changed-path set and exact resulting file hashes, apply the patch only to the clean pinned base, and reject arbitrary tracked edits, tampered patch inputs, or a different revision. Existing verifySource clean-check behavior remains tested. Record patch identity in the compatibility manifest and every build's result. No legacy release tolerance or unpinned dependency. Preserve upstream licenses.

## Invariants and failure behavior

- Local account sign-in is optional UI onboarding; cloud features still need their existing authenticated account at their server boundary.
- The plugin still uses only public BbPluginApi and owns no second agent runtime.
- Protected local API calls still require caller proof; no source patch changes those routes or trusts loopback alone.
- Failed/tampered build inputs refuse before dependency install/start; all code besides the approved UI patch must equal the pinned base.
- Unexpected native work remains uncertain, never blindly repeated. Revocation/permissions are not inferred from UI visibility.

## Contract impact and verification

Targets p01 compatibility manifest, p04 plugin build compatibility, p06 source/install instructions, p07 reproducibility/UI gates. No p02 persistence change is needed for this decision. Publish active notice CL-C001/r1 only after this decision passes.

Focused checks after pass: source-guard regression (clean base -> approved patch, repeat stable, unrelated edit/tampered input/wrong revision reject); pinned UI production build and app typecheck; real unmodified-API native approval and user-question UI visible without hosted account; explicit UI denial/answer recovers matching native turn; desktop first-run/manual setup still exposes Skip setup. Keep browser test tokens memory-only and restrict HTTP/WebSocket to the owned fixture origins. Existing SDK non-inference and Rust contract evidence reused only where code/fixtures unchanged. Whole corrected p01 artifact still needs the later combined batch recheck for F1–F4; this gate is not phase acceptance.

Status: proposed, pending independent pass. No upstream source patch applied and no dependent build-helper implementation yet. Source frozen for gate review after snapshot. Native services/fixtures remain phase-owned at 46985/46986/46987; reviewer must not operate them.

- CL-P01-D01/r1 independent PASS received in note CL-P01-D01-R1; no new findings. Digest 86c0b830a58c31d4dca1329f679edebfd89a837a42222ae094c148dc99a7c477 unchanged during gate. Coordination writer activates CL-C001/r1 at file revision 3, acknowledged by p01; implementation/verification pending. Proceed with the reviewed UI patch and strict source preparation; not phase acceptance and F1–F4 still open pending combined recheck.

## B1 native smoke completion and correction recheck preparation

Coordination authority rechecked at revision 3; CL-I001–004/r1 and active CL-C001/r1 acknowledged. Decision CL-P01-D01/r1 implemented after its independent pass. Exact approved UI patch and strict build helper are in the compatibility manifest. Author status: implementation and focused proof complete; independent batch acceptance pending. p02–p07 remain unimplemented.

User authorization “yes you can use for smoke test” was applied only to small tasks in phase-owned disposable projects using local provider logins. No credential export, commits, deployment, production reset or native permission bypass. Native fixture `/tmp/openbot-cloudroom-p01-profile`, loopback 46985/46986/46987; source fixed base plus exact patch. Native services are now stopped. Pinned native versions: Claude Code 2.1.319, Codex 0.166.0, OpenCode 1.19.5. The installed OpenCode 2.1.4 remained unchanged; ACP initialized but session/new timed out. A disposable npm install under `/tmp/openbot-cloudroom-opencode-1.19.5` supplied 1.19.5 only to the owned host's PATH. Package integrity is retained in the deliverable manifest. An initial Google-model smoke failed; an explicit new OpenCode task with `openai/gpt-6-luna` and `low` completed. No failed operation was retried as if idempotent.

AC1/F1: latest source-built non-inference runner r7 PASS after the approved patch: locked install, SDK/types/CLI/UI builds, lifecycle, server crash after SDK acceptance before mapping, metadata recovery without spawning, independent CLI/SDK queue and profile/host persistence. All logs under `logs/reproducible-probe-r7/`. Earlier failed attempts remain retained.

AC2/F1: real native captures under `logs/native-r1/` now prove all 3 providers complete exact marker tasks, public request-to-accepted-turn-to-terminal correlation, nonempty event replay/pagination, real built UI reading the same SDK thread identity. Claude/Codex native history persists across server restart. Codex approval pending -> actual UI Deny -> matching native turn terminal; Claude question pending -> actual UI answer POST -> matching native turn completed. Browser caller proof remains memory-only and requests are restricted to owned loopback origins. No hosted account used. Manual setup opens and Skip setup dismisses. Codex active shell execution -> public thread-wide stop -> same turn interrupted, with no completion marker emitted. The first stop arrived after completion and is retained as an unsuccessful timing attempt; it is not cancellation proof. `prove-native-stop.mjs` closes this race by observing the active tool and issuing stop in the same bounded script.

Decisive executed assertion command: `node integrations/cloudroom-plugin/compatibility/verify-native-evidence.mjs plan/cloudroom-local/phases/p01/docs/logs/native-r1` PASS, `logs/native-r1/verification.txt`. The verifier validates actual captures, not substitute responses. Native UI and stop scripts are retained beside the public SDK probe. Native model/catalog helper typecheck with `node /tmp/openbot-cloudroom-d8b3cc3/gui/node_modules/typescript/bin/tsc6 -p /tmp/openbot-cloudroom-proof-tsconfig.json` PASS (`logs/native-r1/typecheck-r5.txt`); the previous command used nonexistent `bin/tsc`, retained r4 failure. `node --test .../check-source.test.mjs` PASS including approved patch repeatability, wrong base, tampered patch, other source edit and untracked source rejection (`logs/source-guard-resumed.txt`). UI production build/typecheck PASS (`native-r1/patched-ui-{build,typecheck}.txt` and r7/ui-build.txt). Desktop first-run branch is retained by source inspection; actual desktop wrapper and headless Linux matrix belong to p06/p07 and remain NOT EXERCISED.

AC3/AC4/F2 and optional F4 reuse unchanged Rust contract, bounds, compilation/Clippy and exact discovered-test movement evidence from CL-P01-B1-CORRECTIONS. No Rust source changed since that evidence. Full repository verification remains the p07 gate. Existing unavailable `source_modules` target remains explicitly unavailable, not passed. Native 2.1.4 incompatibility and one failed Google smoke are characterized limitations, not passing provider runs. Terminal correlation requires persisted request/turn evidence; no idle-only success or atomic targeted cancellation is claimed.

Next: freeze whole source using unchanged exact record exclusions, create/validate CL-P01-B1-R2 evidence manifest, then obtain one combined independent correction recheck F1–F4 and D01 implementation. Only after acceptance start p02 schema/concurrency gate.

Frozen B1-R2 digest `30e57585311a0721cc56e3fa4f2220ffcee91886e30949f0534e6885e2fa2346`, start/review base/HEAD unchanged; exact exclusions unchanged. Evidence manifest `logs/CL-P01-B1-R2.evidence.json`, coordination r3 pinned in `logs/coordination-r3-review.md`. Entire implementation worktree frozen for independent correction review.

## R2 dispositions and F5/F6 correction

Independent CL-P01-B1-R2 resolved F1–F4 and confirmed D01 implementation. New F5/P2: catalog refresh success had only unrelated native-thread evidence; F6/P2: actual working OpenCode reasoning input could not be frozen. Both author corrections now implemented; focused Rust compilation/checks pending before freeze and one correction recheck. CatalogSnapshot evidence is exclusive to RefreshCatalog; create/send success still requires native turn identity and controls match their target. ExecutionPolicy.reasoning is required and typed to the pinned SDK's eight-level ladder. Per-model bounded reasoning capabilities and selection validation reject unsupported combinations. New assertions cover catalog success/fabricated evidence, frozen reasoning serialization/digest tampering, and unsupported model reasoning. No architecture decision was changed; no persistence exists yet.

User explicitly replied “Leave Linux acceptance unverified” to the headless Linux environment question. Coordination now revision 4 with CL-I005/r1; p01 acknowledges it and retains platform limitation. All other implementation and verification remains authorized and required.

F5/F6 verification mapping: `cloudroom_contract_catalog_success_requires_catalog_evidence` decodes a successful refresh with only a positive catalog sequence, rejects fabricated native evidence/receipt-only success, invalid zero sequence and catalog evidence on Send. Existing `cloudroom_contract_effect_success_requires_its_thread_and_a_turn` still exercises strict native turn/thread matching. `cloudroom_contract_reasoning_is_frozen_into_the_create_digest` round-trips the exact successful OpenCode model/low policy, proves reasoning changes alter the digest and tampering/missing/unknown reasoning is refused. `cloudroom_contract_catalog_refuses_unsupported_model_reasoning` admits supported low, refuses unsupported none and another provider, and rejects nine advertised reasoning entries. Model catalog bounds still use valid complete model entries at max and max+1. All 12 focused tests pass in `logs/rust-contract-tests-r3-final-review.txt`; final inventory in `logs/rust-test-list-r3-final.txt`. No tests removed/renamed; the previous nine names remain discovered. First run failed because the existing admission fixture omitted the now-required reasoning field; fixed to explicit none and retained the failure in `logs/rust-contract-tests-r3-review.txt`. No compatibility default was added.

Changed-contract impact: ExecutionPolicy producer is typed profile/create input; prospective profiles/sessions and command hashing consume the same ExecutionSelection. ProviderCatalog now advertises per-model bounded ModelCapabilities; the application owns validate_selection before later admission. Exhaustive graph search for ExecutionPolicy/provider_default and DispatchEvidence reconciled typed references and JSON fixtures; no existing SQL/HTTP/plugin production consumer exists yet. Future p02–p05 must persist/validate/pass explicit reasoning and catalog model options. Public native smoke already proves the low input; no repeat inference required. No persistent schema or D01 decision changed.

B1-R3 frozen digest `cfc3dd8052971597cce455f3c7013a5051c0abb74aed605b490017c547d93233`; start/review-base/HEAD unchanged, exact original exclusions unchanged. Clippy all-target/offline/warnings denied and format/whitespace PASS. Evidence `logs/CL-P01-B1-R3.evidence.json`, immutable coordination r4 `logs/coordination-r4-review.md`. Entire implementation worktree now frozen for F5/F6 delta recheck.

## p01 accepted in phase

Independent PASS CL-P01-B1-R3 received for `cfc3dd8052971597cce455f3c7013a5051c0abb74aed605b490017c547d93233`; all F1–F6 closed, D01 implemented/verified. Root recomputed the same digest and compared authority r4 byte-for-byte with pinned input; evidence validator PASS. Accepted in phase for original p01 scope. Not plan completion or final combined integration. Immutable source export `logs/p01-r3-source.patch` applies to base's isolated index, identity in `logs/p01-r3-source-identity.json`, including preserved pre-existing deletions. No actual index/staging/commit changed. p02 may now proceed against this accepted prerequisite.
