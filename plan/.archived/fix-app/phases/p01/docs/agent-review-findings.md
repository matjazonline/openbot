# Agent review findings — fix-app p01

Shared findings log for phase `p01` (workflow `implement-opus-reviewed-2_2`). Task record:
[TASK.md](TASK.md); review record: [REVIEW.md](REVIEW.md); review notes:
[agent-review-notes.md](agent-review-notes.md).

Common context for the G1 findings below (`FIXAPP-P01-G1-F1` … `F7`): date 2026-10-31; batch B1 (plan 01,
batches 1–4); stage architecture gate G1; review round `FIXAPP-P01-G1-R1`; reviewer B, Opus 5.5 /
xhigh; phase `p01`; worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p01`; reviewed decision
revision **G1 r1** (`logs/G1-r1-decisions.md`, sha256
`89fc9a88462bfe3d9b5437604556b177a7682399fdf485e282ddb4e659d7e2a8`) against code at HEAD `e6b0a51`,
digest `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66` (exclusion
`plan/fix-app/phases/p01/docs`). Scratch evidence: `logs/G1-R1-reviewer-scratch.log`.

---

## FIXAPP-P01-G1-F1 — D1 reverse adoption strands the outsider's grants and thread rows on an identity-less principal

- **Classification:** actionable defect · severity **medium** · category architecture / authorization
  correctness. Originates in gated decision D1 (not an implementation error; nothing is implemented yet).
- **Decision text:** D1 "Register new": when the new address is already an outsider identity, the identity
  and its authored messages move to the person, and "The former outsider principal keeps its grants and
  thread rows but no longer has this identity."
- **Evidence (e6b0a51):**
  - Channel allowlist grants are principal grants written for an address by
    `insert_email_allowlist_grants` (`src/adapters/persistence/channel.rs:218-258`), which resolves the
    address through `resolve_or_create_external_identity_on` — an unseen address gets a new external
    principal X with identity provenance `channel_allowlist`.
  - The allowlist read-back joins grant principal → identities
    (`src/adapters/persistence/channel.rs:92-115`, `participant_emails`). After D1 moves the identity off X,
    X's `configured_allowlist` grants no longer produce the address: the allowlist the owner configured
    silently loses the entry.
  - `channel_access` (`channel.rs:186-216`) derives the stored access mode purely from the submitted list;
    the form is populated from the read-back list. If the adopted address was the only entry, the next
    save of that channel's form (for any edit) yields an empty list → `Team` mode: an allowlist channel is
    widened to every team member. With other entries present, the next save drops the address and
    `DELETE ... provenance = 'configured_allowlist'` (`channel.rs:474-479`) removes X's grants for good.
  - `Channel::participant_access` / `viewer_access` (`src/domain/entities/channel.rs:134-191`): in
    `Allowlist` mode a member is authorized only with an explicit grant (owner excepted). After adoption the
    address resolves to the member's person principal, which has no grant, so mail from the very same
    mailbox that was allowed a moment earlier is now refused, and the member cannot view the channel.
  - `Channel::preferred_approver` (`channel.rs:193-199`) picks the earliest `view` grant; that can now be X,
    a principal with no identity (possible approver dead end — inferred, not exercised).
  - `THREAD_SELECT.participant_identities` (`src/adapters/persistence/thread/mod.rs:141-169`) lists the
    identities of a thread's principals; X keeps the `thread_principals` rows but has no identity, so the
    address drops out of every thread it took part in as an outsider, while its authored messages there are
    now the person's — the message author is no longer a party to the thread.
  - Contrast: the existing promotion in `create_person_principal_on` (`participant.rs:315-331`) keeps the
    outsider's grants and threads with the address by promoting the principal itself, which is the
    behaviour the plan cites ("adopts it, as it adopts any outsider"). D1 cannot promote (the user already
    has a person principal) and does not replace what promotion preserves.
- **Reproduction (scenario, concrete):** owner sets channel K's allowlist to `alice.home@x.test`
  (unseen address → external X with grants); member Alice confirms an email change to `alice.home@x.test`;
  D1 moves the identity to Alice's person principal. K now reads `participant_emails = []` with
  `access_mode = allowlist`; mail from `alice.home@x.test` is unauthorized; the owner renames K and saves →
  K becomes `team`.
- **Required:** D1 must decide and document what happens to the adopted outsider principal's channel grants
  and `thread_principals` rows (for example: move them to the person in the same transaction with conflict
  handling, and say what remains of X; or refuse the change; or justify keeping them with the allowlist
  read/mode consequence handled), include the bypass-path implication (see F3), and add a DB-backed
  `account_email_change_*` test covering an adopted address that was on a channel allowlist and a party to a
  thread (allowlist read-back, participant/viewer access, thread participant identities).
- **Instruction lesson:** observation — the decision analysed the composite FK on messages but not the other
  readers that join principal → identity (allowlist read path, thread participant identities, approver
  selection). Candidate guidance (scope: decisions that move an identity between principals): "list every
  query that joins `principals` to `participant_identities` and state the outcome for each side of the
  move." Hypothesis, single occurrence.
- **Resolution history:** 2026-10-31 — open (G1 r1).
  - 2026-10-31 — recheck FIXAPP-P01-G1-R2 against G1 r2 (`logs/G1-r2-decisions.md` sha256 `b85869d6…54ac`, HEAD e6b0a51, digest `9d5bde92…4c66` unchanged): **fixed**. D1 r2 "Adopting an outsider's address": after the one-statement identity+authorship move, when X has no identity left its `channel_principal_grants` are copied to the person with original provenance/capability (`ON CONFLICT DO NOTHING` on PK `(company_id, channel_id, principal_id, capability)`) and deleted from X, and its `thread_principals` rows are copied (`ON CONFLICT DO NOTHING` on PK `(company_id, channel_id, thread_id, principal_id, role)`) and deleted; X is kept, unreachable. This restores allowlist read-back, allowlist access for the mailbox and thread participant identities; widening on the next save remains possible only in the F6 latent case (person already holds a non-allowlist grant), which no production writer can create today. No writer attaches a second identity to an existing external principal, so the "no identity left" test is stable under the advisory lock. Residual lock-order note recorded as F8 (optional).

## FIXAPP-P01-G1-F2 — Verification plan has no competing-claimant tests for the new locking protocols

- **Classification:** actionable defect · severity **low** · category verification. Originates in the G1
  decision record's verification plan.
- **Evidence:** root `AGENTS.md` ("Keep builds and deployment secure and reproducible"): "When adding or
  changing a concurrency protocol, include a test with competing claimants rather than relying only on
  sequential mocks." G1 introduces two protocols whose correctness is argued only in prose:
  - D1: email-change re-key serialized against invite acceptance / company creation by the `users` row lock
    (`UPDATE users` vs `SELECT ... FOR UPDATE` at `company_invite.rs:46-60`, `company.rs:110-116`), plus
    per-identity advisory locks against inbound ingestion (`resolve_or_create_external_identity_on`).
  - D4: `SELECT ... FROM agents ... FOR UPDATE` serializing retirement against `create_agent_principal_on`
    (insert branch via the FK's `KEY SHARE`; the `DO UPDATE` branch actually serializes on the principal row
    lock instead — reviewer analysis, both safe).
  The TASK.md "Verification plan" lists only sequential focused filters and the full suite.
- **Required:** add at least one competing-claimant DB test per protocol, e.g. an email change racing an
  invite acceptance (or company creation) for the same account, asserting a single consistent outcome (the
  new company's account identity is the new address, or the acceptance is refused), and an agent retirement
  racing an agent message write, asserting no `agent` principal survives for a deleted agent and no message
  is lost.
- **Instruction lesson:** violated existing guidance (root AGENTS.md); the decision template could prompt
  "for each new lock, name its competing-claimant test". Cause otherwise unknown.
- **Resolution history:** 2026-10-31 — open (G1 r1).
  - 2026-10-31 — recheck FIXAPP-P01-G1-R2 against G1 r2 (`logs/G1-r2-decisions.md` sha256 `b85869d6…54ac`, HEAD e6b0a51, digest `9d5bde92…4c66` unchanged): **fixed**. Verification plan r2 adds `account_email_change_races_invite_acceptance`, `account_email_change_races_inbound_observation` and `agent_retire_races_agent_principal_upsert`, each with real concurrent transactions and outcome-independent assertions matching the protocols (users row lock; advisory locks; agent `FOR UPDATE` vs both upsert branches). Execution will be verified in the batch review.

## FIXAPP-P01-G1-F3 — Bypass path lets an unverified address adopt an existing outsider (optional)

- **Classification:** optional suggestion · severity low · category security / requirements question.
- **Evidence:** bypass = SMTP absent and `allow_unverified_without_smtp`
  (`src/application/use_cases/user.rs:558-562`, `876-890`); the bypass write clears `email_verified_at`.
  The plan requires re-keying on both paths ("since company creation applies no other rule"). Today an
  outsider in an *existing* company can be adopted only by verified invite acceptance
  (`lock_verified_account_email`); company creation has no prior outsiders. Under D1, a member who switches
  to an unverified address in bypass mode adopts that outsider's identity and authored history in every
  company they belong to (and, if F1 is resolved by moving grants, its grants), and mail from that address
  then carries the member's trust (`access_context_for_identity` ignores account verification,
  `participant.rs:453-502`).
- **Suggestion:** record this consequence explicitly in D1 as an accepted property of the operator's bypass
  mode, or choose a narrower rule for unverified addresses (e.g. conflict when the new address is an
  existing outsider). This is a product/security choice for the user, not a defect against the plan.
- **Resolution history:** 2026-10-31 — open (optional).
  - 2026-10-31 — recheck FIXAPP-P01-G1-R2 against G1 r2 (`logs/G1-r2-decisions.md` sha256 `b85869d6…54ac`, HEAD e6b0a51, digest `9d5bde92…4c66` unchanged): **dispositioned (accepted, reported to manager/user)**. r2 records it as a product note. Reviewer addition: the UI side is bounded because `access_context_for_user` withholds grant authority for an unverified account (`PrincipalGrantAuthority::WithheldUnverifiedEmail`), so adopted grants authorize only mail from the address, as they did on X.

## FIXAPP-P01-G1-F4 — D1 rationale overstates reply routing; state the adopter consequence (optional)

- **Classification:** optional suggestion · severity low · category architecture documentation.
- **Evidence:**
  - D1 says the old address is not added to `thread_principals` "so replies are not routed to an address the
    account no longer owns". Human-completion replies take the target from the latest inbound turn's
    authored identity (`latest_replyable_email_context`, `src/adapters/persistence/thread/views.rs:525-569`;
    `email_reply_context` uses `author_subject`), so threads whose latest inbound came from the old address
    still reply to it. That is ordinary email semantics and follows "history stays attached to the address",
    but the stated rationale only holds for thread participant identities.
  - Plan test 4 (second user adopts the old address) combined with D1's authorship move means the adopter's
    person principal becomes the author of the previous owner's address-sent messages, displayed under the
    adopter's username (promotion rewrites `display_label`, `participant.rs:315-324`).
  - In-flight `reply` waits compare the responder with the destination message's author
    (`src/adapters/persistence/workflow/reply_waits.rs:221-230`); they follow the address after a re-key.
  - The authorship move does not affect workflow requester derivation: `requester_on` only uses
    `application`-origin messages, which carry no authored identity (`workflow/provisioning.rs:255-280`).
- **Suggestion:** correct the routing sentence and state the adopter and in-flight-wait consequences in D1 so
  the batch review checks them against intent.
- **Resolution history:** 2026-10-31 — open (optional).
  - 2026-10-31 — recheck FIXAPP-P01-G1-R2 against G1 r2 (`logs/G1-r2-decisions.md` sha256 `b85869d6…54ac`, HEAD e6b0a51, digest `9d5bde92…4c66` unchanged): **fixed** — D1 r2 corrects the routing sentence and states the adopter and in-flight reply-wait consequences.

## FIXAPP-P01-G1-F5 — Fixture-impact list is incomplete for D2/D3; D3 has a stronger justification (optional)

- **Classification:** optional suggestion · severity low · category verification / test design.
- **Evidence:**
  - D2 lists ownership-transfer and owner-demotion fixtures. Raw company inserts with no owner member row are
    also invalid at commit: `src/adapters/persistence/workflow/decision_provenance_tests.rs:12` (member row
    is `admin`) and `src/adapters/persistence/workflow/wait_guard_tests.rs:286` (no member row).
  - D3 makes every `DELETE FROM company_members` / `DELETE FROM principals` fixture fail at end of statement
    when the deleted person authored a message or their identity is a message participant
    (`principals_company_user_fk` cascades into principals, then `messages_author_principal_fk` /
    `message_participants_identity_fk` refuse). Scratch S4: deleting the owner's member row fails on
    `messages_author_principal_fk` before the deferred owner FK is even reached. `graft grep` finds 22
    member-row and 5 principal deletes in workflow/http tests, several of which assert revocation.
  - Supporting D3: `participant_identities` has no FK to `companies`; identities disappear on company delete
    only through `participant_identities_principal_fk ... CASCADE`. Scratch S16: with that FK also
    `NO ACTION`, `DELETE FROM companies` fails; with it left `CASCADE` (D3) the delete removes everything.
    So D3's choice is required by the plan's own "deleting the company still removes everything" test, not
    merely sufficient.
- **Suggestion:** add these classes to D2/D3's fixture note with a rule that keeps revocation assertions
  intact (e.g. revoke through `remove_member` semantics; keep a principal deletion only where the principal
  has no message history), and cite S16 as the reason for keeping the fourth FK.
- **Resolution history:** 2026-10-31 — open (optional).
  - 2026-10-31 — recheck FIXAPP-P01-G1-R2 against G1 r2 (`logs/G1-r2-decisions.md` sha256 `b85869d6…54ac`, HEAD e6b0a51, digest `9d5bde92…4c66` unchanged): **fixed** — r2 "Implementation notes" lists the fixture classes and the rewrite rule (ownership transfer in one transaction; member removal demote-then-delete; no weakened assertions). S16 rationale not cited in r2 text but already recorded in R1 evidence; no further action needed.

## FIXAPP-P01-G1-F6 — D6 `DO NOTHING` hides an allowlisted address whose principal holds another grant (optional)

- **Classification:** optional suggestion · severity low · category correctness (latent).
- **Evidence:** the grant key is `(company_id, channel_id, principal_id, capability)` with one provenance
  (`init_schema.sql:755-764`). With `ON CONFLICT DO NOTHING`, adding an address whose principal already has
  a `manager`/`conversation_membership` grant keeps that grant, but the allowlist read-back filters
  `provenance = 'configured_allowlist'` (`channel.rs:101,112`), so the address is not shown; `channel_access`
  then re-derives the mode from the shown list on the next save (same mechanism as F1). At e6b0a51 the only
  production writer of `channel_principal_grants` is `insert_email_allowlist_grants`; the other provenances
  are written only in tests, so this cannot happen in production today.
- **Suggestion:** note the limitation in D6 (or the `channel_allowlist_*` test) so a future writer of
  non-allowlist grants (plan 04/05 territory) revisits the read path.
- **Resolution history:** 2026-10-31 — open (optional).
  - 2026-10-31 — recheck FIXAPP-P01-G1-R2 against G1 r2 (`logs/G1-r2-decisions.md` sha256 `b85869d6…54ac`, HEAD e6b0a51, digest `9d5bde92…4c66` unchanged): **dispositioned (accepted limitation)** — r2 records it under D6.

## FIXAPP-P01-G1-F7 — Implementation notes for D2 trigger and D4 error mapping (optional)

- **Classification:** optional suggestion · severity low · category implementation guidance.
- **Evidence:**
  - D2: one plpgsql function for both tables cannot use `CASE WHEN TG_TABLE_NAME = 'companies' THEN NEW.id
    ELSE NEW.company_id END` — it fails with `record "new" has no field "company_id"` when fired on
    `companies` (scratch S17 in `logs/G1-R1-reviewer-scratch.log`); branch with `IF` (or `to_jsonb`, as
    `notification_recheck_channel_access` does). On `UPDATE OF company_id`, checking only the new company is
    enough because the deferred FK covers the old one.
  - D4: `model_usage_calls_company_id_agent_id_fkey` is an auto-generated name of an inline FK
    (`init_schema.sql:10863`; confirmed in the scratch DB). The plan's usage-row test pins it; add the
    `task_harness_runs_company_id_agent_id_fkey` case too, since both deferred FKs surface at `COMMIT`.
- **Resolution history:** 2026-10-31 — open (optional).
  - 2026-10-31 — recheck FIXAPP-P01-G1-R2 against G1 r2 (`logs/G1-r2-decisions.md` sha256 `b85869d6…54ac`, HEAD e6b0a51, digest `9d5bde92…4c66` unchanged): **fixed** — r2 "Implementation notes" adopt `IF TG_TABLE_NAME` branching and tests for both run-history constraint names.

## FIXAPP-P01-G1-F8 — Adoption's grant writes invert the channel-save lock order (optional)

- **ID and context:** 2026-10-31; workflow `implement-opus-reviewed-2_2`; batch B1; stage G1 correction
  recheck; round `FIXAPP-P01-G1-R2`; reviewer B, Opus 5.5 / xhigh; phase `p01`; worktree
  `/Users/mac03/dev/openbot/.worktrees/fix-app-p01`; decision revision **G1 r2** (`logs/G1-r2-decisions.md`
  sha256 `b85869d6a676837b84ec5cb34e8f8361e385a642410f9c4e3d3d08e1a5e454ac`), HEAD e6b0a51, digest
  `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66`.
- **Classification:** optional suggestion · severity low · category concurrency. Introduced by the r2
  correction of F1 (gated decision text), not by implementation.
- **Evidence (inspection):** the re-key holds the per-identity advisory locks and then, by copying/deleting
  X's grants, fires `notification_channel_grant_recheck` (`init_schema.sql:8197`), whose function
  `notification_recheck_channel_access` takes `channels ... FOR UPDATE`. A channel form save takes the
  channel row first (`UPDATE channels`, `src/adapters/persistence/channel.rs:452`), deletes its
  `configured_allowlist` grants, then takes the advisory lock per listed address inside
  `insert_email_allowlist_grants` (`channel.rs:218-235`). A save of channel K listing the adopted address,
  concurrent with that address's adoption, can deadlock; PostgreSQL aborts one side (40P01), so state stays
  consistent (both serial orders converge on the person holding the grant), but the email change or the
  save surfaces as an internal error.
- **Suggestion:** state the accepted behaviour in D1 (a deadlock-aborted email change is reported as a
  retryable failure and changes nothing), or take the affected channel rows `FOR UPDATE` in a fixed order
  before the advisory locks; optionally cover it with a competing-claimant test (adoption vs channel save).
  Not acceptance-blocking.
- **Instruction lesson:** observation — trigger side effects (row locks taken by AFTER triggers) are easy to
  miss in lock-order reasoning; candidate prompt for gate decisions: "list locks taken by triggers on every
  table the transaction writes". Single occurrence.
- **Resolution history:** 2026-10-31 — open (optional).
  - 2026-11-01 — checked in FIXAPP-P01-B1-R1: **dispositioned (accepted)** — TASK.md D6 records the deadlock as retryable for a rare user-initiated action; implementation matches the analysed lock order (`account_identity.rs` grant copy after advisory locks). No further action.

---

Common context for the B1 findings below (`FIXAPP-P01-B1-F1` … `F6`): date 2026-11-01; batch B1 (plan 01,
batches 1–4) as implemented; stage batch review; review round `FIXAPP-P01-B1-R1`; reviewer B, Opus 5.5 / xhigh;
phase `p01`; worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p01`; review base = HEAD `e6b0a51`,
uncommitted changes, digest `b01b09456e7ea3e98d6af9c47bd04cfb52fe2bc2e02f639b31eb00171247f7de` (exclusion
`plan/fix-app/phases/p01/docs`); gated decisions G1 r2 (+ F8 disposition). All originate in implementation, not
in the gated decisions.

