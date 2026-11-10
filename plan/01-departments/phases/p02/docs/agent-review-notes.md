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
