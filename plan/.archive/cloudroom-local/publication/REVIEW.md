# CL-PUB-R1 — independent publication integration review

**PASS for the verified affected scope**, 2026-11-08. This is not a claim that every repository gate passed. The inherited failures, earlier unexplained Cloudroom timeout, and native Linux waiver below remain part of this acceptance.

## Identity and authority

- Workflow: `implement-astra-reviewed-2-2`; reviewer B `/root/cloudroom_reviewer`, Astra/high requested, actual runtime model metadata unavailable. Root is the user-confirmed Sol6.1/xhigh author.
- Worktree: `/tmp/openbot-cloudroom-publication` (real `/private/tmp/openbot-cloudroom-publication`), branch `cloudroom-publication`.
- Reviewed HEAD: `f63f08e00bc5c94dbcd83a33ff58fbcf142f97e4`.
- Parents: accepted feature/archive `e67a6acfc41831d74b55548a1499a0d1d95d50d7` and origin/main `b8c46300e9ea43efa00a3a984249efbe0cd31baf`.
- Accepted artifact: `fbc00a75eb16d9c9cc00f4441976aaa9e176503a54d7dfad8a896d96d1143110`.
- Exact nine exclusions: `plan/.archive/cloudroom-local/COORDINATION.md`; each `plan/.archive/cloudroom-local/phases/p01/docs` through `p07/docs` separately; `plan/.archive/cloudroom-local/publication`.
- Authority: [COORDINATION.md](../COORDINATION.md), revision 38, byte-equal to [the retained pin](logs/coordination-r38.md). Acknowledged I001/r2, I002–005/r1; C001/r2, C002/r2, C003/r1, C004/r1, C005/r1, C006/r2, C007/r3, C008/r1. No new architecture decision was proposed in this composition batch.
- [Final immutable evidence](logs/CL-PUB-R1.final.evidence.json), SHA256 `bfe8b566631dfa99f76d68289c69991129d5d25e1b6a8e184b90209894827d58`: 62 checks, 55 PASS, 6 FAIL, 1 NOT EXERCISED. Both installed and repository evidence validators independently passed with `--require-tracked --expect-artifact`; validation establishes retention and identity, not acceptance.
- The original p01–p07 acceptance, including CL-P07-B1-R2 artifact `bd92212d24d49b9d937eac2c52723eae45085cdb81abc36d12c262dcdf33cbe9`, remains a prerequisite. This review covers composition and archive/publication changes against both parents, not a fresh review of every unrelated upstream feature.

## Reviewed contracts

The [37 shared paths](logs/reviewer-pub-r1-shared-paths.json), nine conflicts, and automatic merge seams were reviewed. Both wait kinds and completion exclusions survive; `wait_commit.rs:5–136` retains the transaction and local-agent prelock, and the actual caller `waits.rs:53–63` commits under the existing timeout. Both fair-poll/sweep readiness paths remain. Shared schema guards preserve exact local-operation and native-turn ownership, deadlines and reopen proofs. Entire appended feature and upstream schema blocks match their respective parents.

Retention preserves company-first locking and the stronger Cloudroom `FOR UPDATE SKIP LOCKED` collector fence, while retaining history payload and live local session/call protections. Reviewed competing retention, history expiry/source cascade, actual fair-poll and recovery tests exercise these seams. Action authorization retains `RunPhase::Claimed`; provisioning retains `step_builtins`; local and native handlers remain distinct. State/routes/modules, eight supervised workers for one slot, shutdown, locked dependencies, CSS union, startup archive links, CI plugin/browser and upstream integration gates are preserved. Detailed bounded source coverage and inspection limits are in the [checkpoint](logs/reviewer-pub-r1-checkpoint.md).

Archive audit accounted for all 1,636 original files: 1,633 byte-identical and three intentional live documentation/coordination changes. Four invalid historical manifests retain their exact bytes under explicit invalid names and an error index. All 123 startup-note paths and live plan links resolve. This does not assert that historical invalid evidence became valid.

## Evidence actually established

