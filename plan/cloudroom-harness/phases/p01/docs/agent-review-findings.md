# Cloudroom harness p01 review findings

## CH-P01-AG1-F1 — Resolve frontmatter identity on library-copy slug collisions

- Date/workflow/reviewer: 2026-11-10; implement-astra-reviewed-2-2; independent B, gpt-6-astra/high (requested role settings).
- Context: p01/B1 architecture gate; review note CH-P01-AG1-N1; [TASK](TASK.md); [REVIEW](REVIEW.md). Worktree `/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree`; base/HEAD `abac7038a08a355156b7680a01d2f79f3de756fd`.
- Artifact: CH-P01-M1/r1, SHA256 `e24182f35493e0a275faa79a0f57479d2f7893907728bee3d379d6c2b4f2fc56`.
- Classification: actionable architecture/correctness defect, medium (P2); originates in the proposed contract, not an implementation.
- Evidence: decision lines 7 and 19 require complete SKILL.md bytes, frontmatter name equal to the stored slug, and whole-bundle library copying. Existing `copied_slug` and `copy_library_skill_on` at `src/adapters/persistence/skill.rs:133-190` use `original-2` (and further suffixes) when the company already owns the source slug, while copying `source.instructions` unchanged. The same copy helper is called by `prepare_library_agent_on` (`src/adapters/persistence/channel.rs:574`) and standalone library copy. An imported library skill with `name: invoice` copied into a company containing `invoice` therefore cannot both retain its SKILL.md bytes and satisfy the new `invoice-2` identity check. A strict validator rejects the established copy path; skipping it persists an inconsistent native bundle.
- Required correction: decide and record the identity/byte-preservation behavior for collisions and rename operations before implementing the persisted contract. Account for both standalone skill copy and indirect library-agent copy; add a collision case that proves the resulting bundle is usable and the source remains unchanged. Do not leave this to an accidental validation failure or silent metadata rewrite.
- Supporting evidence: [inspection excerpts](logs/CH-P01-AG1-inspection.md).
- Instruction lesson: the existing instruction to trace affected producers and consumers applies; no new instruction is warranted from one case.
- Resolution history: 2026-11-10 OPEN; blocks CH-P01-M1/r1 architecture acceptance.

## CH-P01-AG1-F2 — Align the JSON write boundary with valid supporting-file payloads

- Date/workflow/reviewer: 2026-11-10; implement-astra-reviewed-2-2; independent B, gpt-6-astra/high (requested role settings).
- Context: p01/B1 architecture gate; review note CH-P01-AG1-N1; [TASK](TASK.md); [REVIEW](REVIEW.md). Worktree `/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree`; base/HEAD `abac7038a08a355156b7680a01d2f79f3de756fd`.
- Artifact: CH-P01-M1/r1, SHA256 `e24182f35493e0a275faa79a0f57479d2f7893907728bee3d379d6c2b4f2fc56`.
- Classification: actionable architecture/API-contract defect, medium (P2); originates in the proposed contract.
- Evidence: decision line 9 permits 256 KiB per supporting file / 512 KiB total but says existing form/body limits remain unchanged; line 19 exposes that representation to JSON writes. The skill router applies one 163,840-byte body limit to both HTML and `/api/skill-library` JSON POST/PUT (`src/adapters/http/routes/skill_library.rs:38-81`). A single allowed 256 KiB file already requires 349,528 base64 bytes before metadata; the pure size calculation in the linked evidence demonstrates the mismatch. ZIP can create a valid bundle whose canonical JSON representation cannot be written back through the promised JSON surface. The existing `the_body_limit_admits_the_largest_valid_skill_as_json` test explicitly guards this boundary but currently only includes text.
- Required correction: define a bounded JSON upload/write path that admits the declared valid representation, or explicitly reconcile the API contract with a narrower supported bound; do not promise the full representation while retaining an incompatible hard limit. Keep ordinary form boundaries scoped appropriately. Extend enduring boundary coverage for maximum valid serialized JSON and controlled over-limit rejection, including escaping/base64 overhead, so any raised bound retains an early CI signal as required by root AGENTS.md.
- Supporting evidence: [inspection and size calculation](logs/CH-P01-AG1-inspection.md). This is source inspection plus arithmetic, not an executed HTTP test.
- Instruction lesson: existing boundary-enforcement and raised-limit instructions cover this; no new instruction proposed.
- Resolution history: 2026-11-10 OPEN; blocks CH-P01-M1/r1 architecture acceptance.

## CH-P01-AG1-R2 resolution follow-up — 2026-11-10

Review note CH-P01-AG1-N2; decision CH-P01-M1/r2 SHA256 `aabc37ecdcb5960c066b65c58b001f3020c9852ff4f8ebb189d45194eaa61853`; same worktree/base/HEAD and coordination revision 1. This closes decision defects only; their implementation assertions remain required at B1 review.

