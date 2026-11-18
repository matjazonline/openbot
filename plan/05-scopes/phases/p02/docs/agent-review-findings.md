# Plan05 p02 review findings

Common context: 2026-11-17, implement-astra-reviewed-2-3, phase p02 architecture
D2/r1, reviewer B `/root/p05_reviewer`, requested Astra/high; actual setting
telemetry unavailable. Worktree `/Users/mac02/orca/workspaces/openbot/plan03-04`.
Review base/HEAD `e6eb5bc8fff93492f23935b9c84114905801824c`; source digest
`cc06127eac2c2713ece05d3c97376c9c33a62e128a5e390c6c7feb573f9eb484`.
Decision: [retained TASK](logs/D2-R1-TASK.md). Review:
[05SC-P02-D2-R1-N1](agent-review-notes.md#05sc-p02-d2-r1-n1).
Both findings originate in the proposed architecture; p02 source is not implemented.

## 05SC-P02-D2-R1-F1

- Classification: actionable, **medium**, architecture/correctness.
- Evidence: D2B requires all row INSERT guards to prove current owner tenant/type,
  and header insertion to validate owner kinds. The same section requires children
  to copy all sealed parent rows without consulting live topology, retaining plain
  historical owner IDs even when a readable owner is deleted. These promises
  conflict: admit a parent with a nonessential readable report unit, delete that
  unit, then create a child. Its exact inherited row fails the fresh-owner guard.
  If frozen slug equality is checked against the current owner, a mere rename also
  rejects a valid historical copy. Existing child creation shares the root manifest
  (`children.rs:328–377`, `agent_manifest/storage.rs:41–59`); the original 02A2
  requirement says later tree changes affect later admissions, not frozen runs.
- Required correction: explicitly define how fresh validated rows differ from
  trusted inheritance, including seal validation. Child copies must remain exact
  subsets/copies of their actual sealed parent authority with historical owner IDs,
  labels and flags; deleted unrelated readable owners must not prevent copying.
  Do not weaken fresh tenant/type validation or runtime current-owner refusal.
  Add real child creation after readable-owner rename/deletion and SQL forged-copy,
  foreign-parent, changed-tuple, missing-seal refusal tests to the proof contract.
  A owns the precise persistence implementation.
- Instruction lesson: immutable retained owner IDs need a separate provenance rule
  when copied after owner retirement; current identity checks and historical copy
  checks have different inputs. No additional repository instruction is justified
  from this single design defect; existing lifecycle/producer tracing applies.
- Resolution history: **OPEN** at D2/r1. Parent independently identified the same
  conflict while review was underway; its proposed clarification is not accepted
  until retained in and reviewed against the revised decision.

## 05SC-P02-D2-R1-F2

- Classification: actionable, **medium**, architecture/frozen authorization.
- Evidence: D2B describes native brokers creating scopes from authenticated harness
  agent and association, without distinguishing the source of that association.
  `native_mcp/mod.rs:37–70` has two actual origins: Conversation and Workflow. The
  latter reads association and knowledge ceiling from the existing workflow run.
  `local_agents/protocol/mcp.rs:75–118` obtains these facts from the authenticated
  session, but `NativeMcpOwner` (`native_mcp/mod.rs:72–84`) retains no source run ID;
  broker admission has no ordinary manifest or parent_run_id (`:91–162`). Applying
  the proposed fresh resolver to a workflow-origin broker after adding a lead role
  or moving units creates a different/broader sealed scope set for an operation
  launched by a run whose topology was frozen earlier. D2AC2 and original 02 outcome
  require one frozen admission authority serving tools/steps/knowledge.
- Impact limits: current native broker compilation accepts MCP-backed operations
  (`application/workflow/native_mcp/tests.rs:85–120`); this review does **not** claim
  an existing directly reachable memory disclosure through that broker. The defect
  is the proposed third producer writing authority inconsistent with the source run
  and leaving downstream consumers without a safe, explicit inheritance contract.
- Required correction: preserve trusted source kind/identity into snapshot creation.
  Define workflow-origin scopes as inherited from the source run's sealed context
  for the authenticated harness agent (including explicit handling of the base
  group and any genuinely proven private-person eligibility), with no live-tree
  recomputation or union of other actors' reports. Missing source seal/group must
  fail closed. Distinguish genuinely fresh conversation-origin captures and avoid
  granting private scope based on synthetic Manual alone. Add workflow-origin
  broker creation after topology/lead mutation, wrong/missing source group, and
  direct-conversation source proofs. Preserve existing native NOWAIT boundaries.
- Instruction lesson: enumerate source variants of a third admission producer,
  not merely its final association shape. Existing actual-producer and authority
  tracing guidance applies; no new global instruction is proposed.
- Resolution history: **OPEN** at D2/r1; sent to A before implementation.

## D2 revision 2 dispositions — 2026-11-17

Reviewed [D2/r2](logs/D2-R2-TASK.md), note
[05SC-P02-D2-R2-N1](agent-review-notes.md#05sc-p02-d2-r2-n1), unchanged source digest
`cc06127eac2c2713ece05d3c97376c9c33a62e128a5e390c6c7feb573f9eb484`.

- **05SC-P02-D2-R1-F1: RESOLVED AT DECISION LEVEL.** Final validation moves to
  the seal, with fresh tenant/type validation and exact inherited tuples as
  distinct rules. Ordinary child provenance derives only from its actual parent;
  native projection derives only from its actual source session. Historical owner
  absence/rename no longer invalidates legitimate copying; runtime missing owners
  still refuse. Planned proofs cover actual rename/delete children, altered tuples,
  foreign/wrong/missing source, and extra-row refusal. Implementation remains pending.
- **05SC-P02-D2-R1-F2: RESOLVED AT DECISION LEVEL.** Canonical SessionAssociation
  persists into producer logic; the seal follows existing call/session provenance.
  Workflow-origin context is the frozen source base plus exact harness group,
  with same-actor private-person eligibility independently proven. Missing context
  yields sealed-unavailable and zero rows, immutable and unusable by every scope
  consumer/provisioner. This is an acceptable fail-closed scope disposition even
  though a separately authorized MCP-only action still runs. The r1 finding's
  requested failure concerns authority, and does not require breaking unrelated
  MCP execution. Source preparation selects a profile at execution time, so a
  legitimate harness can be absent from the root dependency manifest; this case
  makes unavailable explicit instead of inventing live memory authority. Tests
  must prove the distinction and cannot substitute an empty successful read.
  Conversation-origin fresh capture remains distinct. Implementation remains pending.

No new actionable or optional finding from the revised seal locking design.

## B1 revision 1 common context

2026-11-17; implement-astra-reviewed-2-3; phase p02 whole-batch implementation review;
independent B `/root/p05_reviewer`, requested Astra/high (further setting telemetry
unavailable). Worktree `/Users/mac02/orca/workspaces/openbot/plan03-04`, branch
feature/plan05-scopes; start/review-base/HEAD
`e6eb5bc8fff93492f23935b9c84114905801824c`, digest
`4371836f22d139758bc0e6c2de4e4cf124a12f34c3016e251cf989b1acc7321f`.
[Immutable TASK](logs/B1-R1-TASK.md), [round note](agent-review-notes.md#05sc-p02-b1-r1-n1),
[REVIEW](REVIEW.md), coordination9/C001–C004r1/C005r2. Findings below concern the
implementation/proof, not a reopening of the unchanged D2/r2 architecture.

## 05SC-P02-B1-R1-F1

- Classification: actionable, **medium**, requirements/correctness.
- Evidence: `src/application/workflow/memory.rs:27–51` checks a bundle and its
  `children()` only. `src/adapters/persistence/workflow/run_scopes.rs:51–55` calls
  that helper only on the root binding bundle. However the admission dependency
  closure also walks agent tool bindings: `src/application/agent/dependency.rs:135–156`
  emits a Workflow node for every reachable AgentBindingSource::Workflow, and
  `src/adapters/persistence/workflow/agent_manifest/capture.rs:4–46` reads each such
  bundle and checks resources/roles. It never checks that bundle's memory `own`
  requirement. `agent_manifest/roles.rs:153–161` returns immediately when it has no
  role references. Exhaustive `needs_local_unit` references are only its definition,
  root run_scopes capture and two role_binding checks; none covers these closure nodes.
- Trigger/impact: configure/admit a company-wide root whose agent has a workflow tool
  containing `memory.load` or `memory.save` with `{"unit":"own"}`, with no ordinary
  pinned child carrying that requirement. The root passes preflight and may call the
  model/perform prior effects. When the workflow tool executes, its child inherits a
  base snapshot with no own unit and fails at the memory step. D2C explicitly requires
  descendant requirements to be checked during admission dependency traversal before
  effects; the new helper's comment claims every pinned child but does not cover
  workflow descendants reached through selected agents.
- Required correction: enforce the existing local-context requirement for every
  reachable workflow in the actual bounded admission closure, including agent tools
  and nested descendants, using the admitted base context. Keep local-unit refusal
  before committing root admission/jobs/effects. Add a real root-admission test with
  an agent workflow-tool descendant requiring own and no local context, asserting
  controlled refusal and no run/job/provider effects; include a valid desk-context
  positive path and retained plain child behavior. Do not broaden scopes as a fix.
- Verification: source/caller inspection establishes the missing preflight; the new
  negative scenario has not been executed in this round. Existing 17 focused tests
  pass because none traverses this case.
- Instruction lesson: enumerate dependency *edges* beyond PublishedBundle.children
  when promising all descendants. Existing skill producer/consumer tracing guidance
  already applies; no new global instruction is proposed.
- Resolution history: **OPEN**, sent to A during review; source remains frozen.

## 05SC-P02-B1-R1-F2

- Classification: actionable, **medium**, verification/authorization and concurrency.
- Evidence: D2B and its verification contract promise actual SQL foreign-owner,
  shape, incomplete-snapshot and competing tree/roster/lead admission proof.
  `run_scope_admission_tests.rs:372–406`, despite its broad name, attempts updates,
  deletes and a late insert only against an *already sealed* run. Its foreign/missing
  cases call `require_available_on` using unrelated company/run IDs; they do not
  exercise fresh owner/seal validation. The new append/seal test (`:409–464`) covers
  the per-run mutex, not admission competing with topology/lead changes.
  Frozen-label tests (`:301–369`) and native placement tests
  (`run_scope_native_tests.rs:320–386`) mutate topology sequentially. Existing
  direct-role concurrency proof asserts role manifest/placement, not the newly
  produced memory scope rows. Exhaustive snapshot/row reference inventories are
  retained in logs/B1-R1-reviewer-{snapshot,scope}-references.txt.
- Impact: TASK's AC2 PASS mapping overstates which new authority boundaries were
  asserted. Inspection can find the guards, but neither passing row-reader calls nor
  p01 memory_namespaces shape tests prove workflow_run_scopes fresh-seal refusal or
  the deferred row→header commit constraint. No actual SQL bypass is claimed here.
- Required correction: add focused real SQL checks that reach the fresh seal with
  a foreign/ineligible owner, invalid read/write pairing and a missing required root;
  prove that valid-looking rows cannot commit without a seal (exercise the actual
  deferred FK), with no partial authority retained. Add a controlled real admission
  versus topology/lead-or-roster mutation race in both commit orders and assert exact
  coherent per-actor/base scope sets, old-run immutability and next-run change.
  Retain the existing append/seal and historical child tests. Update criterion to
  actual assertion mappings and record these checks without substituting test names
  or sibling namespace-table constraints. Source-state unavailable consumers should
  also have the promised direct scope_unavailable/no-provisioning assertion while
  existing separately authorized MCP execution remains positive.
- Instruction lesson: fixture paths and decisive assertions must substantiate each
  claimed criterion. Existing verification-contracts.md already states this rule;
  no new instruction needed. This is a proof gap, not a demand for a full-suite rerun.
- Resolution history: **OPEN**, sent to A during review.

## 05SC-P02-B1-R1-F3

- Classification: actionable, **medium**, verification/tool contract.
- Evidence: original 02B/Focused checks and D2C require production tool dispatch
  across Personal/Unit/Ancestors/Reports, overlapping unit dedup, labels and strict
  local writes. `memory_tool_tests.rs` changes current production success cases to
  personal and ancestors. New `memory_scope_proof_tests.rs` verifies namespace
  authority, subject, host context and recall budgets, but does not dispatch unit
  or reports. Exhaustive Trail searches for literal `"reports"`,
  MemoryToolScope::Reports and MemoryToolScope::Unit find no such test consumer;
  [report reference inventory](logs/B1-R1-reviewer-report-selector-references.txt)
  and [unit reference inventory](logs/B1-R1-reviewer-unit-selector-references.txt)
  are retained. The overlapping-chain/report case is only a pure resolver test.
  A valid local write through memory.save *step* and thread automatic capture do
  not prove MemoryToolRuntime's separate target selection/save path.
- Impact: the core new selectors and local default tool write could regress while
  all submitted production tool tests remain green. No incorrect selector outcome
  is asserted from inspection; this is the explicit AC3 behavioral-proof gap.
- Required correction: use a real admitted desk/lead agent through the production
  tool dispatch with distinct local, ancestor and report namespaces (including two
  readable units sharing a category). Assert selector filtering, exact namespace
  references/labels, and one service query for a unit qualifying by overlapping
  relations. Exercise timeline with the new selectors and details roundtrip as
  appropriate. Assert default/explicit unit save freezes the local unit, explicit
  personal saves to the eligible personal target, and a no-local-unit explicit unit
  save refuses without an intent. ContractFake is appropriate for these host policy
  assertions; no extra broad real-service suite is required by this finding.
- Instruction lesson: adapting old fixture vocabulary is not proof of newly added
  enum branches. Existing criterion-to-production-assertion guidance applies.
- Resolution history: **OPEN**, sent to A during review.


## B1 revision 2 resolution history

2026-11-17; independent B `/root/p05_reviewer`; note
[05SC-P02-B1-R2-N1](agent-review-notes.md#05sc-p02-b1-r2-n1); artifact
`7a555e95283fa22b7face97a1463d8efdfb6cf8c2ab92a3b77e5eae734ab51a5`, unchanged
start/review-base/HEAD `e6eb5bc8fff93492f23935b9c84114905801824c`;
coordination9, C001–C004/r1 and C005/r2. Original findings above are preserved.

- **05SC-P02-B1-R1-F1 — RESOLVED.** `agent_manifest/mod.rs:79` stores the resolved
  base context before bounded traversal; `agent_manifest/capture.rs:34–40` checks
  every reached workflow, including agent workflow-tool descendants. The root-only
  check is removed from scope capture. Actual regression tests
  `autonomous_memory_scope_tests.rs:52–139` prove no-context admission refuses before
  new run/job/delivery/provider effects and real desk context admits with the tool
  descendant selected and local write sealed. The retained pre-fix red result and
  current independently executed pair establish the correction.
- **05SC-P02-B1-R1-F2 — RESOLVED.** `run_scope_seal_tests.rs:74–157` now reaches fresh
  foreign/ineligible-owner, missing-root/unpaired-write guard rejection and the actual
  deferred header FK at commit, with zero retained authority. Same-database foreign
  creation and inserted-row assertions prevent false negatives. The production
  admission/reparent race in `run_scope_race_tests.rs:59–155` controls both queue
  orders and asserts exact scope tuples plus historical/future behavior. Actual
  native `eligible_on` refusal in `local_agent_mcp_tests.rs:220–264` leaves namespaces
  unchanged and the following lease/MCP path stays positive. All cases passed in the
  independent correction invocation. Existing append/seal/child proofs remain intact.
- **05SC-P02-B1-R1-F3 — RESOLVED.** `memory_scope_selector_tests.rs:117–210` runs actual
  prepared memory tools on a real desk/lead fixture: Unit, Reports, Ancestors and
  their union; exact namespace/label/consumption sets; overlap dedup and exact query
  counts; timeline/details; default and explicit unit saves; personal save; no-local
  refusal with zero intents. All new cases and the unchanged Company own-session
  case passed independently. ContractFake proves host policy as required; unchanged
  real-service contract evidence is reused and no new service claim is inferred.

Independent [execution log](logs/B1-R2-reviewer-focused.txt): 11 PASS, 0 FAIL,
0 ignored. All original findings are closed for this artifact, with no new findings.
This resolution does not waive phase03's required full source verification or the
explicit user deferral of Doop.
