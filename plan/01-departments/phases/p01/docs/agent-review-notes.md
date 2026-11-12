# p01 agent review notes

## 01DEP-P01-G1-R1 — architecture gate G1, decision revision r1

### Identity

- Date 2026-11-09. Workflow `implement-opus-reviewed-2_2`. Task `TASK.md`, review `REVIEW.md`,
  findings `agent-review-findings.md` (this directory).
- Batch B1 (whole phase 01: 01A + 01B); stage: architecture gate. Phase `p01`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh; not independently observable by
  B), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`.
  Phase start, review base and HEAD are all `3752ea3540e3a2bf6d17178bf887fe2e99a384ac`.
- Gated decisions: DEP-P01-D1, D2, D3, D4, D5 at **r1**. Immutable copy `logs/gate-decisions-r1.md`
  has sha256 `c72805f2d0d41d433dfa37e386daf1ad14c11c62fcc6fd72e526f302cadeb572`, which I verified.
  The TASK.md gate section and its "Verification plan" / "Contract impact" sections are
  byte-identical to the copy at the start and at the end of the review.
- Code snapshot (informational for a gate), with exclusions `plan/01-departments/phases/p01/docs`
  and `plan/01-departments/COORDINATION.md`:
  - at start: `71e8276a3fb431ca1b0f25397d70e3055f0eeadf609157b606b58e75795bd130` (no changed files);
  - at end: `ca321ded89ea8a04f4d4529b55299e8001c86dd798a7cdb3661b4ed592037eed`.

  The author edited non-gated domain files in the meantime, plus the plan-mandated in-place
  `channel_principal_grants_provenance_check` value (AC2). That edit does not depend on D1–D5, and it
  is not what this round reviews.
- Coordination: `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md`
  revision 1, with instructions 01DEP-I001/r1, I002/r1, I003/r1 and no notices. The authority file's
  sha256 equals the pinned `logs/coordination-r1.md` (`babfa9b2…128b`); I checked this at the start
  and again before returning.

### Coverage and evidence

- **Requirements read:** plan README (decisions 1–12, "What does not change"), `01-…`, and the
  needs `02-…`/`03-…` place on p01. Also root `AGENTS.md`, `src/AGENTS.md`, and the application,
  persistence, http and infra `AGENTS.md` files.
- **Code inspected:**
  - `workflow/authority.rs:39-81` (`authorize_company`: `FOR NO KEY UPDATE`, member/principal
    `FOR SHARE`, NotFound for non-managers);
  - `workflow/schedules.rs:148-188` (the schedule write lock this mirrors);
  - `agent_publication/mod.rs:32-150` (`begin_bounded`, `lock_company_on` user-row-first,
    `publish_initial_on`);
  - `workflow/provisioning.rs:44-63, 484-549, 764-881` (`assistant_channel_on` and its lock
    preconditions);
  - `channel_edit.rs` and `channel_gate.rs` (gate → row protocol). The gate has only two callers,
    `channel_edit.rs:106` and `thread/inbound.rs:147`; neither takes company or department locks
    after the gate;
  - `channel.rs:180-189` (`slug_conflict_error` is `BadRequest` and names a channel, confirming D5's
    deviation is needed), `channel.rs:752-776` (channel JSON PUT replace semantics);
  - `use_cases/company.rs:184-253` (`managed_company` returns Forbidden for members);
  - `channel_authorization_tests.rs` and `authorization_test_support.rs:145-172` (`denial_cases`:
    member gets 403);
  - `schedule_tests.rs:598-656` (member/admin get NotFound);
  - the member-removal and principal-demotion paths: `company_invite.rs:537-617`,
    `agent.rs:398-431`, `user_lifecycle.rs:11-61`;
  - init schema: principals and their FKs, `messages_author_principal_fk` NO ACTION,
    `principals_company_id_id_key`, the slug-format regexes, `valid_creation_provenance`, and the
    `RAISE … CONSTRAINT=` precedent;
  - `Channel::participant_access`/`viewer_access`;
  - `ports/channel.rs` and `use_cases/channel.rs` (where `ChannelWrite` and `normalize` live);
  - `NO_BACKWARD_COMPATIBILITY_NOTES` (`src/main.rs:221-226`, already lists the plan).
- **Checks run (fixture-isolated).** Scratch DB `openbot_dept_p01_review`, created and dropped by B,
  PostgreSQL 18.6. Log: `logs/01DEP-P01-G1-R1-sql-probes.txt` (sha256 `9a7b8dde…a711`).
  - Probe A: my reconstruction of D2's shape function and trigger (not the author's code). Results:
    - a 9th-level insert is refused;
    - re-parenting a height-3 subtree under level 5 is allowed, under level 6 refused;
    - re-parenting into the department's own subtree, self-parenting and a one-statement parent swap
      are refused as a cycle.

    Conclusion: D2's two-walk formula is correct for re-parenting subtrees.
  - Probe B: the real init schema plus D1's tables and p02's `channels.department_id`. Results:
    - (a) after `remove_member`'s statements, the member and lead rows survive with kind `external`
      (→ F1);
    - (b) company teardown succeeds with RESTRICT or NO ACTION on both the parent FK and the desk FK,
      and direct deletes are refused under either (→ F6).
- **Assumptions challenged:**
  - whether the company write lock is the right one (yes, for p01 alone);
  - its composition with the channel gate (→ F2) and with nested agent publication (→ F7);
  - NO ACTION versus RESTRICT (→ F6);
  - depth/cycle correctness (sound);
  - Forbidden→NotFound against the plan and existing tests (consistent with the plan's
    `department_write_` text and the schedule tests, and different from the channel route 403
    convention → F8);
  - PATCH semantics (→ F3);
  - member reads (→ F4);
  - verification discriminating power (→ F5).
- **Not executed:** no Rust build or test. Implementation is not under review, and the gate accepts
  the verification plan, not its execution. The SQL probes test my reconstruction of the decisions,
  not the author's future SQL.
- **Reviewed only lightly:**
  - D5's domain type list and `DepartmentSlug` placement: checked against precedent only;
  - the internal relay path's lock order (`workflow/message.rs:50-66`): I confirmed `message.send`
    takes the company lock, but did not trace the whole relay chain;
  - identity resolution of demoted principals: not traced, so F1's security consequence is stated as
    a hypothesis.

### Outcome

**Changes required** for decision revision r1. There is no PASS.

- Actionable defects:
  - 01DEP-P01-G1-F1 (High)
  - 01DEP-P01-G1-F2 (Medium)
  - 01DEP-P01-G1-F3 (Medium)
  - 01DEP-P01-G1-F4 (Low)
  - 01DEP-P01-G1-F5 (Low)
- Optional suggestions: 01DEP-P01-G1-F6, F7, F8, F9.
- Sound as written:
  - D1 appended block, composite tenant FKs, partial lead index with its deliberate literal;
  - D2 two-walk shape function, trigger and preflight sharing one definition;
  - D3 company-lock serialization for p01's own writers, lead replace and idempotency semantics,
    `_on` set and port-wrapper split;
  - D4 under-lock re-authorization;
  - D5 `DepartmentWrite`/`normalize` placement (matches `ChannelWrite`), `Conflict` for a taken
    slug, `/api` JSON prefix.

  None of these departures from the plan text needs the user. Each resolves an internal plan
  inconsistency toward the plan's explicit outcome or the repository's convention.
- Unresolved question for the author, possibly for the user: what F1's demotion should do. Removing
  the rows is the natural reading; keeping them would be a product decision.
- The recheck should cover the r2 text of D1/D2/D3/D4/D5 and the amended verification rows for
  F1–F5. Unaffected parts of the r1 review may be reused if r2 leaves them unchanged.

### Process and instruction feedback

- Batch and gate size were appropriate. One gate for the five decisions was reviewable in one round.
- Gate content was clear and well-sourced. TASK records were sufficient to review from. Pinned copies
  and exclusions worked.
- Defects are in the gated decisions, not in implementation:
  - F1 comes from a cascade rationale based on an assumed delete path;
  - F2 comes from a lock order composed with schedules but not with the advisory gate it promises to
    extend.
- Candidate instructions (scope: architecture gates touching FK, cascade or lock protocols):
  - (1) list every production path that deletes, or changes the discriminating column of, a
    referenced row, with a disposition for each (evidence F1);
  - (2) a gated lock order names the position of every lock kind dependent phases will take,
    advisory locks included, and the writer on the other side (evidence F2);
  - (3) a competing-claimant verification row names the backstop that would yield the same outcome
    and asserts something only the protocol yields (evidence F5).
- Observation: the author started schema-adjacent edits (the AC2 provenance value) before the gate
  passed. It is harmless here because the value is plan-mandated and independent of D1–D5. HANDOFF
  says schema waits for the gate, so a one-line record of that exception would keep the record
  accurate.

---

## 01DEP-P01-G1-R2 — correction recheck of architecture gate G1, decision revision r2

### Identity

- Date 2026-11-09. Workflow `implement-opus-reviewed-2_2`. Records: `TASK.md`, `REVIEW.md`,
  `agent-review-findings.md`.
- Batch B1; stage: correction recheck of gate G1. Phase `p01`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`.
  Review base and HEAD are both `3752ea3540e3a2bf6d17178bf887fe2e99a384ac`.
