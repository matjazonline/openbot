# p05 independent review findings

## CL-P05-D01-R1 — disposition

2026-11-08, architecture review of CL-P05-D01/r1, SHA-256 `4a5173ccf88cc6725553c98e895a14097559c5348d71424cad4812b3602abfe4`. **No actionable findings identified; PASS at architecture gate only.** [Review note](agent-review-notes.md#cl-p05-d01-r1--combined-management-and-durable-workflow-architecture), [REVIEW](REVIEW.md).

Reviewer `/root/cloudroom_reviewer`, requested GPT-6 Astra/high, actual metadata unavailable; workflow implement-astra-reviewed-2-2, p05. Worktree `/Users/mac03/dev/openbot`, main; start/review-base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; unchanged source `512b6e90ecf87aad7f0dc140e0afeb45532d67e2b4fedf5c15952685342cabda`. Runtime/implementation checks remain unexercised. No optional suggestions or instruction changes recorded as findings.

## CL-P05-D01-R2 — disposition

2026-11-08, architecture review of CL-P05-D01/r2, SHA-256 `6c3c67bf78cb1cdf288f2ab271d24691d29fece1ceb2d71e923064ef2c40618d`. **No actionable findings identified; PASS at architecture gate only.** [Review note](agent-review-notes.md#cl-p05-d01-r2--direct-follow-up-and-immutable-operation-origin), [REVIEW](REVIEW.md).

Reviewer `/root/cloudroom_reviewer`, requested GPT-6 Astra/high, actual metadata unavailable; workflow implement-astra-reviewed-2-2, p05. Worktree `/Users/mac03/dev/openbot`, main; start/review-base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; unchanged UNACCEPTED source `e967b0adb919a51c689b99350b2cac39544e88b470952a22ac1365767eb558a1`. Decision/coordination/snapshot identity checks passed; runtime and implementation acceptance were not exercised. No optional suggestions or instruction changes recorded as findings. Earlier R1 disposition remains unchanged.

## CL-P05-B1-R1 — whole-batch findings

Common identity for F1–F5: 2026-11-08; implement-astra-reviewed-2-2; p05, whole B1 implementation review; reviewer `/root/cloudroom_reviewer`, requested GPT-6 Astra/high, actual metadata unavailable. Worktree `/Users/mac03/dev/openbot`, main; phase-start/review-base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`; artifact **`5e3cb39493d6068a837125789a1cb5f6165c45b77af547ecf90ae3e0ed91c505`**. Authority r23 pin and active C006/r2; exact D01/r2 gate unchanged. [Review note](agent-review-notes.md#cl-p05-b1-r1--complete-management-and-workflow-implementation), [coverage/evidence](logs/reviewer-b1-r1-coverage.md), [TASK](TASK.md), [REVIEW](REVIEW.md). All five findings are actionable and OPEN; none changes the accepted design or the historical p04 acceptance.

### CL-P05-B1-F1 — P1: ready local results are absent from production worker discovery

**Category:** correctness/integration/verification. `workflow/waits.rs:336–343` adds call.ready_at to the standalone sweep query, but production `WorkflowWorker::run` (`application/workflow/worker.rs:87–135`) calls `poll_fair`. Both fair-poll discovery statements (`persistence/workflow/fair_polling.rs:55–89`) reuse `polling.rs:98–151`, whose effect readiness at136–145 still recognizes only message receipts/evidence conflicts. There is no production caller of sweep_waits in the exhaustive [inventory](logs/reviewer-b1-r1-sweep-callers.json).

Consequently a completed native operation sets ready_at but yields no production candidate until a deadline/terminal-run condition happens. Typical successful local work remains waiting, potentially for30 minutes or until the run expires, instead of waking its next claim. The core test at `local_agent_tests.rs:297–303` invokes sweep_waits directly, concealing the missing production path.

[Exact-query PG reproduction](logs/reviewer-b1-r1-query-repro-r2.txt) returns `actual_worker_ready_candidates=0`, `sweeper_ready_candidates=1`, then `actual_worker_deadline_candidates=1`. Wire local readiness into actual shared worker discovery and add a real production-poll/worker regression from parked call through native completion to fresh claim before deadline, including duplicate wake. A direct resume_wait/sweep_waits call cannot be its discovery assertion.

**Instruction lesson:** the skill already requires tracing the production consumer and checking stand-ins. The missing consumer was explicitly identified in D01-R1; implementation coverage did not follow it. No new instruction is needed. **Resolution:** OPEN at R1.

### CL-P05-B1-F2 — P1: late positive evidence can turn a ready uncertain workflow into success

**Category:** correctness/recovery. Native `observations.rs:247–324` maps Missing to Uncertain and later matching Completed to Succeeded. `validate_terminal` at326–349 deliberately permits an Uncertain operation to change before its deadline. That was existing native reconciliation behavior, but the new `workflow_local_agent_outcome_wake` trigger (`migration:13817–13825`) immediately marks the first Uncertain outcome ready. `local_agents/continuation.rs:117–144` subsequently reads the mutable current operation outcome, with no preserved workflow uncertainty disposition.

Concrete sequence: parked Workflow operation receives matching Missing/explicit uncertainty before its deadline; ready_at is committed; a matching late Completed observation arrives before wait consumption or between wake and fresh-claim receipt. The native outcome becomes Succeeded, and result_on returns successful output, despite D01 section5 requiring uncertainty to remain classified failure and late evidence to remain audit. Timing of the fresh claimant changes the workflow result. Existing `result_bounds_utf8_and_classifies_native_uncertainty` writes one static SQL outcome and never delivers the later positive evidence, so it cannot detect this.

Preserve the first terminal workflow disposition while still accepting native audit/physical-terminal facts. Exercise actual protocol uncertainty→late success both before wake and after wake/before receipt, and prove the workflow does not consume/publish success while physical capacity can settle. This finding is established by source-path inspection; no full runtime reproduction is claimed.

**Instruction lesson:** D01-R1 notes specifically warned that uncertain→positive native reconciliation must not revive workflow disposition; the design already forbids it. No design rewrite or new general instruction is supported. **Resolution:** OPEN at R1.

### CL-P05-B1-F3 — P1: workflow expiry frees capacity for native work that may still be running

**Category:** correctness/concurrency/resource bounds. `local_agents/continuation.rs:73–74` unconditionally sets a due outcome-null operation to `expired`, even when delivery_attempted_at is set or dispatch reached native submission. The established `protocol/mod.rs:245–254` expiration owner instead distinguishes untouched Expired from possibly delivered Uncertain. The shared outstanding predicate at62–64 counts only null outcomes and physically unsettled Uncertain outcomes.

When the local30-minute deadline precedes the run deadline and workflow resume wins before protocol expiry, this new branch turns attempted/running work into Expired and removes it from physical capacity/reconciliation. New effects can then be admitted although no native terminal proof exists. The [exact SQL reproduction](logs/reviewer-b1-r1-query-repro-r2.txt) yields `expired | running | counted=false`. The existing offline test never dispatched and explicitly calls expire_operations first; it does not exercise this path.

Keep the same physical uncertainty semantics in every expiry producer. Add competing workflow-resume/protocol-expiry tests for attempted work, asserting stable terminal workflow failure, continued capacity/reconciliation until matching native terminal evidence, and no ninth-effect admission while eight remain physically unsettled.

**Instruction lesson:** existing C004 and repository SQL/Rust equivalence guidance cover this; a second expiry writer diverged from the accepted owner. No new instruction needed. **Resolution:** OPEN at R1.

### CL-P05-B1-F4 — P2: profile pagination loses the conversation needed to start native work

**Category:** functionality/UI. `pages/local_agents/sessions.rs:12–20` fills the start selector only from the current100-profile page. The “More profiles” link (`profiles.rs:8–18`) carries only after_profile, dropping the current channel/conversation. `ui_local_agents.rs:214–231` then returns no conversation, so `sessions::start` replaces the start form with instructions to return to Mailbox. Returning through “Run locally” restores page1. A profile beyond the first page therefore cannot be selected for native start through the normal UI; clicking a profile card similarly loses the start association.

Preserve the authorized start association across profile navigation/selection and expose a usable path to start with a later-page profile. Add an actual HTTP/browser navigation test with more than100 profiles: follow the rendered next/select links and submit using the original conversation and intended profile. The current profile-pages test proves only store pagination, and the browser fixture contains one profile.

**Instruction lesson:** existing boundary/consumer coverage guidance applies to the rendered links as well as list queries; no new general instruction is necessary. **Resolution:** OPEN at R1.

### CL-P05-B1-F5 — P2: required Workflow continuation and first-delivery authority paths remain unexercised

**Category:** verification. D01 sections1,5,7 require explicit authorized session continuation with both original/invoking visibility and current Workflow first-delivery authority. All B1 workflow setups (`local_agent_tests.rs:56–114`) omit session_id; the only input customization adds approval. No new test reaches the Some(session_id) branch in `preparation.rs:27–32,85–107` and `admission.rs:136–153`. The post-terminal follow-up test exercises Direct management, not a new Workflow call. Likewise `cloudroom_workflow_machine_withdrawal_refuses_parked_output_and_live_delivery` (`local_agent_behavior_tests.rs:83–109`) calls native_success before disabling the machine, so it tests result consumption only, despite its name and the claimed live-delivery mapping. Inherited p03 withdrawal tests do not exercise the new Workflow call/action/approval branch.

Add decisive real-PG coverage for a valid Workflow continuation and cross-profile/cross-association/withdrawn-original-access refusals; frozen selection/approval subject across profile edits; and authority withdrawal racing the first Workflow delivery, asserting zero unauthorized dispatch and safe reconciliation for prior delivery. Include the promised negative cross-origin call linkage assertion. Correct TASK/evidence claims to match the exercised branches. The required cases are absent from the104-name inventory and the inspected assertions; this is a verification gap, not an assertion that each guard is itself defective.

**Instruction lesson:** existing verification-contract guidance explicitly rejects names and passing stand-ins as proof. Update criterion-to-assertion mapping rather than adding broader generic instructions. **Resolution:** OPEN at R1.

## CL-P05-B1-R2 — combined resolution history

**PASS** for frozen correction **`c7dfb6034c5f3f5001e107ac94a008da7fc55b88054af8208cfa1b79a6685664`**, base/HEAD `97e5d60dc538e26c7cb976a8d63772eed80baff2`, coordination r25/C006-r2. All five R1 findings were rechecked together with affected consumers and unchanged R1 coverage. No remaining/new actionable findings. Original R1 findings above remain historical records.

| Finding | R2 disposition | Decisive evidence |
|---|---|---|
| F1/P1 worker discovery | RESOLVED | Shared candidate SQL now sees exact ready local call; actual fair-poll worker test completes before deadline, consumes once and leaves no candidate. `local_agent_correction_tests.rs:45–88`. |
| F2/P1 uncertain revival | RESOLVED | Atomic immutable Workflow disposition+readiness, receipt reads saved disposition; actual Missing→Completed protocol sequence before/after wake leaves uncertain receipt/failed run while native audit succeeds and capacity releases. `local_agent_correction_tests.rs:91–151`. |
| F3/P1 premature capacity release | RESOLVED | Both expiry writers use shared attempted→Uncertain classification; deterministic Workflow winner plus concurrent worker/protocol expiry retains eight physical effects, refuses ninth until matching terminal proof, never revives expired Workflow. `local_agent_correction_tests.rs:153–313`. |
| F4/P2 later-page start | RESOLVED | Rendered links preserve authorized context; bounded selector includes selected later-page profile. Router test follows next/select and persists exact profile+conversation; independent browser navigation/submission passes. `ui_local_agents_tests.rs:93–162`, `scripts/tests/local-agents-browser.mjs`. |
| F5/P2 missing continuation/live-delivery proof | RESOLVED | Explicit Workflow Send with stable frozen approval/selection after profile edit; profile/association/private-access refusals; lock-blocked first-delivery role race with untouched refusal and attempted Reconcile; independent cross-origin FK refusal. `local_agent_continuation_tests.rs:53–351`. |

Independent focused verification:112 PASS/0 FAIL/1 ignored fixture generator, browser1 PASS. Retained author generator/focused shared-consumer/build/migration evidence inspected; historical r4 fixture and r5 Clippy failures remain retained and superseded by corrected checks. [Full review note](agent-review-notes.md#cl-p05-b1-r2--combined-correction-recheck), [input checks](logs/reviewer-b1-r2-input-checks.json), [final checks](logs/reviewer-b1-r2-final-checks.json). No final p07/native/platform acceptance claimed.
