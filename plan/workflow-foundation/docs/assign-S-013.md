# Assignment S-013 — senior (replacement S3): onboard for C1 and group C review

Role: Senior developer-architect (S3); replaces retired `thr_xku2jgixtv` and
`thr_4pqv29gbbd` (context). Fixed settings: codex / gpt-6-astra / high. Root
`thr_8m3bvxma9d`. Only the S role; never spawn agents/threads; report via
final response. Read-only on source/tests/fixtures; you own GUIDANCE.md,
agent-review-notes.md, agent-review-findings.md (append; keep IDs stable).

Skill: `/tmp/bb-provider-bridge-provider-claude-code-ILWLzX/skills/bb-claude-skill-plugins-SpplXt/9aa755def426bba4/skills/bb-implement-astra-opus/SKILL.md`, `/tmp/bb-provider-bridge-provider-claude-code-ILWLzX/skills/bb-claude-skill-plugins-SpplXt/9aa755def426bba4/skills/bb-implement-astra-opus/references/review-records.md`.

Read: `/home/mh/dev/openbot/AGENTS.md`; in `plan/workflow-foundation/docs/`:
TASK.md (incl. decisions log), REVIEW.md, GUIDANCE.md (esp. DEC-008..014 and the
group C checklist), agent-review-notes.md (R-010..R-012 in full; skim earlier),
agent-review-findings.md (F-001..F-006), IMPLEMENTATION.md "Group C" section;
originals `04-actions-http-and-delivery.md` item 7 + acceptance, `BRIEF-04.8.md`.

D2 (`thr_2n9nrfy6ra`) is implementing batch C1 now (`docs/assign-D-011.md`):
DEC-013 precision fixes, F-006 production fix, DEC-014 dispatch-test
stabilization, then full-suite/budget reruns on a now-quiet host. Do not review
the in-progress worktree yet. Your task now: onboard, then pre-read the code
affected by F-006 (`src/adapters/persistence/workflow/waits.rs`,
`src/application/workflow/waits.rs`, callers) and the DEC-014 tests so the C1
review is efficient. Record R-013 (onboarding; no verdict). Final response: ready
status and anything root/D2 should know before the C1 review.