- Gated decisions DEP-P01-D1..D5 at **r2**. Immutable copy `logs/gate-decisions-r2.md` has sha256
  `184eef17f46d7644b3536c004882ccde9b4f733161bf071ec8147e1a408823cd`, which I verified. The TASK.md
  sections from "Architecture gate — decisions (revision r2)" through "Contract impact" are
  byte-identical to it. r1 is preserved at `logs/gate-decisions-r1.md`, sha `c72805f2…b572`, unchanged.
- Code snapshot (informational for a gate): `ca321ded89ea8a04f4d4529b55299e8001c86dd798a7cdb3661b4ed592037eed`.
  It is unchanged since R1's end. The same non-gated domain edits are in the tree, and the author's
  schema and persistence drafts are outside the repo, so they are not reviewed.
- Coordination: revision 1, sha256 `babfa9b2…128b`, equal to the pinned `logs/coordination-r1.md`;
  I001–I003/r1; no notices. I checked this at the start and before returning.

### Coverage and evidence

- **Recheck by finding ID**, F1–F9, against the r2 text and against code. I reused R1's evidence where
  r2 left the text unchanged:
  - D1's table, key and index list;
  - D2's shape function, which matches R1 Probe A;
  - D3's lead and idempotency semantics and its `_on` set;
  - D5's types and placement apart from `DepartmentEdit`.
