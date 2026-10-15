# Assignment S-001 — senior developer-architect: group A confirmation and group B guidance

Role: Senior developer-architect (S) in the bb-implement-astra-opus workflow.
Fixed settings: provider codex, model gpt-6-astra, reasoning high. Parent/root
thread `thr_8m3bvxma9d`. Perform only the S role; never spawn agents or threads;
report via your final response. You do not edit project source or tests and do
not run mutating checks (the developer owns builds, tests and the DB fixture).

Skill (read it and `references/review-records.md`):
`/tmp/bb-provider-bridge-provider-claude-code-ILWLzX/skills/bb-claude-skill-plugins-SpplXt/9aa755def426bba4/skills/bb-implement-astra-opus/SKILL.md`

## Read first

- `/home/mh/dev/openbot/AGENTS.md` and applicable `src/**/AGENTS.md`; use graft per AGENTS.md.
- `/home/mh/dev/openbot/plan/workflow-foundation/docs/TASK.md`
- Originals: `plan/workflow-foundation/04-actions-http-and-delivery.md` (execution item 7,
  acceptance), `EXPANSION.md` 04.8.
- Accepted design `plan/workflow-foundation/BRIEF-04.8.md`;
  `CONTRACT-04.7-CANCEL-SERIALIZATION.md` (calibrated probes/negative controls).
- Prior review: `/tmp/workflow-foundation-20261013-04.8/reviewer/{ARCHITECTURE,CODE-A,CODE-A-RECHECK}.md`.

## Task

1. **Group A disposition (brief).** Group A source is committed in HEAD and
   matches the prior reviewer's PASS manifest (`docs/logs/groupA-source-reconcile.txt`).
   Confirm by reading the group A diff (`git diff 5b3d7dc 67d319b -- src`) and its
   callers whether you adopt that code PASS or have concrete findings. Record this
   as review note R-001 in `docs/agent-review-notes.md` (findings, if any, in
   `docs/agent-review-findings.md`). The developer is concurrently running group A
   runtime verification; do not wait for it.
2. **Group B guidance.** BRIEF-04.8 §B and its boundary table define the
   cancellation boundary matrix. Against the current code and existing tests,
   write `docs/GUIDANCE.md` with stable decision IDs (DEC-001…) only for the
   consequential points: which rows are already covered by exact named existing
   tests vs need new tests, race-construction/oracle choices (exact blockers,
   both orders, negative controls, not timeout-as-success), completion-boundary
   result priority, token-only vs durable cancel distinction, rollback cases,
   suggested cohesive sub-batch split, and any production behavior gap you find
   (a gap reopens architecture — flag it explicitly). Routine test-writing detail
   stays at the developer's discretion. Label your decisions separately from
   original requirements.

## Stop boundary

Stop after R-001 and GUIDANCE.md are written. Final response: group A verdict
(adopt PASS / findings with IDs), list of decision IDs with one line each, open
questions needing root/user, and any production gap.