## FIXAPP-P01-B1-F1 — A membership-revocation test case now passes on its own fixture's failure

- **Classification:** actionable defect · severity **medium** · category verification (weakened test).
- **Evidence:** `src/adapters/persistence/workflow/action_dispatch_remote_tests.rs:66-94`
  (`workflow_action_dispatch_service_post_marker_revoke_cancel_zero_provider_polls`) still injects
  `"DELETE FROM company_members WHERE company_id=$1"` after the remote marker, through `Intercept::remote`
  (`:36-41`), which runs it with `.execute(self.pool).await?`. Under D2 that autocommit statement deletes the
  owner's member row and is refused at commit by `companies_owner_member_fk`; the `?` turns the refusal into the
  `Err` that the test asserts (`dispatch_remote(..).is_err()`, zero provider calls, facts `(1, 0, 0)`). The
  revocation never happens, so this case no longer tests "membership revoked after the marker cancels with zero
  provider polls" — it tests that a database error aborts dispatch. Reproduced: running only this test on the
  frozen binary passes while the cluster log records `update or delete on table "company_members" violates
  foreign key constraint "companies_owner_member_fk"` (`logs/B1-R1-reviewer-remote-revoke.log`). The sibling
  tests in `action_dispatch_tests.rs` and `action_dispatch_sql_tests.rs` were rewritten to
  `test_support::revoke_membership`; this one was missed because its failure is swallowed rather than panicking,
  so neither the affected run (`t-affected-1.log`) nor the final run flagged it.
