# TASK — fix-app p01 (Identity and ownership)

## Identity

- Phase ID: `p01`; workflow: `implement-opus-reviewed-2_2`
  (skill dir `/Users/mac03/dev/openbot/.claude/skills/implement-opus-reviewed-2_2`).
- Record root: `/Users/mac03/dev/openbot/.worktrees/fix-app-p01/plan/fix-app/phases/p01/docs/`.
- Root = Author (A): phase root agent spawned by the manager, Opus 5.5 at `high` effort (stated
  by the manager at spawn). Reviewer (B): Opus 5.5 / `xhigh`, agent ID recorded below once spawned.
- Worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p01`, branch `fix-app/p01`, starting commit
  `e6b0a51`. Preserved pre-existing changes: none (clean tree at start; `COORDINATION.md` lives
  only in the primary checkout and is not committed).
- Resources (fix-app-I002): private Postgres 18 cluster `127.0.0.1:55401` (trust auth, owned by the
  manager), dev DB `mail_agents`, tests derive `mail_agents_test`; `scripts/check.sh` creates its
  own `mail_agents_check_<pid>`. `CARGO_TARGET_DIR` = worktree `target/`.
  `check.sh` defaults `DATABASE_URL` to localhost:5432, so it must be run with `.env` exported.
- Context: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` is set in `/Users/mac03/dev/openbot/.claude/settings.local.json`
  (verified in this session's environment).

## Coordination

- Authority: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md`, writer = manager root session.
  Last checked revision: 2 (copy `logs/COORDINATION-r2.md`; r1 copy `logs/COORDINATION-r1.md`).
- Applicable instructions: fix-app-I001 r1 (repo/plan rules; edit only named init_schema
  statements in place; final gate reset-db + check.sh) — acknowledged; fix-app-I002 r1 (isolation
  and resources) — acknowledged; fix-app-I003 r1 (commit accepted phase on `fix-app/p01`, no push,
  do not commit COORDINATION.md) — acknowledged; fix-app-I004 r1 (user instruction, revision 2:
  do NOT run the full suite or `scripts/check.sh`; run targeted tests for affected behaviour,
  callers and shared contracts with nonzero counts, after `reset-db --all` when the schema changed;
  `cargo fmt --check` and compiling affected targets allowed; regenerate `.sqlx/`) — acknowledged,
  supersedes the plan's final gate and this file's earlier "full DB-backed lib suite" step. Fixtures
  invalidated by the new schema invariants are found by exhaustive grep of the patterns that
  violate them and run file by file, instead of the full suite.
- Cross-phase notices targeting p01: none at revision 1.
- Pending cross-phase requests: see "Cross-phase notices requested" below.

## Original requirements

- `plan/fix-app/README.md`, `plan/fix-app/01-identity-and-ownership.md` (spans as of `4e92edd`,
  re-checked against `e6b0a51`).
- `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`, `src/adapters/persistence/AGENTS.md`.

### Acceptance criteria (from the plan's Outcome)

1. Changing an account's email (confirmed and bypass paths) moves its trust to the new address in
   every company in the same commit; the old address becomes an ordinary outsider.
2. No invite or member write can change the owner's membership; the database rejects an owner row
   that disagrees with `companies.user_id`.
3. Deleting an agent never deletes a message, a participant row or thread history; the database
   refuses any delete that would.
4. Editing an invite changes only a pending invite of that company. Editing a channel allowlist
   touches only grants the allowlist owns.

## Batches

The plan's four batches are reviewed as one workflow batch **B1** (one cohesive phase; one review),
preceded by one architecture gate **G1** because B1 contains schema/FK changes, a persistent
ownership invariant (constraint trigger), new transaction boundaries and an authorization-relevant
identity rule.

## G1 — Architecture decisions (revision r2; r1 text preserved in logs/G1-r1-decisions.md)

### D1 — Email change re-keys the account identity (plan batch 1)

- New `rekey_account_identity_on(conn, user_id, old_email, new_email)` in
  `src/adapters/persistence/participant.rs`, called inside the transaction of both email writes,
  **after** the `UPDATE users` (which takes the users row lock that company creation and invite
  acceptance also take with `SELECT ... FOR UPDATE`, so a concurrent acceptance/creation either
  commits first and is seen by the re-key, or runs after and reads the new email).
  - Confirmed path (`user.rs` `confirm_pending_account_change`, Email kind): old email read with
    `SELECT email FROM users WHERE id = $1 FOR UPDATE` in the same transaction before the UPDATE.
  - Bypass path (`user.rs` `update_profile`): today runs on the pool; it gets a transaction with
    the same old-email read, UPDATE, then re-key. No re-key when old and new normalize to the same
    qualified identity.
- For each company where the user has a `person` principal (ordered by company id), it takes the
  same per-identity advisory lock `create_person_principal_on` uses for the old and new keys (in
  sorted key order), then:
  - **Detach old:** if the old address's email identity is attached to that person principal,
    insert a new `external` principal (label = old address) and move the identity to it with
    `status = 'observed'`, `provenance = 'transport_ingress'`,
    `claim_metadata = {"kind":"observation","version":1}`, `display_label = NULL` — exactly what
    an observed outsider gets from `resolve_or_create_external_identity_on`.
  - **Authorship follows the address.** `messages_authored_identity_author_fk`
    (`(company_id, author_principal_id, authored_identity_id) → participant_identities(company_id,
    principal_id, id)`, no ON UPDATE action) forbids moving an identity that authored messages
    unless those messages move with it. So the identity UPDATE and
    `UPDATE messages SET author_principal_id = <new external> WHERE authored_identity_id = <identity>
    AND author_principal_id = <person>` run as **one statement** (data-modifying CTE): NO ACTION
    FKs are checked at end of statement, so the pair is consistent. This is the literal reading of
    "History stays attached to the address that actually sent or received it": messages sent from
    the old address keep that address and are now authored by its outsider principal;
    `message_participants` (To/Cc/sender rows) reference the identity id and follow it without a
    write. Messages the person wrote in the UI (`authored_identity_id IS NULL`) stay the person's.
    `thread_principals` is not changed: the person stays a party to its threads, and the old
    address is not added, so the thread *participant list* no longer offers it. (r2, per G1-F4:)
    ordinary email reply targeting still follows the address — a thread whose latest inbound came
    from the old address replies to it (`latest_replyable_email_context` uses the authored
    identity), and in-flight `reply` waits compare against the destination message's author, so
    they follow the address too. Accepted consequences of "history follows the address" plus the
    existing promotion: whoever later registers the old address and is adopted becomes the author
    of the previous owner's address-sent messages (shown under the adopter's name).
  - **Register new:** upsert the new address as the account identity with the statement
    `create_person_principal_on` already runs (`ON CONFLICT (company_id, transport, namespace,
    subject) DO UPDATE SET principal_id = EXCLUDED.principal_id, status='verified',
    provenance='account', ...`), extracted into a shared helper so both use one statement.
    If the new address is attached to another `person`/`agent`/`system` principal, return
    `Conflict` (same rule as `create_person_principal_on`); this aborts the email change.
  - **(r2, per G1-F1) Adopting an outsider's address.** If the new address is already an outsider
    identity on an `external` principal X (seen in mail earlier, or written by a channel allowlist),
    the person adopts what promotion would have kept with the address, in the same transaction:
    1. the identity and its authored messages move to the person in one statement (as above, other
       direction);
    2. if X then has **no identity left** (the normal case — the resolver creates one external
       principal per address), X's `channel_principal_grants` are copied to the person with their
       original provenance and capability (`INSERT ... SELECT ... ON CONFLICT DO NOTHING`, so a grant
       the person already holds for that channel/capability keeps the person's row) and deleted from
       X, and X's `thread_principals` rows are copied to the person (`ON CONFLICT DO NOTHING`) and
       deleted from X. X itself is not deleted (other rows such as workflow message destinations or
       notification actors may reference it); it is left with no identity, no grants and no thread
       rows, so no reader can reach it.
    3. if X still has other identities (not produced by current writers), its grants and thread rows
       stay with X, because they also serve those identities.
    Effect: an allowlisted outsider address that becomes a member's account address stays on the
    allowlist read-back (now via the person's grant), the member keeps allowlist access for that
    mailbox, the channel mode cannot silently widen on the next save, and the address stays in the
    thread participant lists it was in. On the old-address side nothing moves: grants the person
    held (e.g. a `configured_allowlist` grant written when their old address was allowlisted) stay
    on the person, so the allowlist read-back then shows the new address — the trust moved.
  - **(r2, per G1-F3, product note)** The bypass path installs an unverified address, so it can now
    adopt an existing outsider's identity, history and grants in companies the user already
    belongs to. The plan explicitly requires re-keying on both paths ("since company creation
    applies no other rule"), and the bypass is a deployment-level switch for unverified setups; this
    is recorded as a reported consequence for the manager/user, not changed.
- Failure behaviour: any error rolls back the whole email change (users row, pending-change
  consumption and identity moves are one commit).
- No new identity status; `disabled` stays unused (08 drops it).

### D2 — Ownership is one fact (plan batch 2, schema)

- Init schema: `companies_owner_member_fk FOREIGN KEY (id, user_id) REFERENCES
  company_members(company_id, user_id) DEFERRABLE INITIALLY DEFERRED` (NO ACTION). Uses the
  existing `company_members_company_user_key UNIQUE (company_id, user_id)`.
- Init schema: function `enforce_company_owner_agreement()` and two
  `CONSTRAINT TRIGGER ... DEFERRABLE INITIALLY DEFERRED FOR EACH ROW` triggers:
  `companies_owner_agreement` (AFTER INSERT OR UPDATE OF user_id ON companies) and
  `company_members_owner_agreement` (AFTER INSERT OR DELETE OR UPDATE OF company_id, user_id, role
  ON company_members). At commit the function checks, for the affected company if it still
  exists, that the member row for `companies.user_id` has `role = 'owner'`; otherwise
  `RAISE ... ERRCODE '23514', CONSTRAINT 'companies_owner_member_agreement'`. Together with the
  existing `company_members_one_owner_idx` (one owner row per company) and the FK, this makes
  "exactly one owner row, and it is `companies.user_id`'s" a commit-time invariant.
- Concurrency: no write skew is possible — every way to break the invariant either modifies the
  company row or the owner/new-owner member rows (row locks serialize), or is visible to the
  committing transaction's own check (a second owner row hits the unique index; a demoted owner
  row fails its own check; a deleted row fails the deferred FK). No extra locking in the trigger
  (a `FOR UPDATE` on companies at commit would deadlock against FK `KEY SHARE` holders).
- Company delete: `company_members` cascade with the company; the trigger sees no company and
  passes; the FK's referencing row is gone in the same statement.
- Test fixtures that hand-transfer ownership with separate autocommit statements, or demote/delete
  the owner row, become invalid states. They are rewritten to valid ones through one shared test
  helper (single transaction: ensure new owner member row, demote the previous owner, set
  `companies.user_id`, promote the new owner). Assertions are not weakened.

### D3 — History-bearing FKs become NO ACTION (plan batch 3, schema)

- The plan says "the three cascades in (3)" but lists four FKs. Interpretation: the three FKs
  whose cascade deletes history directly:
  - `principals_company_agent_fk` (agent → its principal; root of the chain),
  - `messages_author_principal_fk` (principal → messages it wrote),
  - `message_participants_identity_fk` (identity → To/Cc/sender rows).
  `ON DELETE CASCADE` is dropped (default NO ACTION, not RESTRICT, not deferrable) so a company
  delete that removes agents, principals, identities and messages in one statement still passes.
- `participant_identities_principal_fk` stays CASCADE: deleting a principal's identities is not by
  itself history loss — an identity referenced by any message participant row is now protected by
  `message_participants_identity_fk`, and one referenced as a message's authored identity belongs
  to a principal whose message authorship already blocks the delete. Changing it as well would make
  every person-principal delete fail (each has an account identity) and adds no protection for
  messages or participant rows. Reviewer: please challenge if the plan intended otherwise.
- `thread_principals_principal_fk` is not named by the plan and stays CASCADE; retirement never
  deletes the principal, so agent deletion keeps thread rows.

### D4 — Agents are retired, not cascaded (plan batch 3, code)

- `AgentPersistence::delete` (`src/adapters/persistence/agent.rs`) becomes one transaction:
  1. `SELECT ... FROM agents WHERE id = $1 FOR UPDATE` (serializes with a concurrent
     `create_agent_principal_on` upsert, whose FK check needs `KEY SHARE` on the agent row, so it
     cannot re-create an agent principal between the demotion and the delete);
  2. `UPDATE principals SET kind = 'external', agent_id = NULL, updated_at = now() WHERE
     company_id = <agent company> AND agent_id = $1` (library agents have no company and no
     principal);
  3. grants: left on the demoted principal, as `remove_member` leaves a member's (production writes
     no grants for agent principals; an external principal with no identity cannot be reached);
  4. `DELETE FROM agents WHERE id = $1`; commit.
- Error mapping by constraint name (deferred FKs surface at COMMIT, so both the DELETE and the
  commit are mapped): `task_harness_runs_company_id_agent_id_fkey` and
  `model_usage_calls_company_id_agent_id_fkey` → `Conflict("This agent has run history and cannot
  be deleted.")`. At `e6b0a51` no FK from channels or bindings to agents exists, so there is no
  database cause for "bound to a channel"; every other database error propagates unmapped instead
  of being reported as a channel conflict.

### D5 — Invite writes cannot touch the owner (plan batch 2, code)

- Acceptance upsert: `ON CONFLICT (company_id, user_id) DO UPDATE SET role = EXCLUDED.role WHERE
  company_members.role <> 'owner'`; the result joins membership with `LEFT JOIN` so the invite is
  still reported accepted while the owner row is unchanged.
- `create_company_invite` (and `update_company_invite`, the same hole) refuse the owner's own
  address with `BadRequest`, via a new `CompanyInvitePersistence::is_company_owner_address`
  (case-insensitive, `citext`). The malformed-address error becomes `BadRequest` in both.
  The DB-level guard above still protects a race with an owner email change.
- `update_invite(company_id, id, email, role)`: the `UPDATE` adds `AND company_id = $n AND status =
  'pending'` and returns `Option`; `None` → `BadRequest("Only a pending invitation can be
  changed.")`.

### D6 — Allowlist owns only its grants (plan batch 4, routine)

- `insert_email_allowlist_grants`: `ON CONFLICT ... DO NOTHING`.
- (per G1-F8, accepted after r2 PASS; does not change a decision) Re-key holds identity advisory
  locks and then, when adopting an outsider's grants, the grant trigger locks the `channels` row; a
  channel form save takes the channel row first and then advisory locks. A simultaneous save of a
  channel that lists the adopted address can deadlock; PostgreSQL aborts one side, data stays
  consistent, and the user retries. Accepted as retryable for a rare, user-initiated action.
- (r2, per G1-F6) Known limitation accepted from the plan: an address whose principal already holds
  a non-allowlist `participate` grant is not shown in the allowlist read-back (that filters on
  `configured_allowlist`). No production writer creates non-allowlist grants today.

### Implementation notes (r2, per G1-F5/F7)

- D2 trigger function branches with `IF TG_TABLE_NAME = ...` (a `CASE` reading `NEW.company_id`
  fails when fired on `companies`).
- D4: `model_usage_calls_company_id_agent_id_fkey` (auto-named, confirmed) and
  `task_harness_runs_company_id_agent_id_fkey` both get tests.
- Fixtures known to need rewriting to valid states: raw company inserts without an owner row
  (`decision_provenance_tests.rs`, `wait_guard_tests.rs`), owner transfers in separate statements
  (7 workflow tests), owner-row demotions, and member-row/principal deletes of people with message
  history (now refused by D3). Each is rewritten through shared helpers (ownership transfer in one
  transaction; member removal through demote-then-delete like `remove_member`), never by weakening
  assertions; the full DB-backed lib suite run after the schema edit enumerates them.

## Verification plan

Focused (disposable DB on the phase cluster; `TEST_DATABASE_URL` = `mail_agents_fix_app_01`):
`cargo test --locked --offline --lib <filter>` for `account_email_change_`, `company_owner_`,
`company_invite_`, `agent_retire_`, `channel_allowlist_`, `ui_team`; plus the full DB-backed lib
suite once after the schema edits (shared infrastructure: every fixture that creates companies or
members) to find fixtures the new invariants invalidate. Nonzero test counts recorded.

Competing-claimant tests (r2, per G1-F2; AGENTS.md concurrency rule), each run with real
concurrent transactions on the pool, asserting the invariant whichever claimant wins:
- `account_email_change_races_invite_acceptance`: a confirmed email change and an acceptance of an
  invite (addressed to the old email, in a second company) run concurrently. Either the acceptance
  is refused (it saw the new email) or it committed first and the re-key moved its new principal's
  identity: in no outcome does the old address resolve to a member principal.
- `account_email_change_races_inbound_observation`: the email change runs concurrently with
  `resolve_or_create_external_identity` for the new address and for the old address. Afterwards
  the new address resolves to the person, the old address to an external principal, and exactly
  one principal holds each identity (no orphan duplicate).
- `agent_retire_races_agent_principal_upsert`: an open transaction runs `create_agent_principal_on`
  (as a concurrent agent message would) while `delete` runs; whichever commits first, the delete
  either succeeds with no `agent` principal left for that agent, or the upsert fails on the deleted
  agent — never an FK error from the delete and never a principal still pointing at a deleted agent.
Final gate: `./scripts/reset-db.sh --all --yes` then `scripts/check.sh` with `.env` exported.

## Status

- G1: r1 reviewed (FIXAPP-P01-G1-R1: changes required, F1+F2 actionable). r2 addressed F1–F7.
  **G1 r2 PASS** (FIXAPP-P01-G1-R2, decision text sha256 `b85869d6…54ac`). F8 (optional, new):
  re-key vs channel-form save lock order can deadlock; disposition below.
- Process deviation (recorded by B in G1-R2): implementation of D2–D6 (schema, D6, D5, D4) started
  while the r2 recheck was pending. Rationale: D2–D6 had passed unchanged in r1 and r2 changed
  only D1 and the verification plan; D1 code was held until the r2 PASS. All of it is reviewed in
  B1 against a fresh digest.
- B1: implemented; focused checks passing (see "B1 implementation" below); awaiting freeze and
  review.

## B1 implementation (author record)

### Changed files

- Schema `migrations/20260817000000_init_schema.sql` (named statements only): new function
  `enforce_company_owner_agreement()`; constraint triggers `companies_owner_agreement` and
  `company_members_owner_agreement`; FK `companies_owner_member_fk`; `ON DELETE CASCADE` removed
  from `message_participants_identity_fk`, `messages_author_principal_fk`,
  `principals_company_agent_fk` (D2, D3).
- D1: new `src/adapters/persistence/account_identity.rs` (`rekey_account_identity_on` and its
  steps) instead of `participant.rs` itself, because `participant.rs` would cross the ~1,000-line
  split threshold in `src/AGENTS.md`; `participant.rs` gains the shared `lock_identity_key`,
  `account_email_identity` and `upsert_account_identity_on` (the statement
  `create_person_principal_on` already ran, now also moving authored messages with a re-pointed
  identity). `user.rs`: both email writes read the old address under the users row lock
  (`locked_account_email`) and call the re-key after the `UPDATE`; `update_profile` now runs in a
  transaction.
- D4: `agent.rs` `delete` retires (lock agent, demote principal, delete) and maps errors by
  constraint name (`agent_delete_error`).
- D5: `persistence/company_invite.rs` (`is_company_owner_address`, guarded `update_invite`, guarded
  acceptance upsert), `use_cases/company_invite.rs` (`invite_address`, `refuse_owner_address`,
  `not_pending_error`; trait signature `update_invite(company_id, id, ...) -> Option`).
- D6: `channel.rs` `ON CONFLICT ... DO NOTHING`.
- Tests: `account_identity_tests.rs` (7 `account_email_change_*`, incl. 2 competing-claimant),
  `company_owner_tests.rs` (4 `company_owner_*`, 3 `company_invite_*`, incl. 1 competing-claimant),
  `agent_retire_tests.rs` (3 `agent_retire_*`, incl. 1 competing-claimant),
  `channel_allowlist_tests.rs` (1), plus `agent_retire_with_usage_rows_reports_run_history`
  (`workflow/model_usage_tests.rs`) and `agent_retire_with_harness_runs_reports_run_history`
  (`workflow/harness_owner_tests.rs`), which reuse those modules' run fixtures.
- Fixtures rewritten to states the new invariants allow (assertions unchanged), via new helpers in
  `test_support.rs`: `insert_company_with_owner` (raw company inserts: `decision_provenance_tests`,
  `wait_guard_tests`), `transfer_company_ownership` (7 hand transfers: `message_delivery_tests`,
  `jev_assets_race_tests`, `admission_authority_tests`, `binding_association_tests`,
  `jev_assets_tests`, `http_tests`; `message_authority_tests::demote` reorders its script instead),
  `revoke_membership`/`revoke_membership_now` (owner member-row deletes modelling "actor lost all
  authority": `action_tests`, `admission_authority_tests`, `action_cancellation_order_tests`,
  `action_dispatch_sql_tests`, `action_dispatch_tests`, `control_tests`, `binding_tests`,
  `jev_tests`, `http_tests`, `mcp_tests`, `workflow/tests.rs` x2). The member row is deleted first
  so competitors block where they did before; for the owner a fresh replacement account then takes
  the company in the same transaction.
- `use_cases/company_invite_tests.rs` mock follows the new trait (pending/company-scoped update).

### Focused checks (I004 scope; `TEST_DATABASE_URL=.../mail_agents_fix_app_01`, recreated after
schema edits; dev DB reset with `./scripts/reset-db.sh --all --yes` after each schema edit)

1. `cargo test --lib -- account_email_change_ company_owner_ company_invite_ agent_retire_
   channel_allowlist_` → 24 passed, 0 failed (`logs/t-focused-1.log`). First attempt failed in the
   trigger function (`record "old" has no field "company_id"`), fixed by branching into an array.
2. Affected-callers/fixtures run, 62 filters in `logs/affected-filters.txt` (every module that
   transfers ownership, edits/deletes member rows, deletes companies/principals, inserts raw
   companies, and the callers of every changed function) → 1010 passed, 10 failed, 1 ignored
   (`logs/t-affected-1.log`). All 10 failures were fixtures deleting the owner's member row
   (`companies_owner_member_fk` at commit) — the intended invariant; fixed via `revoke_membership`.
3. The 10 + `persistence::workflow::tests::workflow_` + `persistence::thread` + `inbound_event` +
   `channel_edit` → 93 passed, 0 failed (`logs/t-affected-2.log`).
4. `cargo fmt --all` then `cargo fmt --all -- --check` → exit 0 (`logs/fmt-check.log`).
5. `cargo sqlx prepare -- --all-targets` (live dev DB) → 4 new `.sqlx/` entries
   (`is_company_owner_address`, `locked_account_email`, agent `SELECT ... FOR UPDATE`, agent
   principal demotion) (`logs/sqlx-prepare.log`).
6. `SQLX_OFFLINE=true cargo clippy --locked --offline --lib --tests -- -D warnings` → exit 0
   (`logs/clippy.log`) — compiles every affected target from the committed cache.
7. Final focused run on the frozen artifact: all 66 filters of `logs/affected-filters.txt`,
   `SQLX_OFFLINE=true cargo test --locked --offline --lib -- <filters>` after
   `./scripts/reset-db.sh --all --yes` and a fresh `mail_agents_fix_app_01` → see
   `logs/t-affected-3.log`: **1094 passed, 0 failed, 1 ignored** (pre-existing `#[ignore]`). Per-filter counts: account_email_change_ 7, company_owner_ 4 (+2 pre-existing crud flows), company_invite_ see log, agent_retire_ 5, channel_allowlist_ 1, ui_team see log.
- Not run (I004): full suite, `scripts/check.sh`, stack budget.

### Frozen artifact for B1 review

- Phase start / review base / HEAD: `e6b0a51`; digest
  `b01b09456e7ea3e98d6af9c47bd04cfb52fe2bc2e02f639b31eb00171247f7de`; exclusion
  `plan/fix-app/phases/p01/docs`. Uncommitted working-tree changes only.
- Coordination: revision 2 (amended in place with a disk note and a manager log line; copy
  `logs/COORDINATION-r2-amended.md`); no notices target p01.

## Cross-phase notices requested (for the manager to publish)

- N1 → all phases with DB fixtures (p02, p03, p05–p08), effective once p01 merges: `companies.user_id`
  and the owner `company_members` row are one fact checked at commit (`companies_owner_member_fk`,
  constraint triggers `companies_owner_agreement` / `company_members_owner_agreement`). Fixtures may
  no longer insert a company without its owner row, change `companies.user_id` alone, or demote or
  delete the owner row. Use `test_support::{insert_company_with_owner, transfer_company_ownership,
  revoke_membership, revoke_membership_now}`. A fixture that runs such SQL with `?` or inside an
  expected-error path can silently "pass" on the refusal (see FIXAPP-P01-B1-F1): make injected
  statements `expect` success.
- N2 → p05 (Known overlap: FKs around principals, messages, member removal):
  `principals_company_agent_fk`, `messages_author_principal_fk`, `message_participants_identity_fk`
  are now NO ACTION (CASCADE removed); `participant_identities_principal_fk` and
  `thread_principals_principal_fk` stay CASCADE. Deleting a principal that authored messages or whose
  identity is in `message_participants` is refused; member removal must demote first
  (`remove_member`), agent deletion retires (`AgentPersistence::delete`). New FKs from 05 must not
  reintroduce a cascade into messages/participants, and 05's merge should rerun `agent_retire_`,
  `company_owner_`, `account_email_change_` and the 66 filters in `logs/affected-filters.txt` that
  touch its area.
- N3 → p04: ownership is now enforced as one fact (04's precondition). `CompanyInvitePersistence`
  gained `is_company_owner_address`; `update_invite(company_id, id, email, role) -> Option` matches
  only a pending invite of that company. Re-key on email change lives in
  `adapters/persistence/account_identity.rs`.
- N4 → p08: identity status `disabled` remains unused by p01 (old addresses are detached as
  `observed`/`transport_ingress` outsiders), so 08 may drop it as planned.
- Product note for the manager/user (G1-F3): on the unverified-email bypass path an address change
  can adopt an existing outsider's identity, history and grants (mail authorization only; UI grant
  authority stays withheld for unverified accounts). Implemented as the plan requires.

## Acceptance

- **Accepted in phase** (not integrated): B PASS FIXAPP-P01-B1-R2 for digest
  `ca8cf08ecb8b98c92e23c2585a22823c1605209d746e7cdc22b36f41d55dc23d`; root recomputed the same
  digest; COORDINATION.md revision 2, applicable instructions I001–I004 satisfied (I004 replaces the
  reset-db + check.sh final gate; full suite, check.sh and stack budget not run, by instruction);
  no open acceptance-blocking findings. B1-F5 note corrected: B observes the barrier tests cover
  other locking, not the email-change lock order; the email-change races remain unbarriered
  (optional, accepted).

## Candidate instruction improvements

(none yet)

## B1 corrections after FIXAPP-P01-B1-R1 (changes required)

- B1-F1 (medium): `workflow/action_dispatch_remote_tests.rs` — the "DELETE FROM company_members"
  injection is replaced by `REVOKE_ALL_MEMBERSHIPS` (every member revoked via
  `test_support::revoke_membership` in one transaction); every injected statement now
  `expect`s success so a fixture error can no longer pass as the expected dispatch error.
- B1-F2 (low): `rekey_account_identity_on` returns early when the account has no person principal
  and treats an unparseable old address as nothing to detach; test
  `account_email_change_with_an_unparseable_old_address_and_no_company_succeeds`.
- B1-F3 (low): `user.rs` doc comments of `locked_account_email` / `taken_address_error` restored.
- B1-F4 (optional, done): NO ACTION comments (3 in init schema) and the company-delete test doc no
  longer claim RESTRICT would fail; they state NO ACTION is checked at end of statement.
- B1-F5 (optional, declined): the email-change races run four iterations without a barrier; their
  invariant holds in every interleaving and the lock-ordering protocol is exercised
  deterministically by the invite-edit and agent-retire barrier tests. Not changed.
- B1-F6 (optional, deferred): empty outsider principal left by retiring a silent agent — later plan.
- Checks: dev + test DB reset (schema comment changed); `cargo fmt --all -- --check` exit 0;
  `SQLX_OFFLINE=true cargo clippy --locked --offline --lib --tests -- -D warnings` exit 0
  (`logs/clippy-2.log`); `cargo test --locked --offline --lib -- account_email_change_ action_dispatch
  agent_retire_ company_owner_ company_invite_ channel_allowlist_ persistence::user` → 346 passed,
  0 failed (`logs/t-recheck-1.log`; account_email_change_ 8, action_dispatch 309 incl. the remote
  revoke test). No macro query changed, so `.sqlx/` is unchanged.
- Re-frozen digest: `ca8cf08ecb8b98c92e23c2585a22823c1605209d746e7cdc22b36f41d55dc23d` (base/HEAD e6b0a51, exclusion plan/fix-app/phases/p01/docs).