- **Code checks for the r2 claims:**
  - `principals_company_id_id_kind_key UNIQUE (company_id, id, kind)` exists (init schema `:6600`),
    so updating `kind` takes `FOR UPDATE`, which conflicts with `FOR KEY SHARE`.
  - The three demotion paths all set `kind`: `company_invite.rs:566`, `agent.rs:418`,
    `user_lifecycle.rs:38`.
  - The only other `UPDATE principals` is the promotion external→person in
    `participant.rs:360`, reached from invite acceptance. Acceptance (`company_invite.rs:353-404`)
    locks the account and then the principal, and takes the company row only as an FK KEY SHARE.
    That cannot invert against a department writer's company `NO KEY UPDATE` → principal
    `KEY SHARE`.
  - The gate's callers are still only `channel_edit.rs:106` and `thread/inbound.rs:147`.
- **Executed (fixture-isolated):** scratch DB `openbot_dept_p01_review`, created and dropped by B,
  PostgreSQL 18.6. Log: `logs/01DEP-P01-G1-R2-sql-probes.txt`, sha256
  `c54867ccf54a4e9663d12827e8e966fe4c8c60ae78c59f3837b35cced9c38dfb`. This is my reconstruction of the
  r2 demotion trigger and the `FOR KEY SHARE` kind guard on the real init schema:
  - Case 0: a committed demotion deletes the member row and the lead row;
  - Case A: an insert waits about 2 s behind a held demotion, then is refused by
    `department_principal_kind`, leaving no row;
  - Case B: a demotion waits about 2 s behind a held member+lead insert, then its AFTER trigger
    deletes the committed rows (count 0).

  The author's own probe (on `openbot_dept_p01_sqlcheck`) is reported in the coordinator's message,
  but B did not inspect it.
- **Not executed:** no Rust build or test. The gate accepts the verification plan, not its
  execution.
- **Lightly reviewed:**
  - the full inbound and relay lock chains, beyond confirming that no company, department or
    target-principal lock comes after the gate;
  - identity resolution of demoted principals. p02 now owns the grant side of this through the
    trigger contract.

### Outcome

**PASS for decision revision r2.** The decision is DEP-P01-D1..D5 r2, at
`logs/gate-decisions-r2.md` sha256 `184eef17…23cd`.

