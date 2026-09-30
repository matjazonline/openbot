# 04.7 ordinary Failed terminal constructor independent check

Sol independent preparation PASS for frozen
`CONTRACT-04.7-TERMINAL-REVISION.md` SHA256
`ca4cb3696624c62b365eafc7dd6590b5e56385389c67153dfe5bbc5bc5f341ac`.
No ordinary Failed implementation or database experiment is claimed here; root
owns the preparation gate and subsequent assignment.

Checked against original BRIEF-04.7 sections1/5,194–240,294–299 and execution
item6. Unknown terminal audit, exact command identity/replay, revision invalidation
and no terminal reopening are retained. The proposal correctly avoids an
unreachable Cartesian product and leaves actual recovered-Applied receipt insertion
covered in reachable expiry/Cancelled histories.

Constructor feasibility follows actual owners: `recovery.rs:42–77` replaces supplied
safety with the shared history predicate; `domain/workflow/recovery.rs:83–118`
prioritizes unknown-effect reconciliation. The receipt path is safe because
`20260930180000_workflow_action_evidence.sql:185–207` skips receipted invocations;
the proof path is safe through `workflow_action_not_applied_available` without a
new dispatch. Terminal-class retirement then selects FinalError; a source with no
final_error route makes `recovery.rs:189–238` return false and settle Failed.
Existing `new_claim:197–207` uses competing real claimants and does not dispatch;
the original entry/barrier therefore remains unconsumed. Provider
`barrier:102–114` permanently closes covered absent entries rather than fabricating
Applied facts. Genuine receipt/proof source histories are preserved.

All fresh terminal checks are specified at the post-owner baseline, with prior
command rows identified by key, exact returned/stored/current generated revision,
linked scoped evidence/coverage and actor audit, required receipt preservation or
insertion, justified whole-public deltas, replay equality, distinct stale refusal
and its replay with zero revision/fact/audit bump, and a subsequent real refused
claim. The contract explicitly corrects the reusable helper's first-command and
zero-prior-receipt assumptions. No production policy, new owner, forced verdict,
applied migration edit, limit raise, overflow implementation or04.8 expansion.

Worker UUID `01a0fc80-6d07-76b2-8835-ec7f98c9a8bf`; constructor milestone
87,863/258,400=34.0% at2026-10-02T12:07:20.348Z, Codex helper depth0.
