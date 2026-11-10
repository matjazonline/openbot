# p02 agent review notes

## 01DEP-P02-G1-R1 — architecture gate G1, decision revision r1

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Task `TASK.md`, review `REVIEW.md`,
  findings `agent-review-findings.md` (this directory).
- Batch B1 (02A + 02B); stage: architecture gate. Phase `p02`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`.
  Start, base and HEAD are all `57adfa7ab8ff24d4575365de8e06ddf00849aa59`, which is p01 accepted at
  digest `ccc4ee26…3880`.
- Gated decisions DEP-P02-D1..D7 **r1**. `logs/gate-decisions-r1.md` sha256
  `6c5ec0d1e4ceed6957c9943982df51b71579820ae5f5c692b37ead1cdcdbd214`. `TASK.md` was byte-identical to it at the start
  (`cmp`). At the end its only difference was one appended Status line about delegating B2 to an
  author helper (`diff`); the decision and verification sections are unchanged.
- Code snapshot (informational for a gate), exclusions `plan/01-departments/phases/p02/docs`
  `plan/01-departments/COORDINATION.md`: `bc9109849255ec41795e7937daa7cd8a58bdd5d83d04cb15df4cd329d4590ddc`,
  with no files changed at start. The coordinator noted that a separate helper is editing B2 wording
  files in this worktree meanwhile; that work is outside this gate.
- Coordination: `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md`
  **revision 2**, sha256 `d33f363cf590a516940b769c7a08168257304737acb6f391af76bda35086d02f`, equal
  to the pinned `logs/coordination-r2.md`. Applicable: I001–I003/r1, **01DEP-C001/r1** and
  **01DEP-C002/r1** (both active, both acknowledged by p02). I checked this at the start and before
  returning.

### Coverage and evidence

- **Requirements read:** README decisions 2, 6, 7, 9; `02-desks-ownership-and-access.md` (02A, 02B,
  focused checks), as amended by C001 and C002; what `03-…` needs from p02 (03B.1/03B.3 desk
  ownership inside department writes; 03C.3 escalation per C002); the AGENTS files listed in the
  assignment.
- **C001 code facts verified:**
  - `authorize_channel` refuses a reappearing desk with `LoopCycle` for internal origins
    (`thread/ingest/routing.rs:288-292`);
  - it skips the `participant_access` refusal and the thread-party check for internal origins
    (`:296-311`);
  - `InternalRelayMail` carries `source_channel_id`, and the sender address is checked against it,
    but no actor principal (`transport/ports.rs:184-198`, `relay.rs:189-210`).

  The premise of plan 02B.5 is false, as the notice says.
- **Code inspected for the decisions:**
  - channel persistence: `CHANNEL_SELECT`, `channel_access`, `insert_email_allowlist_grants`,
    `insert_channel_row_on`, `create`, `write_channel_settings_on`, `delete`;
  - `channel_edit.rs` (gate → row, `EditBudget`) and `channel_gate.rs`;
  - the grants PK and conflict targets (`channel.rs:249-252`, `account_identity.rs:235-240`);
  - every production `UPDATE channels`, of which `write_channel_settings_on` is the only one;
  - `notification_recheck_channel_access`, which locks the channel row `FOR UPDATE` on every grant
    write;
  - FKs from inbound tables to channels and principals;
  - `resolve_or_create_external_identity_on`, which uses an advisory identity lock and no principal
    row lock;
  - the p01 adapter and trigger contracts at `57adfa7a`.
- **D4 affected sets checked against C002**, case by case:
  - member add/remove, lead change, create, re-parent, delete: correct as listed;
  - "a department whose parent is deleted" cannot occur, because a parent with children is
    refused;
  - a role holder who is also a member is handled by the full recompute, and duplicates by
    `ON CONFLICT DO NOTHING`;
  - a desk moving between departments is covered by deleting first;
  - a desk **leaving** members-only is not specified (→ F3);
  - demotion is covered by D5's direct delete.
- **Executed (fixture-isolated):** scratch DB `openbot_dept_p02_review` (created and dropped by B),
  init schema of `57adfa7a`, PostgreSQL 18.6. I reproduced the grant provenance collision in both
  orders with the production statements (`logs/01DEP-P02-G1-R1-sql-probes.txt`, sha256
  `e9a994b6…c168b`) (→ F1).
- **Not executed:** no cargo. That is not needed for a gate, and B2 edits plus the shared `target/`
  lock are in flight.
- **Reviewed lightly:**
  - the inbound commit's exact statement order. The F4 deadlock with inbound commits is a
    hypothesis and was not traced;
  - whether a demoted principal still matches as a thread party (F8);
  - B2 / 02C (D7), at batch level only. It gets its own review.

### Outcome

**Changes required** for decision revision r1. There is no PASS.

- Actionable:
  - 01DEP-P02-G1-F1 (High): grants for the same principal and capability collide across
    provenances, so derived and allowlist access overwrite each other;
  - 01DEP-P02-G1-F2 (Medium): the `ownership: None` save path breaks the members-only allowlist
    CHECK;
  - 01DEP-P02-G1-F3 (Medium): derived grants are not specified to go when a desk leaves
    members-only;
  - 01DEP-P02-G1-F4 (Low): the residuals are incomplete — inbound-commit inversion, demotion revision
    bump without the gate;
  - 01DEP-P02-G1-F5 (Low): no contract-impact table.
- Optional: F6 (gate waits under a 1 s `lock_timeout`), F7 (named `_on` ownership setter for p03,
  PUT exception documented, under-lock re-authorization), F8 (thread-party posting after removal).
- **Sound as written:**
  - D1: schema and FK placement, RESTRICT plus teardown test, index;
  - D2's `ownership: Option` choice. The reason holds, since the forms replace `ChannelWrite`
    wholesale and get no fields until phase 04. Taking the company-lock prefix only for ownership
    changes is consistent with p01's rule: every writer that changes `department_id` or
    `members_only` takes the company lock, and `None` edits cannot change the desk set;
  - D3's principal `KEY SHARE` before gates, which covers the agent-delete race;
  - D4's eligibility rule = C002;
  - D6 per C001;
  - D7 batching.
- No item needs the user. The fix for F1 is a technical choice that delivers the plan's stated
  outcome.

### Process and instruction feedback

- Gate size was appropriate.
- C001 and C002 were correctly raised to the user before the gate; the notices are precise and
  checkable.
- Defects are in the gated decisions: F1–F4.
- Candidate instructions (scope: architecture gates):
  - (1) for a new discriminator value, check whether the table's keys include the discriminator,
    and list every writer and conflict target (evidence F1);
  - (2) for a new CHECK over existing columns, list every writer of those columns (F2);
  - (3) for new writes inside a trigger, follow the triggers they fire, and list the lock footprint
    (F4).

---

## 01DEP-P02-G1-R2 — correction recheck of architecture gate G1, decision revision r2

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Records: `TASK.md`, `REVIEW.md`,
  `agent-review-findings.md`.
- Batch B1; stage: correction recheck of gate G1. Phase `p02`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, HEAD `57adfa7a`. B2 helper edits are in
  flight in `pages/**` and elsewhere (informational, outside this gate).
- Gated decisions DEP-P02-D1..D7 **r2**: `logs/gate-decisions-r2.md` sha256
  `0bee3eb0a7ff04b0e67322e8e97913404da355b3ac1607534bf6f866c9061bd6`. The TASK sections from
  "Architecture gate — decisions (revision r2)" through "Batches" are byte-identical to it. The r1
  copy is unchanged (`6c5ec0d1…d214`).
- Coordination revision 2 (`d33f363c…d02f`, equal to the pinned copy); C001/C002 active and
  acknowledged. I checked this at the start and before returning.

### Coverage and evidence

- **Recheck by finding ID, F1–F8,** against the r2 text and the code. I reused R1's evidence for
  the parts r2 left unchanged:
  - D1's FK, index and teardown;
  - D2's `ownership: Option` reason;
  - D4's C002 rule and its affected sets;
  - D6's relays;
  - D7.
- **F1 readers checked exhaustively:** I searched `channel_principal_grants` across `src` and the
  schema. Every production reader is existence-based or tolerates two rows:
  - `CHANNEL_SELECT` (`participant_emails` stays configured-only);
  - `has_capability`/`preferred_approver`;
  - `notification_principal_can_view`;
  - `decisions.rs:317`;
  - `association.rs:38-45`.

  A full search of the old conflict target found the two production writers r2 lists, plus one
  p01 test (→ optional F9).
- **F4 item 3 checked against code:** account deletion, `remove_member`, agent delete, p03 nesting
  and the inbound commit. The no-cycle argument holds.
- **F6 cited code checked:** `app_error_impl.rs:48` carries the 503 message.
- **Not executed:** no cargo or SQL this round. The r1 probe established the collision, and r2
  changes only the decision text.

### Outcome

**PASS for decision revision r2** (`logs/gate-decisions-r2.md` sha256 `0bee3eb0…1bd6`).

- F1–F5 fixed. F6 and F8 (optional) fixed. F7 fixed in part, with its re-authorization part
  declined; I accept the reason.
- New optional suggestion 01DEP-P02-G1-F9: the p01 schema test uses the old conflict target and
  will fail at runtime under the new key. B1's `department_` run would catch it.
- No item needs the user.

### Process and instruction feedback

- One correction round resolved every actionable finding. The per-finding mapping and the "r2 (Fn)"
  markers made the recheck cheap.
- Candidate instruction (scope: contract-impact tables for runtime SQL): include test fixtures that
  name a changed constraint or conflict target, found with an exhaustive literal search, because
  the compiler does not check runtime SQL (evidence F9).

---

## 01DEP-P02-B2-R1 — batch review of B2 (02C, the product word "desk")

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Records: `TASK.md` "B2 implementation
  (02C desk wording)", `REVIEW.md`, `agent-review-findings.md`.
- Batch B2 = 02C only; stage: batch review. Phase `p02`. The author is helper agent
  `a77724bc09910bbe6` under root; this reviewer authored nothing.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`. Base and HEAD are both `57adfa7a`, and the
  changes are in the working tree: 37 tracked files plus the new
  `src/adapters/http/pages/desk_wording_tests.rs`.
- Digest `6cd8b092ebeda61840e7d22172124e546cb80372e1b99ca96c24528115dee5bf`, with exclusions
  `plan/01-departments/phases/p02/docs` and `plan/01-departments/COORDINATION.md`. I recomputed it
  at the start and before returning; it matches the file list in `logs/B2-R1-snapshot.txt`.
- Coordination revision 2 (`d33f363c…d02f`) unchanged. C001 and C002 are not relevant to B2. The p02
  gate r2 PASS stands; B1 is not implemented yet.

### Coverage and evidence

- **Requirements:** 02C.1–02C.4 and the rendering/grep focused check; README decision 2; plan 05
  item 2 (separate commit); `pages/AGENTS.md`.
- **No identifiers changed.** I wrote a diff analysis (scratch scripts):
  - (a) every `href`/`id`/`name`/`class`/`for`/`value`/`action`/`hx-*` (except `hx-confirm`)/
    `data-*`/`aria-controls`/`type`/`role` attribute value, compared between removed and added lines:
    none differ;
  - (b) `/ui`, `/api` and `/companies` route literals and `X-MailAgents-*` headers: none differ;
  - (c) after mapping desk back to channel and collapsing whitespace, the only remaining non-test
    differences are rustfmt reflows of three call sites (`mailbox.rs`, `ui.rs`, `simulation.rs`) and
    the new `pages/AGENTS.md` line.

  So B2 changed prose only. Its tests were updated, and new ones added.
- **Wording read in full**, across pages, routes, the `notices.rs` bounce and help copy, and the
  Agent Builder prompt: consistent and grammatical (one nit → F3). The `pages/AGENTS.md` line is
  present.
- **Agent-facing text:** the only natural-language tool text that said channel is the Agent
  Builder prompt, which is updated and tested. `AgentDirectoryEntry` has no channel prose. The
  provisioning tool's schema descriptions have no channel prose. The workflow `help.rs` text tied to
  `channel.no_reply` and the destination kind `channel` are identifier-bound keeps, which is
  reasonable.
- **Tests not weakened:**
  - the old negative assertion `!contains(">Next: Channel<")` is kept and a desk variant added;
  - the nav-order test gained a `contains(">Desks</a>")` check. Before, it compared `Option`s,
    which passes vacuously when the label is missing;
  - the negative outsider and directory checks were moved to the new heading;
  - the bounce and help bodies gained `!contains("channel")` checks.
- **Source scan:** not vacuous. It requires more than 40 sources and has a self-test. It has a
  blind spot for plain literals passed to helpers (→ F1). My plain-literal scan shows only one
  user-visible hit today (`logs/01DEP-P02-B2-R1-reviewer-plain-literal-scan.txt`).
- **Remaining user-visible "channel":** see F2. The page strings are `simulation/outcomes.rs:315`
  and the bindings refusal notice. The embedded `AppError` texts are deliberate keeps outside
  02C.1's `pages/` scope. No JS asset or inline script emits channel text.
- **Executed by B** (DB `openbot_dept_p02c_test`, `SQLX_OFFLINE=true`, 4 threads):
  - `desk_wording_ pages:: builtin_agent_library use_cases::thread::` → 265 passed, 1 failed, 3
    ignored (`logs/01DEP-P02-B2-R1-reviewer-pages-thread-tests.txt`, sha `3e8cf860…bfb1`);
  - `adapters::http::routes::` → 337 passed, 3 ignored
    (`logs/01DEP-P02-B2-R1-reviewer-routes-tests.txt`, sha `488ff28f…cd1aac`);
  - `--test source_modules` → 8 passed;
  - `cargo fmt --check` clean;
  - `git diff --check` clean.
- **The failing test is unrelated to B2; independently confirmed.**
  `pages::tests::rendered_markup_carries_no_inline_javascript` reports only
  `pages/workflow_authoring/sample.rs:153`. That file has no diff since `57adfa7a`; its last change
  is `48461f66`, earlier than p01. The test body has no diff either. The test scans every
  `src/adapters/http/**` file whose last path component is not `tests.rs`. `Path::ends_with` compares
  components, so `desk_wording_tests.rs` **is** scanned, and it is not reported. B2 adds no offender,
  and the same failure exists at base.
- **Evidence manifest:** `logs/01DEP-P02-B2-R1.evidence.json` validated against the digest: 6
  PASS, 1 FAIL (the unrelated test), 1 NOT EXERCISED.
- **Inspected, not rerun:** the author's clippy log.

### Outcome

**PASS for digest `6cd8b092ebeda61840e7d22172124e546cb80372e1b99ca96c24528115dee5bf`.**

- No actionable defects.
- Optional suggestions:
  - 01DEP-P02-B2-F1: source-scan blind spot for plain helper-argument literals;
  - 01DEP-P02-B2-F2: remaining or mixed user-visible channel wording (two page strings, plus
    embedded `AppError` texts as a possible phase 04 follow-up);
  - 01DEP-P02-B2-F3: "Continue to desk".
- The unrelated failure is to be reported separately, as the author did.
- The full repository gate was not run (I003).

### Process and instruction feedback

- Splitting the wording rename into its own batch made a prose-only review checkable mechanically:
  attribute, route and header diffs, plus a normalised line diff.
  - Candidate instruction (scope: copy or rename batches): ship that normalised-diff check with the
    batch, so the reviewer can confirm that nothing but prose changed.
- `Path::ends_with` compares path components, not string suffixes. Test-file exclusion rules built
  on it should be read with that in mind (observation; no change needed here).

---

## 01DEP-P02-B1-R1 — batch review of B1 (02A ownership, 02B members-only, D5, relay/thread-party)

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Records: `TASK.md` "B1 implementation
  (02A + 02B)", `REVIEW.md`, `agent-review-findings.md`.
- Batch B1; stage: batch review. Phase `p02`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`. Review base and HEAD are both `0c5df403`
  (the B2 commit), plus the working tree: 25 files, with new `department_desks.rs`,
  `department_desks_tests.rs` and `department_desk_relay_tests.rs`.
- Digest `a3b1ed11e9a8ecbce73549e00d0afebef9dc1f6971e124d6d16e24a435faada8`, with exclusions
  `plan/01-departments/phases/p02/docs` and `plan/01-departments/COORDINATION.md`. I recomputed it
  at the start and before returning; it matches `logs/B1-R1-snapshot.txt`.
- Gated decisions DEP-P02-D1..D7 r2 (`0bee3eb0…1bd6`). Coordination revision 2 (`d33f363c…d02f`)
  unchanged; C001/C002 apply.

### Coverage and evidence

- **Whole diff read**, plus the three new files:
  - schema: channels columns and CHECKs, grants PK with provenance, FK RESTRICT and index, D5
    trigger;
  - `department_desks.rs`: merge, mode rule, prelock, eligibility SQL (= C002), principal locks,
    `rederive_on`, `set_desk_ownership_on`;
  - `channel.rs`: select, insert and create with prelock and sync, settings write applying the rule
    to post-write ownership, allowlist conflict target;
  - `channel_edit.rs`: prelock only for ownership `Some`, then sync;
  - p01 `department.rs`: sync per D4 set via `role_reach_on`; delete refuses owned desks, then
    syncs the parent;
  - `account_identity.rs`: conflict target;
  - JSON payload;
  - the remaining `Channel` literal sites;
  - the docs paragraph.
- **Gated decisions against the code:**
  - D1, D2 (the rule on every save) and D4 (unconditional delete, then insert only for members-only
    desks; affected sets match D4; create without a lead needs no sync, which I re-derived and
    agree with) hold;
  - D3 order holds for channel rows (principals → gates → rows) but not for join rows (→ F1);
  - D5 trigger matches;
  - D6/C001: there is no routing change, and the doc paragraph is present.
- **Deviations accepted:**
  - tri-state `ChannelOwnershipWrite`, merged under the locks. This is safer than merging in the
    route and matches D2/D3's intent;
  - create-without-lead needs no sync;
  - `channel_allowlist_leaves_a_manager_grant_alone` is updated consistently and still asserts the
    manager rows survive the email being removed;
  - the plan's form round-trip test is unchanged; the coverage gap is F2.
- **Verification mapping checked** against fixtures and production paths (use cases on Postgres).
  - Competing tests hold the company row and wait for blocked backends. The membership-vs-ownership
    test would fail if the ownership path skipped the company lock, so it discriminates.
  - The relay and thread-party tests run real routing on in-memory ports; that stand-in is declared.
  - `list_readable_channels` is not exercised (row stand-in); its policy is `viewer_access`, which is
    tested.
- **Executed by B** (DB `openbot_dept_p02_test` plus `own_database`, `SQLX_OFFLINE=true`, 4
  threads):
  - `department` → 52 passed (`logs/01DEP-P02-B1-R1-reviewer-department-tests.txt`);
  - affected callers and contracts → 548 passed. Filters: `channel account_identity notification
    participant use_cases::thread:: remove_member agent_retire user_lifecycle account_deletion
    onboarding provisioning assistant_channel library legacy_runtime_schema test_support::tests::
    workflow_routes_mount_in_the_app_router revision_overflow state_witness`
    (`…-reviewer-affected-tests.txt`);
  - `company_invite pages:: smtp:: authorization_cases` → 251 passed, 1 failed. The failure is the
    known unrelated `rendered_markup_carries_no_inline_javascript` (`sample.rs:153`), as in B2-R1
    (`…-reviewer-pages-invite-smtp-tests.txt`);
  - `source_modules` → 8 passed;
  - `cargo fmt --check` clean;
  - `git diff --check` clean.
- **SQL probe** (scratch DB, created and dropped): the join-row / principal deadlock
  (`logs/01DEP-P02-B1-R1-sql-deadlock-probe.txt`) (→ F1).
- **Evidence manifest:** `logs/01DEP-P02-B1-R1.evidence.json` validated against the digest: 5
  PASS, 1 FAIL retained (the pre-update allowlist test), 1 NOT EXERCISED.
- **Inspected, not rerun:** the author's clippy and `sqlx prepare` logs.

### Outcome

**Changes required** for digest `a3b1ed11…aada8`. There is no PASS. All findings are Low:

- 01DEP-P02-B1-F1: a department write deleting a principal's join row and then syncing can
  deadlock with an agent or account deletion of the same principal. Reproduced.
- 01DEP-P02-B1-F2: plan 02B.3's "derived grants not shown as emails" has no decisive assertion.
- 01DEP-P02-B1-F3: the JSON ownership fields (`null` detaches, absent keeps) are untested at the
  route level, though D2 promised a route test.

The recheck can be narrow if the corrections are a lock-order fix plus tests: rerun `department`
and the new tests, and review the delta.

### Process and instruction feedback

- Batch size was reviewable. The new module's top-of-file lock-order comment and the TASK's
  deviation list were accurate and useful.
- F1 shows a gate's lock-order argument can miss rows that the caller of a new step has already
  locked.
  - Candidate (scope: gated lock orders): enumerate the locks held on entry to each new lock
    step, including rows the caller wrote.

---

## 01DEP-P02-B1-R2 — correction recheck of B1

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Records: `TASK.md` "B1 correction
  (B1-R2)", `REVIEW.md`, `agent-review-findings.md`.
- Batch B1; stage: correction recheck. Phase `p02`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`. Base and HEAD are both `0c5df403`, plus
  the working tree.
- Digest `cbca0a00bf06f97d453c5191aac2f98984cf9062aae79101246ba6bd8aa5438f` (same exclusions).
  I recomputed it at the start and before returning. The previous reviewed digest was
  `a3b1ed11…aada8`.
- Decisions r2 unchanged. Coordination revision 2 unchanged (`d33f363c…d02f`).

### Coverage and evidence

- **Recheck by finding ID**, B1-F1 to B1-F3. Delta read:
  - `department.rs`: the new `lock_principals_on` and its five call sites, with lock order
    re-derived for remove, clear, assign/replace, delete-cascade and add (which locks through
    `check_principal_on`, already before the insert);
  - the F2 assertion;
  - the new race test;
  - the new `routes/channel_department_tests.rs` mounted from `routes/channel.rs`.
- **New tests checked for discriminating power:**
  - the race test fails without the fix (the author's mutation log shows a lock timeout);
  - the JSON test deserializes real JSON, so it exercises the `present` deserializer.
- **Executed by B** (`openbot_dept_p02_test` plus `own_database`, `SQLX_OFFLINE=true`, 4 threads):
  - `department` → 54 passed, including the race test and the JSON test
    (`logs/01DEP-P02-B1-R2-reviewer-department-tests.txt`, sha `eb76e57b4c7fad61…`). The binary was
    current with the restored source; a mutated build would fail the race test;
  - `source_modules` → 8 passed;
  - `cargo fmt --check` clean;
  - `git diff --check` clean.
- **Evidence manifest:** `logs/01DEP-P02-B1-R2.evidence.json` validated against the digest: 3
  PASS, 1 FAIL (the retained mutation run), 1 NOT EXERCISED.
- **Reused from B1-R1:** the affected caller and contract run (548 passed), the pages, invite and
  SMTP run, and the inspection of the rest of the batch. Since R1, changes are confined to the
  department `_on` functions and tests.

### Outcome

**PASS for digest `cbca0a00bf06f97d453c5191aac2f98984cf9062aae79101246ba6bd8aa5438f`.**

- B1-F1, B1-F2 and B1-F3 are fixed.
- There are no open actionable findings for p02 B1. B2-F1..F3 (optional) remain open as recorded.
- The full repository gate was not run (I003). The pre-existing `pages::tests::rendered_markup_carries_no_inline_javascript`
  failure is unrelated, as recorded at B2-R1.

### Process and instruction feedback

- The author's mutation run, removing the fix and showing the new race test fail, is exactly the
  evidence a competing-claimant test needs.
  - Candidate instruction (scope: concurrency corrections): retain such a mutation run whenever a
    race fix adds a test.