- Fixed: F1, F2, F4, F5.
- F3 is fixed in the decision. A residual verification obligation is carried to the B1 batch review:
  - a name-only PATCH keeps parent, charter, description and slug;
  - `parent_id: null` makes the department a root.
- Optional suggestions: F6, F7 and F8 are fixed; F9 is dismissed with the author's reason, which I
  accept.
- New optional suggestion 01DEP-P01-G1-F10 (kind race):
  - test the reverse order;
  - in batch review, check that each `_on` takes the target principal's `FOR KEY SHARE` before
    writing join rows.
- Observation for B1, not a finding: `begin_bounded`'s 1000 ms `lock_timeout` constrains the
  held-company-row competing tests. A timeout must not count as the expected refusal.
- No item needs the user. The r2 demotion semantics (remove the rows) is the natural reading of
  decisions 4/5 and AC1.

### Process and instruction feedback

- One correction round resolved every actionable finding. The mapping from findings to changes in the
  coordinator's message, together with "r2:" markers in each decision, made the recheck by ID cheap.
- The author verified the race claims on a scratch DB of their own before the recheck, which is
  useful. Its log is not retained under `docs/logs/`, so it is not durable evidence.
  - Candidate instruction (scope: architecture-gate corrections): keep the author's probe output under
    the phase `logs/` directory when a revised decision cites it.
- No other instruction change is supported.

---

## 01DEP-P01-B1-R1 — batch review B1 (whole phase 01)

### Identity