- **CH-P01-AG1-F1: FIXED in the architecture contract.** The revised authoring section explicitly gives a suffixed copy its new frontmatter identity, retains other YAML semantics and exact body/assets/modes, leaves the source unchanged, validates before insertion, and applies the shared helper to direct and indirect library copying. It distinguishes no-rename exact-byte preservation from an intentional copied identity change, removing the contradiction. Both collision paths and preservation assertions are required focused tests.
- **CH-P01-AG1-F2: FIXED in the architecture contract.** The revised bounds section supplies a separate 2 MiB encoded-body limit for both asset-bearing JSON surfaces; HTML metadata forms retain their existing bound. Canonical maximum-valid serialization must remain below 75% of the JSON cap in CI, and real router tests must admit maximum assets and reject body-limit+1 with 413. Encoded overload rejection is explicit. This reconciles valid file payloads with a bounded request and preserves an early growth signal.

Evidence: [r2 identity and delta inspection](logs/CH-P01-AG1-R2-identity-check.md). No new actionable or optional findings.

## CH-P01-B1-F1 — Library copy can combine instructions and files from different revisions

- Date/workflow/reviewer: 2026-11-10; implement-astra-reviewed-2-2; independent B `/root/cloudroom_harness_reviewer`, assigned Astra/high.
- Context: p01, whole B1 implementation review CH-P01-B1-R1; note CH-P01-B1-N1; [TASK](TASK.md), [REVIEW](REVIEW.md).
- Worktree/base/HEAD: `/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree`; base and HEAD `abac7038a08a355156b7680a01d2f79f3de756fd`. Frozen artifact `cd38325289a55b05843071e92738e5bf97fb8bfeeb06d5a4e4a7afec1e74ebad`.
- Classification: actionable correctness/concurrency defect, medium/P2. Affects B1-AC1/AC3 complete, independent bundle copying. Origin: implementation extends an existing two-read copy pattern to executable assets; the architecture review did not identify this inherited snapshot-coherence obligation.
- Evidence: `src/adapters/persistence/skill.rs:183-239` reads the source Skill without a lock, then each collision attempt inserts previously read instructions but selects `source.files` and required tools in a new READ COMMITTED statement. `copy_library_to_company` at lines 251-262 uses a default transaction. A concurrent source edit between reads (or during a collision retry) can persist a bundle combining old instructions and new script bytes. The shared helper also serves indirect library-agent copying at `src/adapters/persistence/channel.rs:574`.
- Executed reproduction: [probe source](logs/B1-review-copy-race.rs), [output](logs/B1-review-copy-race.txt). A transaction holds an uncommitted conflicting company slug; the actual production copy method reads source A and blocks on its first insert. After observing the real lock wait, another connection updates the source to B. Releasing the slug conflict makes the production copy retry with `-2` and return `instructions="Instructions A"`, script bytes `"B"`. This is a durable mixed bundle, not a sequential mock. All probe-created UUID rows were removed. Initial probe compilation failed only for a missing trait import; the corrected reviewer probe compiled and ran, with both build logs retained.
- Required correction: make all copied bundle content/identity/tool requirements come from one coherent source revision throughout collision retries while preserving existing tenant/deletion authority. Add a deterministic competing source-editor/copy regression through production persistence; cover the shared direct/indirect helper's inherited obligation. A correction that changes the accepted ownership/locking contract needs its affected architecture disposition.
- Instruction lesson: existing producer/consumer tracing and concurrency regression requirements apply. Candidate scoped review reminder supported by this case: when new file bytes extend a multi-statement copy, inspect which statement supplies every part of the copied bundle, including collision retries. No instruction file change authorized.
- Resolution history: 2026-11-10 OPEN; blocks B1 acceptance.

## CH-P01-B1-F2 — ZIP decoding applies the entrypoint bound to ordinary suffix-matching assets

