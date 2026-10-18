# Phase 05B user-stop handoff — 2026-10-18

User requested “at safe checkpoint write handoff and stop”. Work is stopped;
resume only after a new user instruction. No backward compatibility: reset task
fixtures when schema changes require it; do not add backfills or compatibility paths.

## Ownership and scope

Workflow: implement-astra-reviewed-2.1. User confirmed root Astra/high author;
independent reviewer `/root/reviewer_05` requested Astra/medium, separate context.
Reviewer completed 05-R009 and is idle. Root owns TASK and serialized checks;
reviewer owns REVIEW, review notes and findings. Scope was 05A → 05B → final phase05
verification. No commits, deployments, production resets or phase06+ authorized.
Base HEAD: `feefe3e060a851fd8860e4bacb90c11c7b284888`.
Preserve all current work, historical failures and review evidence.

## Accepted and unfinished work

05A accepted after 05-R008 CODE PASS, digest
`2a428cd707ec4427ddb31ca9ea60d875ba829c1bb0c4c743ab7f38929d99bef3`.
F011–F013 resolved; D5+D6 architecture accepted under 05-R007.
05B is implemented but not frozen for review, code-reviewed or accepted.
Read TASK, REVIEW, agent-review-notes and agent-review-findings plus D1–D7 in
`logs/05/` before further work. Prior acceptance is historical, not approval of 05B.

Approval configuration supports literal/parameter reviewers and validated dynamic
deadlines. Subjects bind frozen invocation plus argument and operation digests.
Parking closes an attempt as approval_parked without consuming a retry; approval
completes the decision wait, consumes a retry and reopens the job for a fresh attempt.
Park requires remaining retry capacity. Owner/admin eligibility is checked at park
and submission; self-approval allowed. Rejection/expiry use ordinary terminal action
failure. Completed approval waits are distinct from later effect waits. HTTP/MCP/message
outcomes suspend; MCP opens no session before approval. Exact-attempt history and
ambiguous commit handling recognize committed parks. Production runtime notice wiring
remains phase08; tests supply it explicitly.

D7 closes not-applied-proof replay of consumed protected markers: later attempts refuse
another dispatch, protected markers cannot be reused even after policy removal, and a
deferred DB guard refuses a second remote entry. Receipt replay remains allowed.
D7 architecture 05-R009 PASS accepted at SHA256
`d980ffa58aea5eb8f58b7627e2ba4b68506a8912b47f6dc2db72ec30a13ebcf9`.

Core new implementation: `src/adapters/persistence/workflow/decision_approvals.rs`.
New tests there: action_approval_{http,mcp,message,resolution,sql}_tests.rs and
approval_test_support.rs. Shared HTTP/message preparation strips workflow approval
before effect argument validation; http_for_action recognizes exact persisted approval.
Reject/expiry classification uses frozen target resource kind. reserve_remote SQL work
was extracted into reserve_action; decisions::submit_on business completion was
extracted into submit_business_on. Test-only CompletedOutcome handles completed results.

## Verification and stop boundary

`logs/05/05b-focused-sixth.log`: 18 passed, 0 failed. Covers HTTP/tool/MCP dispatch,
competing dispatchers/reviewers, digest mismatch, D7 genuine not-applied proof and SQL
second-entry refusal, message two-wait history, lost-ack applied/aborted parks,
rejection/expiry/demotion/last attempt, registry reviewers and deadline validation.
The final submit_business_on extraction and test tuple cleanup followed this pass;
runtime tests have not been rerun since. D7 task schema reset passed.

`logs/05/05b-gate-status.log` and `05b-gates.sh` record exact commands and evidence:
- PASS formatting, diff, transport boundary, credential/deploy script tests.
- PASS locked offline all-target compile and strict Clippy.
- PASS fresh migration/schema equality and SQLx prepare (cache updated).
- SQLx prepare --check interrupted at user stop; no pass claimed.
- Not run: all-target test no-run, isolated workflow suite, delivery regressions,
  lease budget and graph refresh in this gate.
- Final phase05 full suite and stock-stack gate remain outstanding.

Do not rerun the whole gate script unchanged: it truncates status and overwrites logs.
Use new log names for remaining checks. No full verification or 05B acceptance claim.

## Resources

Stopped gate shell PID26538 and its then-current descendants with SIGTERM after
freezing the parent to prevent advancing checks. Task PostgreSQL stopped cleanly
with pg_ctl -m fast -w stop. Data retained at `/private/tmp/workflow0410-20261017`.
PG18, port55450, role workflow0410, system identifier 7697669855432439122.
Task DB workflow0410_test; comparison workflow0410_fresh_test.
Preserve workflow0410_presquash and workflow0410_postsquash.
URL: postgres://workflow0410@127.0.0.1:55450/workflow0410_test.
Use CARGO_INCREMENTAL=0 and CARGO_BUILD_JOBS=4. Serialize fixture mutations.
No task verification or reviewer work remains active.

## Eventual authorized continuation

1. Reconcile this handoff, TASK, review records and worktree; restart preserved task PG.
2. Complete outstanding 05B gates without overwriting stopped/failed evidence; fix issues
   and audit the whole batch. Follow graft-first discovery and caller tracing rules.
3. Freeze snapshot with exactly `plan/workflow-foundation/docs` excluded, then request
   whole 05B independent code review. Resolve findings, obtain matching PASS and accept.
4. Run final phase05 full/stock verification and combined integration review; stop resources.

Checkpoint manifest: `logs/05/05b-user-stop-20261018-snapshot.txt`.
This WIP snapshot is a resumable checkpoint, not acceptance.

```text
base:   feefe3e060a851fd8860e4bacb90c11c7b284888
digest: 68b1c01bf2e2e9eb6ae5a21f8d712e9129a8be32d99d0be39b9f8b789a3a2d4b
```
