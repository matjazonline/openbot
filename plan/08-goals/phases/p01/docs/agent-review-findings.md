# 08G-P01 review findings

## 08G-P01-D1-R1-F1 — synchronous informed notices reverse inherited account ownership

- Date/workflow: 2026-11-16; implement-astra-reviewed-2-3; phase p01, B1 architecture gate, round [08G-P01-D1-R1-N1](agent-review-notes.md).
- Reviewer: /root/p01_reviewer; requested gpt-6-astra/high, independent context. Actual model/effort readback and compaction controls unavailable to this worker.
- Worktree: /Users/mac02/orca/workspaces/openbot/plan08_ph1; branch plan08_ph1; review base/HEAD 8fdb892b6e279ed0c09150e2336bbdc7462f877d; digest 70a35e16397e0e6fc0eae14ceaf625e4e9381e9a0b50c3845ce50c10688df5c2; exclusion only plan/08-goals/phases/p01/docs.
- Classification: actionable defect; **high**; architecture/concurrency. Origin is proposed [D1/r1 clause 5](logs/08G-P01-D1-r1.md), not implemented code.
- Evidence: clause 5 composes standalone information deliveries during park/follow refresh while clauses 1 and 3 retain account-before-company ownership. Existing `reviewer_accounts.rs:54-83` acquires account locks before company, but `business_hints_on` at 85-159 deliberately returns without recipient mappings for followed decisions (145-147); non-followed mappings come only from deciding reviewers. `follow_reviewers/storage.rs:28-129` therefore creates durable notification events without synchronously accessing recipient email accounts. `follow_reviewers/mod.rs:96-131` and callers in `department.rs:308-329,459-491,631-673` already hold company ownership. `decisions.rs:393-424` acquires account SHARE locks when loading eligible recipients; `user_lifecycle.rs:31-70` retires an account by locking that account FOR UPDATE before its companies.
- Failure: A role/topology write can own company C and seek informed account U while concurrent retirement owns U and seeks C. Reusing synchronous composition with existing recipient eligibility creates a lock cycle; omitting the account lock instead loses the inherited retirement barrier. Initial park also lacks informed account membership in its current prelock set. This contradicts the gated invariant that the live account prefix remains intact.
- Required correction: define a notice production/projection boundary that preserves the existing account-before-company protocol for both ordinary park and every follow-refresh producer. Identify the production owners, eligibility recheck and rollback boundary, and map verification to actual competing retirement/notice producers. This review does not select or author the replacement design.
- Verification: source inspection and caller tracing, no proposed implementation or DB race executed. Required regression must discriminate the actual lock ordering, not only sequential event creation.
- Instruction lesson: the existing verification-contract guidance to enumerate inherited side effects and container dependencies applies. Candidate scoped lesson: before reusing synchronous recipient composition inside an existing transaction, trace recipient account lock acquisition and every transaction-owning caller. Evidence supports the narrow concurrency lesson; no instruction edits authorized.
- Resolution history: **OPEN** 2026-11-16. Root independently identified the same conflict while B was reviewing; B confirmed it against actual production paths. A later proposed fix does not close this finding.

## 08G-P01-D1-R1-F2 — separate standalone information deliveries escape retirement cancellation