B executed source/parent comparisons, independent schema/archive/package/inventory/image-input/browser-retention audits, evidence-retention validation and its 12 fixture tests, final manifest validation, and fresh artifact/authority checks. B inspected root's runtime logs and assertions; B did not rerun Cargo, PostgreSQL, browsers, image or native provider inference in this publication round.

- Complete current stock run at **2,048 KiB**: **3,746 PASS / 1 inherited CSP FAIL / 14 explicit ignores**, all 3,761 results accounted. All **129 executable Cloudroom tests PASS**, including the full cut-point recovery test; the other four Cloudroom entries are one separately exercised browser fixture and three native tests supported by prior acceptance. No stack overflow. Actual fair-poll, local/history competing retention, history recovery, production shutdown and MCP cancellation pass in this run.
- Regular full run: **3,745 PASS / 2 FAIL / 14 ignores**. Main **5 PASS** and source-module reachability **8 PASS** were separately run because the library failure stops `--all-targets` before those targets.
- Lease budget: **133 PASS**, timing assertions PASS, script exit 0 in 204.77 seconds. Formatting, offline all-target compilation, Clippy, fresh migration, SQLx, CSS and applicable static checks pass except the inherited transport scanner.
- Plugin: 90 tests and 4 guards PASS; install/typecheck/build, reproducible package and fresh packaged locked runtime installation PASS. Generated browser evidence includes 10 Rust fixture invocations, 8 driver invocations containing 12 actual tests, 25 protocol tests and 2 run-count tests, all PASS. All 54 retained browser inputs independently match their hashes.
- Current ARM64 production image build/check PASS; all 1,784 build inputs independently match frozen source. Image `sha256:88dcd985a941e55312b0c9c0ff88a4d1d999dc4fbf797425a810e87df7285af8` ran non-root, read-only, network-none with 2,048 MiB container/1,536 MiB RSS guard; peak RSS 1,522,728,960 bytes, retrieval 16/16 and exact-first 3/3, missing/tampered CLI and serve checks PASS. Initial read-only fixture-location failure is retained; the unchanged checker passed from a writable guest copy.
- Test inventory grows 3,332 → 3,774, with 442 additions, no removals, and all 133 Cloudroom names retained.

## Explicit limits and failure dispositions

See [review findings/dispositions](agent-review-findings.md). Transport boundary and CSP scanners reproduce on exact unchanged origin inputs. MCP's identical `completed 3 of 4` failure reproduces in an actually compiled origin test; B audited 1,698 baseline inputs and its binary hash. These are confirmed inherited failures and are left unchanged. Whole regular/stock/static gates remain failed where their logs say FAIL.

The regular run's Cloudroom cut-point test timed out at `local_agent_e2e_tests.rs:282` while polling durable `workflow_outcome` for 15 seconds. The log does not identify the cut or cause. The focused rerun passed in 34.41 seconds and the complete stock run also passed this test. These establish current affected proof; they do **not** explain, erase, or classify the earlier timeout as unrelated. Residual intermittent-test risk remains explicit. No timeout or resource limit was raised.

The initial C-locale database run was aborted and retained; both Unicode authentication failures pass on the corrected `en_US.UTF-8` fixture. Diagnostic origin builds that failed for disk capacity establish no behavioral result; the later actual origin test does. The retained complete PostgreSQL log has no storage/write/fsync/PANIC errors under independent inspection.

Prior actual macOS native acceptance is reused only for unchanged packaged runtime bytes: all 23 prior archive files and their modes/types/link targets match; the new archive only adds `compatibility/patches/.gitattributes`. Current package SHA `0c96eb901ec5f51b2edde3caef4470468488b9d3aa38f329848923d439647576` differs from the prior native-tested archive. No fresh native inference or identical archive hash is claimed. **Native Linux provider/platform acceptance remains user-waived and NOT EXERCISED**; current Linux image/core proof is separate.

No actionable composition/source defect remains established. Source and manifest identities were rechecked at acceptance. B holds no runtime resource. Cleanup evidence records root's removal of owned fixtures while preserving native artifacts and the current package. This PASS permits the already authorized records commit and normal publication of this reviewed source; it does not authorize deployment or widen the platform claim.
