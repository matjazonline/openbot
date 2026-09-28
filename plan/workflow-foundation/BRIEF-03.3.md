# 03.3 — bounded pure transactional batches

Status: implementation and independent final verification complete2026-09-28 13:33Z;
root acceptance pending. Latest verification checkpoint below supersedes historical
implementation/capacity handoffs.
Original authority:03-durable-runtime-and-persistence.md Execution and commits
item2, retaining its nested execution/failure/acceptance requirements.03.1/03.2
are accepted. Scope is pure `data.map` and `decision.rule`; no worker loop or I/O.

## Reconciled implementation contract

- Application owns a narrow batch port, validated batch budget, and synchronous
  pure handler. Mapping returns the frozen `value`; ordered rules return declared
  `choice` and frozen `data`. Reuse compiler output validation and domain routing;
  unsupported step types produce a boundary result, never an invented handler.
- Decision predicates are stored separately from `with` bindings in the compiler.
  Extend activation to freeze their selected choice alongside ordinary inputs,
  include their referenced dependencies in bounded context loading, and persist
  the choice atomically with the existing activation tuple. Replays never evaluate
  predicates again. Additive immutable choice column/guard; no changes to applied
  migration130000. This is an integration extension to03.2, requiring its tests.
- One run-first transaction owns pure advancement, then execution and job locks.
  Only the exact scoped pending, due, unleased workflow job may start advancement.
  A terminal/completed duplicate returns saved progression without following its
  successor and executing another batch. A processing/leased or foreign job fails
  closed.03.4 will integrate fenced I/O ownership separately; a batch call is not
  an external-effect permission or lease claim. No task_attempts parallel ledger.
- Add run state matching domain semantics (`queued`, `running`, `waiting`,
  `succeeded`, `failed`, `cancelled`, with waiting reason when relevant), immutable
  execution route/target and scoped successor identity. Admission defaults queued;
  first batch starts running. End route marks succeeded (business rejection is
  still successful execution), records terminal execution, and validates workflow
  output schema. Existing terminal/waiting runs cannot advance. Error/retry/final
  error routing is not fabricated here: a failed preparation/validation rolls the
  transaction back; classified durable failure/backoff belongs to03.5/03.9.
- Each pure completion writes inputs/output/selected route, completes its existing
  job, and allocates at most one successor execution with next run-wide ordinal
  and an ID-only background_tasks job, all inside the same transaction. Add scoped
  database constraints for successor relationships, immutable route facts, and
  one workflow job per logical execution. Distinct worker attempts reuse the job.
  A compact run audit event uses the existing workflow_run_events owner and saved
  admitting actor; no new audit ledger.03.5 generalizes this atomic progression
  seam to non-pure/fenced outcomes; it must not create a competing commit path.
- Stop after a validated positive per-call step cap (hard ceiling64) or bounded
  elapsed transaction work budget; bound lock/statement waits too. Each value obeys
  the admitted context/output limits, so aggregate batch work is bounded. The next
  pending successor job is the continuation, including at an I/O boundary; no
  recursive call/spin or auxiliary queue. Return typed completed/yielded/boundary/
  replay disposition with identifiers, not serialized context in jobs.
- Recheck deadline at writes/final transaction completion; enforce max_steps
  before allocating successors. No replenishment on retries. Parent-child wakeups,
  waits, polling fairness, worker heartbeats and poison backoff retain their later
  owners. Public API wiring/cutover is outside03.3.

## Affected seams and acceptance

Application workflow batch module; persistence workflow batch/commit helpers;
activation application/adapter extension; additive migration newer than130000;
isolated DB batch tests and affected activation fixture updates. Existing graph
entry points: activation.rs activate_on33–87/resolve106–144; application activation
input_dependencies57–107; compiler rule88–90; domain OrderedRule::decide47–61,
select_route29–62, RunState start/apply. Caller tracing found activation helper
used only by its port and focused tests; input_dependencies only by its resolver.

Required checks:

1. Map→ordered rule→map/end resolves committed outputs and records every input,
   output and route. First matching predicate/default selection and JSON null;
   terminal workflow output validation. Frozen rule choice survives changed
   available predecessor context, lost acknowledgement and replay.
