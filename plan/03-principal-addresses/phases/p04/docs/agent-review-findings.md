# P04 independent reviewer findings

## 03PA-P04-B1-R1/F1 — Exercise the native role-origin continuation boundary

- **Context:** 2026-11-15, `implement-astra-reviewed-2-3`, whole-integration review [03PA-P04-B1-R1/N1](agent-review-notes.md); requested reviewer Astra/high, actual runtime settings unexposed. Current original worktree; base `3fceb7d3435f3b821c3b37820200e1c5400206d9`, HEAD `e5a13246f054325ac97ad5c2355de3b927ac1002`, artifact `42490f46b0a9225c43501d04fef6db942d506a876769c43b3865823754530ed1`.
- **Classification:** actionable **medium**, verification; FC2 and accepted D1/r1 criterion-to-assertion proof. Architecture decision itself remains accepted.
- **Evidence:** `src/adapters/persistence/workflow/role_model_runtime_tests.rs:95–192` creates and completes a role-selected hosted run, then directly calls `agent_manifest::execution_selection_with_locks_on` with `RowLocks::NoWait` (`:156`, `:171`). It expects the internal `AppError::DatabaseTimeout`, and its zero-dispatch assertion follows a helper that never attempts native admission or continuation. The test does prove the selector's exact captured identity and its three lock edges. It does **not** execute the integrated chain through native pre-body/admission/live authority, the changed outer account prefix, or public busy classification/whole-transaction rollback. The existing `local_agent_mcp_tests.rs:75–106,149–189` fixture starts a plain workflow `local_agent.run` with company scope, not a role-selected agent-origin action. Exhaustive selector consumer inspection found no second native role fixture; A confirmed no known retained end-to-end case.
- **Required correction:** add a production-path fixture where an actual role-selected agent produces/owns the parked local-agent operation and native MCP continues through the real entry/live service boundary. With competing transactions holding the sealed ownership/role/revision rows, assert bounded `ServiceUnavailable`, no new call/run/dispatch state, and successful retry selecting the same captured agent/revision after release. Include the responsible actor/requester account contention case for the newly shared pre-company prefix, as required by the R1 implementation obligation. Preserve the helper-level test as useful narrower coverage or replace it with demonstrably equivalent coverage. Update the mapping to name the real exercised branch and assertions.
- **Impact:** green selector tests would survive an integration mistake that drops `NoWait`, acquires a late account, or mishandles busy errors at the outer native boundary. D1 explicitly promises actual role-selector continuation; source inspection alone does not fulfill that behavioral obligation.
- **Instruction lesson:** existing verification-contract guidance already says to trace fixture → production branch → decisive assertion and name what stand-ins leave unproved. Apply it to this test; no new skill rule is required.
- **Resolution:** OPEN. No reviewer runtime check performed; this finding follows actual fixture/caller inspection and the accepted criterion.

## 03PA-P04-B1-R1/F2 — Remove the automatic-desk promise from agent creation help

- **Context:** same 2026-11-15 review, identities, worktree and note as F1.
- **Classification:** actionable **low**, user-facing correctness/documentation; original Plan03 README decisions 6–7 and FC1/FC2 integration consistency.
- **Evidence:** `src/adapters/http/pages/agent_settings.rs:872–875` renders the creation-only model help as “Leave both blank to use the company's default model for the new agent's desk.” The integrated creation transaction provisions a principal direct binding and creates no desk; the six updated documents correctly describe that behavior. This newly integrated model-help branch retains main's pre-Plan03 promise.
- **Required correction:** describe the new agent's model/direct workflow without implying an automatically created desk. Keep the keyed-model prerequisite. The existing create-pane test `src/adapters/http/pages/tests.rs:1398–1403` checks that prerequisite and can remain; a new test is unnecessary for this wording fix.
- **Impact:** users configuring a new agent are told it gets a desk even though desks are now optional separately created shared entry points.
- **Instruction lesson:** inspect changed sibling UI text when combining an addressing migration with an incoming form change. This is a concrete application of existing consumer tracing, not a new general instruction.
- **Resolution:** OPEN.


### 2026-11-15 — F1 corrected by 03PA-P04-D1-R3/N1

**AMENDED / still OPEN for revised real-boundary proof.** Independent producer inspection confirms `FrozenAction::local_agent` rejects Agent origin, and every initial producer supplies Workflow origin. The original F1 requirement to create a native role-selected Agent-origin local operation is **WITHDRAWN as unreachable under the current contract**. This corrects B's unsupported assumption, also present in the earlier D1 §4 proof wording. It does not authorize a new tool/producer or relaxed origin validation.

[D1/r3](logs/D1-r3.md), SHA256 `3ef997db3f23aa25babd827570f8edc2fdfcf0f69c51bdca15f76f0bc1d050cd`, independently passes on artifact `2345b4d4ec0ed95bacd4da34c9deafc24b732b02be6cd3d316e8d35e7942c26f`. F1's remaining correction is the actual native Workflow-origin actor/requester/selected-owner contention, public busy classification, unchanged state and successful retry proof described there, plus discriminating non-Workflow rejection if not already covered. Preserve helper-level role testing with accurate scope. No test proof has yet been accepted and F1 is not closed. See [D1-R3/N1](agent-review-notes.md) and [source trace](logs/D1-R3-reviewer-producer-trace.json).

### 2026-11-15 — F2 resolved by 03PA-P04-D1-R3/N1 inspection

**RESOLVED** on artifact `2345b4d4ec0ed95bacd4da34c9deafc24b732b02be6cd3d316e8d35e7942c26f`. `agent_settings.rs:875` now says “new agent's direct workflow”; the keyed-model prerequisite remains. This independent one-string correction needs no new test. Whole integration/final verification remains pending.
