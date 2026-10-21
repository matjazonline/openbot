# Workflow foundation resume

## PHASE06 ACCEPTED — 2026-10-21

Phase06 complete: 06A/06B/06C accepted; final gate PASS; combined review 06-R024 corrections
(spam guardrail removed, prompt generator unmetered, no memory in this release — user decisions
2026-10-21) accepted by 06-R025 PASS, digest 0d8bba5d…9b00. Next phase (07) not started; await
user instruction. No commits/deployments. Phase-08 obligations: see "Phase-08 obligations (from
06C)" below, plus: decide delete-or-dormant for memory-provider infrastructure; ToolId comment
wording (value_objects.rs) vs workflow tool diagnostics. Fixtures: always 127.0.0.1 (localhost
self-connect flakes on port 55450).

## RESUMED — 2026-10-20 (user: "continue"): final phase06 gate rerunning

Final gate rerun started (docs/logs/06/06-final-gate.sh); then freeze, B combined integration
review, report. The stop note below is superseded.

## USER-REQUESTED STOP — 2026-10-20: after 06C acceptance

User explicitly requested "write handoff and stop". The stop is intentional; resume only on new
user instruction (implement-opus-reviewed-2.1; root authors; Reviewer B `a9387720a47e21d5f`).
06B2b, 06B3 and 06C are accepted (06C: 06-R023 PASS, digest dbeb4953…b4d5). The final phase06
gate (docs/logs/06/06-final-gate.sh) was STOPPED on request after fmt/diff/check/clippy/
migrate/sqlx-check passed. The all-target suite, stack budget and lease budget have NOT run, so
rerun the whole script. Then freeze, run the combined phase06 integration review with B, and
report. **Read next:** docs/logs/06/HANDOFF-20261020-06C.md. No checks or agents are running.
All disposable workflow06* DBs were dropped, and the retained cluster and workflow0410_* DBs are
untouched. No commits or deployments; preserve all WIP, including assets/app.css.

## RESUMED — 2026-10-19/20 under implement-opus-reviewed-2.1 (current)

Root authors (user confirmed xhigh; no developer sub-agent). HEAD 76fe99c + uncommitted
06B2b WIP: shared Rig driver bindings, native agent.run preparation/handler/ActionService
tools (R1–R4), message delivery-wait continuations (R5), and accepted gate 06-D5 rev2
(agent-owned workflow capabilities; 06-R015 PASS) implemented with focused proofs (agent
e2e incl. HTTP tool, approval cycle, message delivery continuation, revocation races).
06B2b ROOT ACCEPTED 2026-10-20 (06-R017 PASS, digest d9ea9e59…2ef3). 06B3 ROOT ACCEPTED
2026-10-20 (gate 06-D6 rev2; 06-R021 PASS, digest afbb5ffb…2c3717): context.history,
ai.classify/decision.agent on the shared driver; memory deferred by user decision. 06C
(user decisions 2026-10-20: remove ai-agents, legacy catalogue, skill tool steps,
granted_tool_ids, agent MCP selections; worker wiring + create_agent_channel/
list_company_agents in phase 08) IMPLEMENTED; static/migration/SQLx gates pass
(logs/06/06C-gates.sh). 06C ROOT ACCEPTED 2026-10-20 (06-R022 docs-only F028 fixed; 06-R023
PASS, digest dbeb4953…b4d5; B full lib suite 2701/0). Next: final phase06 gate
(logs/06/06-final-gate.sh) and combined integration review.
Phase-08 obligations (from 06C): create_agent_channel + list_company_agents as workflow tools and
Agent Builder creation (revise its prompt: workflow tools, explicit provider/model); production
workflow worker wiring + refusal of unhandled step kinds; replace scheduled legacy runs with
workflows (no test drives a scheduled run through real Rig today); remove single-value
HarnessKind/HarnessRegistry, legacy AgentRunner, outreach/quorum/ApprovalUseCases machinery and
mcp_connections_by_ids (test-support only) with the legacy binding. Reviewer B handle
`a9387720a47e21d5f`. Read latest docs/TASK.md sections ("06C implementation complete").
No commits/deployments. Legacy Binding is temporary (user: legacy behaviour not needed
after workflows) and is removed with its callers in 06C/08.