2. Small budget yields exactly one pending continuation with ID-only payload;
   later batch resumes it. Non-pure boundary stays pending and unexecuted.
   Duplicate original job replay does not execute that continuation.
3. Two synchronized independent DB callers cannot double-complete or create extra
   successor/audit records. Run-first contention deadline expiry fails closed.
   Reject foreign/mismatched/leased/not-due/terminal run requests without writes.
4. Inject failure after activation, output, audit and successor staging; full
   rollback restores original job and allows clean retry. Lost commit response and
   reconnect return saved progression. Direct SQL rejects route/choice mutation,
   foreign successor and duplicate logical job. Bounds cover invalid/zero/excessive
   batch cap, run deadline/max_steps, invalid output, and repeated step ordinals.
5. Applied migrations immutable; task-only PG retained on55439. Run isolated
   migration, SQLx prepare sequential with builds, stock2MiB focused activation+
   batch tests, mandatory full DB library suite, locked offline all-target check
   and Clippy, fmt/diff, graph refresh, then independent actual-code/combined review.
   Preserve all existing edits and external index.html work; no stage/commit/reset.

Root owns PROGRESS/RESUME acceptance. Implementer owns this brief. Implementation
pauses during actual review.

## Retirement handoff

Root chose rotation after expansion because completed03.2 context dominates.
No03.3 source or migration edits were made; only this expansion was added.
Retired implementer `/root/activation_gates`, UUID01a0e810-4390-7870-8bae-ce6d69530aac,
latest expansion sample89646/258400=34.69%2026-09-28T12:58:40.680Z,
token_usage_record.usage/task_started.model_context_window. Nested reviewer
`/root/activation_gates/combined_review` UUID01a0e810-a6dd-7eb0-b913-6db615622a85
is completed/quiescent (verified by list_agents); do not reuse retired subtree.

Task PostgreSQL RUNNING at `/private/tmp/workflow-admission-pg-e3aa`, port55439,
database `workflow_admission`, socket `/private/tmp`, max_connections200. Explicit
DATABASE_URL and TEST_DATABASE_URL both
`postgres://mac03@127.0.0.1:55439/workflow_admission`. No test/build/prepare jobs
remain running. Applied migrations through20260928130000 are IMMUTABLE. Fresh
implementer takes resource ownership. Reuse accepted03.2 evidence in BRIEF-03.2
except checks invalidated by03.3 edits. Preserve terminal replay and full rollback
semantics;03.9 poison backoff remains unimplemented, not implicitly promised.

## Capacity blocker after handoff

At 2026-09-28T13:00Z fresh implementer `/root/pure_batches`, session UUID
`01a0e819-f790-7232-9863-4a7778cc7071`, attempted the required nested
`gpt-6-astra`/medium reviewer with `fork_turns="none"`. The collaboration service
rejected creation with `agent thread limit reached`. No source edits or builds
were made. No close-agent control was available; no retry or self-review
substitution was attempted. 03.3 remains expanded, unimplemented, and blocked
pending a fresh session with reviewer capacity. Accepted 03.2 evidence remains.

Cleanup supersedes the RUNNING resource status above: task-owned PostgreSQL at
`/private/tmp/workflow-admission-pg-e3aa` was stopped with
`pg_ctl -D /private/tmp/workflow-admission-pg-e3aa -m fast -w stop` (escalated
after sandbox signal denial). The command confirmed `server stopped`; its data
directory is preserved for resumption. No test/build/prepare jobs were started.

## Implementation checkpoint — 2026-09-28 13:25Z (UNVERIFIED)

Root requested early rotation at the natural correction boundary near50% context.
03.3 source is implemented and independently reviewed; acceptance is NOT complete.
Do not restart discovery or discard this work. No next point was started.

Changed implementation: application `workflow/batch.rs` adds validated positive
1..64 step /1ms..5s work budgets, one additional second for hard transaction finish,
pure map/rule output validation and domain state/route decisions. Activation freezes
rule choice with inputs and loads rule predicate dependencies. Persistence
`workflow/batch.rs` locks run→execution→job, checks exact pending/due/unleased jobs,
replays completed jobs without advancing successors, and yields one pending ID-only
continuation. `batch_commit.rs` owns atomic output/route/state/audit/successor writes.
Applied additive migrations133000 and133100 add run state, immutable choices/routes,
scoped successor identity, unique execution job/audit, and deferred commit deadline.
**Migrations through20260928133100 are now immutable.**

