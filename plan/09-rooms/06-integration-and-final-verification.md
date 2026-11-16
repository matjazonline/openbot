# 06 Integration and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Order

Implement phases 01 through 05 in order after the accepted 01–08 sequence. Integrate all phases
before deploying the feature. Use the reset schema; do not convert old channels, copy existing
direct history into rooms or keep dual formats. All phase check families are proposed tests,
not evidence of implementation today.

## Batch 06A Complete journeys and regressions

Use scripted providers and real database workers in isolated fixtures:

1. Two people and two agents in a room: human mentions Researcher; Researcher publishes findings
   and explicitly addresses Writer; Writer sees those findings and responds; the second human
   replies to Writer without typing a mention. All messages stay in one room thread with correct
   authors. Direct inboxes receive no copies and unmentioned agents do not run.
2. A human mentions both agents in order. Concurrent delivery retries and two workers produce one
   turn per addressed member, run sequentially; the second sees the first reply. Ordinary agent
   reply provenance starts no extra turn. Another thread runs while this one waits for review.
3. Quoted/email/code/attachment mentions and raw tool output start nothing. A human notification
   is deduplicated and cannot start another run. A reply consumed by a workflow question resumes
   that wait exactly once, without also creating a new discussion.
4. Pause/removal/retirement races with preparation, side effects and publication: the winner's
   authority fence holds, no stale reply or notice leaks, and resume/rejoin never revives cancelled
   work. Worker loss, poison batches and deadline expiry release ordering without duplicate runs.
5. Cross-unit members work under room scopes and charge its unit; unavailable resources refuse.
   A lower-level invitee cannot receive over-level history or output. A goal room's runs and
   authorized delegated work appear in 08's rollup. Queued work prevents archive until cancelled
   or terminal, including a competing enqueue at the archive boundary.
6. Agent cycles and branching exhaust shared turns/tokens/deadline without resetting them. Races
   for the last allowance cannot overspend reservations; every provider/native execution path is
   exercised. Existing unit/goal admission caps remain separate and effective.
7. Ordinary desk responders, public-desk/private-agent behavior, direct person privacy, agent
   direct bindings, workflow approvals, transport delivery and account teardown still work.

Use `room_journey_` plus each changed interface's focused regression filters, checking nonzero
test counts and recording commands/results before proceeding to the final gate.

## Batch 06B Final full-suite gate, once after integration

1. Finish init-migration edits using fresh disposable databases; never reset a shared database.
   After SQL changes, run `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets` against the
   migrated disposable database and retain the generated `.sqlx` metadata.
2. Run `scripts/check.sh` with `DATABASE_URL` explicitly set for this task's disposable database
   setup. Record each gate, including failures; compare its entry points with
   `.github/workflows/ci.yml` and execute required gates it omits.
3. Evidence must include formatting, patch whitespace, locked offline all-target compilation,
   clippy, fresh migrations, SQLx metadata, source reachability, the full database-backed suite,
   network isolation, stack/lease budgets, frontend protocols and the required deployment/service
   checks. Keep thresholds unchanged; a raised bound requires the early CI failure signal and
   justification required by AGENTS.md.
4. Rerun affected checks after verification fixes; repeat the full suite if the impact warrants
   it. Mark missing browser, Docker, model or other prerequisites unavailable, never passed. Do
   not claim complete verification with required gates missing or failing.
5. Run `python3 scripts/check-integration-docs.py`, check every new plan/document link, and verify
   all seven plan paths occur exactly once in `NO_BACKWARD_COMPATIBILITY_NOTES`. Run `trail build`
   after the implementation changes and `trail check` before any authorized publication.

## Completion record

| Evidence | Status |
| --- | --- |
| 01 schema, grants, access and competing membership writes | Pending |
| 02 normalized addressing, reply/wait semantics and notices | Pending |
| 03 bindings, actor preservation, ordered turns, recovery and pause | Pending |
| 04 scopes, audience, budgets, goals and competing archive | Pending |
| 05 native UI/browser, SMTP/webhook and documentation checks | Pending |
| Complete journeys and desk/direct regressions | Pending |
| Formatting, offline compilation, clippy and reachability | Pending |
| Fresh migrations and SQLx metadata | Pending |
| Full database-backed suite and network isolation | Pending |
| Stack/lease budgets and remaining required CI gates | Pending |
| Startup plan paths, links and graph refresh | Pending |

Plan-writing checks validate these documents and their startup inventory only. They do not change
the pending implementation evidence above.