## USER-REQUESTED STOP — 2026-10-19: native checkpoint updates

User explicitly requested “write handoff and stop”. Stop is intentional; resume only on
new user instruction. Continue with implement-astra-reviewed-2.1, root authors (no developer
sub-agent), retained independent reviewer /root/reviewer_06. No whole06B2b source review
or acceptance yet.06B2a and D4 revision2 remain accepted; D4 root-pause addendum revision3
accepted under06-R013. Preserve all WIP, especially assets/app.css; no commits/deployments.

**Read next:** docs/logs/06/HANDOFF-20261019-native-updates.md, then latest docs/TASK.md
entries. Latest native harness40/0 PASS at stock2MiB/four threads in
06B2b-checkpoint-updates-tests.log. Native tool/final mutation port and SQL progression
checks now exist; settlement approval can finish without an extra model call. Received
tool result survives sibling refusal; new preparation is refused by Rust and SQL.
These latest SQL/source edits have focused tests only; static/SQLx checks must be refreshed.
Prior scheduling gates passed fully for the preceding artifact, not this latest delta.
Trusted preparation, shared Rig driver, effect/delivery waits and06B3/06C remain outstanding.
No checks are running at handoff. Retained PostgreSQL cluster/evidence DBs remain untouched.


## Phase06 / 06B2b root-pause scheduling — 2026-10-19 (current)

Continue under implement-astra-reviewed-2.1; root authors, no developer child.
User did not request a stop.06B2a/D4 revision2 remain accepted; D4 root-pause
revision3 addendum accepted under06-R013, reviewer /root/reviewer_06 retained.
06B2b remains unfinished and unreviewed. Token22/0 PASS at stock2MiB/four threads
in06B2b-token-scheduling-tests-8.log. Pending I/O and pure work defer without debit;
processing interruption consumes one failure, last retry terminalizes; denial,
deadline, competing claims/no ancestor run lock, native approved-request repark,
notification/approval commit rollback, SQL bypass and reviewer revocation proved.
Pure activation now distinguishes Ready/Deferred/Settled and returns an explicit
BatchDisposition::Deferred with no immediate continuation. Prior failed fixture and
compile logs retained. Affected regressions/static/fresh migration/SQLx checks are
running via docs/logs/06/06B2b-token-scheduling-gates-resume.sh; inspect logs before claiming.
113 scheduling regressions passed; after hard-limit diagnostic preservation, budget-runtime9
and action-uncertainty10 pass, offline all-target check54.19s passes. Strict Clippy/fresh
migration/SQLx remain in progress. Initial Clippy Copy-clone failure was corrected.
No full phase gate or whole06B2b source acceptance. Next finish remaining token
adversarial coverage as needed, then trusted preparation/shared Rig driver/native
checkpoint tool/final mutations/effect waits,06B3/06C and final full verification.
Preserve all WIP/assets/app.css, PostgreSQL retained cluster and evidence databases.
No commit/deploy or production reset authorization. Do not stop at internal checks.


## Phase06 / 06B2b token protocol in progress — 2026-10-19 (current)

Continue under implement-astra-reviewed-2.1, root authors per explicit no-developer-child
instruction. No stop requested; do not end at internal test boundaries.06B2a and D4
revision2 remain accepted;06B2b remains unfinished and unreviewed. New atomic model
admission/settlement and token proposal/approval/denial/expiry plumbing are implemented
as WIP. Native checkpoint tests19/0 passed, including four token lifecycle/replay tests.
Root refusal now typed; activation/recovery/new effect entry/final completion check it,
and SQL guards cover new effects/completion/reopen. Latest focused run initially3/3:
SQL reopen guard incorrectly rejected a transient running state during terminal decision
settlement; changed to deferred final-state check. Child test reused an admission key;
corrected. Rerun06B2b-token-refusal-tests-2.log is in progress at this checkpoint.