New `batch_tests.rs` and `batch_recovery_tests.rs` cover15 cases. Old association
fixtures now remove each completed test insertion between independent races; retained
history index fixture creates one actual execution per job (12000 distinct rows).
No application guards or constraints were weakened to preserve old fixtures.
Unrelated deleted index.html/marketing images and untracked website are preserved.
No commit/stage/reset/deploy performed.

### Review and current correction

Independent reviewer `/root/pure_batches/reviewer` (UUID
`01a0e81e-8fbb-7e61-adb9-6e8dbdd5e5d6`, Astra/medium) inspected actual code,
locking/callers/schema/tests. Main review: no implementation defects found, two
required test corrections: missing correlation_id in changed-context replay fixture;
foreign-successor negative masked by preexisting immutable route. Both corrected:
job now SELECTs association/correlation from scoped run; new first-link test creates
actual same-company foreign-run and foreign-company executions with savepoints and
otherwise-valid local control. Targeted rereview confirms both corrections, but
**new test does not compile**: `batch_recovery_tests.rs:134` accesses private
`f.binding.version` (E0616). This is the exact next edit:
read `version_id` from `workflow_runs` with company_id and id bound to request,
construct `VersionId::new(...)`, and assign `publication.version`; do not expose
fixture fields merely for this test. Rereviewer explicitly accepted this approach.
Then run corrected tests and gates below, and have a fresh independent reviewer
inspect the mechanical fix plus final integration/evidence. Final review NOT passed.

Review evidence: full actual-code review covered all brief criteria; targeted
review of corrections found no other issue. Reviewer last sample86005/25840033.28%
13:25:17Z usage/runtime, completed/quiescent. Fresh subtree must create its own
reviewer; do not reuse this retiring child.

### Exact commands and results

All test commands explicitly set both DATABASE_URL and TEST_DATABASE_URL to
`postgres://mac03@127.0.0.1:55439/workflow_admission`; stack gate sets
`RUST_MIN_STACK=2097152`; offline commands set `SQLX_OFFLINE=true`.

- `cargo sqlx migrate run` PASS: `/private/tmp/workflow-batch-migrate.log`133000,
  `/private/tmp/workflow-batch-migrate-2.log`133100. Isolated DB fixtures also migrated.
- `cargo check --locked --offline --all-targets` initial PASS:
  `/private/tmp/workflow-batch-check.log`.
- `cargo test --locked --offline --lib workflow_batch -- --nocapture` first6PASS:
  `/private/tmp/workflow-batch-focused.log`.
- `cargo test --locked --offline --lib workflow_ -- --nocapture` initial205PASS3FAIL:
  `/private/tmp/workflow-batch-workflow.log`; failures were duplicate-job old fixtures.
  Corrected rerun208PASS0FAIL: `/private/tmp/workflow-batch-workflow-2.log`.
- `cargo test --locked --offline --lib` first full2005PASS1FAIL22ignored:
  `/private/tmp/workflow-batch-full.log`. Only new fixture missing correlation_id failed;
  all runtime/activation/legacy tests passed. Fix above has NOT yet passed full suite.