- Date/workflow/phase/round: 2026-11-16; implement-astra-reviewed-2-3; p01 B1 architecture gate; [08G-P01-D1-R1-N1](agent-review-notes.md).
- Reviewer/worktree/artifact: same reviewer, requested settings, worktree, base/HEAD, digest and exact exclusion as F1; decision [08G-P01-D1/r1 clause 5](logs/08G-P01-D1-r1.md).
- Classification: actionable defect; **medium**; architecture/privacy/lifecycle. Origin is proposed decision.
- Evidence: clause 5 retains information deliveries only in separate current `workflow_decision_informed` rows and promises reuse of existing cancellation, explicitly naming cancellation on replacement. Existing `delivery/cancellation.rs:148-179` discovers a retiring recipient's deliveries exclusively through `notifications.email_delivery_id` or `workflow_decision_reviewers.notice_delivery_id`. Membership retirement calls it at `company_invite.rs:622`; account retirement calls it at `user_lifecycle.rs:90`. The proposed standalone information delivery belongs to neither source. Existing `workflow_reviewer_principal_retirement_cancels_pending_and_inflight_notices` (`reviewer_principal_tests.rs:55-117`) proves the inherited pending-terminal/in-flight-cancellation-request contract for both retirement paths.
- Failure: an informed account can be removed after information delivery enqueue but before delivery; unchanged cancellation cannot discover the new delivery. The email remains pending or sending without recipient-revoked cancellation. Current-account eligibility at enqueue alone does not satisfy later retirement, and role-follow replacement is not a substitute for cancellation of a non-followed decision's frozen recipient.
- Required correction: make retirement discover and cancel information deliveries through the chosen authoritative notice relationship; define how historical identity survives retirement and how pending/in-flight cancellation, preference opt-out and replacement interact. Add focused cases through actual membership removal and account deletion for both followed and non-followed information notices, retaining unrelated recipients.
- Verification: inspected actual cancellation query, both callers, and existing behavioral assertions; no new behavior tested.
- Instruction lesson: existing contract-impact instructions already require all consumers of stored delivery identities. Candidate scoped lesson: when adding a durable notification relationship, include retirement cancellation discovery as an explicit consumer; generic reuse of a delivery cancel helper is not proof that its discovery query sees the new relationship.
- Resolution history: **OPEN** 2026-11-16; no correction reviewed.

## 2026-11-16 resolution update — 08G-P01-D1-R2-N1

Rechecked both findings against immutable [08G-P01-D1/r2](logs/08G-P01-D1-r2.md). Source base/HEAD and digest remain 8fdb892b6e279ed0c09150e2336bbdc7462f877d / 70a35e16397e0e6fc0eae14ceaf625e4e9381e9a0b50c3845ce50c10688df5c2; exact exclusion unchanged. Coordination r2 remains current with no notices. Same independent B and requested settings; runtime readback remains unavailable.

- **08G-P01-D1-R1-F1: FIXED at architecture level.** Revised clause 5 eliminates synchronous recipient composition from every park/follow/role/topology/retirement transaction. Producers commit source/event/current-membership facts only. Projection reuses the established sorted account prefix before company/run/decision ownership and revalidates current source, recipient and preferences. Independently inspected `notification/projection_lock.rs:15-78`, `projection.rs:33-95,174-241,243-365`, and `reviewer_notice.rs:18-112`: these are actual existing seams, not an assumed capability. The corrected verification plan explicitly includes recipient-account contention and both retirement/projection schedules. No new runtime path exists yet; correction closes the design conflict, not its future implementation proof.
- **08G-P01-D1-R1-F2: FIXED at architecture level.** All information emails are now owned by `notifications.email_delivery_id`; existing `cancel_member_notices_on` discovers that relationship by recipient user for both lifecycle callers. R2 explicitly retains historical source identity while deactivating removed current sources, prevents stale events from recreating notices, withdraws/cancels replaced notifications, and requires pending/in-flight membership/account retirement plus projection-after-retirement tests. Eligibility and active/current source revalidation remain required at projection/commit. This closes the separate-owner cancellation gap; execution evidence is still required during B1 implementation review.

No new actionable or optional findings in this recheck. Original r1 findings and verdict remain retained above.

## 08G-P01-B1-R1-F1 — Information-only emails still request action

Identity: 2026-11-16; implement-astra-reviewed-2-3; p01/B1 implementation review; note 08G-P01-B1-R1-N1; B /root/p01_reviewer, independently requested gpt-6-astra/high (runtime readback unavailable); worktree /Users/mac02/orca/workspaces/openbot/plan08_ph1; base/HEAD 8fdb892b6e279ed0c09150e2336bbdc7462f877d; artifact 1b179b0de6235e6c456a742bcf63b6da0d55db5ddd11e64ad35f1cd7a7f405d6; exact exclusion plan/08-goals/phases/p01/docs. [TASK](TASK.md), [REVIEW](REVIEW.md), [decision D1/r2](logs/08G-P01-D1-r2.md).

Classification: actionable defect; **medium**; correctness / requirements.

Evidence and correction:

`src/application/services/notification_worker.rs:208` unconditionally prefixes every email subject with `Action required: `. The new action label in `src/domain/entities/notification.rs` is `Workflow decision — for information only`, producing the contradictory subject `Action required: Workflow decision — for information only`. This shared composer is used by actual deferred sponsor information deliveries. Original 01B.4 and D1/r2 clause5 require information-only notices distinguished from requests to review. The new MIME assertion at `role_sponsor_runtime_tests.rs:210-217` checks only that the phrase `for information only` occurs, so it passes despite the action request. This confuses holders who have no settlement authority. Inspection establishes the string construction; B did not send external email.