Next: finish token SQL/descendant/expiry/cancellation/repark/race/rollback proofs, then
trusted preparation/shared Rig driver/effect waits. Whole06B2b review only after full
cohesive implementation, followed by06B3/06C and final full gates. No full verification
or operational native model handler claimed. TASK carries evidence; preserve WIP,
assets/app.css, retained PostgreSQL and prior failed logs. No commits/deployments.


## Phase06 / 06B2b active request ownership — 2026-10-19 (latest)

Continue under implement-astra-reviewed-2.1, root authors per explicit user instruction
(no developer sub-agent); fresh independent Astra/medium reviewer remains needed for
whole06B2b. Existing06B2a and D4 revision2 acceptance remain intact. Native checkpoints
now carry an exact active request/generation/worker/attempt slot with relational FK,
SQL admission/settlement guards and ordered append-only model ledger. New claimed
execution reservation releases a retired slot without refunding/deleting its request;
a newer owner cannot settle the old request or admit its ID again.
Three focused owner/race/recovery tests join nine prior native tests. Latest commands,
results and progress artifact are in docs/TASK.md. No06B2b review or acceptance, full
phase verification, provider-entry permit or operational native model handler claimed.
Next: atomic request reservation/usage/root model-call debit, token proposal/approval/
refusal/settlement, trusted preparation/shared Rig driver/effect waits, then06B3/06C.
Preserve all WIP and retained PostgreSQL evidence. No commits/deployments/user stop.


## Phase06 / 06B2b native checkpoint ownership/store — 2026-10-19 (latest)

Continue under implement-astra-reviewed-2.1. User confirmed current root high in
response to the Astra/high role question and reiterated no backward compatibility;
use fresh/reset databases, no old-data support.06B2a and D4 revision2 remain accepted.
Native task_harness_runs ownership now has disjoint legacy/workflow shapes, exact
run/execution/job/attempt FKs, immutable preparation and live-fence SQL write checks.
Application-owned WorkflowCheckpointStore implements native open/load and execution
reservation using real WorkflowFence, current actor authority and bounded transactions.
Shared decode/state validation is reused; native writes never project legacy invocations.
Nine native schema/store/race/recovery/rollback tests and ten current-caller checkpoint
regressions pass at stock2MiB. See latest TASK for final static/migration/SQLx results.

This remains unfinished06B2b, NOT independently reviewed or accepted. The preparation
payload is only a bounded immutable document at this storage boundary; trusted runtime
preparation, frozen tool/target bindings and live capability checks still need wiring.
Next: active model-request owner slot; atomic reservation/usage/root model-call debit;
root token proposal/approval/refusal and settlement; shared Rig driver/effect waits;
then06B3/06C and full phase verification. No operational native model handler is claimed.
No commits/deployments or user stop. Preserve all prior WIP and retained PostgreSQL
pre/post squash evidence. Fresh independent Astra/medium reviewer is still needed for
the whole06B2b artifact; no reviewer was spawned for this internal implementation step.


## Phase06 / 06B2b admission continuation — 2026-10-19 (latest)

Continue under implement-astra-reviewed-2.1. User reconfirmed current root Astra/high.
06B2a and D4 revision2 remain accepted. Fresh root and descendant admissions now check
live owner/admin/person authority for the actual root token reviewers; children read
inherited immutable policy without locking the root run. Saved admission replay creates
no new work and retains its existing behavior. Focused admission22/0, locked offline
all-target compilation and strict Clippy passed; latest TASK has schema/SQLx results.
All prior WIP is preserved. Native checkpoint/store, token continuation/refusal, shared
Rig driver and effect waits remain unfinished, as do06B3/06C and final phase verification.
06B2b is not independently code-reviewed or accepted. No commits/deployments, no user
stop request, and no new authorization needed. Historical reviewer handles are not live
in this session; spawn a fresh independent Astra/medium reviewer when the batch is ready.


