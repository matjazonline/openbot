# p01 agent review findings

Shared findings log for phase `p01` of `plan/01-departments/` (workflow `implement-opus-reviewed-2_2`).
Records: task `TASK.md`, review `REVIEW.md`, notes `agent-review-notes.md` (all in this directory).

Common context for the G1 findings below (unless a section says otherwise):

- Date 2026-11-09; batch B1 (whole phase 01); stage: architecture gate G1; round note
  `01DEP-P01-G1-R1`; reviewer B = Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent
  `ad409b2d3d149c2c6`; worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch
  `codex/departments-addressing-plans`, HEAD `3752ea3540e3a2bf6d17178bf887fe2e99a384ac`.
- Reviewed: decision revision **r1** (DEP-P01-D1..D5), immutable copy `logs/gate-decisions-r1.md`
  sha256 `c72805f2d0d41d433dfa37e386daf1ad14c11c62fcc6fd72e526f302cadeb572`. Code snapshot at
  review start (informational for a gate) `71e8276a3fb431ca1b0f25397d70e3055f0eeadf609157b606b58e75795bd130`.
- SQL evidence: `logs/01DEP-P01-G1-R1-sql-probes.txt` (scratch DB `openbot_dept_p01_review`,
  PostgreSQL 18.6; my reconstruction of the decisions as written, not the author's code).

---

## 01DEP-P01-G1-F1 — A teammate or agent leaving the company stays a department member and lead as an `external` principal

- **Classification:** actionable defect · **High** · architecture / requirements (schema invariant,
  cross-subsystem contract). Originates in gated decisions D1 (cascade rationale) and D2 (kind guard),
  and affects D3's serialization claim.
- **Evidence:**
  - D1 justifies `department_members/department_roles (company_id, principal_id) → principals ON DELETE
    CASCADE` with "removing a teammate or agent principal must not be blocked by a department row".
    The codebase never deletes those principals: it **demotes** them in place.
    - `remove_member` (`src/adapters/persistence/company_invite.rs:549`, update at `:566`):
      `UPDATE principals SET kind = 'external', user_id = NULL …`; the doc comment above it says why
      (messages keep their author); `company_invite_tests.rs:426-451` asserts the principal survives as
      `external`.
    - Agent delete (`src/adapters/persistence/agent.rs:398`, update at `:418`): `kind = 'external',
      agent_id = NULL`; comment at `:412` "Grants stay on the demoted principal".
    - Account deletion (`src/adapters/persistence/user_lifecycle.rs:38`): same demotion across every
      company.
    - `messages_author_principal_fk` is NO ACTION, which is why principals are not deleted.
  - D2's `guard_department_principal_kind` fires only on INSERT/UPDATE of the join tables, so a later
    `UPDATE principals SET kind='external'` is never checked.
  - Reproduced (Probe B in `logs/01DEP-P01-G1-R1-sql-probes.txt`, real init schema + D1 tables):
    after the `remove_member` statements, the member row and the **lead** row both remain, joined to a
    principal of kind `external`.
  - Consequences: AC1's "external/system refused" and decision 4 ("the holder is an agent or a person")
    stop holding after ordinary product operations, not only after raw SQL. `visible_departments`/GET
    then list a removed person or deleted agent as member and lead. p02's
    `sync_department_grants_on` derives `participate`/`view` grants from these rows, and
    `Channel::participant_access` admits any principal with a participate grant on an allowlist channel
    regardless of kind (`src/domain/entities/channel.rs:135-152`). Whether a demoted principal can still
    be reached depends on what identity resolution does with it, which I did not trace. That risk is a
    hypothesis; the invariant break is observed. In p03 an agent lead that has been deleted stays
    "lead", with no defined vacancy.
  - D3's "all department writes of one company serialize on the company row" cannot hold for the
    cleanup either way: agent delete takes only the agent row (`agent.rs:405-409`), and account
    deletion takes users/owned companies/memberships but not the company lock of companies the user
    merely belongs to (`user_lifecycle.rs:14-27`).
- **Required:** state in r2 what happens to department rows when a principal leaves the company.
  The natural reading of decisions 4/5 and AC1 is that its `department_members` and
  `department_roles` rows go in the same transaction as the demotion, e.g. through an
  `AFTER UPDATE OF kind ON principals` trigger, which covers all three paths in one place, or through
  explicit cleanup in the three paths. Also say how that removal relates to D3's company-lock
  serialization and to p02's re-derivation hook, so p02 does not have to retrofit a writer outside
  `department.rs`. Correct the CASCADE rationale: CASCADE only matters at company teardown. Add
  `department_schema_`/`department_write_` cases: remove a member who is a department member and lead,
  and delete an agent that is a member; then assert no department rows remain for that principal.
  If the author would rather keep historical rows, that is a product decision to raise with the user
  rather than leave implicit.
- **Instruction lesson:** (observation) the gate cited the FK pattern of `channel_principal_grants`
  without checking how the referenced row actually leaves. Candidate: for a gated FK or cascade
  decision, list every production path that deletes **or mutates the discriminating column of** the
  referenced row (`graft grep` for `UPDATE <table> SET`), and give each one a disposition.
- **Resolution history:** 2026-11-09 open (G1-R1, decision r1).
  - 2026-11-09 **fixed** in decision r2 (recheck `01DEP-P01-G1-R2`, `logs/gate-decisions-r2.md` sha256 `184eef17…23cd`):
    - D1 adds `department_principal_demoted` (`AFTER UPDATE OF kind ON principals … WHEN (NEW.kind NOT IN ('person','agent'))`, deletes that principal's member and role rows), corrects the CASCADE rationale, states that losing the lead leaves the department without one, and sets the p02 contract to extend the trigger to `department_membership` grants.
    - D2's kind guard and the preflight read the principal `FOR KEY SHARE`. A demotion is a key update, because `principals_company_id_id_kind_key UNIQUE (company_id, id, kind)` exists (init schema `:6600`).
    - Verification rows added: raw demotion, `remove_member`, agent `delete`, and the add-member-behind-demotion race.
    - B's evidence, `logs/01DEP-P01-G1-R2-sql-probes.txt` (reconstruction on the real schema):
      - a committed demotion removes an existing member row and lead row;
      - an insert waits behind a held demotion and is then refused by `department_principal_kind`;
      - a demotion waits behind a held member+lead insert, and its trigger then deletes the committed rows.

      No order leaves a row on an external principal.

---

## 01DEP-P01-G1-F2 — D3's lock order leaves out the channel gate, the rule for p02's channel-settings writer, and the department row lock strength

- **Classification:** actionable defect · **Medium** · concurrency / architecture (stable contract
  p02 extends). Originates in gated decision D3.
- **Evidence:**
  - D3 fixes "company → department → (p02) the department's members-only channels" as the stable
    order. The existing channel protocol takes the **advisory gate first, then the channel row**, with
    no company lock (`src/adapters/persistence/channel_edit.rs:100-114`, gate doc
    `channel_gate.rs:1-25`). Only two callers take the gate: `channel_edit.rs:106` and
    `thread/inbound.rs:147`. Neither takes company or department locks after it, so company →
    department → gate is safe against **today's** code. The risk is in the p02 extension.
  - p02 runs `sync_department_grants_on` inside `write_channel_settings_on` when `members_only` or
    `department_id` changes (02B.2), which happens after that path's gate. With D3 as written, p02 has
    two options and both are broken:
    1. Read department membership without locking it. Then this interleaving loses an update: T1 adds a
       member to D (company lock, D locked). T2 toggles `members_only` on desk C, which D already owns
       (gate(C), C row; no FK check, so no lock on D). T1 lists D's members-only desks without C, since
       T2 is uncommitted. T2 derives grants from membership read before T1 commits. Both commit, and C
       is members-only without the new member's grant.
    2. Lock the department (`FOR SHARE`) after the gate. That inverts D3: T1 holds D `FOR UPDATE` and
       waits for gate(C), while T2 holds gate(C) and waits for D. Deadlock.
  - D3 picks `FOR UPDATE` for department rows without saying what that strength relies on.
    `FOR UPDATE` blocks the KEY SHARE check from p02's `channels.department_id` FK. That happens to
    serialize *assigning* a desk to D against a D write: the assigning edit waits for the D write to
    commit, and its later statements see the new membership. `FOR NO KEY UPDATE`, the company-row
    choice and its rationale in `authority.rs:39-44`, would not block the FK check, and the assign
    interleaving would then lose the update the same way as case 1. So the lock strength is a
    load-bearing part of the protocol and has to be stated together with the p02 rule.
  - `channel_gate.rs:58-61`: a writer that takes several gates must take them all in one ordered
    `acquire_channel_gates_on` call. Otherwise it can deadlock against inbound commits that hold shared
    gates in sorted order. Department writers will take several: decision 7 means a lead change on P
    also re-derives the members-only desks of P's child departments.
- **Required:** state in D3 (r2), as part of the stable contract:
  - Every transaction that writes department-derived grants takes the company write lock **before any
    channel gate**. That includes p02's channel-settings path when it changes `department_id` or
    `members_only`. An equivalent rule is acceptable if it is stated, for example locking the department
    rows before the gate.
  - Department writers take every affected desk's gate in one sorted call after the department rows.
  - The department row lock strength, and which of the properties above it is relied on for. Under
    the company-lock-first rule, either strength is correct. `FOR NO KEY UPDATE` then just avoids
    blocking unrelated FK inserts.
- **Instruction lesson:** (observation) the gate composed its order with the schedule writes but
  not with the advisory-lock protocol it declared it would extend. Candidate: a gated lock order must
  place every lock kind the dependent phases will take (advisory gates included) and name the order
  for each writer on the other side.
- **Resolution history:** 2026-11-09 open (G1-R1, decision r1).
  - 2026-11-09 **fixed** in decision r2 (recheck `01DEP-P01-G1-R2`):
    - D3 states the plan-wide order: account → company `FOR NO KEY UPDATE` → department rows → target principals `FOR KEY SHARE` → one sorted `acquire_channel_gates_on` → channel rows.
    - It requires every writer of department-derived grants, p02's channel-settings edit of `department_id`/`members_only` included, to take the company lock before any gate.
    - It names the desk set: the department's members-only desks, plus the children's on a lead change.
    - It says what each lock is relied on for: the company lock for consistency; the department row as an existence check under that lock, not relied on for the tree.
    - It leaves the demotion trigger's grant deletion to p02's gate. That is acceptable because p01 writes no grants.

    I checked this against code: the gate's only callers (`channel_edit.rs`, `thread/inbound.rs`) take no company, department or target-principal lock after the gate. `remove_member` already takes account → company → principal. Invite acceptance (`company_invite.rs:353-404`) locks the account and then promotes the principal without taking the company lock `FOR NO KEY UPDATE`, so it cannot invert against a department writer.

---

## 01DEP-P01-G1-F3 — A rename sent as PATCH also moves the department to the top level, clears its charter and changes its address

- **Classification:** actionable defect · **Medium** · API contract. Originates in gated decision D5.
- **Evidence:** D5 defines PATCH as "the full editable set `{ name, slug?, description?, charter?,
  parent_id? }` (PATCH carries the whole write, like the channel PUT)". The channel PUT it copies
  re-derives the slug from the name when it is omitted (`src/adapters/http/routes/channel.rs:759-765`)
  and documents "a PUT replaces rather than patches" (`:184-191`). Under the PATCH verb that the plan
  names (01B.4), a client sending `PATCH {"name": "New name"}` therefore re-roots the department
  (`parent_id` absent becomes `None`), clears `description` and `charter`, and changes the slug, which
  is the department's address and, in p03, the department desk's address. The AC4/AC5 round trip
  (rename, re-parent) would pass either way, so nothing in the verification plan catches it.
- **Required:** pick one and say so in r2:
  - (a) True PATCH semantics: an absent field is unchanged, `parent_id: null` makes the department a
    root, and an absent slug keeps the current slug. Add a round-trip case where a name-only PATCH keeps
    parent, charter and slug.
  - (b) PUT with replace semantics and a required `slug` on update, recorded as a deliberate deviation
    from the plan's verb.
- **Instruction lesson:** cause: an established pattern was copied under a different HTTP verb.
  No instruction change supported beyond the existing "routine use of established patterns" review.
- **Resolution history:** 2026-11-09 open (G1-R1, decision r1).
  - 2026-11-09 **fixed in the decision** in r2 (recheck `01DEP-P01-G1-R2`): D5 adds `DepartmentEdit` with `Option` name/slug and tri-state description/charter/parent_id (absent = unchanged, `null` = clear / make root), merged under the row lock. PATCH changes only present fields.
    - **Residual verification obligation, carried to the B1 batch review** (it does not block the gate): the r2 rows do not name the decisive case. B1's mapping must include a name-only PATCH that leaves parent, charter, description and slug unchanged, and a `parent_id: null` PATCH that makes the department a root. Until those exist this stays a tracked gap.
  - 2026-11-09 **closed** at B1-R1 (`01DEP-P01-B1-R1`, digest `c2c75fad…07ed`): the carried verification obligation is met.
    - `department_write_a_name_only_edit_keeps_every_other_field` asserts full-struct equality after a name-only edit, so slug, description, charter and parent are unchanged. It also asserts that `parent_id: None` re-roots the department.
    - `department_routes_round_trip_and_refuse_an_unknown_role` covers JSON `{"charter": null}` clearing and a name-only PATCH keeping the slug.
    - Both pass in B's run (`logs/01DEP-P01-B1-R1-reviewer-department-tests.txt`).

---

## 01DEP-P01-G1-F4 — The decisions don't say which JSON reads a plain member may use

- **Classification:** actionable defect · **Low** · authorization model / requirements. Originates
  in gated decisions D4/D5.
- **Evidence:** The plan (01B.3) routes plain-member reads through `visible_departments`. D4 defines
  that method but only says "writes and manager reads start with `managed_company`" without listing
  which reads those are. D5 lists `GET` list and `GET` one without saying which use-case method each
  calls for a member. The AC6 verification row asserts "404 for member and foreign" on the JSON handlers.
  If that includes `GET /departments`, then `visible_departments` has no HTTP consumer and a member's
  list read is refused. If it does not, the row is wrong. The same question applies to `GET` one for a
  department the member belongs to.
- **Required:** in r2, give each JSON route the method it calls and the status or result for a plain
  member. Suggested: the list returns `visible_departments`. `GET` one returns the department for a
  member or role holder and 404 otherwise, or 404 for every non-manager if that is the intent. Make
  the AC5/AC6 rows assert exactly that.
- **Instruction lesson:** cause unknown. No change supported.
- **Resolution history:** 2026-11-09 open (G1-R1, decision r1).
  - 2026-11-09 **fixed** in decision r2 (recheck `01DEP-P01-G1-R2`): D4 names `visible_departments`/`visible_department` (`readable_company`) behind `GET` list and `GET` one for plain members, says that a department the member cannot see is `NotFound`, and says every write route returns 404 to member, stranger and foreign company. The AC6 row asserts exactly that.

---

## 01DEP-P01-G1-F5 — The concurrency tests as planned would pass even without the company lock

- **Classification:** actionable defect · **Low** · verification (the gate accepts the verification
  plan). Originates in the TASK "Verification plan" r1.
- **Evidence:**
  - AC4 competing row: "two lead assigns … exactly one holder". The partial unique index
    `department_roles_one_lead` also yields exactly one holder without the company lock. The second
    claimant then fails with 23505 instead of replacing the holder. So "one holder" does not show that
    the protocol works. What does is: **both** assignments return `Ok`, the final holder is the one
    committed last, and the first holder's row is gone.
  - Same-slug creates: the `UNIQUE` backstop gives one row with or without the lock. The decisive
    assertion is one `Ok` plus one `AppError::Conflict`, not a raw database error.
  - The tests have no step that forces the claimants to overlap. Two spawned tasks often run one after
    the other. The repository already forces overlap by holding the company row `FOR NO KEY UPDATE`
    on a third connection while the claimants start, then releasing it
    (`src/adapters/persistence/company_access_level_race_tests.rs:7-34`).
  - Depth boundaries: the AC1 depth row should include the exact re-parent boundaries: parent at level
    5 plus subtree height 3 is allowed, level 6 plus height 3 is refused (shown by the reconstruction
    in Probe A), and the insert-at-8 versus insert-at-9 pair.
- **Required:** amend the AC4 competing and AC1 depth rows as above before implementation.
- **Instruction lesson:** (observation) matches the verification-contracts guidance on
  discriminating cases. Candidate for a competing-claimant row: "name the backstop that would also
  produce the asserted outcome, and assert something it would not produce".
- **Resolution history:** 2026-11-09 open (G1-R1, decision r1).
  - 2026-11-09 **fixed** in r2 verification rows (recheck `01DEP-P01-G1-R2`):
    - Overlap is forced behind a company row held on a third connection (`own_database`).
    - Re-parents: one `Ok` + one `BadRequest` and one parent set. Creates: one `Ok` + one `Conflict`. Leads: both `Ok`, one holder. Each of these distinguishes the protocol from the backstops.
    - The depth row has exact boundaries: a 7-deep subtree under level 2 is refused (9), under a root accepted (8); the 9th-level insert is refused.

    Implementation note for B1, not a finding: `begin_bounded` uses a 1000 ms `lock_timeout`. The held row must be released promptly once both claimants are seen blocked, and a lock-timeout refusal must not count as the expected `BadRequest`/`Conflict`.

---

## 01DEP-P01-G1-F6 — The stated reason for NO ACTION is wrong: RESTRICT does not block company teardown

- **Classification:** optional suggestion · Low · architecture (rationale accuracy). D1.
- **Evidence:** D1 picks NO ACTION for `departments_parent_fk` "so a company teardown cascade …
  is checked at statement end". In Probe B (real init schema, PostgreSQL 18.6), `DELETE FROM companies`
  succeeds with a three-level tree, a member, a lead and a desk owned by a department. It succeeds with
  RESTRICT on both the parent FK and p02's `channels.department_id` FK, and with NO ACTION on both.
  Direct deletes of a parent with children, or of a department owning a desk, are refused under either
  action. NO ACTION is harmless; the reason given for it is not accurate.
- **Suggested:** correct the rationale, or use RESTRICT as the plan does for p02. Either way, p02
  should not depart from its plan's `ON DELETE RESTRICT` on the strength of this rationale. Keep the
  `department_schema_` teardown test (nested tree, members, roles) as the regression guard.
- **Resolution history:** 2026-11-09 open (optional; G1-R1).
  - 2026-11-09 **fixed** (optional) in r2: D1 withdraws the RESTRICT claim and keeps NO ACTION as the default. p02 keeps its plan's RESTRICT.

---

## 01DEP-P01-G1-F7 — Department writes should lock the actor's account row first, as nested agent publication does

- **Classification:** optional suggestion · Low · concurrency. D3.
- **Evidence:** D3's transaction wrappers call `authorize_company` directly. p03 will nest
  `assistant_channel_on` inside `create_department_on`, and that reaches `publish_initial_on` →
  `lock_company_on`, which takes `users … FOR SHARE` (`agent_publication/mod.rs:50-61`; the comment at
  `:62-64` says callers take the actor account before any company lock). The standalone callers take
  the user row first: `provisioning.rs:772-778` and `channel.rs:513-523`. The Builder action path
  (`provisioning.rs:44-53`) already takes the company row before the user row, so this order is not
  enforced everywhere. The only deadlock partner I found is the actor's own account deletion
  (`user_lifecycle.rs:14-27`), which is rare.
- **Suggested:** have the department wrappers use `lock_company_on` (user row, then company), so the
  p03 nesting keeps the documented order.
- **Resolution history:** 2026-11-09 open (optional; G1-R1).
  - 2026-11-09 **fixed** (optional) in r2: D3 department writes use `agent_publication::lock_company_on` (users `FOR SHARE`, then `authorize_company`).

---

## 01DEP-P01-G1-F8 — Authorization messages and route test cases should be stated explicitly

- **Classification:** optional suggestion · Low · authorization / verification. D4.
- **Evidence:**
  - The under-lock re-check `authorize_company` refuses with
    `AppError::NotFound("Workflow resource")` (`workflow/mod.rs:124-126`). In a demotion race the
    department API would return that message instead of D4's department or company not-found wording.
  - `channel_authorization_tests.rs` uses `Fixture::denial_cases()`, which expects **403** for a plain
    member (`authorization_test_support.rs:145-172`). D4 deliberately returns 404, following the
    plan's `department_write_` text. The AC6 row already says 404. D4 could add that the route tests
    use their own case table rather than `denial_cases()`, so the batch review can check this was
    intentional.
- **Suggested:** map the adapter's re-authorization `NotFound` to the department/company wording.
  Record the 404-for-member deviation from the channel route convention in D4.
- **Resolution history:** 2026-11-09 open (optional; G1-R1).
  - 2026-11-09 **fixed** (optional) in r2: D3 re-words the under-lock `NotFound` to `company_not_found()`. D4 records that the route tests assert 404 for members and do not reuse `denial_cases()`.

---

## 01DEP-P01-G1-F9 — Make the `_on` precondition a type rather than a comment

- **Classification:** optional suggestion · Low · architecture. D3.
- **Evidence:** D3's `_on` functions require that "the caller has already taken the company write
  lock", but nothing checks it. p02 and p03 add callers from other subsystems. The repository already
  has a type that witnesses a lock: `LockedAgentPublication` (`agent_publication/mod.rs:65-91`).
- **Suggested:** a small `LockedCompany<'a, 'tx>` (or similar) returned by the lock function and
  required by every `department::*_on`. That turns the precondition into a compile-time obligation for
  p02 and p03 callers.
- **Resolution history:** 2026-11-09 open (optional; G1-R1).
  - 2026-11-09 **dismissed** (optional; author declined in D3 r2). Reason accepted: the `_on` callers are few, each sits behind `lock_company_on`/`assistant_channel_on`, and a witness type would have to be threaded through p03's existing provisioning API. Batch review still checks each `_on` caller holds the company lock.

---

## 01DEP-P01-G1-F10 — Kind race: test the reverse order too, and lock the principal before writing its rows

- **ID and context:** 2026-11-09 · `implement-opus-reviewed-2_2` · batch B1 · correction recheck of
  gate G1 · round `01DEP-P01-G1-R2` · B = Opus 5.5, agent `ad409b2d3d149c2c6` · reviewed decision
  **r2** (`logs/gate-decisions-r2.md` sha256 `184eef17f46d7644b3536c004882ccde9b4f733161bf071ec8147e1a408823cd`).
- **Classification:** optional suggestion · Low · verification / concurrency. D1/D2 r2.
- **Evidence:**
  - The r2 "D2 kind race" row covers only an add-member waiting behind a held demotion. D2's own
    argument has a second order: the demotion waits behind a held join-row insert, and its trigger
    then deletes the committed row. Nothing in the plan tests that order. I showed it on a
    reconstruction (Case B, `logs/01DEP-P01-G1-R2-sql-probes.txt`), but that does not exercise the
    author's trigger.
  - When the demotion trigger runs, it holds the principal `FOR UPDATE` and then takes join-row locks.
    Suppose an `_on` function wrote a join row for that principal (for example, deleted its member row)
    and only afterwards took `FOR KEY SHARE` on the same principal. The two transactions would then
    wait on each other. D3's lock order puts principal locks before writes, so this is an
    implementation-ordering point, not a decision defect.
- **Suggested:**
  - Add the reverse-order case to `department_competing_`: hold a member+lead insert open, demote,
    commit both, and assert no rows remain.
  - In the B1 batch review, check that every `_on` that touches join rows takes the target
    principal's `FOR KEY SHARE` before writing them.
- **Instruction lesson:** none supported (task-specific).
- **Resolution history:** 2026-11-09 open (optional; G1-R2).
  - 2026-11-09 **fixed** at B1-R1 (`01DEP-P01-B1-R1`):
    - `department_competing_demotion_behind_a_member_add_removes_the_row` pauses a real add after its principal lock and insert, using an advisory-lock trigger on its own database. It then blocks a demotion behind it and asserts the trigger removed the committed row. It passes in B's run.
    - By inspection, `assign_department_role_on` and `add_department_member_on` (and `create_department_on` through assign) call `check_principal_on` (`FOR KEY SHARE`) before writing join rows. `clear_*`/`remove_*` only delete (`src/adapters/persistence/department.rs`).

---

## 01DEP-P01-B1-F1 — No test checks that department writes are re-authorized inside the transaction

- **ID and context:** 2026-11-09 · `implement-opus-reviewed-2_2` · batch B1 (whole phase 01) ·
  batch review · round `01DEP-P01-B1-R1` · B = Opus 5.5, agent `ad409b2d3d149c2c6` · worktree
  `/Users/mac02/.codex/worktrees/2050/openbot`, HEAD `3752ea35` · artifact digest
  `c2c75fadaa78619940c7b9c7b02ee20f9983d4d5116faa075d85a35d24ae07ed`.
- **Classification:** actionable defect · **Low** · verification / security. This is an
  implementation-evidence gap; the gated decision D4 is sound.
- **Evidence:**
  - D4 r2: "Persistence re-authorizes under the lock (D3)". The plan's `department_write_` check says
    a member or non-team user gets `NotFound` on every write "as the schedule tests assert",
    referencing `schedule_management_requires_an_owner_or_admin_who_sees_the_channel`. That test also
    calls persistence directly, skipping the use-case preflight, and asserts `NotFound` there too
    (`src/adapters/persistence/workflow/schedule_tests.rs:657-671`).
  - `department_write_refuses_members_and_strangers_with_not_found`
    (`src/adapters/persistence/department_tests.rs:941-1031`) goes only through `DepartmentUseCases`.
    Its member and stranger refusals therefore come from the use case's `managed()`.
    `DepartmentPersistence` write methods are never called with an unauthorized `DepartmentScope`;
    a search for direct `persistence.create/update/…` calls in the department tests finds none.
  - By inspection the property holds today. Every trait write starts with `begin_department_write`,
    which calls `lock_company_on`, which calls `authorize_company`, which refuses non-managers with
    `NotFound`; the result is mapped to `company_not_found()` (`department.rs:102-121`, impl
    `:470-552`). But nothing would fail if a later edit dropped or bypassed that call. p02 and p03 will
    add `_on` callers.
- **Required:** add a DB-backed test that calls each `DepartmentPersistence` write directly with a
  plain member's and a stranger's `DepartmentScope`, and asserts `NotFound` with nothing changed.
  Preferably also add the demotion race in the style of `company_access_level_race_tests.rs:7-34`:
  hold the company row, demote an admin to member in the holding transaction, commit, and assert
  that the admin's waiting write is refused.
- **Instruction lesson:** (observation) a plan that says "as test X asserts" carries all of X's
  decisive assertions. Candidate for verification mappings: when a criterion cites an existing test
  as the model, list which of its assertions are mirrored.
- **Resolution history:** 2026-11-09 open (B1-R1).
  - 2026-11-09 **fixed** (recheck `01DEP-P01-B1-R2`, digest `ccc4ee26be865cf9806a1e192f05bdddb8753eb4993704e2b8f98fcc78f03880`):
    - `department_write_persistence_reauthorizes_every_write_under_the_lock` calls all seven `DepartmentPersistence` writes directly with a member's and a stranger's scope. The targets are an existing holder and member, so `NotFound` cannot come from "not a holder". Every call returns `NotFound`, and a before/after comparison of departments, members and roles shows nothing changed.
    - `competing::department_competing_write_behind_an_admin_demotion_is_refused` demotes the admin inside a transaction holding the company row. The admin's create passes `managed()` and then blocks on the lock. After the demotion commits it gets `NotFound`, and 0 departments exist.
    - Both would fail if the under-lock check were dropped. Both pass in B's run (`logs/01DEP-P01-B1-R2-reviewer-department-tests.txt`).

---

## 01DEP-P01-B1-F2 — The schema comment still gives the withdrawn reason for NO ACTION

- **ID and context:** as B1-F1 (round `01DEP-P01-B1-R1`, digest `c2c75fad…07ed`).
- **Classification:** optional suggestion · Low · maintainability (implementation). D1 r2.
- **Evidence:** `migrations/20260817000000_init_schema.sql`, `departments_parent_fk` comment: "NO
  ACTION rather than RESTRICT: a company teardown deletes parents and children in one cascade,
  checked when that statement ends". D1 r2 withdrew the claim that RESTRICT would break teardown:
  `logs/01DEP-P01-G1-R1-sql-probes.txt` shows RESTRICT tears down too. p02 is meant to keep RESTRICT
  for `channels.department_id`, and this comment invites the opposite reading.
- **Suggested:** say NO ACTION is the default and that both actions refuse orphaning, or drop the
  "rather than RESTRICT" clause.
- **Resolution history:** 2026-11-09 open (optional; B1-R1).
  - 2026-11-09 **fixed** (optional) (recheck `01DEP-P01-B1-R2`, digest `ccc4ee26be865cf9806a1e192f05bdddb8753eb4993704e2b8f98fcc78f03880`): the comment now reads "Deleting a parent that still has children is refused; a company teardown removes the tree."

---

## 01DEP-P01-B1-F3 — Route tests leave out the failing-persistence case and the non-member list case

- **ID and context:** as B1-F1.
- **Classification:** optional suggestion · Low · verification (AC6).
- **Evidence:** `channel_authorization_tests.rs` (via `Fixture::denial_cases()`,
  `authorization_test_support.rs:145-172`) includes a third case: company persistence failing →
  500. That case guards the `src/AGENTS.md` rule "Don't collapse errors into defaults on
  authorization paths". `department_routes_hide_foreign_companies_and_refuse_members_and_strangers`
  covers owner-on-foreign, member and stranger, but has no failing case. It also does not call
  `list_departments_json` for the stranger or the foreign company. That case is covered only at the
  use-case level (`department_write_shows_a_member_only_their_departments`, stranger → NotFound).
  By inspection, `managed()` and `readable_company` propagate errors other than `Forbidden`.
- **Suggested:** add the `failing_companies()` case (500) for one write route and the list route,
  and the list route for the stranger and the foreign company (404).
- **Resolution history:** 2026-11-09 open (optional; B1-R1).
  - 2026-11-09 **fixed** (optional) (recheck `01DEP-P01-B1-R2`, digest `ccc4ee26be865cf9806a1e192f05bdddb8753eb4993704e2b8f98fcc78f03880`): the route test adds list 404 for owner→foreign and for the stranger, and 500 for create and list with `MockCompanyPersistence::failing()`.

---

## 01DEP-P01-B1-F4 — Small tidy-ups

- **ID and context:** as B1-F1.
- **Classification:** optional suggestion · Low · maintainability.
- **Evidence and suggestions:**
  - `department_on(.., lock: &str)` (`src/adapters/persistence/department.rs`) formats a caller
    string into SQL to choose the lock clause. Every caller passes a literal, but a small `RowLock`
    enum would make the choice closed. This is the `src/AGENTS.md` "No flag parameters" and
    bound-parameter spirit.
  - The `DepartmentWrite` doc in `src/application/ports/department.rs` still says "Everything one
    department create or edit sets … ignored by an edit". Edits use `DepartmentEdit` now.
  - `department_shape_problem` says "Both walks stop after nine steps", but `level <= 9` allows a
    tenth row. This is harmless.
  - `src/adapters/persistence/department_tests.rs` is 1,370 lines, past the `src/AGENTS.md` ~1,000-line
    guidance. Its `schema_` / `write_` / `competing_` sections are natural seams.
- **Resolution history:** 2026-11-09 open (optional; B1-R1).
  - 2026-11-09 **fixed** (optional) (recheck `01DEP-P01-B1-R2`, digest `ccc4ee26be865cf9806a1e192f05bdddb8753eb4993704e2b8f98fcc78f03880`):
    - a `RowLock` enum replaces the `&str` clause;
    - the `created_by` doc is fixed;
    - the shape comment now says the walks "stop at level ten";
    - the tests are split into `department_tests.rs` (950 lines), `department_schema_tests.rs` and `department_competing_tests.rs`. The inventory diff shows 14 moved, 2 new, none lost; I spot-checked that the moved tests' assertions are unchanged.

    Trivial leftover, not tracked further: the `DepartmentWrite` struct doc (`ports/department.rs:20`) still says "create or edit".
