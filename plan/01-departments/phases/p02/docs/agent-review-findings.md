# p02 agent review findings

Shared findings log for phase `p02` of `plan/01-departments/` (workflow `implement-opus-reviewed-2_2`).
Records: `TASK.md`, `REVIEW.md`, `agent-review-notes.md` (this directory).

Common context for the G1 findings below:

- Date 2026-11-10; batch B1 (02A + 02B); stage: architecture gate G1; round note `01DEP-P02-G1-R1`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`,
  HEAD/start `57adfa7a`.
- Reviewed: decision revision **r1** (DEP-P02-D1..D7). Immutable copy `logs/gate-decisions-r1.md`,
  sha256 `6c5ec0d1e4ceed6957c9943982df51b71579820ae5f5c692b37ead1cdcdbd214`.
- Coordination revision 2 (`logs/coordination-r2.md`, sha256 `d33f363c…d02f`). Applicable:
  I001–I003/r1, C001/r1, C002/r1.
- SQL evidence: `logs/01DEP-P02-G1-R1-sql-probes.txt`, a scratch DB on the real init schema at
  `57adfa7a`.

---

## 01DEP-P02-G1-F1 — A person who is both a department member and on the email allowlist can lose access, or lose their allowlist entry

- **Classification:** actionable defect · **High** · architecture / correctness (access control).
  Originates in gated decisions D4, D3 and D2.
- **Evidence:**
  - `channel_principal_grants_pkey` is `(company_id, channel_id, principal_id, capability)`, with
    no provenance in it (init schema `:6049`). Each principal has one row per capability, and
    the first writer's provenance owns it.
    - The allowlist writer inserts `ON CONFLICT … DO NOTHING` (`channel.rs:249-252`).
    - The form save deletes only `configured_allowlist` rows (`channel.rs:479-486`).
    - `participant_emails` lists only `configured_allowlist` rows (`CHANNEL_SELECT`,
      `channel.rs:88-135`).
    - The identity merge moves grants with the same conflict target (`account_identity.rs:235-240`).
  - D4 deletes a desk's `department_membership` rows, then inserts derived rows `ON CONFLICT DO
    NOTHING`. D3 says an `ownership: None` edit "never touches derived grants". Together:
    - **A — the member loses access.** M's email is on the allowlist, so a configured row exists.
      M is added to the department, and the derived insert is a no-op. A form save then drops M's
      email: the configured row is deleted and nothing re-derives. M is still a member but has no
      grant on the members-only desk.
    - **B — the allowlist entry is lost.** M is a member, so a derived row exists. M's email is
      added to the allowlist, and the configured insert is a no-op. The form no longer shows M's
      email, because `participant_emails` reads configured rows only. When M later leaves the
      department, the sync deletes the row, and M loses access although they were explicitly
      allowlisted.
  - Probe (`logs/01DEP-P02-G1-R1-sql-probes.txt`) reproduces both on the real schema with the exact
    statements. A: 0 grants left for a current member. B: 0 configured rows recorded and 0 grants
    left for an allowlisted person.
  - Requirement: 02B.3 "Configured allowlist entries … stay alongside derived ones". The plan's
    check "a configured allowlist address survives a membership change". AC2 "rewritten in the same
    transaction as every … change". A teammate allowlisted by email on a members-only desk is an
    ordinary case. A test using only an outside address would not catch this.
- **Required:** decide in r2 how two provenances for one principal coexist.
  - Recommended: make provenance part of the key, `(company_id, channel_id, principal_id,
    capability, provenance)`. No backward compatibility is needed (I001). Then update the conflict
    targets of `insert_email_allowlist_grants`, the identity merge and D4.
  - Readers that tolerate duplicate rows need no change: `Channel::has_capability`,
    `preferred_approver`, `notification_principal_can_view` (EXISTS), the JSON aggregation. Confirm
    any reader that assumes uniqueness.
  - Any other rule is acceptable only if it is stated and both A and B are tested.
  - Add both scenarios to the `department_desk_members_only_` rows.
- **Instruction lesson:** (observation) the gate reused a table's existing conflict rule without
  checking whether its key let the new value coexist with old ones. Candidate: for a new value of a
  discriminator column, check whether the table's primary or unique keys include that column, and
  list every writer and its conflict target.
- **Resolution history:** 2026-11-10 open (G1-R1, decision r1).
  - 2026-11-10 **fixed** (recheck `01DEP-P02-G1-R2`, decision r2, `logs/gate-decisions-r2.md` sha256 `0bee3eb0a7ff04b0e67322e8e97913404da355b3ac1607534bf6f866c9061bd6`):
    - D1 adds `provenance` to `channel_principal_grants_pkey`. The conflict targets of `insert_email_allowlist_grants`, the identity merge and D4 gain `provenance`.
    - I checked every production reader: `CHANNEL_SELECT` (its `participant_emails` filter stays configured-only), `Channel::has_capability`/`preferred_approver`, `notification_principal_can_view` (EXISTS), `workflow/decisions.rs:317` (EXISTS) and `workflow/association.rs:38-45` (`fetch_optional`, which takes the first of two rows, with `FOR SHARE` on both). Each is existence-based.
    - Both orders from the probe now have verification rows.
    - Leftover runtime-SQL consumer not listed: see G1-F9.

---

## 01DEP-P02-G1-F2 — Saving the existing channel form on a members-only desk fails with a database error

- **Classification:** actionable defect · **Medium** · correctness / requirements. Originates in
  gated decisions D2 and D3.
- **Evidence:**
  - `write_channel_settings_on` derives `access_mode` from the form's email list alone
    (`channel_access`, `channel.rs:193-221`). An empty list gives `team`, and `@public` gives
    `public`. It is the only production writer of `channels.access_mode` (searched every
    `UPDATE channels` outside tests).
  - A members-only desk usually has no configured emails, since its grants are derived. Its HTML
    form, which carries `ownership: None` until phase 04, therefore posts an empty list. The update
    writes `team` against `channels_members_only_allowlist` (D1) and fails with a 23514 constraint
    error. An `@public` entry would fail the same way instead of returning D2's worded `BadRequest`.
  - D2's "members_only forces access_mode = allowlist even with an empty email list" sits under
    "Validation after merge, under the locks of D3". D3 says an `ownership: None` edit "keeps
    today's protocol exactly". Nothing says the `None` path reads the row's `members_only` before
    computing the mode.
  - No verification row covers an `ownership: None` save of a members-only desk. The "round trip"
    row only checks that ownership is kept.
- **Required:** state in r2 that every settings write, `ownership: None` included, applies the
  members-only rule against the row's current ownership. The read is stable under the row lock,
  because ownership writers also take the exclusive gate. The rule is: allowlist with an empty
  list, `BadRequest` for `@public`. Add rows for an HTML or `ownership: None` save of a members-only
  desk:
  - with an empty list: the mode stays allowlist and `members_only` stays set;
  - with `@public`: `BadRequest`;
  - with emails: allowlist.
- **Instruction lesson:** cause: keeping one writer path "exactly as today" let a new CHECK
  constraint go unchecked on it. Candidate: when a gate adds a CHECK over existing columns, list
  every writer of those columns and say how each one satisfies the check.
- **Resolution history:** 2026-11-10 open (G1-R1).
  - 2026-11-10 **fixed** (recheck `01DEP-P02-G1-R2`, decision r2, `logs/gate-decisions-r2.md` sha256 `0bee3eb0a7ff04b0e67322e8e97913404da355b3ac1607534bf6f866c9061bd6`): D2 applies the members-only rule on every settings write against the row's post-write ownership, which for `ownership: None` is the current row's, read under the row lock. Allowlist even with an empty list; `@public` is `BadRequest` with a worded message. Verification row added.

---

## 01DEP-P02-G1-F3 — Former members could keep derived grants on a desk that stops being members-only

- **Classification:** actionable defect · **Medium** · correctness / security. Originates in gated
  decision D4 (and D2).
- **Evidence:**
  - D4 describes the department-level sync as "recomputes every members-only desk owned by any
    listed department". The channel-edit path is `sync_desk_grants_on`, "the same statements
    restricted to one channel". The text does not say what happens to a desk's
    `department_membership` grants when the desk **leaves** the members-only set: `members_only`
    turned off, department cleared while the desk stays allowlist with configured emails, and so on.
  - If the deletion is restricted to members-only desks, as the "recompute every members-only
    desk" wording suggests, a desk switched to `members_only = false` keeps its old derived rows.
    If it stays allowlist because configured emails exist, those rows keep admitting the department's
    former and current members. Later department syncs skip the desk, so the rows go stale and are
    never removed. That is an over-grant.
  - D2 also does not say what `access_mode` becomes when `members_only` is cleared with no
    configured emails. `channel_access` gives `team`, which opens the desk to the whole team. That is
    probably intended, but it should be stated.
- **Required:** state in r2 that any ownership change deletes the desk's `department_membership`
  rows unconditionally: off members-only, department cleared, or moved to another department. Only
  a desk that is members-only afterwards gets the set re-inserted. State the resulting
  `access_mode`. Add verification rows:
  - a members-only desk switched off keeps no derived grants and its configured ones survive;
  - a desk moved from D1 to D2 holds exactly D2's set.
- **Instruction lesson:** cause unknown (wording). No change supported.
- **Resolution history:** 2026-11-10 open (G1-R1).
  - 2026-11-10 **fixed** (recheck `01DEP-P02-G1-R2`, decision r2, `logs/gate-decisions-r2.md` sha256 `0bee3eb0a7ff04b0e67322e8e97913404da355b3ac1607534bf6f866c9061bd6`): D4 deletes a desk's `department_membership` grants unconditionally on every re-derivation, and inserts only for members-only desks. It states the invariant that only members-only desks hold them. Every ownership change runs `sync_desk_grants_on`, and the mode follows D2. The verification row covers off, cleared, and moved D1→D2.

---

## 01DEP-P02-G1-F4 — D3's recorded "known residual" understates the new lock-order and gate risks

- **Classification:** actionable defect · **Low** · concurrency (failure-mode accuracy). Originates
  in gated decisions D3 and D5.
- **Evidence:**
  - D5 makes the demotion path delete grants. Every grant write fires
    `notification_channel_grant_recheck`, which locks the channel row `FOR UPDATE` (init schema
    `notification_recheck_channel_access`, `:966-980`; trigger `:8506`). So a demotion now holds the
    principal `FOR UPDATE` (`kind` is a key column) and then wants channel rows.
  - D3 records only the deadlock against an `ownership: None` channel edit. The inbound commit is
    another writer with the opposite order. It takes channel KEY SHARE through FK inserts
    (`threads_channel_fk`, `message_deliveries_channel_fk`) and principal KEY SHARE
    (`messages_author_principal_fk`, `thread_principals_principal_fk`). `FOR UPDATE` on the channel
    conflicts with KEY SHARE. Whether the inbound statement order makes the cycle real for a person
    mailing a members-only desk while being removed was **not traced**; that part is a hypothesis.
    Before p02, the demotion path took no channel locks.
  - D5 bumps `settings_revision` without taking the channel gate. The gate exists so that "a
    message is stored under the settings it was routed against" (`channel_gate.rs:1-25`). An inbound
    commit that has already passed its revision check can therefore still store a message from the
    person just removed, within the commit window.
- **Required:** in r2, either restructure, or record the complete residual:
  - which writers can deadlock with the demotion trigger, and the outcome: detected deadlock, one
    transaction aborted, the transport retries or `remove_member` returns an error;
  - that the demotion's revision bump does not take the gate, and the narrow admission race that
    leaves.

  Add a competing test for the residual you keep, or state why one is not feasible.
- **Instruction lesson:** (observation) a trigger that writes rows guarded by other triggers inherits
  their lock footprint. Candidate: when a gate adds writes inside a trigger, follow the triggers those
  writes fire and list the locks they take.
- **Resolution history:** 2026-11-10 open (G1-R1).
  - 2026-11-10 **fixed** (recheck `01DEP-P02-G1-R2`, decision r2, `logs/gate-decisions-r2.md` sha256 `0bee3eb0a7ff04b0e67322e8e97913404da355b3ac1607534bf6f866c9061bd6`): D3 "Residual deadlocks" lists D5 vs inbound commit, and D5 vs an `ownership: None` edit writing a configured grant, with outcomes (40P01, one side aborted, both retryable). Item 3 argues that the sync and ownership paths cannot cycle. I checked that against account deletion, `remove_member` (company lock), agent delete, p03 nesting (agents row first) and inbound (gate shared, KEY SHARE only); it holds. D5 records the window left by the revision bump without the gate, and why taking a gate there would create a cycle with channel edits (principal → gate vs gate → principal).

---

## 01DEP-P02-G1-F5 — The TASK has no contract-impact table

- **Classification:** actionable defect · **Low** · verification records (gate content).
- **Evidence:** p02 TASK has "Verification plan" and "Batches", but no "Contract impact" mapping.
  The skill requires one: "Contract impact" table; architecture gate "affected contracts". p01's
  TASK had one. p02 changes or extends:
  - `Channel`/`ChannelDb`/`CHANNEL_SELECT`, which changes the JSON shape of every channel API
    response;
  - `ChannelWrite`, with 82 literal sites;
  - the `ChannelPersistence` mock impls;
  - every `channel_principal_grants` writer and conflict target (F1);
  - the only `access_mode` writer (F2);
  - the notification grant trigger (F4);
  - p01's `_on` functions and `department_principal_demoted`;
  - p01's `delete_department_on` FK→`Conflict` mapping, now reached by the desk FK;
  - `begin_bounded`'s 1000 ms `lock_timeout`, now covering gate waits (F6).

  An inventory like this would have surfaced F1 and F2.
- **Required:** add the contract-impact table, with producers, consumers and equivalent surfaces,
  a disposition and a check for each.
- **Instruction lesson:** none beyond the existing reference (`verification-contracts.md`).
- **Resolution history:** 2026-11-10 open (G1-R1).
  - 2026-11-10 **fixed** (recheck `01DEP-P02-G1-R2`, decision r2, `logs/gate-decisions-r2.md` sha256 `0bee3eb0a7ff04b0e67322e8e97913404da355b3ac1607534bf6f866c9061bd6`): "Contract impact (r2, F5)" table added, covering Channel/JSON, `ChannelWrite`, impls, grant key and conflict targets, the `access_mode` writer, the notification trigger, p01 `_on` functions and trigger, and the budget. One runtime-SQL test consumer is missing (G1-F9, optional).

---

## 01DEP-P02-G1-F6 — Department writes now wait on channel gates within a 1-second lock timeout

- **Classification:** optional suggestion · Low · concurrency / operability. D3.
- **Evidence:** p01 department writes use `begin_bounded` (`lock_timeout` 1000 ms,
  `statement_timeout` 5000 ms). With p02 they take `ChangeSettings` gates, which wait for every
  in-flight inbound commit holding the gate shared (`channel_gate.rs:50-56`). Channel edits budget
  2 s / 30 s (`channel_edit.rs:36-41`). A membership change on a busy desk can therefore time out
  where a channel edit would not.
- **Suggested:** state the budget department writes use once they take gates (the channel
  `EditBudget`, or a named one), and the refusal the user sees.
- **Resolution history:** 2026-11-10 open (optional).
  - 2026-11-10 **fixed** (optional) (recheck `01DEP-P02-G1-R2`, decision r2, `logs/gate-decisions-r2.md` sha256 `0bee3eb0a7ff04b0e67322e8e97913404da355b3ac1607534bf6f866c9061bd6`): D3 "Budgets" keeps `begin_bounded`. A timeout is `AppError::DatabaseTimeout`, rendered 503 "The database is busy. Try again shortly." (`src/adapters/http/app_error_impl.rs:48`), with nothing changed.

---

## 01DEP-P02-G1-F7 — Name the function p03 calls to set a desk's ownership, and state two contract details

- **Classification:** optional suggestion · Low · architecture (cross-phase contract). D2/D3.
- **Evidence and suggestions:**
  - 03B.1 says `create_department_on` provisions the desk and then "set[s] `channels.department_id`"
    inside the department transaction. D3 defines ownership changes only through
    `update_channel_on`, and creation through JSON. Name the `_on` function, for example
    `set_channel_ownership_on(tx, company, channel, ownership)`, holding the caller's company lock,
    that p03 must use. It would carry D2's validation and run the D4 desk sync, so p03 does not
    write the column directly.
  - The JSON channel PUT is documented as replace semantics: "a PUT replaces rather than patches"
    (`routes/channel.rs:184-191`). D2 makes the ownership fields keep-when-absent. That is
    reasonable, but the payload doc should say it is an exception.
  - Channel persistence does not re-authorize under the lock, as today. The ownership path already
    takes the company row, so an under-lock re-authorization would be cheap and would match p01 D4.
    Optional.
- **Resolution history:** 2026-11-10 open (optional).
  - 2026-11-10 **fixed / declined in part** (optional) (recheck `01DEP-P02-G1-R2`, decision r2, `logs/gate-decisions-r2.md` sha256 `0bee3eb0a7ff04b0e67322e8e97913404da355b3ac1607534bf6f866c9061bd6`):
    - `set_desk_ownership_on(tx, company, channel, ChannelOwnershipWrite)` is named as the only in-transaction ownership setter for p03;
    - the JSON keep-when-absent exception is documented;
    - under-lock re-authorization is declined, with a reason (no actor in channel persistence; existing edits don't re-authorize). I accept the reason.

---

## 01DEP-P02-G1-F8 — Say whether a removed member can still post in threads they were already in

- **Classification:** optional suggestion · Low · requirements (documentation). D4/D6.
- **Evidence:** `authorize_channel` admits a sender who is not on the access list when the sender is
  a party to the existing thread (`thread/ingest/routing.rs:296-306`). Members-only desks are
  allowlist desks, so a person removed from the department, though still a company member, may
  still post into threads they are a party to. The plan's check "removing the member revokes both in
  one commit" covers the grants. This thread-party path is existing allowlist behaviour and survives.
  I did not trace whether a person demoted out of the company still matches as a party.
- **Suggested:** state the intended behaviour in D4/D6 and the docs paragraph. Add a row if it is
  meant to hold.
- **Resolution history:** 2026-11-10 open (optional).
  - 2026-11-10 **fixed** (optional) (recheck `01DEP-P02-G1-R2`, decision r2, `logs/gate-decisions-r2.md` sha256 `0bee3eb0a7ff04b0e67322e8e97913404da355b3ac1607534bf6f866c9061bd6`): D6 records the thread-party outcome. A removed member may still reply within threads they were party to; new threads and reads are refused. A test asserts it.

---

## 01DEP-P02-G1-F9 — A p01 test still uses the old grant conflict target and will fail at runtime

- **ID and context:** 2026-11-10 · `implement-opus-reviewed-2_2` · batch B1 · gate recheck · round
  `01DEP-P02-G1-R2` · B = Opus 5.5, agent `ad409b2d3d149c2c6` · decision **r2**
  (`logs/gate-decisions-r2.md` sha256 `0bee3eb0…1bd6`).
- **Classification:** optional suggestion · Low · contract impact (records).
- **Evidence:** besides the two production writers r2 lists, one runtime-SQL consumer names the
  old key. `src/adapters/persistence/department_schema_tests.rs:263`
  (`department_schema_grant_provenance_check_accepts_every_variant`) upserts with `ON CONFLICT
  (company_id, channel_id, principal_id, capability) DO UPDATE SET provenance = EXCLUDED.provenance`.
  Under the new key that conflict target matches no constraint, so the statement errors at
  runtime. The upsert-relabel shape no longer fits either: each provenance becomes its own row. B1's
  planned `department_` run would catch it, so it is not acceptance-blocking. A complete search of
  the conflict target found only these three sites (`channel.rs:252`, `account_identity.rs:240`, the
  test).
- **Suggested:** add the test to the contract-impact row for the grant key and rewrite it as
  insert-per-provenance.
- **Resolution history:** 2026-11-10 open (optional; G1-R2).

---

## 01DEP-P02-B2-F1 — The source scan does not check plain strings passed to page helpers

- **ID and context:** 2026-11-10 · `implement-opus-reviewed-2_2` · batch B2 (02C desk wording) ·
  batch review · round `01DEP-P02-B2-R1` · B = Opus 5.5, agent `ad409b2d3d149c2c6` · worktree
  `/Users/mac02/.codex/worktrees/2050/openbot`, base/HEAD `57adfa7a` + working tree · digest
  `6cd8b092ebeda61840e7d22172124e546cb80372e1b99ca96c24528115dee5bf`.
- **Classification:** optional suggestion · Low · verification (regression guard).
- **Evidence:** `desk_wording_page_sources_keep_channel_out_of_text_nodes`
  (`src/adapters/http/pages/desk_wording_tests.rs`) checks two things: raw `>Channel`/`Channel<`
  patterns, and, inside each string literal, text nodes `>…<` plus title, aria-label, placeholder,
  alt and hx-confirm values. It is not vacuous: it requires more than 40 sources, and a self-test
  proves the forms it flags.
  - It does **not** see a plain literal that a helper renders as text. Examples:
    `sidebar_header("Desks", …)`, `ui_shell` titles, `empty_detail_pane("Select a desk …")`,
    `busy_submit_button(…)`, `lazy_card(…, "Loading the workflows on this desk…")`,
    `status_banner(…)`. These are exactly the strings B2 had to change by hand. If "Channels" came
    back in one of them, the scan would still pass. The rendering tests cover only the specific
    places they assert.
  - My lexer scan of non-markup literals under `pages/` outside tests
    (`logs/01DEP-P02-B2-R1-reviewer-plain-literal-scan.txt`) finds exactly one user-visible hit:
    the deliberate keep at `simulation/outcomes.rs:315`. Every other hit is a route, query,
    `channel:{id}` value or `channel.no_reply` identifier. So the guard can be extended with a short
    allowlist.
- **Suggested:** also flag `\bchannels?\b` in literals without markup, when the literal reads as
  prose (contains a space, or starts with a capital), with an explicit allowlist for identifier
  literals and the one deliberate keep.
- **Resolution history:** 2026-11-10 open (optional; B2-R1).

---

## 01DEP-P02-B2-F2 — A few user-visible messages still say channel or mix both words

- **ID and context:** as B2-F1.
- **Classification:** optional suggestion · Low · requirements (wording consistency).
- **Evidence:**
  - `pages/simulation/outcomes.rs:315` "Unauthorized Sender: Email 'from' address is not listed in
    channel participant_emails." is a status-banner text node. The author kept it as mixed prose and
    field name, but the UI labels that field "Participant Emails", not `participant_emails`.
  - `routes/workflows/ui/authoring/bindings.rs:160` is a binding refusal notice built in a UI route
    and shown in the bindings form: "A chosen channel, version, run-as or resource is not available…".
    It is the same kind of page notice as the `routes/ui*.rs` ones B2 changed, but sits outside the
    globs B2 covered.
  - `AppError` strings that B2 kept on purpose are embedded in desk-worded flashes. For example,
    "Failed to create desk: Address 'x' is already in use by another channel in this company."
    (`slug_conflict_error`), "Failed to save desk: The channel name cannot be empty." (from
    `ChannelWrite::normalize`), the `validate_slug` nouns "channel address"/"channel alias", and
    `NotFound("Channel not found")` pages. One sentence then uses both words.
- **Suggested:**
  - reword the two page strings;
  - either reword the user-facing `AppError` texts that flashes embed (`slug_conflict_error`,
    `ChannelWrite::normalize`, `SlugKind` nouns, the `Channel not found` messages), or record them as
    a follow-up for phase 04. Plan 02C.1 strictly scopes `pages/`, so this is not a B2 defect.
- **Resolution history:** 2026-11-10 open (optional; B2-R1).

---

## 01DEP-P02-B2-F3 — Minor wording

- **ID and context:** as B2-F1.
- **Classification:** optional suggestion · Low · wording.
- **Evidence:** the onboarding step button `busy_submit_button("Continue to desk")`
  (`pages/onboarding.rs`) mechanically replaces "Continue to channel". It reads as if a desk already
  exists. "Continue to your first desk", or matching the step title "Create your first desk", would
  read better. Everything else I read is grammatical and consistent: singular, plural and
  capitalisation follow the original wording.
- **Resolution history:** 2026-11-10 open (optional; B2-R1).

---

## 01DEP-P02-B1-F1 — A department write can deadlock with an agent or account deletion of the same principal

- **ID and context:** 2026-11-10 · `implement-opus-reviewed-2_2` · batch B1 (02A + 02B) · batch
  review · round `01DEP-P02-B1-R1` · B = Opus 5.5, agent `ad409b2d3d149c2c6` · worktree
  `/Users/mac02/.codex/worktrees/2050/openbot`, base/HEAD `0c5df403` + working tree · digest
  `a3b1ed11e9a8ecbce73549e00d0afebef9dc1f6971e124d6d16e24a435faada8`.
- **Classification:** actionable defect · **Low** · concurrency. The cause is in the implementation
  order. It also shows that the gated D3 r2 "Residual deadlocks … complete" item 3 is incomplete.
- **Evidence:**
  - `remove_department_member_on`, `clear_department_role_on`, and the lead-replacing `DELETE` in
    `assign_department_role_on` delete a principal's `department_members`/`department_roles` row.
    That leaves a row lock on it.
  - They then call `sync_department_grants_on`. `rederive_on` → `lock_principals_on` locks every
    eligible principal `FOR KEY SHARE` (`department_desks.rs`). That includes the principal just
    removed, if it is still eligible another way: a member removed who is still lead, a lead
    replaced who is still a member, or the lead of a neighbouring department.
  - A demotion of that principal through agent delete or account deletion, neither of which takes
    the company lock, holds the principal `FOR UPDATE`. Its trigger then deletes the same join row
    (`department_principal_demoted`).
  - The two orders are join row → principal and principal → join row, so they form a cycle.
  - D3 r2 item 3 says sync takes principals "before gates and rows". That covers channel rows, but
    not the join rows the `_on` function wrote first.
  - Reproduced: `logs/01DEP-P02-B1-R1-sql-deadlock-probe.txt` (scratch DB, working-tree schema)
    shows T1 delete member row → KEY SHARE vs T2 demotion, and PostgreSQL reports `deadlock
    detected`, aborting the demotion. It needs a members-only desk in the affected set and a
    concurrent demotion of that very principal: rare, detected, nothing partial.
- **Required:** in each `_on` function that deletes a principal's join rows, take that principal
  `FOR KEY SHARE` (sorted, with any others) before deleting. That gives principal → join row
  everywhere, as `check_principal_on` already does for inserts, and the sync's later lock is then
  already held. The alternative is to add the case to D3's residual list. Preferably add a competing
  test with a held demotion against a remove/replace that keeps the principal eligible.
- **Instruction lesson:** (observation) a new lock taken late in an existing write inherits that
  write's earlier row locks. Candidate: when a gate places a new lock step "before gates and rows",
  also list the rows the caller already wrote.
- **Resolution history:** 2026-11-10 open (B1-R1).
  - 2026-11-10 **fixed** (recheck `01DEP-P02-B1-R2`, digest `cbca0a00bf06f97d453c5191aac2f98984cf9062aae79101246ba6bd8aa5438f`):
    - `department.rs` `lock_principals_on` locks, `FOR KEY SHARE` and sorted, every principal whose department rows a write deletes, before deleting them. That covers the member in `remove_department_member_on`, the holder in `clear_department_role_on`, the replaced and new holders in `assign_department_role_on` (one statement, before `check_principal_on`), and every member and holder before `delete_department_on`'s cascade. So the order is principal → join row everywhere, and the sync's later locks on those principals are already held.
    - The remaining eligible principals have no rows written by this transaction, and in-company demotions lock one principal each, so lock order among `KEY SHARE` holders cannot form a cycle.
    - TASK records the correction to gate r2 D3 item 3.
    - Test `desks::department_desk_competing_removal_and_demotion_of_one_principal_both_commit` pauses the real removal after its row delete, blocks a demotion behind it, then asserts both commit and the final grants are empty. The author's mutation run without the lock fails the test with a lock timeout (`logs/B1-R2-mutation-without-fix.txt`), so it discriminates. It passes in B's run (`logs/01DEP-P02-B1-R2-reviewer-department-tests.txt`).

---

## 01DEP-P02-B1-F2 — No test checks that a derived grant's email stays out of the form's allowlist

- **ID and context:** as B1-F1.
- **Classification:** actionable defect · **Low** · verification (plan 02B.3).
- **Evidence:** 02B.3 asks the form round-trip test to show that derived grants are "neither shown
  as emails nor deleted by a form save". The author kept the original test unchanged and records
  that two new tests cover this. `department_desk_members_only_keeps_an_allowlist_entry_and_a_membership_apart`
  proves the **not deleted** half: after a save with an empty list, the member's derived grants
  remain. No test asserts the **not shown** half. In the first part, `first` has an email identity
  and derived grants, yet `unlisted.participant_emails` is never checked. The `Departments` fixture
  principals have no identities, so other tests cannot exercise it. The SQL filter (`provenance =
  'configured_allowlist'`) makes it true today, but nothing would catch a regression.
- **Required:** assert that `participant_emails` holds no derived holder's address. For example,
  in that test assert `unlisted.participant_emails == Some(vec![])` while `first` holds derived
  grants. Or extend the round-trip test as the plan says.
- **Resolution history:** 2026-11-10 open (B1-R1).
  - 2026-11-10 **fixed** (recheck `01DEP-P02-B1-R2`, digest `cbca0a00bf06f97d453c5191aac2f98984cf9062aae79101246ba6bd8aa5438f`): the allowlist/membership test now asserts `participant_emails == Some(vec![])` while the derived holder `first` (who has an email identity) holds grants.

---

## 01DEP-P02-B1-F3 — The JSON channel payload's ownership fields are untested

- **ID and context:** as B1-F1.
- **Classification:** actionable defect · **Low** · verification (D2 r2, F7 follow-up).
- **Evidence:** D2 r2 says the keep-when-absent exception is "Documented on the payload type and in
  the route test". `routes/channel.rs` adds `department_id` (with the `present` deserializer:
  `null` → `Some(None)` detaches) and `members_only`, plus `ChannelJsonPayload::ownership()`.
  No test in `routes/channel.rs` or elsewhere touches them; a search for `ownership()`,
  `"department_id"` and `members_only` in the route tests finds nothing. The persistence merge is
  tested with `ChannelOwnershipWrite` built directly (`an_ownership_edit_keeps_what_it_does_not_name`,
  desk tests). An error in the deserializer or the mapping would silently detach desks, or fail to
  detach them, through the JSON API.
- **Required:** a route-level test, deserializing JSON as `department_routes_round_trip_…` does in
  p01, for three cases: absent fields keep ownership, `"department_id": null` detaches, and an id plus
  `"members_only": true` assigns. Also cover `"members_only"` alone.
- **Resolution history:** 2026-11-10 open (B1-R1).
  - 2026-11-10 **fixed** (recheck `01DEP-P02-B1-R2`, digest `cbca0a00bf06f97d453c5191aac2f98984cf9062aae79101246ba6bd8aa5438f`): `routes/channel_department_tests.rs` `department_desk_ownership_json_fields_are_kept_when_absent` deserializes real JSON into `ChannelJsonPayload` and drives `update_channel_json`. It covers an id plus `members_only: true` → allowlist; absent → kept; `members_only: false` alone → department kept, mode team; `department_id: null` → detached. It passes in B's run.
