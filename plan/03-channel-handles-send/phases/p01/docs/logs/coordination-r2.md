# Plan coordination: 03-channel-handles-send

- Authority: `/Users/mac02/dev/openbot/plan/03-channel-handles-send/COORDINATION.md`
- Writer: `/root`, working in `/Users/mac02/dev/openbot` on `main`.
- Revision: 2
- Workflow: `implement-astra-reviewed-2-2`.

## Plan-wide instructions

- CHS-I001 / r1: Implement original phases 01–07 in order. Accept each phase with independent review before dependent implementation. Preserve the accepted autonomous-agent and knowledge implementations.
- CHS-I002 / r1: No backward compatibility; use fresh disposable databases for incompatible schema changes. Never reset a development or production database.
- CHS-I003 / r1: Root is the author, user-confirmed Sol 6.1/xhigh. Request independent reviewer B as Astra/high with no inherited context. Actual runtime metadata beyond user confirmation is unavailable. No worker may spawn agents.
- CHS-I004 / r1 (superseded by r2): Focused checks per batch; the original plan and repository AGENTS.md require the final full verification gate. Record failures, unavailable checks, and selected/executed test counts. Do not claim deployment or live-provider readiness.
- CHS-I004 / r2 (active): User explicitly instructed targeted tests only. Run affected focused tests and interface/build/migration checks, including final combined affected-scope verification; do not run full suites or claim full-suite verification. This supersedes the original plan/repository default full gate. Record failures, unavailable checks, and nonzero selected/executed counts. Do not claim deployment or live-provider readiness.
- CHS-I005 / r1: Sequential phases share this checkout; freeze all implementation during review. Root owns TASK/HANDOFF and coordination; B owns review records. No commit, push, or deployment authorization is inferred.
- CHS-I006 / r1: After plan03 is implemented and independently accepted, continue with `plan/04-autonomous-agent-history` using the same workflow and targeted-only verification. Do not start dependent plan04 work before plan03 acceptance.

## Phase directory

| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 | `/root` / author | `/Users/mac02/dev/openbot` | `phases/p01/docs/{TASK,HANDOFF}.md` |
| p02–p07 | `/root`, authorized, pending dependencies | `/Users/mac02/dev/openbot` | `phases/pNN/docs/{TASK,HANDOFF}.md` when started |

## Cross-phase notices

None. Register reviewed producer contracts here before dependent phases begin.
