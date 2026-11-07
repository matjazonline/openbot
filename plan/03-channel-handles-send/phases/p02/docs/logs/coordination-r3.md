# Plan coordination: 03-channel-handles-send

- Authority: `/Users/mac02/dev/openbot/plan/03-channel-handles-send/COORDINATION.md`
- Writer: `/root`, working in `/Users/mac02/dev/openbot` on `main`.
- Revision: 3
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
| p01 | `/root`, accepted `0995151e`, combined integration pending | `/Users/mac02/dev/openbot` | `phases/p01/docs/{TASK,HANDOFF}.md` |
| p02 | `/root`, starting from accepted p01 | `/Users/mac02/dev/openbot` | `phases/p02/docs/{TASK,HANDOFF}.md` |
| p03–p07 | `/root`, authorized, pending dependencies | `/Users/mac02/dev/openbot` | `phases/pNN/docs/{TASK,HANDOFF}.md` when started |

## Cross-phase notices

### CHS-C001 / r1

- State: active.
- Origin: p01, root; D1/r3 and `phases/p01/docs/logs/CHS-P01-root-acceptance.json`.
- Targets: p02–p07 and final combined integration.
- Producer: source digest `0995151ed93e67f2e5feee8092cdc08687580b79f1d988d40d5d072d5e39decf`, HEAD `769a23aa6ef423e964f8221a8ee6b0743325c6ba`, B PASS CHS-P01-B1-R2, architecture PASS CHS-P01-AG2-R1.
- Contract: `ChannelReplies` configures a version1 bounded subject/body projection and explicit pinned responder with CAS. `channel_reply_contexts` is a passive immutable ingress snapshot; only `channel_reply_runs` established under the real message claim grants a designated root obligation. Never finalize/publish by enumerating passive snapshots. Current activity/authority still gates admission. Inactive configured responders retain snapshots/refusal diagnostics; explicit clear, quiet ingress, trigger mismatch and consumed/deferred human replies confer no automatic authority. Selection rechecks current run-as private-channel visibility. Source/run/company retention guards preserve snapshots/ownership.
- Dependencies/order: p01 accepted before p02; later phases consume actual accepted source, not status alone. Preserve inherited source and inspect affected contracts if changing this producer.
- Evidence/review: p01 TASK, REVIEW, immutable R1/R2 source archives and manifests, final author-check receipt and root acceptance.
- Verification: 8 contract +19 binding cases, relevant regressions and both actual competing-claimant protocols; two onboarding failures reproduced at clean original HEAD remain unrelated, no full-suite claim.
- Responses: p02 -> acknowledged r1 at startup (TASK); p03–p07/final -> pending registration/impact check.