Correction: give DecisionInformation a neutral subject while preserving actual actionable notifications, and assert the rendered information email subject contains no action/review request.

Instruction lesson: Existing contract-impact guidance already requires checking inherited behavior of reused components. Candidate scoped lesson: when adding an informational variant to an actionable notification pipeline, inspect actual subject/body rendering as well as labels and preference mappings. This is an implementation oversight, not a D1/r2 design defect.

Resolution history: 2026-11-16 **OPEN** against B1/R1. No correction examined.

## 08G-P01-B1-R1-F2 — Registry authoring help omits Sponsor and its kind contract

Identity: 2026-11-16; implement-astra-reviewed-2-3; p01/B1 implementation review; note 08G-P01-B1-R1-N1; B /root/p01_reviewer, independently requested gpt-6-astra/high (runtime readback unavailable); worktree /Users/mac02/orca/workspaces/openbot/plan08_ph1; base/HEAD 8fdb892b6e279ed0c09150e2336bbdc7462f877d; artifact 1b179b0de6235e6c456a742bcf63b6da0d55db5ddd11e64ad35f1cd7a7f405d6; exact exclusion plan/08-goals/phases/p01/docs. [TASK](TASK.md), [REVIEW](REVIEW.md), [decision D1/r2](logs/08G-P01-D1-r2.md).

Classification: actionable defect; **low**; requirements / authoring.

Evidence and correction:

`src/application/workflow/registry/help.rs:65-69,80-81` still says the literal roles are `(lead, reviewer)` for `ai.classify`, `agent.run`, and `decision.agent`. The `decision.human` note at lines77-78 still describes only an explicit static user list and explains no Sponsor decider. These notes are exposed through `authoring_help` (18-53) and consumed by the real authoring catalogue (`authoring/catalogue.rs:26-66`). The updated Markdown language guide and reviewer-picker text therefore do not fulfill original 01D.4's separate authoring-help requirement: Sponsor resolves to one actor, with an agent required by those three steps and a person by decision.human. Existing publication help assertions (`publication/role_tests.rs:119-149`) only require role/reviewer/ancestor wording.

Correction: update the shared registry help for all four affected step types with Sponsor vocabulary and the deciding actor/kind contract; extend the existing help test so this surface stays consistent with the supported vocabulary.

Instruction lesson: Cause is an incomplete consumer disposition: picker help was updated but registry help remained stale. Existing exhaustive consumer-tracing guidance is sufficient; no new general instruction is justified.

Resolution history: 2026-11-16 **OPEN** against B1/R1. No correction examined.

## 08G-P01-B1-R1-F3 — Promised live-policy SQL forgery coverage is absent

Identity: 2026-11-16; implement-astra-reviewed-2-3; p01/B1 implementation review; note 08G-P01-B1-R1-N1; B /root/p01_reviewer, independently requested gpt-6-astra/high (runtime readback unavailable); worktree /Users/mac02/orca/workspaces/openbot/plan08_ph1; base/HEAD 8fdb892b6e279ed0c09150e2336bbdc7462f877d; artifact 1b179b0de6235e6c456a742bcf63b6da0d55db5ddd11e64ad35f1cd7a7f405d6; exact exclusion plan/08-goals/phases/p01/docs. [TASK](TASK.md), [REVIEW](REVIEW.md), [decision D1/r2](logs/08G-P01-D1-r2.md).

Classification: actionable defect; **medium**; verification / SQL authority.

Evidence and correction:

D1/r2 explicitly commits to targeted SQL tamper tests for all three producer/guard paths, including forged binding-policy/default selection. `escalation_default_tests.rs:38-214` exercises legitimate run-failure, admission-refusal, and empty-follow producers and validates the selected policy, but never submits a forged case. Existing `escalation_source_guard_tests.rs:108-147,173-208` forges reason/step/execution/role while copying `run.escalation`; `follow_escalation_tests.rs:247-275` does the same for empty-reviewers. The other source-guard tests check terminal audit reasons, not the new current-default branch. Thus green producer tests do not establish that the changed SQL guard rejects an incorrect or stale default or a policy overriding explicit binding policy. No production bypass is asserted by this finding; the promised enduring boundary proof is missing.

