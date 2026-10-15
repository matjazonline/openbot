# Assignment D-009 — developer (replacement): group B3 of 04.8

Role: Developer (D2) in the bb-implement-astra-opus workflow; replaces retired
developer thread `thr_6iqmr9bdad` (stopped). Fixed settings: claude-code /
claude-opus-5-5 / high. Root thread `thr_8m3bvxma9d`. Perform only the D role;
never spawn agents/threads; report via final response.

Skill: `/tmp/bb-provider-bridge-provider-claude-code-ILWLzX/skills/bb-claude-skill-plugins-SpplXt/9aa755def426bba4/skills/bb-implement-astra-opus/SKILL.md` and `/tmp/bb-provider-bridge-provider-claude-code-ILWLzX/skills/bb-claude-skill-plugins-SpplXt/9aa755def426bba4/skills/bb-implement-astra-opus/references/review-records.md`.

## Read first (records are authoritative; do not rely on chat history)

- `/home/mh/dev/openbot/AGENTS.md`, applicable `src/**/AGENTS.md`; use graft.
- `/home/mh/dev/openbot/plan/workflow-foundation/docs/TASK.md`, `REVIEW.md`,
  `GUIDANCE.md` (DEC-001..012), `IMPLEMENTATION.md` (your predecessor's record:
  fixture commands, runner conventions, B1/B2 sections), `agent-review-notes.md`
  (R-001..R-009) and `agent-review-findings.md` (F-001..F-005 — learn what
  oracles reviewers reject).
- Originals: `plan/workflow-foundation/04-actions-http-and-delivery.md` item 7 and
  acceptance; `BRIEF-04.8.md` §B table rows "Completion boundary" and
  "Parked/reconciled"; `CONTRACT-04.7-CANCEL-SERIALIZATION.md`.

## State

Accepted artifact: HEAD `8533a79` + worktree per
`logs/groupB/source-manifest-B2r1.json` (SHA256 `1b109cc6…`, 21 files, 3 untracked).
Verify the worktree matches it before editing. Task PG fixture container
`workflow-foundation-048-20261013` is running (127.0.0.1:55448; identity and
credentials in IMPLEMENTATION.md / handoff; Docker via escalated
`newgrp docker <<'EOF' … EOF`). Touch no other DB/container.

## Task — B3 (DEC-007)

Both shared-completion orders (DEC-005: completion winner determines result
priority), SafeRepeat/ProviderIdempotency terminal refusal after cancellation,
exact accepted uncertainty/reconciliation rechecks (DEC-001), including the
applied-then-hung recovery deferred from B1 (R-006 Q3). Per R-008, oracles must
distinguish ordinary completion from ambiguity recovery. DEC-002 race rules
(actual blockers, both orders, negative controls; timeout never passes).
DEC-008: a production gap stops affected edits and returns evidence to root.

Expand in a "Group B3" section of IMPLEMENTATION.md (obligation → exact existing
test or new test), implement, verify at stock 2 MiB on the fixture: new tests
isolated ×3, all B1–B3 tests, full `workflow_action_`, `workflow_control_`, and
the reused reconciliation/uncertainty filters; fmt, diff-check, offline check,
strict Clippy; SQLx prepare/check if queries change. Counts must match `--list`.
Write `logs/groupB/source-manifest-B3.json` (all files changed since `8533a79`,
incl. untracked) and its SHA256. Stop at review-ready B3; final response with
coverage summary, checks/logs, manifest SHA256, open questions.
Not authorized: commits, 04.9, migration squash, deployment.
