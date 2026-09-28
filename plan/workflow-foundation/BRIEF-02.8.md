# 02.8 — Representative fixtures

Original phase02 publication item6 and acceptance criteria are authoritative. Reconciled with
EXPANSION.md and accepted02.1–02.7 on2026-09-27. Add reusable checked-in YAML sources and
illustrative frozen dependency snapshots for reviewed support, autonomous saved defaults,
classification/routing, bounded human revision (parent and child), and explicit MCP typed calls.
Compile/freeze every source; exercise typed activation/output validation and invalid mutations.
Use the same sources in later handler/runtime tests rather than introducing another execution path.

The existing repeat registration has pinned child input, max_iterations, result and rounds only.
Fixtures demonstrate that contract and human review data/feedback, with parent accept/escalate
routing. Next-round mapping, exit/exhaustion scheduling and exact accepted-artifact delivery are
phase07 requirements, not executable behavior in phase02. Production SQL/CAS/admission races
remain03.1; use-time secrets/revocation remain04. No service or external effects are added.

Affected: new application/workflow/fixtures module and YAML files; module export; language docs.
Acceptance: all fixtures compile/freeze; autonomous has neither classifier nor capability_profile,
with nonempty saved defaults captured; human edits feed delivery and rejection ends; triage uses
ordered cases/default; repeat is bounded and pins a real child; MCP arguments preserve integer,
boolean, array/object/null and structured results need an explicit missing-value fallback.
Negative tests cover unavailable references, invalid typed arguments/routes, missing repeat bound,
recursive dependency, invalid schema and source-located diagnostics. Reuse accepted phase02 tests
for immutable publication, null/missing distinction and unauthorized binding checks; run combined
workflow suite at2MiB plus locked offline all-target check/Clippy, fmt/diff and graph refresh.
Independent nested Astra/medium review includes combined phase02 integration; stop before03.1.

Baseline of accepted uncommitted workflow tree and language docs:
/private/tmp/workflow-02-8-baseline. Root owns PROGRESS.md. No commit/reset/deploy authorized.
Implementer /root/implement_fixtures UUID01a0e485-35ec-70d2-8f7f-d43bc6585147,
startup28577/258400=11.06% at2026-09-27T20:19:08.672Z,
token_usage_record.usage / task_started.model_context_window. Checks/review pending.

## First independent review and correction

Nested reviewer /root/implement_fixtures/review_02_8, UUID01a0e48c-0d86-7f50-abad-edf6d6826d85,
verified Astra/medium; no edits/delegation. Initial review found P1: schema projection discarded
sibling required/properties when allOf existed, making required human-review data unavailable.
Two P2 test harness assumptions: autonomous supplied invalid review params; wrong type diagnostic.
Focused first run4pass4fail confirmed these. Corrected schema::project/merge_all to retain
sibling constraints, leaving direct reviewed-data delivery intact. Added targeted projection
regressions (required/nested, optional, impossible, scalar traversal and branch constraints),
corrected parameter/diagnostic expectations. Preserved human feedback runtime validation.
Combined suite/static gates and correction review pending.

Implementer intermediate89029/258400=34.45% at2026-09-27T20:28:45.398Z,
usage/task_started sources. Reviewer initial end68884/258400=26.66% at20:27:35.126Z;
parent verified69272=26.81% at20:27:51.712Z (token_count.info fields). No SQL/service changes.

## Independent correction review / phase02 integration

Same reviewer inspected actual corrected code and combined library integration: PASS, no further
findings. Reviewer final75723/258400=29.30% at2026-09-27T20:30:07.432Z; parent confirmed
76706=29.68% at20:30:27.911Z (token_count.info.last_token_usage/model_context_window).
One review correction round. First corrected suite160pass/1fail compiled before the final
step.control expectation edit; rerun current tree required. No unreviewed runtime implementation.

