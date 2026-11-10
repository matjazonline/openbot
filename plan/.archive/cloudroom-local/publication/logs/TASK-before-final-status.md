# Cloudroom publication integration

Original request: implement `plan/cloudroom-local` with `implement-astra-reviewed-2-2`; after full completion, push and move the plan to `.archive`. The user confirmed root Sol6.1/xhigh authorship and waived native Linux provider acceptance while retaining Linux core/image verification.

Root/author: `/root`. Reviewer: existing `/root/cloudroom_reviewer`, Astra/high requested. Worktree `/tmp/openbot-cloudroom-publication`, branch `cloudroom-publication`. Original implementation base `97e5d60dc538e26c7cb976a8d63772eed80baff2`, accepted artifact `bd92212d24d49b9d937eac2c52723eae45085cdb81abc36d12c262dcdf33cbe9`; feature/archive commit `e67a6acf`. Remote integration base `b8c46300` (11 newer commits).

Authoritative coordination is now `plan/.archive/cloudroom-local/COORDINATION.md`; revision37 acceptance and applicable I001–005/r1,C001/r2,C002/r2,C003/r1,C004/r1,C005/r1,C006/r2,C007/r3,C008/r1 rechecked and acknowledged. Original historical records retain original paths. All prior source behavior remains subject to accepted invariants. No new wire/schema ownership decision is proposed: this batch composes independently implemented capability sets.

## Scope and invariants

Archive the whole plan with byte preservation and update startup warning/CI documentation links. Historical invalid manifests are retained byte-for-byte with explicit invalid names and indexed validation errors; final-v2 remains current source acceptance evidence. Preserve nine pre-existing local skill deletions, which newer origin independently removes. Do not stage or publish ordinary credential data or build caches.

Merge current origin/main without losing vault, history or Cloudroom behavior. Resolve nine conflicting files by retaining both dependency/module/state/setup additions; both `local_agent_parked` and `agent_turn_parked`; both wait readiness predicates and both completed-effect exclusions; both CSS utility additions; eight supervised workers for one workflow slot. Retain the stronger Cloudroom company `FOR UPDATE SKIP LOCKED` fence for tree retention, which also serializes competing collectors; do not weaken the company→run→machine→operation/call retention/admission order. Existing foreign-key protection and competing-retention proofs remain required.

## Verification mapping

- Archive: compare original file hashes (archive-move.json), inspect deliberate relative CI link update, resolve runtime note paths, validate final-v2 and all valid tracked manifests. Explicitly index invalid historical evidence without rewriting output.
- Combined composition: offline all-target compilation, source-module reachability, supervisor shutdown test and combined full Rust suite.
- Shared wait/retention/schema contracts: fresh PostgreSQL18.6 migration; SQLx metadata check; existing competing local retention and agent-history tests in full suite; stock2MiB and lease budgets because dispatch/worker composition changed.
- Plugin/browser/CSS: pinned npm installs, typecheck/build,90tests/4guards, generated CSS and composed local-management browser proof; remote browser suites as final integration checks.
- Formatting, whitespace, Clippy, boundaries and evidence-retention CI contracts.

Use only owned PostgreSQL cluster `/tmp/openbot-cloudroom-publication-pg` port56389, database `cloudroom_publication_test`; normal provider accounts and other PostgreSQL clusters untouched. Root target cache is reused; dependencies are locked/offline for Rust. Retain prior macOS provider and Linux image acceptance where byte-identical plugin/image inputs support it; report any current unexercised image rerun rather than inventing proof. Full original platform matrix is not re-run for archive-only paths; affected server composition is exercised through real PostgreSQL/WSS counted peers.

Implementation and verification in progress. Freeze source and request affected final integration review after compilation and focused proofs establish the combined contracts.

## Live verification checkpoint

See HANDOFF.md for current resource/failure truth. Correct fixture is now UTF8 PG56390; initial C-locale run was aborted and retained. VM image proof passed and owned VM was removed. All generated browser gates and main/module tests executed PASS. Current full-suite cut-point failure and intermittent MCP cancellation remain under investigation; no source correction or new acceptance yet.
