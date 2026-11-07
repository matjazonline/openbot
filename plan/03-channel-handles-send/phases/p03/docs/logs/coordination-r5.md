# Plan coordination: 03-channel-handles-send

- Authority: `/Users/mac02/dev/openbot/plan/03-channel-handles-send/COORDINATION.md`
- Writer: `/root`, working in `/Users/mac02/dev/openbot` on `main`.
- Revision: 5
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
| p02 | `/root`, accepted `aa7cd3e8`, combined integration pending | `/Users/mac02/dev/openbot` | `phases/p02/docs/{TASK,HANDOFF}.md` |
| p03 | `/root`, starting from accepted p02 | `/Users/mac02/dev/openbot` | `phases/p03/docs/{TASK,HANDOFF}.md` |
| p04–p07 | `/root`, authorized, pending dependencies | `/Users/mac02/dev/openbot` | `phases/pNN/docs/{TASK,HANDOFF}.md` when started |

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
- Responses: p02 -> acknowledged r1, accepted; p03 -> acknowledged r1 at startup; p04–p07/final -> pending registration/impact check.

### CHS-C002 / r1

- State: active.
- Origin: p02, root; D1/r2 and `phases/p02/docs/logs/CHS-P02-root-acceptance.json`.
- Targets: p03–p07 and final combined integration.
- Producer: source digest `aa7cd3e8ddd572c6dd7a551cc69a8119769b09b8ace32b567744f4d1fc11ed5e`, HEAD `769a23aa6ef423e964f8221a8ee6b0743325c6ba`, B PASS CHS-P02-B1-R2, architecture PASS CHS-P02-AG1-R1 (D1/r2).
- Contract: trusted pure `channel.no_reply` returns its explicit business value and requires success to `$end`. Execution-owned `ResponseDisposition` is `automatic`/`suppressed`; shared completion stores it atomically and replay reads the immutable value. Failure never suppresses, and child/repeat output cannot select a parent's disposition. Schema-valid empty/null business values remain valid; suppression skips automatic response projection, not business-schema validation. Explicit messages/receipts persist. Compiler8/catalogue1.7, exhaustive retry-safety and three-copy schema preflight updated; no legacy bundle decoder.
- Consumers: p03 finalization must read the actual terminal execution's typed metadata and C001's authentic designated-root link, never output flags or passive contexts. p04 publication consumes only an immutable committed decision under its own claim, never an invented workflow lease. p06 may add bounded authorized status/sample projections; p02 introduces no unguarded reader. No final intent/publisher exists in p02.
- Evidence/review: p02 TASK/REVIEW, immutable R1/R2 source archives/manifests, exact correction bridge, before/after regressions, final SQLx verification addendum and root acceptance.226 distinct affected tests PASS,0ignored; fresh migrations/SQLx and offline lint/fmt/graph PASS; targeted-only, no full-suite claim.
- Responses: p03 -> acknowledged r1 at startup; p04–p07/final -> pending registration/impact check.

### CHS-C003 / r1 — proposed final-effect policy and staging

- State: proposed; dependent edits blocked on p03 architecture PASS.
- Origin/writer: p03, root, CHS-P03-D1/r1 in p03 TASK and immutable decision log.
- Targets: p03–p07 and final integration; extends the p01 settings/snapshot contract while preserving C001 ownership and C002 disposition.
- Proposal: add a bounded optional channel final-reply approval policy using the existing static reviewer/deadline form. Freeze it with the selected responder context; use a typed channel-final action origin and current messaging authority, not an agent tool origin or fabricated workflow lease. Stage exact validated terminal output/content before required approval, then resume the same execution from persisted completion rather than rerun its handler. Final response decisions remain unique per owned root and atomic with terminal success; publication uses a separate bounded claim.
- Compatibility/lifecycle: sole init schema changes and fresh owned databases; no legacy policy decoder/backfill. Policy changes can block stale final publication without redirecting the frozen response or reopening succeeded work.
- Producer identity: accepted p02 `aa7cd3e8` is the baseline; new p03 producer not yet implemented/accepted.
- Responses: p03 -> proposal acknowledged and gate pending; p04–p07/final -> pending accepted producer.