- Date/workflow/reviewer: 2026-11-10; implement-astra-reviewed-2-2; independent B `/root/cloudroom_harness_reviewer`, assigned Astra/high.
- Context: p01, B1 implementation review CH-P01-B1-R1; note CH-P01-B1-N1; [TASK](TASK.md), [REVIEW](REVIEW.md).
- Worktree/base/HEAD: `/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree`; base and HEAD `abac7038a08a355156b7680a01d2f79f3de756fd`. Frozen artifact `cd38325289a55b05843071e92738e5bf97fb8bfeeb06d5a4e4a7afec1e74ebad`.
- Classification: actionable correctness/round-trip defect, medium/P2. Affects B1-AC2 and the documented 256 KiB supporting-file contract. Origin: implementation.
- Evidence: `src/adapters/http/routes/skill_library/archive.rs:138-145` chooses the 20 KiB SKILL.md source/header limit using `path.ends_with("SKILL.md")`, even when the basename is an ordinary supporting asset such as `reference-SKILL.md`. The domain and persistence accept this path and a 32 KiB payload; encode exports it, but decode refuses the resulting archive as oversized. Exact entrypoint detection elsewhere requires `SKILL.md` or a wrapper component plus `/SKILL.md`, so the suffix rule is broader than the root contract.
- Executed reproduction: [probe source](logs/B1-review-archive-probe.rs), [output](logs/B1-review-archive-probe.txt). The probe compiles the actual unmodified archive module and links the current private-target domain library. Both `reference.md` and `reference-SKILL.md` with 32,768 bytes validate and export; only the latter fails reimport with `Skill ZIP entry exceeds its byte limit.` No simulated reimplementation of the decoder was used.
- Required correction: apply the special instruction-file limit only to actual entrypoint candidates; ordinary suffix-matching files retain the supporting-file bound. Add an export/import regression above the instruction bound but within the asset bound, both unwrapped and wrapped as relevant.
- Instruction lesson: existing round-trip and discriminating-boundary test requirements suffice; no instruction change proposed.
- Resolution history: 2026-11-10 OPEN; blocks B1 acceptance.

## CH-P01-B1-R2 resolution follow-up — 2026-11-10

Review note CH-P01-B1-N2; frozen artifact `4cd3cf46d84474c01e74508751adc5dd6918a9c8e75c41cf8f0c1f52a37f3b13`. Same worktree, phase start/review-base/HEAD `abac7038a08a355156b7680a01d2f79f3de756fd`, coordination revision 1. Original findings remain preserved above.

- **CH-P01-B1-F1: FIXED.** `SkillCopyDb` captures the full Skill and required_tools in the same initial SQL statement. Every insert attempt binds instructions, files and required tools from that captured revision, with source UUID/global-library existence still checked by the INSERT. The actual new production regression `skill_bundle_competing_editor_and_both_copy_paths_keep_one_revision_during_slug_retries` blocks on a real competing unique-index insertion, edits the source after an observed backend lock wait, and checks all copied content/tool requirements remain revision A while the source becomes B. It runs both direct skill and indirect library-agent copying on isolated migrated databases. This fulfills the accepted coherence contract without a new ownership/locking protocol.
- **CH-P01-B1-F2: FIXED.** The decoder selects an exact root or unique wrapper entrypoint and applies the smaller bound only to that exact path. The new regression covers 32 KiB `reference-SKILL.md` and nested `references/SKILL.md` at both root and wrapped locations, asserting bytes/modes survive export/reimport. Broadened review covered the new raw central-directory name/count preflight and post-reader count comparison. Separate regressions demonstrate real ZipArchive duplicate-name and Unicode-Path alias collapse and assert production rejection; no silent deduplication is relied upon.

Evidence: [R6 skill execution](logs/B1-focused-R6-all-skill-callers.txt) records both correction regressions and affected variants passing (77 passed, 1 explicit generator ignored); the generator and browser are exercised separately in [R2 evidence manifest](logs/CH-P01-B1-R2.evidence.json). Reviewer inspected exact source/decisive assertions and independently validated the manifest. No new findings.

## CH-P01-AG2-F1 — Reconcile required ownership with the fork-lineage discriminator

- Date/workflow/reviewer: 2026-11-10; implement-astra-reviewed-2-2; independent B `/root/cloudroom_harness_reviewer`, assigned Astra/high.
- Context: p01/B2 architecture CH-P01-M2/r1; note CH-P01-AG2-N1; [TASK](TASK.md), [REVIEW](REVIEW.md). Worktree `/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree`; base/HEAD `abac7038a08a355156b7680a01d2f79f3de756fd`; unchanged B1 source `4cd3cf46d84474c01e74508751adc5dd6918a9c8e75c41cf8f0c1f52a37f3b13`.
- Decision SHA256: `c52199f08340f99aa95e9e17664dd16eb1c9541e17343a3b19c89a1152b4bb42`.
- Classification: actionable architecture/correctness defect, medium/P2; originates in proposed persisted ownership contract.
- Evidence: M2 chooses a new managed `origin_kind` alongside `origin_plugin_id` as durable required-configuration identity. In the pinned core, nonnull `originKind` means a source-derived thread: `thread-create.ts:725-768` reinterprets parent as source and rejects creation without sourceThreadId; lines 945-965 require an available source session to fork. Nonnull predicates also change seedWithoutRun (`thread-provisioning.ts:258-267`) and parent notifications (`thread-parent.ts:22-28`). A new managed root has no source thread. Treating capability ownership as another lineage value therefore breaks spawn or changes hierarchy/provisioning behavior unless these meanings are explicitly reconciled. Root separately reported the create-path evidence; B independently inspected these consumers.
- Required correction: record the persisted identity/lineage semantics before dependent patches. State how fresh managed root creation, parented creation, ordinary fork/import, startup/restart and parent notifications preserve intended behavior while authenticated owner identity stays immutable. Include the affected-consumer inventory and actual-core tests for a managed root without a source and ordinary fork behavior without inherited managed authority. This does not mandate a storage alternative; the author must make the consequential choice concrete.
- Evidence: [pinned source excerpts](logs/CH-P01-AG2-source-evidence.md).
- Instruction lesson: existing contract-impact rules cover semantic consumers; a field named origin is not necessarily generic provenance. No instruction change authorized.
- Resolution history: 2026-11-10 OPEN; blocks M2/r1 acceptance.

