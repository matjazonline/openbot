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

