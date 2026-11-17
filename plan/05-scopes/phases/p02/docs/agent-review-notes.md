# Plan05 p02 persistent independent review notes

## 05SC-P02-D2-R1-N1

- Identity: 2026-11-17; implement-astra-reviewed-2-3; p02 architecture D2 revision1;
  independent B `/root/p05_reviewer`, requested Astra/high, further actual-setting
  telemetry unavailable. Worktree `/Users/mac02/orca/workspaces/openbot/plan03-04`;
  review-base/HEAD `e6eb5bc8fff93492f23935b9c84114905801824c`; digest
  `cc06127eac2c2713ece05d3c97376c9c33a62e128a5e390c6c7feb573f9eb484`.
  Exact snapshot exclusions: `plan/05-scopes/phases/p02/docs` and
  `plan/05-scopes/COORDINATION.md`. [TASK](logs/D2-R1-TASK.md),
  [REVIEW](REVIEW.md), [findings](agent-review-findings.md).
- Coordination: authority `/Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md`;
  [immutable revision7](logs/D2-R1-coordination.md), C001–C004/r1 active/ACK,
  C005/r1 proposed. Before return `cmp` confirmed equality to current coordination
  and snapshot.sh reproduced the exact digest. Original Plan05 README/02 and
  downstream03 examined; accepted p01 review remains closed and unchanged.
- Coverage: D2A bounded pure resolver, per-agent isolation, singleton/ambiguous DM,
  origin overlap and static group; D2B root/child/native producers, company prefix,
  sealed immutable rows, current-owner versus historical-copy invariants; D2C own
  manual root eligibility, two personal owners, static grammar/activation, namespace
  reference identity, frozen preparation and private automatic capture; D2D strict
  service SaveItem, opaque subject, existing managed tenant capability, bounded
  list/delete, expired/disabled data, deletion locking, CLI scope and retry semantics.
  Read current actual admission_write, children/manifest attachment, native MCP
  source_facts/owner/registry, user_scope_owner, memory-tool reference types, service
  lock_tenant/lock_writer/save/delete paths. Trail used before source inspection;
  relevant truncated spans were opened where relied upon.
- Challenged assumptions: child prefix is already acquired by waits::lock →
  lease::lock_lease_company_on before run locks; no speculative child lock inversion
  finding. Native source authorization already obtains company prefix with its
  RowLocks policy; new row reads must preserve NOWAIT, but no specific existing
  lock inversion was found. Service observation IDs use UUIDv7; no unsubstantiated
  random-pagination failure is asserted. Managed subject APIs are an explicit new
  contract, not evidence the old service supported subject enumeration.
- Verification: **INSPECTION ONLY / NOT EXERCISED** for all proposed AC1–AC5
  implementation assertions. No p02 tests, compilation, migrations, HTTP service,
  CLI or Doop work executed. Inspected declared criterion/proof mapping and native
  contention tests. Actual service storage/contract correction retention and purge
  competition must be demonstrated in the implementation round; design acceptance
  alone cannot meet C004. No unavailable checks represented as passing.
