# AWC-P02 reviewer findings

## AWC-P02-D1-F1 — The lock order misstates what it locks and leaves the `run_as` account outside the account protocol

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage architecture gate; round
  [AWC-P02-D1-R1-N1](agent-review-notes.md#awc-p02-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`; decision [AWC-P02-D1 r1](logs/AWC-P02-D1-r1-decision.md), sha256
  `09f3101e1d851569a10a86dba4e77b49c069d1b4ad725619b1bb806976c1fb46`. Originates in the gated
  decision; no implementation exists.
- **Classification:** actionable defect; medium; concurrency.
- **Evidence.**
  - D3: "Steps 5–6 take users before channels, the same order as `configure_reply_on` (users
    share → channels update), so the two cannot deadlock." Step 5 is
    `authorize_association_on`, which locks `companies FOR NO KEY UPDATE`, then
    `department_members`/`principals FOR SHARE`, then `channels FOR SHARE`
    (`workflow/authority.rs:72-108`, `management.rs:76-90`, `:116-127`). It takes no `users`
    lock. `configure_reply_on` does: it locks the actor, reviewers and the responder's `run_as`
    in `users … ORDER BY id FOR SHARE` first (`channel_reply.rs:211-240`).
  - The repository's protocol is one sorted account union before company ownership, from
    unlocked hints with a post-lock equality check
    (`workflow/action_account_hints.rs:1`, `:361-392`: "Never acquire a replacement account
    after company ownership"). The caller's prelock (step 1) follows it for the caller's
    accounts and already takes the company lock (`authorize_run_with_locks`). The decision
    learns `run_as` at step 4, after the company lock and the fence.
  - An account writer locks `users FOR UPDATE` first and the company second
    (`user_lifecycle.rs:30-62`). If anything in the call transaction locks `users(run_as)`
    after step 1, including a foreign key on `workflow_channel_calls.run_as_user_id` (the
    table it is modelled on has `actor_id … REFERENCES users`, init schema 13012) or a p03
    launch write, the two wait on each other. `workflow_runs.actor_id` has no such key, so the
    child insert alone does not; the decision does not say which applies.
- **Impact.** The deadlock claim is unproven and the stated reason is wrong. A deadlock here is
  recoverable (PostgreSQL aborts one side, the job retries), but it is the hazard the existing
  protocol was built to remove.
- **Required.** Either put `run_as` into the step 1 account union (read the settings hint before
  the prelock, lock sorted, recheck after `channels FOR SHARE`), or state that no
  `users(run_as)` row lock is taken after company ownership and that
  `run_as_user_id` therefore carries no key to `users`. Correct the sentence about step 5 and
  list each row lock it takes. Add a competing test: dispatch against an account write on the
  `run_as` person.
- **Instruction lesson.** Candidate (scope: openbot persistence gates): a lock order names the
  table and lock mode per step and cites the existing account-prefix owner.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed in the decision (recheck AWC-P02-D1-R2-N1, decision r2 sha256 `62e44a25…87d3`): `run_as` joins the step-1 sorted account union through a new hint purpose with the existing post-lock recheck; step 5 turns a changed source into `Conflict`; each step's row locks are listed; the `run_as_user_id` key touches a row already share-locked; V5 is the competing test. The step-4 principal and grant share locks match the existing `agent_preparation.rs:97-108` read, taken in the same position (after the company lock, inside a fenced transaction); writers of those rows were not exhaustively traced. Two residual points are in F7.

## AWC-P02-D1-F2 — The test composition omits rows an existing guard requires, so the forged-row tests can pass for the wrong reason

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage architecture gate; round
  [AWC-P02-D1-R1-N1](agent-review-notes.md#awc-p02-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`; decision [AWC-P02-D1 r1](logs/AWC-P02-D1-r1-decision.md), sha256
  `09f3101e1d851569a10a86dba4e77b49c069d1b4ad725619b1bb806976c1fb46`. Originates in the gated
  decision; no implementation exists.
- **Classification:** actionable defect; medium; verification.
- **Evidence.** D3 "Port placement": the DB tests compose the functions "with a child row
  written by `admission_write::insert_run` (ParentLink to the caller, actor run_as, desk
  association)". A run with `parent_run_id` is refused at commit by the deferred constraint
  `workflow_child_parent_wait` unless the parent execution has a `child_run` wait naming the
  child and a `workflow_admissions` row whose source key carries the same tool-call id
  (init schema 1418-1438, 9348). The verification plan asserts only "fails at commit" (V2, V6,
  V7). A fixture without the wait and admission fails every case, the legitimate one included,
  on that older guard. V6's "with an invocation it commits" cannot pass without them.
- **Required.** Name the fixture rows (parked parent execution, wait with
  `child_tool_call_id = call_id`, admission, first job as needed) and make each negative case
  assert the constraint that refused it (the new trigger or foreign key by name), beside one
  positive control built from the same fixture.
- **Instruction lesson.** `verification-contracts.md` already asks that the assertion
  distinguish the failure; candidate: for SQL-refusal tests, assert the constraint name.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed in the decision (recheck AWC-P02-D1-R2-N1, decision r2 sha256 `62e44a25…87d3`): D4 routes the tests through `children::create_on`, so the wait, admission and job exist; the fixture is named; V0 is a commit-level positive control and each negative names its constraint.

## AWC-P02-D1-F3 — Moving the port to p03 is not carried to p03, and what p02 then leaves unproven is not listed

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage architecture gate; round
  [AWC-P02-D1-R1-N1](agent-review-notes.md#awc-p02-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`; decision [AWC-P02-D1 r1](logs/AWC-P02-D1-r1-decision.md), sha256
  `09f3101e1d851569a10a86dba4e77b49c069d1b4ad725619b1bb806976c1fb46`. Originates in the gated
  decision; no implementation exists.
- **Classification:** actionable defect; low; requirements / coordination.
- **Evidence.** 02 A.1 asks for "a narrow application port for channel-call target
  preparation/dispatch". D3 moves it to p03 and gives a sound reason (only the launch
  transaction may compose capture, record and child). 02's acceptance stays provable at the
  persistence level for the record, the scope cases and capture. It is not provable in p02 for:
  convergence under the job fence (V3 proves the primary key only); a replay that returns the
  invocation without re-reading settings through the real dispatch; changed arguments on a
  real saved call. No notice tells p03 it owns the port and these checks; AWC-C002 r1 predates
  this.
- **Required.** A notice (or AWC-C002 revision) targeting p03, and these three recorded as NOT
  EXERCISED in p02's mapping rather than covered by V1/V3.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed (recheck AWC-P02-D1-R2-N1, decision r2 sha256 `62e44a25…87d3`): notice AWC-C003 r1 (proposed, coordination r8) gives p03 the port and the unproven items; the decision lists them NOT EXERCISED. See F6 for checks it still leaves unowned.

## AWC-P02-D1-F4 — Smaller inaccuracies and gaps in D1 and D3

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage architecture gate; round
  [AWC-P02-D1-R1-N1](agent-review-notes.md#awc-p02-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`; decision [AWC-P02-D1 r1](logs/AWC-P02-D1-r1-decision.md), sha256
  `09f3101e1d851569a10a86dba4e77b49c069d1b4ad725619b1bb806976c1fb46`. Originates in the gated
  decision; no implementation exists.
- **Classification:** actionable defect; low; precision. One line each fixes them.
- **Evidence.**
  - "Settings … are written only by `configure_reply_on`": `bootstrap_native_on` also inserts
    them (`channel_reply.rs:67-101`, called from `provisioning_native.rs:282` and
    `provisioning_department.rs:496`) and holds only `channels FOR SHARE`, and
    `agent.rs:683` clears them for principal inboxes. Neither breaks the design (a bootstrap
    only fills an empty desk, which a call sees as `no_responder`; calls address desks), but
    the reconfiguration argument should say so.
  - Insert guard item 3 compares the child's `binding_id` and `channel_id` but not its
    `binding_revision`, which the run row also carries.
  - "p03 also adds FK `workflow_waits(…, child_tool_call_id, child_run_id) →
    workflow_channel_calls` for channel-call waits": workflow-tool waits use the same two
    columns (`workflow_wait_child`, init schema ~6168), so a plain foreign key would refuse
    them. It needs a discriminating column or a trigger, and a matching unique key on the new
    table if kept.
  - A settings mismatch at step 6 "rolls back and redoes steps 4–6": rolling the transaction
    back releases the fence. Say savepoint, or simply treat the locked read as authoritative
    and re-authorize the new `run_as`.
  - `capture_on` sets `lock_timeout` and `statement_timeout` for the rest of the transaction
    and allows itself five seconds, while the comparable dispatch is bounded at two
    (`workflow_tool_calls.rs:28`). State the outer bound p03 must use.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed (recheck AWC-P02-D1-R2-N1, decision r2 sha256 `62e44a25…87d3`): three settings writers listed; `binding_revision` in guard item 3; the wait link is a trigger or discriminator for p03; no in-transaction retry; outer bound 10 s.

## AWC-P02-D1-F5 — Unverified assumption to test early (optional)

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage architecture gate; round
  [AWC-P02-D1-R1-N1](agent-review-notes.md#awc-p02-d1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`; decision [AWC-P02-D1 r1](logs/AWC-P02-D1-r1-decision.md), sha256
  `09f3101e1d851569a10a86dba4e77b49c069d1b4ad725619b1bb806976c1fb46`. Originates in the gated
  decision; no implementation exists.
- **Classification:** optional suggestion; low.
- **Evidence.** B found no database guard that forces a child run's actor, binding or desk to
  equal its parent's (the `workflow_runs` triggers were listed, not all bodies read). If one
  exists, V6's positive control will show it. Run that control first.
- **Resolution history.** 2026-11-22 open (optional).
  - 2026-11-22, answered (recheck AWC-P02-D1-R2-N1, decision r2 sha256 `62e44a25…87d3`): the decision lists the `workflow_runs` triggers and V0 is the control. Closed.

## AWC-P02-D1-F6 — Four admission checks from gate AWC-P01-D1 have no place in the order and no owning phase

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage architecture gate recheck; round
  [AWC-P02-D1-R2-N1](agent-review-notes.md#awc-p02-d1-r2-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`; decision [AWC-P02-D1 r2](logs/AWC-P02-D1-r2-decision.md), sha256
  `62e44a2554254d7f2c0240763971b4436811377391120d5d913798bd43ee87d3`. Originates in the gated
  decision.
- **Classification:** actionable defect; medium; requirements / authority. Later-discovered:
  r1 had the same gap and B did not report it; D4 makes it matter, because p02 now writes the
  child's knowledge levels.
- **Evidence.** AWC-P01-D1 r3 D1.1 orders, after the target checks: step 6 intake
  (`check_intake` against the captured intake; 02 A.4 names the intake among the captured
  facts), step 7 levels (`release_request`, error `audience_refused`), and in step 8 the
  destination unit's spend cap (denied closure `budget.unit_exhausted`,
  `unit_budgets::check_admission_on`) and role resolution (denied
  `destination_unavailable`). r2's D3 steps 1–8 and D4 mention none of them (no occurrence of
  "intake", "release", "content", "exact", "spend" or a role refusal mapping), and AWC-C003
  does not give them to p03. D4 says the child is written with "the child ceiling" but not
  with which content level and exactness; p01 D1.4 says the request carries the calling run's
  current content level and exactness. `RunKnowledge::new` refuses content above ceiling
  (`access_level.rs:193-200`), so with today's level-0 ceiling a caller that has read anything
  above public reaches the child writer and fails as an internal error instead of
  `audience_refused`, unless the check ran first.
- **Impact.** Each phase can assume the other owns these; the first proof would be p06.
- **Required.** Place steps 6 and 7 and the spend-cap check in D3's order before D4, state the
  child's content level and exactness in D4, say how a role-resolution refusal inside manifest
  capture maps to a denied closure, and assign each to p02 or to p03 in AWC-C003 with a test.
- **Instruction lesson.** Candidate (scope: a gate that implements another gate's ordered
  checks): list every step of the governing order with its owner, including the ones deferred.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed in the decision (recheck AWC-P02-D1-R3-N1, decision r3 sha256 `7e189031…2125`): steps 6–9 place intake, levels, destination admission, role resolution and the unit cap before the child exists or under `SAVEPOINT channel_call_child`; D4 item 1 states the child's ceiling, content level and exactness; an owner table assigns each check with V12–V15; tree limits and the destination-audience case go to p03.

## AWC-P02-D1-F7 — Two details of the step-1 hint source

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage architecture gate recheck; round
  [AWC-P02-D1-R2-N1](agent-review-notes.md#awc-p02-d1-r2-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`; decision [AWC-P02-D1 r2](logs/AWC-P02-D1-r2-decision.md), sha256
  `62e44a2554254d7f2c0240763971b4436811377391120d5d913798bd43ee87d3`. Originates in the gated
  decision.
- **Classification:** actionable defect; low; precision.
- **Evidence.**
  - The unlocked source reads "the desk's current settings" for a `channel_id` the model
    supplied, before the write check. It must be scoped to the caller's company, so a foreign
    desk contributes no account and no lock.
  - When a hinted account row is missing, the existing prelock returns `missing()` unless the
    source is recorded (`action_account_hints.rs:372-388`). For a `run_as` account that no
    longer exists, that would end the dispatch as a refusal of the caller on every
    re-dispatch, where D1.1 step 8 wants the denied closure `destination_unavailable`. Say
    which it is.
- **Resolution history.** 2026-11-22 open.
  - 2026-11-22, fixed in the decision (recheck AWC-P02-D1-R3-N1, decision r3 sha256 `7e189031…2125`): the hint statement is scoped to the caller's company; a missing `run_as` account is optional in the union while the base accounts must still all be locked; step 5 gives `destination_unavailable` for a dead account and `Conflict` for a changed one.

## AWC-P02-D1-F8 — Capture refusals other than a role refusal fail the caller instead of answering it

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage architecture gate recheck; round
  [AWC-P02-D1-R3-N1](agent-review-notes.md#awc-p02-d1-r3-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3`; decision [AWC-P02-D1 r3](logs/AWC-P02-D1-r3-decision.md), sha256
  `7e1890315ee48b02fa62362fd4738bd1fd01fce75928f3f803e793d6db472125`. Originates in the gated
  decision.
- **Classification:** actionable defect; low; failure behavior. Not blocking the gate: it is one
  error mapping inside step 9, cheap to settle in the batch, and B will check it there.
- **Evidence.** r3 step 9: only `AppError::WorkflowRole` and the unit-cap refusal roll back to
  the savepoint; "Any other error propagates and the dispatch transaction rolls back as a
  whole." Manifest capture also refuses a destination whose dependency graph is unavailable,
  cyclic or over its limits, and one whose agent grants no longer hold
  (`agent_manifest/mod.rs:40-52`, `BadRequest`). Those are facts about the destination. 01 A.4
  asks for typed errors for "dependency/recursion limit" and says ordinary domain failures
  reach the model as tool errors. As written, a broken destination fails the calling run's
  dispatch on every re-dispatch.
- **Required (in the batch).** Map capture's destination refusals to the denied closure
  `destination_unavailable` under the same savepoint, keep timeouts and database errors
  propagating, add one test, and record it in TASK's acceptance mapping.
- **Resolution history.** 2026-11-22 open; deferred by B to batch AWC-P02-B1.


## AWC-P02-B1-F1 — No test separates the destination's unit scopes and spend cap from the caller's

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage batch review; round
  [AWC-P02-B1-R1-N1](agent-review-notes.md#awc-p02-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3` (work uncommitted); artifact digest
  `543ef9e9e262f537971b9bc5fd6abcb6038825c712893c811a7036ae97181777`. Originates in the batch's tests.
- **Classification:** actionable; medium; verification gap (stand-in limit) on gate D1.2/D4.
- **Evidence.** `CallSetup::with` creates the desk with `new_channel` and no unit, so the desk
  sits in the root unit; the caller runs on no desk, so its write unit is the root too
  (`channel_call_snapshot_tests.rs:88`, and V15's own comment at `:881`). V0's assertion
  `write_unit == desk_unit` (`:430-438`, "not copies of the caller's") and V15's cap would both
  pass if the child copied the caller's scopes or charged the caller's unit. TASK declares the
  V15 half only; the V0 mapping row claims more than the fixture can show. D1.2 ("the
  destination unit pays") and D4 item 3 are therefore unproven by any test.
- **Required.** Put the desk in a non-root unit in at least V0 and V15: assert the child's
  write unit is that unit and differs from the caller's, and that the cap on the desk's unit is
  the one that refuses. Correct the TASK mapping row.
- **Instruction lesson (candidate).** When a criterion says "X's, not Y's", the fixture must
  make X and Y different values; the reviewer checks that before reading the assertion.
- **Resolution history.** 2026-11-22 open.

## AWC-P02-B1-F2 — The write right through a `participate` grant is never exercised positively

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage batch review; round
  [AWC-P02-B1-R1-N1](agent-review-notes.md#awc-p02-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3` (work uncommitted); artifact digest
  `543ef9e9e262f537971b9bc5fd6abcb6038825c712893c811a7036ae97181777`. Originates in the batch's tests.
- **Classification:** actionable; medium; verification gap on user decision U2 (the only gate
  on a call).
- **Evidence.** Every started call in the suite targets a desk set to `access_mode='public'`
  (`channel_call_snapshot_tests.rs:89`). `agent_participates_on`
  (`channel_call_target.rs:355-381`) is two runtime SQL strings (agent principal, then
  `channel_principal_grants` with `capability='participate'`); the only case that reaches a
  non-public desk expects `target_unavailable` (`:704-720`), which also passes if the lookup
  always returns false. A members-only desk is the case U2 is about. TASK declares the gap
  ("members-only-with-grant positive not exercised"); gate V11 listed it as required.
- **Required.** One case: members-only desk, grant the calling agent's principal
  `participate`, expect `Started`; remove the grant, expect `target_unavailable`.
- **Instruction lesson (candidate).** An authorization rule needs its allow case and its deny
  case on the same fixture; a deny-only test cannot tell a rule from a constant.
- **Resolution history.** 2026-11-22 open.

## AWC-P02-B1-F3 — Gate verification cases dropped or weakened (V2, V11, V3)

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage batch review; round
  [AWC-P02-B1-R1-N1](agent-review-notes.md#awc-p02-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3` (work uncommitted); artifact digest
  `543ef9e9e262f537971b9bc5fd6abcb6038825c712893c811a7036ae97181777`. Originates in the batch's tests; partly declared in TASK.
- **Classification:** actionable; low; verification gap against gate r2's verification table.
- **Evidence.**
  - V2 (`:493-592`): the gate lists foreign-company channel (FK), other binding revision and
    child actor ≠ `run_as` (each `workflow_channel_call_snapshot`). The first two are absent.
    The actor case's `Err(_) => {}` arm accepts any error, including a malformed statement, so
    it names no constraint, against the gate's "each negative asserts its constraint by name".
  - V11: the archived unit (`unit.archived`, step 5) has no database case; the
    `archived_at IS NOT NULL` read in `target_on` is only ever false.
  - V3 (`:595-611`): both transactions hold the same fence, so the fence serializes them and
    the loser fails on ownership, not on the key. The test asserts only "one Ok, one row"; the
    primary key the gate names is not what refuses.
- **Required.** Add the two V2 cases; make the actor case assert the refusing constraint's
  name (whichever guard it is); add the archived-unit case; in V3 either assert the loser's
  error or state in TASK that the fence, not the key, decided it.
- **Instruction lesson (candidate).** A test arm that accepts any error is not evidence; name
  the expected refusal.
- **Resolution history.** 2026-11-22 open.

## AWC-P02-B1-F4 — Unnamed three-element tuple returned by `prelock_with_native`

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage batch review; round
  [AWC-P02-B1-R1-N1](agent-review-notes.md#awc-p02-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3` (work uncommitted); artifact digest
  `543ef9e9e262f537971b9bc5fd6abcb6038825c712893c811a7036ae97181777`. Originates in the batch's code.
- **Classification:** actionable; low; repository rule (`src/AGENTS.md` "Name your tuples").
- **Evidence.** `action_account_hints.rs`: `prelock_with_native` now returns
  `AppResult<Option<(WorkflowActor, Source, Vec<Uuid>)>>`, and its three callers destructure it
  with `|(actor, ..)|` or a three-place pattern. The rule makes any tuple of three or more
  elements a named struct.
- **Required.** Return a small named struct (actor, source, locked accounts).
- **Instruction lesson.** None new; the rule exists.
- **Resolution history.** 2026-11-22 open.

## AWC-P02-B1-F5 — Optional: remaining declared gaps accepted as recorded

- **Context:** 2026-11-22; workflow `implement-opus-reviewed-2_4`; [TASK.md](TASK.md),
  [REVIEW.md](REVIEW.md); batch AWC-P02-B1, stage batch review; round
  [AWC-P02-B1-R1-N1](agent-review-notes.md#awc-p02-b1-r1-n1); reviewer B, Fable 5.2 (effort as
  assigned: medium); phase AWC-P02; worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`;
  HEAD `fe7791d3` (work uncommitted); artifact digest
  `543ef9e9e262f537971b9bc5fd6abcb6038825c712893c811a7036ae97181777`. Declared by the author in TASK.
- **Classification:** optional; low; verification notes, no change required.
- **Evidence.** V0 does not assert `content_exact`; V6 has no `verify_selection` or
  resource-revocation case (both run through the same `destination_refusal` mapping the role
  case exercises); V9 checks the workflow version pin, not an agent revision pin; V14 uses a
  role with no holder. `destination_refusal` maps every `BadRequest` to the closure, so a
  defect surfacing as `BadRequest` inside capture would read as a destination fact.
- **Resolution history.** 2026-11-22 recorded; B accepts these as declared.

### AWC-P02-D1-F8 — resolution update

  - 2026-11-22, fixed (batch review AWC-P02-B1-R1-N1, digest `543ef9e9…1777`):
    `write_call_on` maps capture errors through `destination_refusal` (BadRequest, NotFound,
    Forbidden, ResourceOutsideUnit, UnitArchived, WorkflowRole) to `destination_unavailable`
    under the savepoint; timeouts, database errors, conflicts and defects propagate
    (`children.rs`, `channel_call_target.rs:494-504`). Evidence: the classification unit test
    plus V14, which drives the same branch through the database with a role refusal. No
    database-backed graph refusal exists; B accepts that, since the branch taken is identical.
    TASK records it. Closed.


### AWC-P02-B1-F1…F4 — resolution updates (recheck AWC-P02-B1-R2-N1, digest `0639c0f0…2112`)

- **AWC-P02-B1-F1.** 2026-11-22, fixed. `CallSetup::with` puts the desk in `finance-unit` under
  the root. V0 asserts the child's write unit is that unit (the helper falls back to the root
  when a run has no write scope, so a child without its own scope would fail) and that the
  caller's differs. V15 admits callers from a desk in a sibling unit: caller's unit exhausted →
  the call starts and the child's write unit is the desk's; desk's unit exhausted →
  `budget.unit_exhausted`, fence held, no child. The gate sentence is tested for a sibling
  caller unit only; caps roll up, so a root or ancestor caller cap refuses both by Plan07
  design. B accepts that narrowing; TASK records it. Closed.
- **AWC-P02-B1-F2.** 2026-11-22, fixed.
  `channel_call_target_facts_participate_grant_opens_members_only_desk`: members-only desk,
  grant → `Started`, revoke → `target_unavailable`, one record. Closed.
- **AWC-P02-B1-F3.** 2026-11-22, fixed. V2 cases go through `forged()`, which names the
  refusing constraint in every arm; added same key twice (`workflow_channel_calls_pkey`) and a
  real second company's channel (`workflow_channel_call_binding_fk`); the actor case asserts
  `workflow_channel_call_snapshot`. Archived unit: a new test archives through
  `archive_department` and expects `unit.archived`, zero rows. V3 asserts the loser is
  `OwnershipLost`. "Other binding revision" is dropped. The author's reason ("unforgeable") is
  not exact: a forged INSERT naming another existing revision of the binding would reach the
  snapshot clause, as the other forged inserts do. B accepts the omission anyway: the clause
  sits in the same conjunction that four cases already show refusing, and V4 covers the real
  path. Closed.
- **AWC-P02-B1-F4.** 2026-11-22, fixed. `prelock_with_native` returns
  `Prelocked { actor, source, locked }`; callers use field patterns. No behaviour change by
  inspection. Closed.