- Full rerun `/private/tmp/workflow-batch-full-2.log` stopped at E0616 before tests.
- `DATABASE_URL=... cargo sqlx prepare -- --all-targets` PASS before final fixture edit:
  `/private/tmp/workflow-batch-sqlx.log`; no .sqlx diff expected for runtime queries.
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets` PASS before final
  fixture edit: `/private/tmp/workflow-batch-check-final.log`.
- `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`
  PASS before final fixture edit: `/private/tmp/workflow-batch-clippy.log`.
- `cargo fmt --all -- --check`, `git diff --check` PASS current tree; `graft build`
  refreshed graph at checkpoint: `/private/tmp/workflow-batch-graft.log`.

Pending after exact fixture correction: focused `workflow_batch` (all15), full DB
library suite at stock2MiB, SQLx prepare sequential with builds, locked offline
all-target check/Clippy, fmt/diff, graft build and independent final review. Do NOT
claim prior passing static gates cover current uncompiled fixture.

### Transfer and context

Task-only PG RUNNING, retained `/private/tmp/workflow-admission-pg-e3aa`, port55439,
DBworkflow_admission, socket/private/tmp, max_connections200. No production/dev DB
changed. No running build/test/prepare jobs at handoff. Root owns PROGRESS/RESUME;
next implementer owns this brief and PG. Existing stage/index state untouched.

Implementer `/root/pure_batches` UUID`01a0e81e-4dc9-7920-8825-ea6ec7c67e9e`,
Astra/medium. Startup21269/2584008.23%13:05:05Z; intermediate11026642.67%13:17:05Z;
latest before checkpoint12368847.87%13:23:57Z, all usage/runtime sources.
Final fresh sample reported to root. Reviewer quiescent, both retiring per root.
Graft reported savings ~2.8million tokens (dominated by whole-repo map baseline;
estimate only), plus reviewer separately ~2.706million.

## Final verification checkpoint — 2026-09-28 13:33Z

Fresh implementer `/root/batch_verify` fixed the last E0616 test compilation issue:
`batch_recovery_tests.rs:134` reads `version_id` from `workflow_runs`, scoped by
request company and run, and constructs `VersionId`. Fixture visibility unchanged.
Rustfmt applied; no runtime or migration edits in this correction. Applied migrations
through20260928133100 remain immutable. Both prior review findings now execute and
pass, including first-link foreign-run/foreign-company negatives with valid local
control and frozen-choice replay against changed predecessor context.

Current-tree gates PASS (all logs under `/private/tmp/workflow-batch-verify-`):

- `DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission TEST_DATABASE_URL=postgres://mac03@127.0.0.1:55439/workflow_admission RUST_MIN_STACK=2097152 SQLX_OFFLINE=true cargo test --locked --offline --lib workflow_batch -- --nocapture`
  —15passed/0failed, `focused.log`.
- Same explicit database/stack/offline environment, `cargo test --locked --offline --lib`
  —2007passed/0failed/22existing ignored,73.68s, `full.log`.
- Same explicit database URLs, `cargo sqlx prepare -- --all-targets` —PASS18.06s,
  `sqlx.log`; sequential after tests and before offline builds. No `.sqlx` diff.
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets` —PASS, `check.log`.
- `SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings`
  —PASS23.51s, `clippy.log`.
- `cargo fmt --all -- --check`, `git diff --check`, `graft build` —PASS,
  `fmt.log`, `diff.log`, `graft.log`. Prior applied/isolated migration evidence above
  remains applicable; full isolated tests migrate fresh databases too.

Independent nested Astra/medium reviewer `/root/batch_verify/review` UUID
`01a0e833-51cf-78f1-a208-4907154ee36c` returned final PASS, no remaining findings.
Inspected actual scoped correction, both corrected regressions, activation integration
and fixture adjustments, verified all final logs; reused unchanged runtime/schema/
locking review above. Source edits paused throughout review. No outstanding03.3
verification; worker loop/fenced I/O/poison backoff remain later scope, not promised.

Implementer Astra/medium UUID`01a0e832-e897-7952-b436-ce88d876a887`: startup
21379/2584008.27%13:27:38.760Z; milestone5068219.61%13:29:44.772Z; final gate sample
5612821.72%13:33:09.356Z, token_usage_record.usage/task_started.model_context_window.
Reviewer startup211738.19%13:28:07.045Z; final5326120.61%13:33:07.266Z, same sources.
Final post-checkpoint implementer sample is reported directly to root.

TaskPG remains RUNNING at retained `/private/tmp/workflow-admission-pg-e3aa`, port55439,
DBworkflow_admission; root may transfer ownership at next accepted checkpoint.
No build/test/prepare process remains. Reviewer completed/quiescent. Root owns queue
acceptance and PROGRESS/RESUME. No03.4 started; all unrelated edits/deletions/untracked
website preserved; no staging, commit, reset or deploy. Graft estimated savings this
verification worker~2.55million, reviewer separately~2.57million.
