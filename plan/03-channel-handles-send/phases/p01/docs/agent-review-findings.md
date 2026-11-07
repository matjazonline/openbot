# Reviewer findings

## CHS-P01-AG1-F1 — frozen context relies on mutable intent facts

- Date: 2026-11-06; workflow: `implement-astra-reviewed-2-2`; phase: `p01`; batch/stage: `CHS-P01-AG1`, architecture gate.
- Reviewer: `/root/channel_reply_reviewer`; requested Astra/high, actual runtime model/effort not exposed in authoritative metadata.
- Worktree: `/Users/mac02/dev/openbot`, branch `main`; starting commit/review base/HEAD: `769a23aa6ef423e964f8221a8ee6b0743325c6ba`.
- Artifact digest: `4868f6c61a7b3620e19ecddffc2d982d1f51f95c1d206e6fb7f39b8ac3b5b707`; exact nine exclusions and adapter PATH in [manifest](logs/CHS-P01-AG1-manifest.json).
- Decision: [CHS-P01-D1/r1](logs/CHS-P01-D1-r1.md); task: [TASK.md](TASK.md); review: [REVIEW.md](REVIEW.md).

- Review round: [CHS-P01-AG1-R1](agent-review-notes.md#chs-p01-ag1-r1--changes-required).
- Classification: actionable defect; **P2**; architecture/correctness; status **open**.
- Evidence: D1.3 says source/channel/thread/actor/policy are stored through “existing tenant-qualified immutable intent/configuration references.” `migrations/20260817000000_init_schema.sql:5391–5426` defines the intent's source association, admissibility, origin and delivery policy as ordinary mutable columns. The migration has no UPDATE guard for `workflow_message_intents`; its checks restrict valid values, not changes. `:10732–10733` enforces a valid association but permits a different valid same-tenant association. `:10748–10749` explicitly sets `responder_principal_id` to NULL on principal deletion. `src/adapters/persistence/workflow/message_intents.rs:36–67` reloads these values for admission, and `src/application/workflow/message_admission.rs:475–509` carries the reloaded policy into the run. A FK to `(company_id,intent_id)` alone does not freeze these facts.
- Required vs proposed: original 01A.5 requires immutable original association, policy and acting authority, retained through continuation; D1 currently asserts that existing intent references already provide that property. For example, changing an accepted intent from `in_app_only` to `deliver` remains schema-valid and changes the next admission's policy if the context merely references the intent. Changing an accepted association to another valid same-tenant association similarly defeats source stability. D1.5's broad mention of immutable/validation triggers does not identify this pre-existing mutable dependency or its deletion behavior.
- Required correction: explicitly identify the authoritative frozen facts, the persistence boundary that enforces their immutability, and permitted deletion/nulling behavior. Preserve the necessary mutable queue state separately. Add targeted mutation/deletion and round-trip checks proving an accepted context cannot change source/policy/authority through its references. The author owns the implementation/design choice.
- Verification: established by schema and caller inspection; no database exploit executed in this architecture-only round.
- Instruction lesson: the existing persistence rule “application checks ... are not substitutes for constraints” applies. Candidate improvement is recorded in R1: verify update/delete semantics for every referenced frozen fact.
- Resolution history: 2026-11-06 opened against D1/r1; awaiting author correction and independent recheck.

## CHS-P01-AG1-F2 — context cardinality conflicts with reply-first admission

- Date: 2026-11-06; workflow: `implement-astra-reviewed-2-2`; phase: `p01`; batch/stage: `CHS-P01-AG1`, architecture gate.
- Reviewer: `/root/channel_reply_reviewer`; requested Astra/high, actual runtime model/effort not exposed in authoritative metadata.
- Worktree: `/Users/mac02/dev/openbot`, branch `main`; starting commit/review base/HEAD: `769a23aa6ef423e964f8221a8ee6b0743325c6ba`.
- Artifact digest: `4868f6c61a7b3620e19ecddffc2d982d1f51f95c1d206e6fb7f39b8ac3b5b707`; exact nine exclusions and adapter PATH in [manifest](logs/CHS-P01-AG1-manifest.json).
- Decision: [CHS-P01-D1/r1](logs/CHS-P01-D1-r1.md); task: [TASK.md](TASK.md); review: [REVIEW.md](REVIEW.md).

- Review round: [CHS-P01-AG1-R1](agent-review-notes.md#chs-p01-ag1-r1--changes-required).
- Classification: actionable defect; **P2**; architecture/requirements; status **open**.
- Evidence: D1.3 creates a `channel_reply_contexts` row in the ingress insertion transaction whenever a responder is selected, before reply correlation. D1.4 correctly says a consumed reply or trigger mismatch gets no run link, but its invariants say “one run link per accepted context” and “No context/link for reply-consumed.” In current code, `src/adapters/persistence/thread/inbound.rs:93–136,712–769` commits the intent before `src/application/workflow/message_admission.rs:234–305` decides whether a reply is matched, unmatched or deferred. D1.3 cannot know that future result. Thus a successfully matched human reply already has the ingress snapshot, and a nonmatching trigger can also leave a snapshot with no link.
- Impact: the proposed schema's meaning and cardinality are contradictory at the ownership boundary. A later completion/publisher must not treat a passive accepted snapshot as an owned automatic-response obligation. Requiring one link per snapshot is also inconsistent with the specified no-link cases; deleting immutable snapshots after correlation is not a recorded design.
- Required correction: state the exact meaning/lifecycle of the pre-admission snapshot and the point at which response authority exists, reconcile zero-link snapshots with the invariants, and make the matched/deferred/unmatched/trigger-mismatch verification assert the intended persisted shape. Preserve the original requirement that a consumed reply creates no new automatic-response obligation.
- Verification: existing ordering and outcomes inspected; no test run at this pre-implementation gate.
- Instruction lesson: no new rule needed; the skill already requires explicit invariants and failure modes for ownership gates. This is a contradiction in the gated decision rather than a code defect.
- Resolution history: 2026-11-06 opened against D1/r1; awaiting author correction and independent recheck.

## 2026-11-06 correction recheck — CHS-P01-AG1-R2

- Workflow `implement-astra-reviewed-2-2`, phase p01, architecture gate; reviewer `/root/channel_reply_reviewer`, requested Astra/high, actual runtime settings not independently exposed.
- Reviewed [CHS-P01-D1/r2](logs/CHS-P01-D1-r2.md) as explicit amendments to retained r1. Source base/HEAD and digest unchanged: `769a23aa6ef423e964f8221a8ee6b0743325c6ba` / `4868f6c61a7b3620e19ecddffc2d982d1f51f95c1d206e6fb7f39b8ac3b5b707`.
- **CHS-P01-AG1-F1: fixed in architecture decision.** r2 explicitly recognizes mutable intents, records immutable snapshot copies, requires INSERT provenance validation and UPDATE guards, and names the referenced intent fields protected while queue state remains mutable. Its tenant-qualified deferred principal constraint and retained-context deletion rules now state what principal/source deletion can do; rollback-scoped mutation/partial-deletion/full-company-teardown checks are required. This closes the false existing-immutability premise. Enforcement remains to be verified against implementation; this disposition does not claim those future tests passed.
- **CHS-P01-AG1-F2: fixed in architecture decision.** r2 states zero-or-one links per passive snapshot and exactly one link for an admitted designated responder, grants authority only through the authentic-message-claim/run-creation transaction, excludes passive snapshots from finalization/publication enumeration, and specifies matched/deferred/unmatched/mismatch/refused persisted shapes. This resolves the contradiction with reply-first admission without requiring premature correlation or deleting the snapshot.
- Recheck coverage/evidence: [CHS-P01-AG1-R2](agent-review-notes.md#chs-p01-ag1-r2--pass-for-d1r2). No unresolved actionable findings for D1/r2.

## CHS-P01-B1-F1 — selection omits current run-as channel visibility

- Date: 2026-11-06; workflow `implement-astra-reviewed-2-2`; phase p01; batch review CHS-P01-B1-R1.
- Reviewer `/root/channel_reply_reviewer`, requested Astra/high; actual model/effort not independently exposed.
- Worktree `/Users/mac02/dev/openbot`, branch main; starting commit/review base/HEAD `769a23aa6ef423e964f8221a8ee6b0743325c6ba`.
- Reviewed artifact `eec077d537f975767e3d1d0deed360b88a76b40730a910c85caad126bc6fc4a7`; [manifest](logs/CHS-P01-B1-R1-manifest.json); gated decision D1/r2; [TASK](TASK.md), [REVIEW](REVIEW.md), [review note](agent-review-notes.md#chs-p01-b1-r1--changes-required).

- Classification: actionable defect; **P2**, correctness/authorization-validation; status **open**.
- Evidence: `src/adapters/persistence/workflow/channel_reply.rs:141–144` checks `authorize_company(configuration.run_as())`, bundle selection and resource readiness, but never checks `authorize_association` for that actor. `authorize_settings:119–121` authorizes only the settings editor. Existing `bindings.rs:181–216` explicitly verifies both company management and channel visibility for a binding's run-as; `resources.rs:61–136` has no actor/channel visibility check. Admission authorizes the actual run actor against the channel again in `admission.rs:24–31`, so it rejects the accepted selection later.
- Reproduction by code path: configure/activate a message binding running as an admin who can view an allowlist channel; revoke that admin's view grant while retaining admin membership and the active binding; as company owner, select it as the responder with current revisions and a valid literal contract. All checks in `selected_on` pass, although the chosen run-as cannot view the channel and the first matching admission is refused.
- Required behavior: original01B.1 and D1/r1 selection require an active eligible binding with current authority/readiness at configuration. Invalid current configuration must be rejected before accepting the selection. This is not a send-authorization bypass: later admission currently fails closed, leaving an avoidable unusable responder configuration.
- Requested correction: apply the existing run-as association visibility requirement inside the selection transaction and cover grant revocation (including authorized owner selecting a different run-as) with a focused DB test that proves no settings change is committed. Preserve infrastructure error propagation. No new architecture is required.
- Verification: source/caller inspection; current17 new binding tests do not exercise revoked run-as channel access. Reviewer runtime invocation unavailable due nested sandbox guard; author test evidence inspected.
- Instruction lesson: reuse the complete established eligibility check when adding another configuration path; existing source guidance already says one decision, one place. No separate instruction change proposed.
- Resolution history: 2026-11-06 opened against B1 digest above; awaiting correction/recheck.

## CHS-P01-B1-F2 — deactivation before ingress silently erases responder refusal

- Date: 2026-11-06; workflow `implement-astra-reviewed-2-2`; phase p01; batch review CHS-P01-B1-R1.
- Reviewer `/root/channel_reply_reviewer`, requested Astra/high; actual model/effort not independently exposed.
- Worktree `/Users/mac02/dev/openbot`, branch main; starting commit/review base/HEAD `769a23aa6ef423e964f8221a8ee6b0743325c6ba`.
- Reviewed artifact `eec077d537f975767e3d1d0deed360b88a76b40730a910c85caad126bc6fc4a7`; [manifest](logs/CHS-P01-B1-R1-manifest.json); gated decision D1/r2; [TASK](TASK.md), [REVIEW](REVIEW.md), [review note](agent-review-notes.md#chs-p01-b1-r1--changes-required).

- Classification: actionable defect; **P2**, requirements/observability; status **open**.
- Evidence: `migrations/20260817000000_init_schema.sql:12606–12612` filters the selected binding by `binding.active` before creating its passive snapshot. The settings row remains selected when binding activity changes. `src/adapters/persistence/workflow/message_intents.rs:97–104` includes an inactive binding only when the response snapshot exists; absent that snapshot it is omitted entirely. `MessageAdmissions::process:249–283` can record a refusal only for returned candidates and otherwise finishes the intent with no responder outcome.
- Reproduction by code path: select an active `Any` responder, deactivate it, then accept an ordinary admissible message and drain admission. The message is accepted with zero response context and zero selected-binding outcome, so an unavailable configured responder is indistinguishable in admission history from no responder being selected. Current `channel_reply_binding_trigger_mismatch_and_revocation_create_no_obligation` tests deactivation only AFTER the eligible intent was inserted, which takes a different, correctly observable path.
- Required behavior: original01B.2 requires an observable admission refusal/failure when the selected responder should start but cannot. D1/r1.4 specifically says deactivation remains visible in existing admission outcome records; D1/r2 makes snapshots passive so capturing selection does not itself authorize execution. Current activity must still refuse execution, but must not erase the accepted selected responder identity and its refusal record.
- Requested correction: preserve the configured selection/refusal evidence for admissible matching ingress when its selected binding is already inactive, and retain the existing current-activity refusal at admission. Add a targeted before-ingress deactivation case asserting no run/link and one inspectable refusal; preserve no-selection, quiet-ingress and trigger-mismatch behavior.
- Verification: migration/candidate/process inspection; runtime reproduction not executed by reviewer due sandbox restriction. Author's passing deactivation-after-ingress test does not cover this ordering.
- Instruction lesson: exercise revocation on both sides of snapshot creation when the snapshot controls later refusal visibility. Scope: asynchronous admission snapshots; evidence current test's ordering and the capture predicate. No instruction files changed.
- Resolution history: 2026-11-06 opened against B1 digest above; awaiting correction/recheck.

### 2026-11-06 architecture classification follow-up — B1-F2

Root requested explicit classification against the earlier gate. D1/r1.3 did not spell out whether current activity gates the passive capture; r1.2 required an active selection and r1.4 promised visible deactivation refusals. The implementation and passed gate did not settle the deactivation-before-ingress ordering. B1-F2 therefore exposes an **under-specified lifecycle ordering missed by CHS-P01-AG1-R2**, alongside the concrete implementation omission. Retain the earlier PASS as a decision-level historical result, not proof of this uncovered ordering. Before the capture/admission correction, root must record D1/r3 clarifying the inactive-selected ingress observation, its persisted refusal evidence and separation from current execution authority, and obtain an explicit architecture recheck. B1-F1 is an implementation eligibility omission under existing authority requirements; it may be included in the same narrow r3 clarification. Author owns the actual decision and correction.

### 2026-11-06 decision correction — B1-F1/F2

[CHS-P01-AG2-R1](agent-review-notes.md#chs-p01-ag2-r1--pass-for-d1r3-corrective-amendment) passes the author-owned [D1/r3 proposal](logs/CHS-P01-D1-r3-proposed.md) against unchanged source digest eec077d537f975767e3d1d0deed360b88a76b40730a910c85caad126bc6fc4a7. The decision now explicitly requires current run-as channel visibility and passive capture of an inactive configured responder for observable refusal. The architecture-ordering gap linked to AG1-R2 is addressed in the decision. **Both B1 code findings remain open** pending actual correction and one combined independent recheck.

### 2026-11-06 combined implementation correction recheck — CHS-P01-B1-R2

- Workflow `implement-astra-reviewed-2-2`, phase p01, reviewer `/root/channel_reply_reviewer` requested Astra/high; actual runtime settings not independently exposed. Worktree `/Users/mac02/dev/openbot`, branch main; base/HEAD 769a23aa6ef423e964f8221a8ee6b0743325c6ba.
- Corrected artifact **0995151ed93e67f2e5feee8092cdc08687580b79f1d988d40d5d072d5e39decf**, [R2 manifest](logs/CHS-P01-B1-R2-manifest.json); D1/r3 previously passed AG2-R1. Independent archive/hash bridge confirms exactly three corrected files and 13 unchanged source/test files relative to R1.
- **CHS-P01-B1-F1: FIXED.** `channel_reply::selected_on` now calls the existing association authorization owner for `configuration.run_as()` and the selected channel before any settings write. Its current company authority check remains. The new private-channel test verifies initial denied selection leaves no row, a verified admin with a view grant can be selected, and revoked access refuses a contract update without changing persisted settings. Inspected guarded `correction-binding-tests-02`19 PASS and existing `correction-actor-tests-02`1 PASS. No assertion weakened.
- **CHS-P01-B1-F2: FIXED.** The capture trigger retains the selected passive snapshot without requiring current binding activity. Unchanged admission lookup/current verification still refuse inactive execution; no run/link is created. The new before-ingress deactivation test verifies one passive snapshot, one refused outcome and no run, then verifies intentional clear produces no snapshot/outcome. All 19 binding tests include existing after-ingress revocation, quiet/mismatch and HITL cases;14 existing message-intent tests also pass. Decision ambiguity is resolved by retained D1/r3/AG2-R1.
- Evidence: [R2review note](agent-review-notes.md#chs-p01-b1-r2--pass-for-corrected-p01-artifact), correction test inventories/receipts/logs02, fresh migrations01, SQLx prepare/check02, offline all-target Clippy02 and fmt/diff02. All pass at the corrected source identity. Prior initial fixture failure remains recorded; positive control required a verified email, now explicitly provisioned.
- No unresolved p01 actionable findings. This closes the implementation findings, not later plan phases or live-provider/full-suite readiness.