## CH-P01-AG2-F2 — Preserve files excluded by the reused native tree transfer

- Date/workflow/reviewer: 2026-11-10; implement-astra-reviewed-2-2; independent B `/root/cloudroom_harness_reviewer`, assigned Astra/high.
- Context: p01/B2 architecture CH-P01-M2/r1; note CH-P01-AG2-N1; [TASK](TASK.md), [REVIEW](REVIEW.md). Same worktree/base/HEAD/source identity as F1.
- Decision SHA256: `c52199f08340f99aa95e9e17664dd16eb1c9541e17343a3b19c89a1152b4bb42`.
- Classification: actionable architecture/requirements defect, medium/P2; proposed downstream transfer contract is incomplete.
- Evidence: B1 accepts root supporting file `.bb-registry-skill.json` through SkillFilePath::parse and preserves its bytes. M2 requires complete directories while reusing bb-skill-tree-v1 and host-daemon staging. The pinned collectSkillTreeEntries deliberately skips that exact root filename (`injected-skills.ts:227-258`; constant `registry-skill-provenance.ts:6`). readSkillTreeManifest uses that collector, and the actual `/skills/tree/:hash` transfer endpoint invokes it again (`internal/skills.ts:16-47`). Default collection silently omits a valid asset; a new required collector that includes it would instead disagree with the reused transfer endpoint's hash. A script reading that adjacent file cannot receive the promised complete bundle through the current contract.
- Required correction: explicitly reconcile required collection, hashing, registry lookup and downstream transfer so every supported B1 file is preserved or a deliberate reviewed boundary rejects it before admission. Silent loss is unacceptable. Preserve ordinary registry provenance behavior for unrelated skills. Add an actual pinned-core/host-transfer regression containing this filename, asserting exact staged bytes and matching tree hash. Review transfer-side read bounds on that same path, since the old collector itself is unbounded.
- Evidence: [pinned source excerpts](logs/CH-P01-AG2-source-evidence.md). Source inspection only; no native launch or credentials accessed.
- Instruction lesson: inherited transformations/exclusions belong to the reused API contract. Existing producer/consumer and complete-file requirements apply; no instruction edit authorized.
- Resolution history: 2026-11-10 OPEN; blocks M2/r1 acceptance.

## CH-P01-AG2-R2 resolution follow-up — 2026-11-10

Review note CH-P01-AG2-N2; decision CH-P01-M2/r2 SHA256 `08357154aecaa1b044a42688a0f25230eff0e7ca2f9ca56d7bb7989889be9403`. Original findings and decision remain preserved. Accepted B1 source remains `4cd3cf46d84474c01e74508751adc5dd6918a9c8e75c41cf8f0c1f52a37f3b13`.

- **CH-P01-AG2-F1: FIXED in the architecture contract.** A separate immutable thread_required_agent_configurations row captures authenticated plugin ownership atomically with thread creation and survives plugin unload/uninstall. Existing origin_kind and source/parent meanings remain unchanged. Explicit root, parented creation, fork/import, provisioning, notification and restart dispositions prevent inheriting required authority from lineage. Actual-core root-without-source, parented/fork and missing-owner cases are required; their execution remains outstanding.
- **CH-P01-AG2-F2: FIXED in the architecture contract.** Explicit required-tree source/registry/authenticated transfer uses one bounded all-file collector, including `.bb-registry-skill.json`, with the existing exact hash and host staging. Ordinary registry provenance behavior stays separate. Transport union/fetch dispatch, registration misses, cached-store/final injection and failure propagation are explicitly covered. The actual route-to-production-daemon regression must compare bytes/modes/hash and execute a script reading the reserved-name asset; ordinary provenance exclusion gets its own assertion. This is a planned correction, not an executed transfer claim.

Evidence: [correction source inspection](logs/CH-P01-AG2-R2-inspection.md), [revised decision](logs/CH-P01-M2-r2.md). No remaining actionable or optional findings in this correction recheck.