## Phase06 / 06B2b implementation in progress — 2026-10-19

Continue under implement-astra-reviewed-2.1; root remains user-confirmed Astra/high
and independent reviewer /root/reviewer_06 is retained.06B2a is accepted.06-D4
revision2 is root-accepted after06-R012; no architecture permission is pending.
06B2b source changes are WIP, not code-reviewed or accepted. Shared checkpoints now
retain bounded excess reported usage and stop on hard token overflow. Optional root
token policy compiles/freezes with explicit reviewers; compiler4 and SQL admission
match. Agent snapshots carry trusted UUIDs; agent.run reuses action approval inputs.
The native workflow owner/checkpoint/driver, root token decisions and continuation,
model-tool delivery waits,06B3/06C and final phase verification remain unfinished.
See latest docs/TASK.md for checks and exact next work. Preserve all existing changes
and task PostgreSQL evidence; no commits/deployments. No user stop was requested.


## Phase06 resumed; usage requirements resolved — 2026-10-19 (current)

User authorized phase06 continuation under implement-astra-reviewed-2.1 and confirmed
root as Astra/high author.06A (pure frozen capability contract) is accepted after
independent architecture06-R001 and whole-batch code06-R002 PASS. Ordered saved defaults,
explicit profile replacement, workflow capability ceilings, requirement enforcement,
trace-ready snapshots and pure current-revocation checks are implemented.96 focused
tests, formatting, locked offline all-target compilation and strict Clippy passed.
No operational agent handler or full phase06 completion is claimed.

User resolved the budget choice: measure agent/workflow model calls and tokens, display
analytics by provider/model or workflow, and calculate cost when an optional per-million
provider/model price is configured. No monetary caps. An optional workflow token limit
pauses automated progression for manual approval to continue, preserving usage and saved
execution state. Continue06B architecture and implementation under these requirements;
there is no pending budget-choice blocker.06B durable Rig/preparation and06C migration/
alternate-runtime removal, plus final full verification, remain outstanding.
Read latest [TASK](docs/TASK.md) and [REVIEW](docs/REVIEW.md) for exact accepted artifact,
evidence, roles and next action. No DB started/reset, no commits/deployment. Preserve
all current work and retained task PostgreSQL/pre/post squash evidence. This section
supersedes older phase scope/status below.

## Phase05 verification complete — 2026-10-18 (current)

05A and 05B passed independent review and their batch gates. Final corrected all-target
suite passed at 2 MiB: 2727 library + 5 binary tests, zero failures, 22 existing ignored.
Lease118/0, formatting, offline compilation, strict Clippy, fresh migration/schema,
SQLx and transport checks passed. Earlier failed/interrupted evidence is retained.
The final fixture correction and combined-integration review, artifact digest, root
acceptance and resource status are authoritative in [TASK](docs/TASK.md) and
[REVIEW](docs/REVIEW.md). Production runtime composition/member routes remain phase08;
phase07 owns child feedback/timeout-code mapping. No commits or deployments.
Next implementation phase is06, only under a new user instruction. Preserve the task
PostgreSQL data and pre/post squash evidence databases described in TASK.

## RESUMED — phase 05B, 2026-10-18 (historical)

User authorized continuation under implement-astra-reviewed-2.1. Root retains
confirmed author role; resume unfinished 05B gates, independent review and final
phase05 verification. See latest docs/TASK.md. Prior stop is superseded.

## USER STOP — phase 05B, 2026-10-18 (historical)

