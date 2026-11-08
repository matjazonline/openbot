## G1 — Architecture decisions (revision r1)

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
    `thread_principals` is not changed (the person stays a party to its threads; the old address
    is not added, so replies are not routed to an address the account no longer owns).
  - **Register new:** upsert the new address as the account identity with the statement
    `create_person_principal_on` already runs (`ON CONFLICT (company_id, transport, namespace,
    subject) DO UPDATE SET principal_id = EXCLUDED.principal_id, status='verified',
    provenance='account', ...`), extracted into a shared helper so both use one statement.
    If the new address is already an outsider identity (seen in mail earlier), the same
    one-statement authorship move is applied in the other direction (its authored messages move to
    the person) so the composite FK holds. The former outsider principal keeps its grants and
    thread rows but no longer has this identity. If the new address is attached to another
    `person`/`agent`/`system` principal, return `Conflict` (same rule as
    `create_person_principal_on`); this aborts the email change.
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