Self-audit: all six sources pass real decoder/compiler/freeze paths; static schema projection
correction retains conservative impossible/scalar checks and runtime validators. No handwritten
executor, fake grants, secret loading, external request, production persistence or concurrency
protocol introduced. All touched functions remain below80 lines; test modules below500.
Prior uncommitted01/02 work preserved. No task databases/services or cleanup obligations.

Acceptance mapping for phase02: fixtures + schema_projection_tests add10 tests; prior accepted
compiler/registry/publication/binding/lifecycle suites retain malformed graphs/types/routes,
source spans, typed null/missing, immutable hashes/content, and unauthorized resource checks.
Publication/admission transaction integration and real database race tests remain03.1; repeated
review execution remains07. No database gate is claimed by this library-only checkpoint.

Exact final commands/logs:
- SQLX_OFFLINE=true RUST_MIN_STACK=2097152 cargo test --locked --offline --lib workflow
  — /private/tmp/workflow-02-8-tests-final.log
- SQLX_OFFLINE=true cargo check --locked --offline --all-targets
  — /private/tmp/workflow-02-8-check-final.log
- SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
  — /private/tmp/workflow-02-8-clippy-final.log
- cargo fmt --all -- --check; git diff --check; graft build — PASS,
  /private/tmp/workflow-02-8-{fmt,diff,graft}.log.
First three final outcomes recorded below when completed. Graft discovery estimated2,575,919
whole-file-equivalent tokens saved; not measured runtime savings. Implementer audit97557/258400
=37.75% at2026-09-27T20:30:49.239Z (usage/task_started sources).

## Additional integration corrections

The unmasked MCP invalid-argument mutation exposed another pre-existing gap: constructed binding
checks skipped allOf branches (runtime validation still rejected mismatches). shape.rs now checks
conjunctive branches, with regressions for wrong literal/concat type, missing required argument,
and forbidden nested field. Final2 suite162 tests at2MiB passed, offlinecheck/Clippy passed.
Reviewer found a new combinatorial defaults×allOf traversal risk in that correction. Fixed by
peeling default fallbacks once before schema traversal;12 nested defaults against12 nested allOf
schemas now test both valid and invalid fallback through real MCP publication. No bound raised.
Same independent reviewer correction PASS, no remaining findings; final89735/258400=34.73%
at2026-09-27T20:34:21.171Z (usage/task_started sources), parent verified90197=34.91% at
20:34:27.570Z (token_count.info sources). Three correction review rounds total; all findings closed.
Final3 checks now supersede previous final/final2 checks because of these source edits.

Final changed files beyond the initial fixture scope: compiler/schema.rs,
compiler/schema_projection_tests.rs and compiler/shape.rs. Both compiler corrections are required
to make representative typed review/MCP fixtures valid and reject statically invalid arguments.
Fixture/schema additions total12 tests (163 combined). No production SQL/concurrency changed.
Implementer111585/258400=43.18% at2026-09-27T20:34:37.045Z (usage/task_started sources).
Final graft discovery estimate2,599,628 tokens across calls; no runtime/cost savings claim.

## Final checkpoint — ready for root acceptance

Final current-tree gates all PASS:
-163 workflow tests at stock2MiB: /private/tmp/workflow-02-8-tests-final3.log
-locked offline all-target check: /private/tmp/workflow-02-8-check-final3.log
-locked offline all-target Clippy -D warnings: /private/tmp/workflow-02-8-clippy-final3.log
-fmt/diff/graft: /private/tmp/workflow-02-8-{fmt,diff,graft}-final3.log
Use the exact commands above with final3 log names. Final2/final logs are superseded.
Independent correction code/combined phase02 library review PASS; all concrete findings closed.
No required library gates outstanding. Production transactions, handler execution and later-phase
acceptance remain explicitly outstanding. Root owns acceptance and PROGRESS.md.

Final implementer115123/258400=44.55% at2026-09-27T20:36:16.721Z,
token_usage_record.usage/task_started.model_context_window. Natural stop before03.1; no50%
threshold claim. Reviewer completed, no live task processes/databases/services. Existing code
and all fixture/compiler corrections remain uncommitted; no reset/deploy/commit performed.