- Outcome: **CHANGES REQUIRED**, findings
  [F1](agent-review-findings.md#05sc-p02-d2-r1-f1) and
  [F2](agent-review-findings.md#05sc-p02-d2-r1-f2), both medium architecture defects.
  No optional findings. Other reviewed proposals require no decision change on
  present evidence; this is not a uniform implementation pass or a guarantee of
  test adequacy before real assertions exist.
- Process feedback: cohesive architecture batch is appropriately sized for linked
  snapshot/tool/service decisions. Independent trace of third admission producer
  exposed source-kind information loss; explicit immutable inputs and coordination
  made the review reproducible. Existing skill guidance covers both findings; no
  new scoped instruction is supported yet. Preserve conclusions/evidence rather
  than private reasoning. Parent's same F1 observation acknowledged without
  treating its message as a revised decision.

## 05SC-P02-D2-R2-N1

- Identity: 2026-11-17; implement-astra-reviewed-2-3; p02 architecture D2 revision2;
  independent B `/root/p05_reviewer`, requested Astra/high, further actual-setting
  telemetry unavailable. Worktree `/Users/mac02/orca/workspaces/openbot/plan03-04`;
  review-base/HEAD `e6eb5bc8fff93492f23935b9c84114905801824c`; source digest
  `cc06127eac2c2713ece05d3c97376c9c33a62e128a5e390c6c7feb573f9eb484`.
  Same exact snapshot exclusions as R1. [TASK](logs/D2-R2-TASK.md),
  [REVIEW](REVIEW.md), [dispositions](agent-review-findings.md#d2-revision-2-dispositions--2026-11-17).
- Coordination: authority `/Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md`;
  [retained revision8](logs/D2-R2-coordination.md), active C001–C004/r1 unchanged,
  proposed C005/r2 supersedes proposed r1. `cmp` matched current file; snapshot.sh
  reproduced original digest and git HEAD equaled review base before return.
- Recheck coverage: fresh versus inherited seal validation, historical owner
  rename/deletion, canonical parent/source identity, exact whole-child tuples and
  per-harness native projection, same-actor private-person eligibility, immutable
  unavailable native state, and company SHARE NOWAIT → new-run UPDATE NOWAIT
  append/seal serialization. Read all new revision paragraphs and proof obligations;
  unchanged A/C/D decisions reuse R1 inspection, not a fictitious new full review.
- Additional actual source evidence: Trail-led inspection of
  `workflow/local_agents/preparation.rs:118–158` confirms native harness selection
  is captured from the selected profile at execution/use time or restored from its
  session, not necessarily present in the original manifest. Init schema
  `:13938–13962` binds local sessions to exactly one conversation/workflow source
  and agent revision; `:19386–19440` binds live native calls to the canonical source
  session and original broker run. `native_mcp/mod.rs:164–187` actually writes
  source_session_id from authenticated owner.session. The existing durable source
  chain suffices for the proposed guard without an extra parallel source identity.
- Unavailable-state assessment: acceptable as zero scope authority for a current
  MCP-only broker. No newly captured root/personal/report fallback, later upgrade,
  namespace provisioning, or successful empty scope read is permitted. A missing
  per-harness group can occur legitimately through profile selection; demanding
  failure of unrelated MCP dispatch would be an unnecessary new coupling. The
  source current-authority/lease/ceiling fences remain required. This disposition
  resolves F2 rather than deferring it; future tests must exercise actual consumers.
- Lock assessment: shared company prefix conflicts with production topology
  writers; root keeps its existing stronger prefix. Row and seal writers take the
  same new-run mutex and recheck the seal, while deferred row→header FK prevents
  unsealed commit. NOWAIT preserves bounded native refusal instead of introducing
  a waiting upgrade. Actual concurrent SQL append/seal, native busy-owner rollback,
  and FK/cascade behavior remain implementation proof obligations, not executed facts.
- Verification: **INSPECTION ONLY / NOT EXERCISED** for proposed runtime assertions;
  no tests, migrations, compile, service/CLI execution, source edit, or Doop work.
  No new unavailable check relabeled passing. Source/coordination freshness checks
  executed successfully. Real service subject retention/purge and focused/full
  verification gates remain exactly as declared in AC4/AC5 and C004/C003.
- Outcome: **PASS AT DECISION LEVEL**, F1/F2 resolved as above, no new findings.
  C005/r2 suitable for owner activation. Implementation and batch acceptance still
  pending. Prior r1 CHANGES REQUIRED and original findings remain retained.
- Process feedback: precise source-kind and unavailable-authority disposition
  improved the correction without expanding runtime MCP permissions or duplicating
  provenance schema. No new skill/AGENTS instruction change supported. Correction
  batch was small enough for complete delta review; no review-only cleanup performed.

## 05SC-P02-B1-R1-N1

- Identity: 2026-11-17, implement-astra-reviewed-2-3, p02 whole-batch implementation
  B1/R1; B `/root/p05_reviewer`, requested Astra/high, extra actual-setting/context
  telemetry unavailable. Worktree `/Users/mac02/orca/workspaces/openbot/plan03-04`,
  feature/plan05-scopes; start/review-base/HEAD
  `e6eb5bc8fff93492f23935b9c84114905801824c`; digest
  `4371836f22d139758bc0e6c2de4e4cf124a12f34c3016e251cf989b1acc7321f`.
  All 92 changed paths in scope; no preserved pre-existing source.
  Exact exclusions only `plan/05-scopes/phases/p02/docs` and exact
  `plan/05-scopes/COORDINATION.md`. [TASK](logs/B1-R1-TASK.md), [REVIEW](REVIEW.md),
  [manifest](logs/B1-R1.evidence.json), [accounting](logs/B1-R1-test-accounting.json).
- Authority/freshness: authoritative coordination absolute workspace plan05 path,
  root writer, retained [revision9](logs/B1-R1-coordination.md); ACK active
  C001–C004/r1/C005/r2, no changed notice during review. Start and return snapshots
  reproduce digest; `cmp` matches retained coordination. [Return snapshot](logs/B1-R1-reviewer-final-snapshot.txt).
  D2/r2 was decision-only and p01 remains closed. Read applicable AGENTS and installed
  skill/review-notes/findings/verification contracts; no delegation or source edits.
- Requirements/impact reviewed: bounded resolver and chain/lead facts; root, child
  and canonical workflow/conversation-native producers; available/unavailable seal,
  shape, historical copy, SQL triggers/FK/cascade and prefix/mutex protocol; publication,
  activation, direct-placement mutation and actual agent dependency edges; static
  steps and model memory tools; per-agent/base/person eligibility; namespace lookup,
  exact result/consumption/ref/citation identity and empty-result replay; automatic
  recall global budgets, revalidation, frozen PreparedAgent8 and both SQL readers;
  host preamble before history; local thread/private capture; subject visibility,
  delivery producer/subject/body freeze; opaque service storage/correction retention,
  managed HTTP list/delete ACL/expired-disabled handling and lock ordering; actual
  operator CLI traversal/deadline/partial reporting; generated SQLx/OpenAPI surfaces.
  Trail used before source discovery; exact source ranges opened when needed.
- Coverage depth: production changes and authority/transaction boundaries reviewed
  substantively. Generated metadata/OpenAPI, mechanical fixture producer fields,
  version updates and selector substitutions inspected more lightly as derived or
  repetitive surfaces, with compilation/contract evidence checked. New guard, child,
  native, scope/subject, budget and service/CLI fixtures traced to decisive assertions.
  Existing unrelated test bodies were not reread wholesale. No uniform exhaustive
  correctness claim for unexecuted branches; concrete missing promises are F2/F3.
- Independent execution: root command `cargo test --locked --offline --lib --
  memory_scope_admission_ memory_scope_tools_subject_ run_scopes_resolve_ --skip doop
  --skip Doop` **17 PASS / 0 FAIL**, owned pa05_p02_core_test and per-test isolated
  databases, [log](logs/B1-R1-reviewer-focused.txt). Standalone service command
  `SQLX_OFFLINE=true cargo test --manifest-path services/memory/Cargo.toml --locked
  --offline --lib memory_subject_ -- --skip doop --skip Doop`, DATABASE_URL unset,
  owned pgvector test fixture and external memory-target: **3 PASS / 0 FAIL**,
  [log](logs/B1-R1-reviewer-service-subject.txt). No outstanding reviewer process.
- Evidence inspected: current root affected 207/0/8 opt-in; service178/0/2 opt-in;
  real service+actual CLI7/0; migrations, SQLx prepare, formatting, clippy all-targets,
  source-module reachability, retained failures and corrected passes. Real CLI proof
  uses production frozen delivery with trusted injected run-label fixture; it proves
  wire/storage/list/purge, not separate workflow subject visibility. Its 105 records
  cross a100-row page and two units; principal/unrelated data survive. Service tests
  separately exercise corrected/expired/disabled cleanup and competing real requests.
  Root subject dispatch tests prove directory/foreign/missing visibility and frozen
  producer/subject target; actual thread participant helper proof is explicitly
  narrower than a second end-to-end dispatch.
- Manifest/inventory validation: checker PASS15/retainedFAIL4/NOTEXERCISED1,
  [validator log](logs/B1-R1-reviewer-manifest-validation.txt). These are declared
  retained results, not automatic acceptance. Independently parsed retained runner
  inventories:4768→4799, exactly1 removed/32 added, every removed name has a present
  replacement, [accounting log](logs/B1-R1-reviewer-test-accounting.txt). Read the
  renamed thread capture diff: actual unit namespace target replaces old agent
  target; label/delivery/level assertions remain. Six original failed memory tests
  have exact current passes; seventh has the declared unit-capture replacement.
  Full repository gate remains NOT EXERCISED until03; Doop remains user-deferred.
- Outcome: **CHANGES REQUIRED**. All actionable findings together:
  [F1](agent-review-findings.md#05sc-p02-b1-r1-f1) missing descendant preflight,
  [F2](agent-review-findings.md#05sc-p02-b1-r1-f2) AC2 proof gap,
  [F3](agent-review-findings.md#05sc-p02-b1-r1-f3) AC3 consumer proof gap;
  each medium; no optional suggestions. No direct memory disclosure or proven SQL
  bypass claimed. D2/r1 F1/F2 architectural corrections are present in source and
  historical copy/native projection tests pass, but whole-phase acceptance awaits
  this implementation round's corrections and evidence.
- Process/instruction feedback: phase is large but cohesive and reviewable with
  exact frozen inputs; no extra gate proposed. Names in the acceptance table hid
  narrower test paths than promised (F2/F3); actual dependency-edge trace caught
  F1. Existing skill verification mapping/producer-consumer rules cover all three;
  no new skill or AGENTS change justified now. Retain original architecture PASS
  and this source failure separately; neither implies the other reviewed assertions
  that it did not execute. Shared servers/resources and closed phase01 untouched.


## 05SC-P02-B1-R2-N1

- **Identity:** 2026-11-17; implement-astra-reviewed-2-3; p02/B1 correction recheck;
  independent B `/root/p05_reviewer`, requested Astra/high, further actual-setting
  telemetry unavailable. Worktree `/Users/mac02/orca/workspaces/openbot/plan03-04`,
  branch feature/plan05-scopes; start/review-base/HEAD
  `e6eb5bc8fff93492f23935b9c84114905801824c`; artifact
  `7a555e95283fa22b7face97a1463d8efdfb6cf8c2ab92a3b77e5eae734ab51a5`.
  Exact exclusions: `plan/05-scopes/phases/p02/docs` and
  `plan/05-scopes/COORDINATION.md`. D2/r2 governs without architectural change.
- **Inputs and freshness:** original README/02 criteria, retained R1 findings and
  whole-batch coverage, [R2 TASK/mapping](logs/B1-R2-TASK.md),
  [evidence manifest](logs/B1-R2.evidence.json),
  [test accounting](logs/B1-R2-test-accounting.json). Authority is
  `/Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md`,
  root writer, revision9; C001–C004/r1 and C005/r2 acknowledged. Current coordination
  byte-matches the [pinned input](logs/B1-R2-coordination.md). Start and final source
  snapshots reproduce the same digest and 102 paths.
- **F1 coverage:** inspected `agent_manifest/{mod,capture,capture_tests}.rs`,
  `run_scopes::capture_on`, shared graph-publication fixture and new
  `autonomous_memory_scope_tests.rs`. The existing resolved base context is stored
  before walking; every Workflow node checks `needs_local_unit`, including nodes
  supplied by agent tool dependencies. The old redundant root-only scope-capture
  check is removed; bounded traversal and activation checks remain. Negative test
  uses an actual published unbound tool workflow, then real root admission, and
  asserts the controlled error, unchanged run/job/delivery counts and zero provider
  calls. Positive desk test proves the descendant was selected and the base local
  write sealed. Retained pre-fix red evidence shows the negative previously Created.
- **F2 coverage:** read full new fresh-seal and race tests, module registration and
  native unavailable-consumer assertion. Fresh foreign owner lives in the same DB,
  and the inserted owner row is asserted present before seal refusal; an external
  principal, missing root and unpaired write reach the named 23514 seal guard.
  Valid-looking rows reach COMMIT without a header and fail deferred FK 23503;
  all failures leave zero scope/header authority. Real admission and department
  update queue behind a controlled company lock; waiter chains establish both
  production claimants before release, with joined bounded futures and both orderings.
  Exact actor/base tuples, old-run immutability and later-run change are checked.
  Native test calls actual `eligible_on` on the genuine broker, observes
  `memory.scope_unavailable` and unchanged namespace count, then retains successful
  lease/MCP preparation. This is memory eligibility proof, not model-memory dispatch
  on an MCP-only graph.
- **F3 coverage:** read new selector tests and SearchFixture/Tools changes. LeadDesk
  establishes actual root/local lead roles, direct reports, a local viewer membership
  and application domain; Company retains its prior configuration. Production tool
  preparation/dispatch checks Unit/Reports/Ancestors and their union, exact namespace
  sets and consumption, one query per unique namespace, canonical labels, timeline
  and details. Default/explicit unit save commits only the local unit, personal save
  commits the eligible agent, and explicit unit without a local context commits no
  intent. No authorization or assertions were weakened to fix fixture failures.
- **Executed:** sourced the private p02 environment without printing secrets; ran
  `cargo test --locked --offline --lib -- memory_scope_admission_agent_workflow_tool_own
  memory_scope_admission_fresh_seal memory_scope_admission_valid_looking
  memory_scope_admission_competing_topology
  memory_scope_tools_unit_reports_filter_deduplicate_and_keep_exact_labels_and_references
  memory_scope_tools_default_and_explicit_unit_saves_freeze_only_the_local_target
  memory_scope_tools_explicit_unit_save_without_a_local_unit_creates_no_intent
  native_mcp_workflow_created_origin_obeys_original_knowledge_ceiling_and_parent_run_locks
  memory_scope_user_memory_requires_the_actors_own_manual_session --skip doop --skip Doop`:
  **11 PASS / 0 FAIL / 0 ignored**, [log](logs/B1-R2-reviewer-focused.txt). Used owned
  p02/per-test databases on the borrowed server; no reset or server/container change.
  Independently parsed retained runner inventories: 4768→4799→4808, nine additions
  and no R2 removals; baseline rename and 41 additions match the manifest accounting.
  Revalidated manifest retention: 15 PASS / 8 FAIL / 1 NOT EXERCISED,
  [validator log](logs/B1-R2-reviewer-manifest-validation.txt).
- **Inspected/reused:** current author format, SQLx/all-target compilation, clippy,
  source_modules (8), clean correction (10) and Company regression evidence. Earlier
  broad 273 PASS / 3 FAIL / 1 ignored invocation remains FAIL; all three failed names
  pass now. The shared fixture refactor preserves its original Company branch and
  the independent own-session case exercises it. Unchanged R1 whole-source review,
  root 207-case pre-review log, reviewer 17 root cases, actual service subject 3,
  service suite 178/2 ignored and real service/CLI 7/0 remain reusable for unchanged
  contracts. The scope of those proofs, including the trusted live-label fixture,
  remains as documented in R1. Service/CLI/schema were not re-reviewed or rerun this
  round. Generated metadata and unchanged mechanical migrations retain R1's lighter
  inspection; this correction does not extend those claims. Current Trail lexical
  conflicts retain their prior unrelated disposition, not a clean Trail-check claim.
- **Outcome:** **PASS** for the exact digest above; F1/F2/F3 resolved together, no new
  actionable or optional findings. See [dispositions](agent-review-findings.md#b1-revision-2-resolution-history).
  Phase03 integration and the full repository gate remain outstanding; no Doop work,
  publication, source edits or phase03 work occurred. No reviewer process remains.
- **Process feedback:** one cohesive correction round was appropriate. The fresh
  foreign-row count assertion exposed an invalid cross-database fixture instead of
  producing false security proof; retaining the failed invocation and exact current
  passing replacement was useful. The missed dependency edge was an implementation
  omission under the already accepted D2C contract, not a design-gate failure.
  Existing edge-tracing and criterion-to-production-assertion instructions suffice;
  no new global rule is supported. The new Trail learning records the concrete F1
  trap. Preserve the original R1 outcome and failed evidence when accepting R2.