- **Required:** rewrite the case to a revocation the schema allows (e.g. `revoke_membership` for the run actor),
  and make the injection fail loudly if its statement errors (so a fixture error can never satisfy the assertion).
  Re-run the remote dispatch tests.
- **Instruction lesson:** observation — fixture enumeration by "tests that fail" misses fixtures whose errors
  are propagated into an expected-failure path. Candidate guidance (scope: schema changes that invalidate
  fixtures): "grep every fixture statement the new constraint can refuse, not only failing tests, and check that
  injected statements in expected-error tests cannot fail silently." Single occurrence.
- **Resolution history:** 2026-11-01 — open.
  - 2026-11-01 — recheck FIXAPP-P01-B1-R2 (digest `ca8cf08e…dc23d`, HEAD e6b0a51): **fixed**. The case now uses `REVOKE_ALL_MEMBERSHIPS` (every member revoked through `test_support::revoke_membership` in one transaction, committed with `expect`), and every other injected statement `expect`s success. Reviewer run of the test alone on the rebuilt binary: passes with **no** ERROR lines in the cluster log (`logs/B1-R2-reviewer-tests.log`), so the revocation now really commits before the assertion.

## FIXAPP-P01-B1-F2 — Accounts whose stored address does not parse can no longer change it

- **Classification:** actionable defect · severity **low** · category correctness (regression, edge case).
- **Evidence:** `rekey_account_identity_on` (`src/adapters/persistence/account_identity.rs:58-62`) parses the
  old address with `account_email_identity` before it looks for person principals, and both email writes now
  call it (`src/adapters/persistence/user.rs` bypass `update_profile` and confirmed Email change). Registration
  only checks `is_plausible_address` (`src/application/use_cases/user.rs:1143-1155`), which accepts addresses
  that `EmailIdentity::parse` (lettre `Mailbox`, `email_address` local-part/domain rules) rejects, e.g.
  `a,b@example.com` or `x@a..b.example`; such accounts are creatable in bypass mode. Before this batch such an
  account could replace its address with a valid one; now both paths fail with
  `BadRequest("Invalid account email: …")`, even when the account has no company. (An unparseable old address
  can never have an identity row, since `create_person_principal_on` rejects it.)