- Date 2026-11-09. Workflow `implement-opus-reviewed-2_2`. Records: `TASK.md` (sections "B1
  implementation", "Verification mapping (actual, B1)", "Contract impact (actual, B1)", "Check
  results (B1-R1)"), `REVIEW.md`, `agent-review-findings.md`.
- Batch B1 = phase 01 (01A + 01B); stage: batch review. Phase `p01`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`.
  Phase start, review base and HEAD are all `3752ea3540e3a2bf6d17178bf887fe2e99a384ac`. All changes
  are uncommitted working-tree changes; new files are intent-to-add.
- Artifact digest `c2c75fadaa78619940c7b9c7b02ee20f9983d4d5116faa075d85a35d24ae07ed`, with
  exclusions `plan/01-departments/phases/p01/docs` and `plan/01-departments/COORDINATION.md`. I
  recomputed it at the start, before writing records, and before returning; it matches the 20 files in
  `logs/B1-R1-snapshot.txt`.
- Gated decisions DEP-P01-D1..D5 r2 (`logs/gate-decisions-r2.md`, sha `184eef17…23cd`, unchanged).
- Coordination revision 1 (sha `babfa9b2…128b`, equal to the pinned copy); I001–I003/r1; no notices.
  I checked this at the start and before returning.

### Coverage and evidence

- **Whole diff read** (`git diff HEAD` plus the new files):
  - init schema: the provenance check in place, and the appended Departments block with its 3 tables,
    `department_shape_problem`, `guard_department_shape`, `guard_department_principal_kind`
    (`FOR KEY SHARE`) and `department_principal_demoted`;
  - domain: `department.rs`, the `GrantProvenance::DepartmentMembership` variant plus `ALL`,
    `DepartmentSlug`;
  - the port, with `DepartmentWrite`, `DepartmentEdit` and `DepartmentScope`;
  - use cases: `managed()` mapping Forbidden→NotFound, `visible_*`, normalization;
  - persistence adapter: every `_on` function and every trait method;
  - JSON routes and their tests;
  - wiring: AppState and its `FromRef`, `infra::setup`, `accounts.rs`, routes, the `mod.rs` files,
    `SlugKind::DepartmentAddress`.
- **Requirements traced** against README decisions 1–12 and 01A/01B items 1–5 / 1–4:
  - 01A.5 (delete refusal) is FK plus preflight `Conflict`;
  - 01B.2's channel locking is deferred to p02, as the plan intends;
  - 01B.2's `slug_conflict_error` reuse is superseded by D5's `Conflict`.
- **Gated decisions checked against the code; all hold:**
  - D1: tables, keys, composite FKs, indexes, partial lead index with its deliberate literal, NO
    ACTION parent FK, appended block;
  - D2: shape function with both walks, trigger raising `departments_shape`, preflight using the same
    function, kind guard `FOR KEY SHARE`;
  - D3: `begin_bounded`, then `lock_company_on`, then department `FOR UPDATE` / parent `FOR SHARE`,
    then `check_principal_on` before join-row writes; lead replace in one transaction;
    idempotent add; NotFound for remove/clear of a non-holder; delete `Conflict` with children;
  - D4: the use cases' `managed()`, `visible_*` via `readable_company`; every write route goes
    through the use cases;
  - D5: placement, tri-state edit merged after the row lock, `Conflict` wording, `/api` routes,
    role parsed with `FromStr`.
- **Concurrency, by inspection:**
  - no lock-order inversion found among department writes, `remove_member`, agent delete, account
    deletion, invite promotion and the demotion trigger;
  - `assign_*` deletes the old holder after locking the new holder's principal;
  - `clear`/`remove` take no principal lock, so they cannot invert against the trigger.
- **Executed by B:**
  - `cargo test --locked --offline --lib department -- --test-threads=4` → 32 passed
    (`logs/01DEP-P01-B1-R1-reviewer-department-tests.txt`, sha `c118afad…da63`). The filter `--list`
    also gave 32 names: schema 9, write 12 (2 of them unit tests), competing 5, routes 2, plus 4 unit
    tests. I confirmed the tests hit the DB: the departments table holds rows afterwards, and all 4
    department triggers exist;
  - affected callers and contracts (`workflow_routes_mount_in_the_app_router remove_member
    agent_retire user_lifecycle account_deletion legacy_runtime_schema company_invite
    every_grant_provenance participant:: use_cases::channel:: channel::tests::`) → 80 passed
    (`logs/01DEP-P01-B1-R1-reviewer-affected-tests.txt`, sha `06e372e2…ca6b`);
  - `cargo test --locked --offline --test source_modules` → 8 passed;
  - `git diff HEAD --check -- src migrations` → clean;
  - `cargo fmt --all -- --check` → clean.

  Fixture: phase-owned `openbot_dept_p01_test`, plus `own_database` for the competing tests;
  `SQLX_OFFLINE=true`; 4 threads; worktree `target/`.
- **Inspected, not rerun:** the author's clippy (`logs/B1-R1-clippy.txt`) and
  `sqlx prepare --check` (`logs/B1-R1-sqlx-prepare.txt`). All department queries are runtime queries,
  and each one is exercised by a DB test.
- **Evidence manifest:** `logs/01DEP-P01-B1-R1.evidence.json` validated with `check-evidence.py
  --expect-artifact c2c75fad…`: 6 PASS, 1 NOT EXERCISED (full gate, I003). Declared results are not
  acceptance; I checked the mapping myself, below.
- **Verification mapping checked against fixtures and assertions:**
  - every row's named test exists and asserts what the row claims;
  - G1 F3: `department_write_a_name_only_edit_keeps_every_other_field` asserts full-struct
    equality, so the residual obligation is met;
  - G1 F10: the reverse race test pauses a real add after its insert;
  - G1 F5: the competing tests hold the company row on a third connection and wait until 2 backends
    are blocked, and assert `Ok`+`BadRequest`, `Ok`+`Conflict`, and both `Ok` with one holder.
- **Gap found:** no test drives `DepartmentPersistence` writes directly with an unauthorized scope,
  so the under-lock re-authorization is established by inspection only (→ B1-F1).
- **Stand-in limits I accept:** handlers are called directly, not through the `Router`, and route
  mounting is covered by the full-router mount test. Account deletion is covered by the raw-demotion
  mechanism test plus its existing tests.
- **Reviewed lightly:** catalog-enumerating tests beyond those run (for example
  `control_wait_order_tests::public_state`, which snapshots every table — new empty tables do not
  change before/after equality); the HTTP CSRF and body-limit middleware, which the new router merge
  inherits unchanged.

### Outcome

**Changes required** for digest `c2c75fad…07ed`. There is no PASS.

- One actionable defect: 01DEP-P01-B1-F1 (Low): add the persistence-level re-authorization test,
  ideally with the demotion race. Nothing else blocks.
- Optional suggestions:
  - B1-F2: the schema comment restates the withdrawn NO ACTION rationale;
  - B1-F3: route tests lack the failing-persistence 500 case and the stranger/foreign list case;
  - B1-F4: tidy-ups (`&str` lock clause, stale `DepartmentWrite` doc, "nine steps" comment, test
    file size).
- G1-F3 closed, G1-F10 fixed (dated in the findings).
- A recheck can be narrow if the correction adds only tests: rerun the department filter and check
  the new test's assertions. Everything else in this round can be reused while the digest's other
  files are unchanged.

### Process and instruction feedback

- Batch size was reviewable in one pass. The persistence adapter (700 lines) and its tests (1,370
  lines) were the bulk of the work.
- The author's mapping table with exact test names and stand-in limits was accurate and saved time.
  Every claim I spot-checked held.
- Defect origin: an implementation-evidence gap, not a gated-decision defect.
- Candidate instruction (scope: verification mappings): when a plan cites an existing test as the
  model ("as X asserts"), list which of X's decisive assertions are mirrored (evidence B1-F1).

---

## 01DEP-P01-B1-R2 — correction recheck of batch B1

### Identity

- Date 2026-11-09. Workflow `implement-opus-reviewed-2_2`. Records: `TASK.md` "B1 correction
  (B1-R2)", `REVIEW.md`, `agent-review-findings.md`.
- Batch B1; stage: correction recheck. Phase `p01`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`. HEAD `3752ea35` plus the working tree; the
  two new test files are untracked and included in the snapshot.
- Artifact digest `ccc4ee26be865cf9806a1e192f05bdddb8753eb4993704e2b8f98fcc78f03880`, with the same
  exclusions. I recomputed it at the start and before returning. The previous reviewed digest was
  `c2c75fad…07ed`.
- Decisions r2 unchanged (`184eef17…23cd`). Coordination revision 1 unchanged (`babfa9b2…128b`);
  I001–I003/r1; no notices.

### Coverage and evidence

- **Recheck by finding ID**, B1-F1 to B1-F4.
- **Correction delta read:**
  - the two schema comments (`departments_parent_fk`, `department_shape_problem`) — comment-only, so
    the SQL is unchanged;
  - the port doc line;
  - `RowLock` in `department.rs`, with the same two clauses as before;
  - the test split, plus the 2 new tests and the route-test additions.
- **New tests checked for discriminating power:** the re-authorization test targets an existing
  holder and member, and the demotion race has the admin pass `managed()` before blocking. Both would
  fail without the under-lock check.
- **Moved tests:** spot-checked against R1. The count of named-constraint `refused_by` assertions
  is 14 in the schema file, and the competing tests' decisive assertions are unchanged. The inventory
  diff (`logs/B1-R2-test-inventory-diff.txt`) maps the 14 moved names and the 2 new ones; none are
  lost.
- **Executed by B:**
  - department filter → 34 passed (`logs/01DEP-P01-B1-R2-reviewer-department-tests.txt`, sha
    `a1728fb3…a32f`);
  - `test_support::tests:: legacy_runtime_schema workflow_routes_mount_in_the_app_router` → 10 passed.
    Run because the comment edits changed the init-schema checksum and file;
  - `--test source_modules` → 8 passed, so the new test files are connected;
  - `cargo fmt --check` clean;
  - `git diff --check` clean, and no trailing whitespace in the two new files.

  Fixture: the phase DB `openbot_dept_p01_test`, which the author recreated after a
  `VersionMismatch` (retained as FAIL evidence), plus `own_database`; `SQLX_OFFLINE=true`; 4 threads.
- **Evidence manifest:** `logs/01DEP-P01-B1-R2.evidence.json` validated against `ccc4ee26…`:
  4 PASS, 1 FAIL (the stale DB, retained), 1 NOT EXERCISED (full gate, I003).
- **Reused from B1-R1:** the B1-R1 caller and contract runs (80 passed), the inspection of the
  rest of the batch, and the author's clippy log. Since R1 only SQL comments, one doc line, the
  internal lock-clause type and tests changed.

### Outcome

**PASS for digest `ccc4ee26be865cf9806a1e192f05bdddb8753eb4993704e2b8f98fcc78f03880`.**

- B1-F1 fixed. B1-F2, B1-F3 and B1-F4 (all optional) fixed.
- One trivial doc leftover is noted under B1-F4 and does not block.
- No open actionable findings for p01 B1. The full repository gate was not run (I003), so this is
  not full verification.

### Process and instruction feedback

- Changing the init schema, even only its comments, changes the migration checksum. A test DB that
  was already migrated then fails with `VersionMismatch`. The author handled this correctly and kept
  the FAIL evidence.
  - Candidate (scope: persistence workflow notes): after any edit to
    `migrations/20260817000000_init_schema.sql`, including comment-only edits, recreate phase test
    databases before rerunning focused checks.