Correction: add direct-SQL rejection tests for run, admission-refused and empty-reviewers sources under live defaults and explicit-policy precedence, reaching the authoritative case guard and asserting its named rejection. Include valid control candidates so unrelated constraints cannot satisfy the tests. Record actual focused execution and update the AC5 mapping.

Instruction lesson: Architecture PASS accepted a verification plan, not execution. Existing verification-contract guidance is sufficient: reconcile each promised tamper case against a decisive assertion, separately from legitimate producer behavior.

Resolution history: 2026-11-16 **OPEN** against B1/R1. No correction examined.

## 08G-P01-B1-R1-F4 — Later People Sponsor capacity seals lack the promised negative tests

Identity: 2026-11-16; implement-astra-reviewed-2-3; p01/B1 implementation review; note 08G-P01-B1-R1-N1; B /root/p01_reviewer, independently requested gpt-6-astra/high (runtime readback unavailable); worktree /Users/mac02/orca/workspaces/openbot/plan08_ph1; base/HEAD 8fdb892b6e279ed0c09150e2336bbdc7462f877d; artifact 1b179b0de6235e6c456a742bcf63b6da0d55db5ddd11e64ad35f1cd7a7f405d6; exact exclusion plan/08-goals/phases/p01/docs. [TASK](TASK.md), [REVIEW](REVIEW.md), [decision D1/r2](logs/08G-P01-D1-r2.md).

Classification: actionable defect; **medium**; verification / canonical authority.

Evidence and correction:

D1/r2 clauses3-4 and its verification additions require capacity-aware later People documents and SQL canonical-tamper proof. New `sponsor_notice_tests.rs:215-262` rejects an extra admission-zero informed row and forged information sources, but never tampers with a later People resolution. Existing `people_resolution_tests.rs:16-37,65-172` builds only Reviewer documents with `informed: Vec::new()`; the sole change to that fixture is the empty field. The legitimate followed-Sponsor runtime test proves replacement succeeds, not that the expanded later-document boundary rejects missing/extra informed members, mismatched capacity, deciding/informed overlap, non-Sponsor informed membership, or nonempty informed membership in a refusal. These are materially changed canonical authority predicates; existing deciding-only tests do not distinguish them. No demonstrated production bypass is claimed.

Correction: extend later-resolution SQL tests with successful Sponsor controls plus targeted invalid documents/normalized rows for the new informed/capacity invariants, including refusal emptiness. Ensure tests actually reach the SQL guards rather than being rejected only by Rust validation; assert the relevant constraints and rollback/unchanged retained state. Record focused execution and the AC3/AC4 evidence.

Instruction lesson: Existing verification-contract and immutable-seal guidance already applies. Candidate scoped lesson: extending a sealed collection requires negative tests for the added collection and its relationship to existing collections, not only an admission-zero extra-row test.

Resolution history: 2026-11-16 **OPEN** against B1/R1. No correction examined.

## 08G-P01-B1-R2 — correction dispositions, 2026-11-16

Recheck note [08G-P01-B1-R2-N1](agent-review-notes.md); same independent B/worktree/workflow as R1. Artifact `55433874cdfe70da7de428e2981dfe8c3a173fb640b866859bf9a6fa4240b3a1`, base/HEAD `8fdb892b6e279ed0c09150e2336bbdc7462f877d`, exact exclusion `plan/08-goals/phases/p01/docs`; coordination r2/no notices unchanged. [Focused evidence](logs/B1-R2-focused.txt):29 passed/0 failed. Original findings and R1 outcome above remain retained.

### 08G-P01-B1-R1-F1 — FIXED

2026-11-16: The composer now uses the neutral DecisionInformation label as subject and retains the original prefix for other actions (notification_worker.rs:208-213). The actual Deployment notice test reads message_delivery_parts.payload.body.subject and requires exact “Workflow decision — for information only” (role_sponsor_runtime_tests.rs:170-200); it runs through the real deferred worker and passes. Informed submission denial plus unchanged submitted_at and frozen nofollow reassignment tail also pass.

### 08G-P01-B1-R1-F2 — FIXED

2026-11-16: Registry help now enumerates Sponsor on all three Agent step types and decision.human, explaining one deciding agent/person and non-executing/non-settling informed holders (registry/help.rs:65-81). The existing publication help test now checks every DepartmentRole::ALL value on all four surfaces and their distinct deciding-kind language (publication/role_tests.rs:119-161); it passes.