- **Required:** do not fail on the old address: return early when there are no person principals, and treat an
  unparseable old address as nothing to detach. Add a unit/DB test for an account with an unparseable stored
  address changing to a valid one.
- **Instruction lesson:** cause — a new hard dependency on stricter parsing inserted into an existing write path.
  No instruction change proposed.
- **Resolution history:** 2026-11-01 — open.
  - 2026-11-01 — recheck FIXAPP-P01-B1-R2 (digest `ca8cf08e…dc23d`, HEAD e6b0a51): **fixed**. `rekey_account_identity_on` returns early with no person principal and treats an unparseable old address as nothing to detach (`account_email_identity(old).ok()`); the new address must still parse when there is something to re-key. New test `account_email_change_with_an_unparseable_old_address_and_no_company_succeeds` (fixture asserts the address is rejected by the parser) passes. An unparseable old address together with companies cannot arise any more, since such an address can neither create a company nor be installed while the account has one.

## FIXAPP-P01-B1-F3 — `taken_address_error`'s doc comment now heads `locked_account_email`

- **Classification:** actionable defect · severity **low** · category documentation.
- **Evidence:** `src/adapters/persistence/user.rs:825-830`: the new function was inserted between
  `taken_address_error`'s doc comment ("The address having been taken in the fifteen minutes …") and its
  `fn`, so rustdoc attaches both paragraphs to `locked_account_email` and `taken_address_error` (`:843`) loses
  its documentation.
