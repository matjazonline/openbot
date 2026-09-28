# 02.4 — frozen publication dependency bundles

Original: phase02 Publication and binding item2 + immutable bundle/source diagnostic
acceptance. Expanded in EXPANSION.md; implementation is application-owned pure library
construction. No production publish command/adapter/resource activation is claimed.

## Decisions and changed files

- `src/application/workflow/publication/{mod,types,validation,dependencies}.rs`: private
  immutable checked bundle with compiler artifact, captured dependencies, child Arcs,
  manifest and SHA-256 identity. Compiler facts derive from captured snapshots.
- Finite catalogue supports parameter-selected agents; publication input hook rejects
  unresolved catalogue members at activation. Runtime authorization remains separate.
- Agent instructions/model settings/ordered tools/skills, skill requirements, profiles,
  tool schemas and trusted approved capability/effect/recovery policy are hashed.
- Resource declarations retain existing kind/contract. MCP facts reference declared
  MCP slots. Binding readiness is02.5; secrets never have a snapshot field.
- Child schema/closure derive from actual immutable child bundles; reject foreign,
  wrong/missing/duplicate pins, conflicting version content, recursion and excessive
  closure/depth. Different versions of one workflow may share a closure if acyclic.
- Domain workflow identifier macros gain transparent Serialize only; no unchecked
  deserialization. Existing template/revision additions are preserved. Registry facts
  gain Clone/Serialize for tool/profile snapshots, without behavior changes.
- `publication/tests.rs` covers deterministic hashing/mutation isolation, defaults,
  incomplete/foreign/duplicate/oversized dependencies, skills without granting tools,
  dynamic agent membership, MCP policy/schema/slot identity, child pin/closure/depth
  and pre-clone schema bounds. No concurrency protocol is introduced in this point.
- `docs/workflow-language-v1.md` distinguishes implemented builder from pending
  production persistence/lifecycle/runtime wiring.

## Verification and review

Self-audit complete: checked immutable ownership, derived compiler facts, bounded borrowed preflight, child identity/closure, current-authorization separation, existing caller preservation and function/module size. Initial full workflow suite129/129 at2MiB passed; final all-target check passed. Clippy style-only correction is being rechecked, followed by independent review. Baseline tracked diff is
`/private/tmp/workflow-02-4-start.diff`; accepted02.1–02.3 new files also existed then.
This point adds publication files and only touches module export, identifier Serialize,
registry ToolContract/CapabilityProfile derives, docs and progress/expansion records.
No accepted compiler/registry behavior should be reimplemented or discarded.

## Context

Main `01a0e429-0099-7a31-b5f0-a9c5b7e9eb47`, verified gpt-6-astra/medium from session
turn_context. Startup23,781/258,400=9.20% at2026-09-27T18:39:43.984Z;
post-expansion79,416/258,400=30.73% at18:42:36.747Z;
implementation milestone93,492/258,400=36.18% at18:46:39.416Z.
Measurement source token_usage_record.usage, capacity task_started.model_context_window.
No resources requiring cleanup; temporary logs retained as evidence. No commits,
deployments or database changes.


## Review1 and correction handoff

Reviewer `/root/review_02_4`, verified UUID01a0e43b-1d4b-7e03-bd28-bc9e81f879f4,
Astra/medium. Review1 NOT PASS: inline profiles bypassed frozen selection checks;
dependency errors lacked identifiers/reasons/source fields. Both corrections written
in validation/mod/dependencies and tests, with shared static/runtime selection logic.
First correction compile failed on inferred closure lifetime; explicit closure
parameter types added. Final correction tests pending outcome; all-target check,
Clippy, formatting/whitespace, graph refresh and correction review remain due.
No reviewed passing gate applies to the changed correction behavior yet.
Reviewer post-turn depth028.11%,72,638/258,400 at2026-09-27T19:00:35.713Z,
token_count.info.last_token_usage/model_context_window. Eligible if handle usable and
fresh measurement suitable; no correction-review assignment yet.

