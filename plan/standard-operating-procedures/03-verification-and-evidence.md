# 03 — Evidence-backed verification and pause points

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Make consequential checks enforceable against the exact result or action being accepted. Depends
on [01](01-shared-contracts-and-workflow-integration.md); runs in parallel with 02, 04, 05A, and 06.
This group owns verification rules, evidence resolution, and the action-entry integration. Shared
decision persistence changes must be integrated sequentially with 04.

## Batch A — Check definitions and evidence validation

1. Implement typed verification policies: bounded check keys/prompts, responsible role, required
   evidence schema, failure route, optional independent verifier, and a reason for the check.
   `read_do` presents checks while performing work; `do_confirm` presents them after work. These
   presentation modes do not change authority or permit unsupported execution paths.
2. Support pause points before accepting a result, before an external effect, and before declaring
   business success. Represent automatic checks with existing `decision.rule`/mapping contracts and
   human checks with `decision.human` steps. Compile declared dependencies into the workflow's
   explicit routes; reject missing check paths or a route that bypasses a required check. Use the
   existing workflow graph, not a separate checklist-task or result-acceptance state machine.
3. Resolve evidence through 01's typed references and current source-record authorization. Reject
   foreign-company, unrelated-run, missing, stale, or inaccessible evidence. Compare declared
   structured values using deterministic validators. A model can propose evidence; it cannot
   manufacture an authoritative verification receipt by returning receipt-shaped JSON.
4. Bind each accepted check to the run, source execution, check definition, exact result/artifact
   digest, verifier, and decision revision. Its authoritative record is an execution output or
   human submission; any read index references that record rather than storing another pass flag.
5. Require separate actors for a declared independent check and recheck their current authority.
   Failed checks take declared revision/escalation/stop routes with bounded rework. No implicit
   waiver choice exists; any permitted waiver needs explicit authority, rationale, and evidence.

Focused checks: add `sop_verification_` compiler and evidence tests for bypass paths, invalid
references, cross-company access, changed outputs, forged receipts, independent-review violations,
failure routes, and repeat exhaustion. Include a valid fixture control proving a supported check
can pass. Run existing rule, human-decision, and bounded-repeat regressions.

## Batch B — Human review and effect integrity

1. Show the exact proposed result/action, evidence, check status, and changes since earlier review
   in the existing decision page. Submit through the current revision/artifact-checked decision
   command. Comments and preliminary checkbox edits never settle the decision.
2. At shared action entry, resolve required verification from committed source records and compare
   it with the frozen prepared payload, including recipients/destinations and action identity.
   Add a narrow guard to the existing preparation/dispatch path where necessary; never dispatch a
   different artifact under an old check. Dynamic bindings cannot substitute a later payload.
3. Keep quality verification distinct from protected-action authorization. Both must pass when
   required. A quality check cannot grant a tool, broaden an agent's capabilities, replace an
   approval, or prove that an unknown remote effect did not occur.
4. Once dispatched, retain existing idempotency, receipt, cancellation, and reconciliation rules.
   Retrying a checked step cannot recreate a known effect or replay an uncertain one. Expired or
   revoked review authority follows existing fail-closed behavior; publication of new guidance
   never retroactively changes the old run's verification.
5. Add a compact authoring preview of check volume, consequences, evidence types, and failure
   routes. Feed check counts, failures, overrides, rework, and reported escaped defects to 05's
   measure catalogue. Checks with few catches are candidates for human review, not auto-deletion.

Focused checks: add real DB-backed `sop_verification_` tests for competing verifiers, submit versus
cancel/timeout, duplicate commands, role revocation, and a changed action payload after review.
Use a counted scripted provider to prove an invalid/stale check performs zero effects and a valid
accepted check performs one logical effect across restart/retry. Run decision/action-approval,
reviewed-delivery, and unknown-effect regressions plus affected decision browser protocols.

## Acceptance

- Missing or mismatched required evidence prevents progress at the enforcing boundary.
- A verification belongs to one exact artifact and cannot authorize changed recipients or data.
- The final delivered/performed artifact matches the checked artifact and retains normal approval
  and uncertain-effect behavior.
- Quality evidence lives in ordinary executions/decisions, with no second checklist engine.