- **Required:** move `locked_account_email` (with its own doc) above the old doc comment.
- **Resolution history:** 2026-11-01 — open.
  - 2026-11-01 — recheck FIXAPP-P01-B1-R2 (digest `ca8cf08e…dc23d`, HEAD e6b0a51): **fixed** — `locked_account_email` now precedes `taken_address_error`'s restored doc comment (`user.rs:825-843`).

## FIXAPP-P01-B1-F4 — The NO ACTION comments give a reason that does not hold (optional)

- **Classification:** optional suggestion · severity low · category documentation accuracy.
- **Evidence:** the three D3 FK comments in `migrations/20260817000000_init_schema.sql` ("Not RESTRICT, which is
  checked row by row and would fail a company delete that removes both sides in one statement") and the doc of
  `agent_retire_company_delete_still_removes_everything`. Reviewer probe on the frozen schema with the three FKs
  switched to `ON DELETE RESTRICT`: the G1 fixture's company delete still succeeds
  (`logs/B1-R1-reviewer-restrict-probe.log`); PostgreSQL implements both as after-statement checks, the
  documented difference being that NO ACTION can be deferred. The plan itself states the RESTRICT claim, so NO
  ACTION is the right choice to keep; only the stated reason is inaccurate.
- **Suggestion:** reword to the true reason (NO ACTION is the plan's choice and the deferrable default; both it
  and RESTRICT see the end of the statement's cascades) so a future reader does not rely on a false property.
- **Resolution history:** 2026-11-01 — open (optional).
  - 2026-11-01 — recheck FIXAPP-P01-B1-R2 (digest `ca8cf08e…dc23d`, HEAD e6b0a51): **fixed (optional)** — the three schema comments and the test doc now state only that NO ACTION is checked at the end of the statement.

## FIXAPP-P01-B1-F5 — The two email-change races have no barrier (optional)

- **Classification:** optional suggestion · severity low · category verification strength.
- **Evidence:** `account_email_change_races_invite_acceptance` and `account_email_change_races_inbound_observation`
  (`src/adapters/persistence/account_identity_tests.rs`) spawn the claimants and join them, four iterations,
  without forcing an overlap; the invite-edit and agent-retire races use `wait_until_a_backend_is_blocked` to
  make the contended interleaving certain. The outcome-independent assertions are correct, but most iterations
  may run serially.
- **Suggestion:** in at least one iteration, hold the users row (or the identity advisory lock) in an open
  transaction, start the competitor, wait until it blocks, then release — so the serialization the D1 comment
  relies on is actually exercised.
- **Resolution history:** 2026-11-01 — open (optional).
  - 2026-11-01 — recheck FIXAPP-P01-B1-R2 (digest `ca8cf08e…dc23d`, HEAD e6b0a51): **declined by A (optional), accepted**. Note: the stated reason is partly inaccurate — the invite-edit and agent-retire barrier tests exercise other protocols, not the users-row/advisory-lock ordering of D1 — but the race tests' assertions are outcome-independent and the suggestion was optional.

## FIXAPP-P01-B1-F6 — Retiring an agent that never spoke leaves a hollow principal (optional)

- **Classification:** optional suggestion · severity low · category data hygiene (plan-mandated behaviour).
- **Evidence:** `AgentPersistence::delete` (`src/adapters/persistence/agent.rs:360-401`) always demotes the agent's
  principal to an `external` principal with no identity, as decided. Callers that delete fresh agents — e.g. the
  onboarding rollback (`src/adapters/http/routes/onboarding.rs:323-328`) — therefore leave one unreachable
  principal per rolled-back agent, where the cascade used to remove it.
- **Suggestion:** none required for plan 01 (the plan mandates demote-then-delete); consider, in a later plan,
  deleting the principal when nothing references it.
- **Resolution history:** 2026-11-01 — open (optional).
  - 2026-11-01 — recheck FIXAPP-P01-B1-R2 (digest `ca8cf08e…dc23d`, HEAD e6b0a51): **deferred by A (optional)** to a later plan; plan 01 mandates demote-then-delete.