Stopped at the user's safe-checkpoint request. Read the
[stop handoff](docs/HANDOFF-05B-USER-STOP-20261018.md) and latest [TASK](docs/TASK.md).
05A accepted (05-R008); D7 architecture accepted (05-R009). 05B implemented but
not independently code-reviewed or accepted. Focused tests 18/0 passed before the
last helper extraction; static/offline/Clippy/schema/SQLx prepare passed afterward.
SQLx check interrupted; remaining batch and final verification outstanding.
Task PostgreSQL stopped cleanly, data retained; verification process tree stopped.
No reviewer work running. Preserve all WIP and pre/post squash evidence databases.
Continue only on a new user instruction. No commits, deployment or phase06+.

## RESUMED — phase 05B implementation, 2026-10-18 (historical)

User authorized continuation with `implement-astra-reviewed-2.1` and confirmed root as
Astra/high author. Scope: 05A → 05B → final phase05 verification; no commits, deployment
or phase06+. 05A is root-accepted after 05-R008 CODE PASS; D5+D6/05-R007 accepted.
05B is WIP, not frozen or reviewed. Third all-target offline compile and fresh task-test
schema migration passed; first approval integration tests are being added and debugged.
Read latest [TASK](docs/TASK.md), [REVIEW](docs/REVIEW.md), and D1–D6 in `docs/logs/05/`.
Independent reviewer `/root/reviewer_05` retained. Task PostgreSQL
`/private/tmp/workflow0410-20261017`, port55450, running; root owns serialized checks.
Preserve pre/post squash evidence databases. Use `CARGO_INCREMENTAL=0` for builds.
Historical stops below are superseded.

## USER STOP — phase 05A in progress, 2026-10-18 (historical)

**Stopped at a safe checkpoint.** Read the [handoff](docs/HANDOFF-05A-USER-STOP-20261018.md).
Batch S (migration squash) accepted (S-R003). Phase 05 architecture gate accepted
(05-R004, decision D1–D4 in `docs/logs/05/`). 05A is partially implemented, compiles,
mostly untested and unreviewed: WIP digest `c90f78c3…f52d` on base `feefe3e`. Fixture
`/private/tmp/workflow0410-20261017` stopped, data retained; test databases need a reset.
No processes running. No commits, deployment or phases 06+. Continue only after a new
user instruction.

## Accepted — 04.11 and phase04 complete, 2026-10-18 (historical)

**04.11 VERIFIED and root-accepted** under implement-opus-reviewed-2.1 after independent
architecture gate (04.11-R005 PASS, decision D2+D3+D4 in `docs/logs/04.11/`) and
whole-batch/final-integration review (04.11-R007 PASS). Digest
`e76ef46c07965bb7b233d21813ac413efb6c5ad763e207ac3bc242168235c2cf` on base `5c1de32`
(excluding `plan/workflow-foundation/docs`). Combined phase04 gate passed: full suite
2674/0, stack budget 2669/0, lease budget 99, schema/SQLx/clippy/fmt/transport/graft.
Gaps: CI frontend steps, queryid probe, stack-frames.sh, F018 baseline rerun not run.
Fixture `/private/tmp/workflow0410-20261017` stopped, data retained. Records: latest
[TASK](docs/TASK.md) / [REVIEW](docs/REVIEW.md) entries. Next: phase 05+ only under a new
instruction; no commit, deployment or migration squash performed by the workflow.

## USER STOP — 2026-10-17 (historical)

**Stopped at a safe checkpoint.** Read the [current handoff](docs/HANDOFF-04.11-USER-STOP-20261017.md).
04.10 is accepted (R005 PASS); 04.11-D1 architecture review is incomplete
(R001, NO PASS), with provisional F001–F004 saved. No04.11 implementation.
User chose users/channels and rejection of groups. Reviewer stopped; task PostgreSQL
at `/private/tmp/workflow0410-20261017` stopped cleanly, data retained; no author
checks running. Worktree and all evidence preserved. Combined full-suite/stock/lease
verification remains pending. Continue only after a new user instruction.