Main reached50.15%,129,598/258,400 at2026-09-27T19:05:10.226Z
(token_usage_record.usage/task_started.model_context_window). Stopped implementation
and saved RESUME.md. Point remains implemented-but-unverified;02.5 must not advance.

## Final correction test result

`SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow`
finished **FAIL:131 passed,1 failed**, exit101. Log:
`/private/tmp/workflow-02-4-review1-tests-final.log`. Failure:
`workflow_publication_inline_profiles_check_static_and_runtime_dependencies`,
`publication/tests.rs:398`, final positive constructed-profile assertion
`build(&source, facts).is_ok()`. No further diagnosis/fix was performed after the
main50% threshold. The preceding static/dynamic rejection assertions passed.
Start by exposing the diagnostic for that positive case and checking the fixture's
binding syntax against the existing parser; do not assume production logic is at fault.
No test/build processes remain running. Correction Clippy/check/fmt-check/diff-check,
graph refresh and independent rereview are still outstanding.

## Fresh-session correction audit (2026-09-27)

Diagnostic probe confirmed the remaining failure was fixture syntax (`syntax.unknown_field`
at `/steps/start/with/capability_profile/skills`): object construction lacked its
`object` operator. Fixed the positive fixture and retained `unwrap()` for diagnostics.
No production behavior changed in this fresh session. Reconciled correction code:
shared static/runtime selection checks, frozen catalogue ambiguity and skill requirements,
child ownership/pins/conflicts, bounded snapshot handling and specific diagnostic paths.
Existing accepted compiler/registry/templates remain preserved.

`SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow`:132/132 PASS.
`SQLX_OFFLINE=true cargo check --locked --offline --all-targets`:PASS.
`cargo fmt --all -- --check`, `git diff --check`:PASS; `graft build`:PASS.
Logs `/private/tmp/workflow-02-4-resume-{diagnostic,tests,check,fmt,diff,graft}.log`.
Clippy and fresh independent correction review remain pending. Previous reviewer handle
is unavailable in this fresh session. Original Review1 concerns remain subject to review.

## Fresh independent Review2 and correction

Reviewer `/root/review_02_4_resume`, verified session
`01a0e448-7a4f-77d1-9a4d-2bfe7acbaeb7`, Astra/medium, NOT PASS:
remaining P2 generic preflight/registry errors for duplicate tool/profile contracts,
invalid model settings and repeated selections. Other publication and prior profile
corrections accepted. Post-review depth0 54,408/258,400=21.06% at
2026-09-27T19:13:47.400Z, token_count.info.last_token_usage/model_context_window.

Correction extracts bounded snapshot preflight to `publication/preflight.rs`, names
invalid model fields and duplicated dependency/list members before registry compilation,
and shares selector-path derivation with post-compile diagnostics. New child test module
`diagnostic_tests.rs` covers tool/profile duplicates, empty model, zero token budget,
and repeated agent/profile selections, including source locations. No changed compiler
or registry behavior. Self-audit: borrowed schema bounds retained before cloning; limits
unchanged; native and distinct MCP-slot tool identities remain distinct. Checks pending
in `/private/tmp/workflow-02-4-review2-*.log`; same reviewer reserved for correction review.

Review2 correction final checks:135/135 workflow tests at2MiB PASS
(`review2-tests-verified.log`); all-target offline check PASS (`review2-check-final.log`),
final all-target Clippy PASS (`review2-clippy-verified.log`), fmt/diff/graft PASS.
Prefix for all logs `/private/tmp/workflow-02-4-`. Initial new test failures were
span assertions only: parser container spans mark an opening token; final assertions
verify the exact expected binding begins at the reported offset. Production correction
unchanged across those runs. Same reviewer correction review is next.

Final correction review: **PASS;02.4 VERIFIED**. Reviewer verified UUID above, final23.55% at19:21:23.709Z. All requested diagnostics and original point criteria accepted; no outstanding point checks.