### 08G-P01-B1-R1-F3 — FIXED

2026-11-16: Three direct actual-table tests in escalation_default_tests.rs:216-364 exercise run, admission-refusal, and empty-reviewers source guards, each under inherited-default and explicit-policy branches. Candidate rows retain the genuine source identity; valid candidates traverse BEFORE INSERT validation then become uniqueness no-ops. Stale-default/overriding-default candidates fail specifically with workflow_escalation_case_guard, and retained original policy remains unchanged. The controls are non-vacuous because the forged candidate must produce a named error and retained policies must contain exactly the expected original case. All three tests pass.

### 08G-P01-B1-R1-F4 — FIXED

2026-11-16: Registered people_sponsor_resolution_tests.rs:4-227 bypasses Rust validation via raw fact/member inserts. It exercises missing informed key, deciding/informed overlap, duplicate informed member, nonempty refusal information, multiple deciding sponsors, non-Sponsor informed values with a genuine frozen Reviewer requirement, missing/extra normalized informed rows, and mismatched capacity. Tests assert the specific SQL guards and no retained rejected facts; successful Sponsor, empty-refusal, Reviewer and raw normalized controls demonstrate admissible paths and canonical roundtrip. All three tests pass.

All four findings closed for this artifact; no new findings. Full required gate and final serialized SQLx regeneration remain outstanding independently of these corrections.

## 08G-P01-B1-R3-F1 — Shipped Sponsor sample omitted required human-decision bindings

Identity: 2026-11-16; implement-astra-reviewed-2-3; p01/B1, author-found final-verification defect, independent recheck [08G-P01-B1-R3-N1](agent-review-notes.md). B /root/p01_reviewer, independently requested gpt-6-astra/high, actual runtime readback unavailable; worktree /Users/mac02/orca/workspaces/openbot/plan08_ph1. [TASK](TASK.md), [REVIEW](REVIEW.md). Defect present in R2 artifact55433874cdfe70da7de428e2981dfe8c3a173fb640b866859bf9a6fa4240b3a1; corrected artifact7c658d6ea48bf801b7e7225c10c4e1fda406eca2f6412036cd4eacabf3a6ae3a; base/HEAD8fdb892b6e279ed0c09150e2336bbdc7462f877d; exact exclusion `plan/08-goals/phases/p01/docs`.

Classification: actionable defect, medium, correctness/verification; author found and fixed before this recheck. Original requirement01D.4 requires a usable shipped Sponsor approval sample. `src/application/workflow/fixtures/role-sponsor-approval.yaml` omitted required literal data_schema on decision.human. The real all-fixture compile/freeze, standard-input and storage tests failed with `registry.literal` at `/steps/review/with/data_schema` in [full-run output](logs/B1-full-check-owned-pg.txt:5561). The scripted Deployment used a registry-derived human step and did not compile this YAML. This is an implementation defect, not a change to the accepted architecture.

Review-gap linkage: R1/R2 inspected the sample source but missed the required binding. R2 explicitly retained sample-publication/full-suite proof as pending and was not a phase acceptance; nevertheless its affected-source PASS did not identify this concrete sample defect. This entry supplements those notes without rewriting their outcomes. Existing direct-fixture coverage catches it when exercised.

Resolution, 2026-11-16: **FIXED**, independently rechecked at artifact7c658d6e…ae3a. YAML review step now supplies `data_schema: {literal: {type: "null"}}` and `feedback_required: {literal: []}`. The null payload matches an approve/reject decision that forwards the lead draft unchanged on approval; optional empty feedback is consistent with the sample contract. Original choice routes, Sponsor reviewer, lead Agent role, deadline and rejection suppression remain intact. Actual Fixture::ALL maps RoleSponsorApproval to this YAML; publication tests freeze its decoded source and roundtrip immutable stored content. [Focused rerun](logs/B1-R3-fixtures.txt):16 passed/0 failed, including all three formerly failing checks. No test or assertion weakened.

Instruction lesson: include existing actual shipped-fixture compilation/input/storage checks in the focused batch when adding a sample. A registry-derived equivalent runtime fixture does not establish that the shipped source compiles. This is supported by the three failures and the corrected16-test pass; no new test infrastructure or architecture gate is needed. Root's matching Trail learning is included in the artifact; B changed no instructions.