## 04.10 accepted; 04.11 next — 2026-10-17 (historical, before latest stop)

User renewed continuation under implement-astra-reviewed-2.1 and confirmed root as
Astra/high author. Scope remains04.10 then04.11; no commits, deployment, migration
squash or phases05+. Preserve all existing work. Task fixture at
`/private/tmp/workflow0410-20261017` (port55450) is running for serialized checks.
04.10 is root-accepted after independent R004 whole-batch inspection and R005
correction/evidence PASS; no blocking finding remains. Accepted digest
`2be42e3e5d8b0651ad1d6bc523cf356dd6bb55ac254050b99914bede669ebc4c`
(before this administrative status edit). Focused504/0, correction12/0 and
scoped stock393/0 passed, as did fresh migrations/schema, SQLx and all static gates.
Prior failed logs are preserved. See latest TASK/REVIEW for exact evidence.
04.11 investigation is recorded in `docs/logs/04.11/investigation.md`; no architecture
gate or implementation yet. User confirmed users/channels and rejection of groups.
Combined full-suite/stock/lease verification remains required after04.11.

## 04.10 USER STOP — 2026-10-17 (historical)

**04.10 incomplete; stopped at the user's checkpoint request.** Read
[stop handoff](HANDOFF-04.10-USER-STOP-20261017.md) before resuming. User authorized
04.10 then 04.11 under implement-opus-reviewed-2.1. Architecture gate accepted at decision
level (D2 as amended by D3, independent PASS 04.10-R003); implementation is written but
unverified: all-target compile not yet clean, no tests run, not frozen or code-reviewed.
Open finding 04.10-F012 must close before batch acceptance. Task fixture
`/private/tmp/workflow0410-20261017` (port 55450) stopped, data retained. 04.11 not started.
No commits, deployment, migration squash or phases 05+. 04.9 entry below is accepted history.

## 04.9 crash recovery — 2026-10-17 (accepted; historical)

User authorized continuation and confirmed root as Astra/high author under
implement-astra-reviewed-2.1. Starting HEAD is `e634a82837d50e7ed4aba4bb003f0a98ec294b10`,
containing the previously uncommitted 04.9 implementation. Preserve pre-existing
edits to `.claude/helpers/graft-hooks.cjs` and `graft-statusline.cjs`.
Earlier Linux fixture and ignored check logs are unavailable on this Mac;
old pass summaries are historical and verification is being rebuilt.

PostgreSQL 18.6 installed at the user's request. Its default data symlink points
to an unavailable external volume and is preserved. New task-only fixture:
`/private/tmp/workflow049-crash-20261017`, localhost port55449, role `workflow049`,
database `workflow049_test`, system identifier `7697645681053396076`.
Root owns serialized tests; `docs/logs/04.9/crash-gates.sh` records commands and
`crash-gate-status.log` records outcomes. Architecture D2/R002 is accepted, and
independent whole-batch code review R003 passed with no new findings.
The current verification, final acceptance, artifact and resource status are in
the latest [TASK](docs/TASK.md) and [REVIEW](docs/REVIEW.md) entries; they supersede
all pre-crash statuses below. Scope is 04.9 only; later phases need a new instruction.

## 04.9 RESUMED — 2026-10-17 (historical, pre-crash)

User authorized continuation under implement-astra-reviewed-2.1 and confirmed root
as Astra/high author. Root restarted the identified retained fixture, resumed HTTP
verification, added credential inventory/rotation/CAS and exact-resource
binding/admission tests, and corrected scalar secret redaction.
Read the latest 04.9 entries in [TASK](docs/TASK.md) and [REVIEW](docs/REVIEW.md)
for the authoritative verification, review, artifact identity, acceptance and
resource status. Earlier stop and partial-check entries below are historical.
Preserve all WIP. Scope is 04.9 only; later phases require a new instruction.

## 04.9 USER STOP — 2026-10-17 (superseded by continuation above)

**04.9 incomplete; stopped at the user's next secure checkpoint request.** Read
[stop handoff](HANDOFF-04.9-USER-STOP-20261017.md) before resuming. Architecture
04.9-D2 passed independent review04.9-R002 and root accepted that gate only;
implementation has not been code-reviewed or accepted. Earlier focused HTTP
suite11PASS predates latest additions; expanded check was stopped during compile
(status143), so latest source and full gates remain unverified. HTTP rotation-row
coverage is still needed. Task fixture `workflow-foundation-049-20261017` is stopped
with data retained; no author-owned checks remain running. Preserve all WIP.
Snapshot/evidence are linked in handoff and TASK. New instruction required to resume;
no later phases, commit, migration squash, deployment or host firewall mutation.

## Accepted — 04.8 complete, 2026-10-16

**04.8 VERIFIED and root-accepted** under bb-implement-astra-opus. Records:
[docs/TASK.md](docs/TASK.md) (decisions, workers), [docs/REVIEW.md](docs/REVIEW.md)
(acceptance per batch A, B1–B3, C), [docs/GUIDANCE.md](docs/GUIDANCE.md)
(DEC-001..014), [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md) (evidence),
review notes R-001..R-017 and findings F-001..F-009 (all resolved).
Final artifact manifest `docs/logs/groupC/source-manifest-C2.json`
(`5f818107…`): HEAD `94c77e0` plus two uncommitted test edits (F-007, F-009).
Gaps: CI network-isolation wrapper not run locally. Task PG fixture container
`workflow-foundation-048-20261013` stopped, data retained. Next: 04.9 only under a
new instruction; no migration squash or deployment authorized.

## 04.8 checkpoint — 2026-10-14, USER STOP (superseded by the entry above)

**04.8 incomplete; group A implemented and independently code-reviewed, required
verification pending.** Read [current handoff](HANDOFF-20261014-04.8.md) and
[continuation](CONTINUATION-20261013-04.8.md). Public cancellation/action lock-order
correction and competing-operation tests are in uncommitted source. Independent
architecture and corrected code PASS; no root acceptance of group A or full04.8.
Resume first unverified group A checks before dependent boundary-matrix work.
Preserve all WIP and the identified task-owned fixture. No04.9 or migration squash.
The user requested handoff and stop; further work requires a new instruction.

## Accepted foundation — 04.7 complete, 2026-10-12

**Current user-authorized04.7 VERIFIED and root-accepted.** Independent final
actual-code/criteria/integration PASS; stock library2532PASS/0FAIL/22unchangedignored,
lease87PASS/max35.7%<50%, all required static/SQLx/fresh53schema/graft gates PASS.
Read [final handoff](HANDOFF-04.7-COMPLETE-20261012.md) for exact evidence, artifacts,
retained historical failure and scope dispositions. Its stop and macOS resource
records are historical; the 04.8 handoff above owns the current resumption state.

## Remaining work and retained documents

04.8 is accepted. Current04.9 status is recorded in TASK/REVIEW as linked above.
Later phase04 items and phases05–10 remain outside this continuation.

- [README](README.md), numbered phase plans and [EXPANSION](EXPANSION.md) define
  the roadmap; [REPLACEMENT-MAP](REPLACEMENT-MAP.md) records replacement gates.
- Briefs and contracts retain subsystem decisions and acceptance requirements.
  Their older pending statuses do not override the current handoff.
- [Phase 10](10-verification-and-cutover.md) retains the mandatory SQL A/C
  reassessment; [NOT_PLANNED](NOT_PLANNED.md) retains exclusions.

Superseded checkpoint logs and unreferenced completed-work evidence were removed
on 2026-10-15. Their tracked versions remain available in Git history, for example
`git show HEAD:plan/workflow-foundation/PROGRESS.md` before committing this cleanup,
or `git log --all -- plan/workflow-foundation/PROGRESS.md` afterward.
