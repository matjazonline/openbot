# 01 — Identity and ownership

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Part of [fix-app](README.md). No prerequisites. [04](04-authorization-in-application.md) starts
after this merges, and [08](08-schema-cleanup.md) relies on the identity decision below.

## Outcome

- Changing an account's email moves its trust to the new address in every company, in the same
  commit. The old address becomes an ordinary outsider.
- No invite or member write can change the owner's membership. The database rejects an owner row
  that disagrees with `companies.user_id`.
- Deleting an agent never deletes a message, a participant row or thread history. The database
  refuses any delete that would.
- Editing an invite changes only a pending invite of that company. Editing a channel allowlist
  touches only the grants the allowlist owns.

## What happens today

### 1. An email change keeps the old address trusted

- **The identity is insert-only.** The account's verified email identity is created at company
  creation (`src/adapters/persistence/company.rs:142`) and at invite acceptance
  (`src/adapters/persistence/company_invite.rs:276`), through `create_person_principal_on`
  (`src/adapters/persistence/participant.rs:260`, upsert at `:345-358`).
- **Neither email write touches it.** That covers the confirmed change
  (`src/adapters/persistence/user.rs:723`) and the unverified bypass (`user.rs:317`, reached from
  `src/application/use_cases/user.rs:874-880`).
- **Trust is read through that identity.** `access_context_for_identity` (`participant.rs:453-469`)
  resolves membership through it, and the SMTP adapter uses the result to decide whether a sender
  is trusted (`src/adapters/smtp/server.rs:653-665`).
- **Effect:**
  - Mail from the old address keeps the person's owner or member authority.
  - Mail from the new address is treated as a stranger's.
  - Whoever registers the old address next inherits the trust.

### 2. An owner can demote themselves

- **How:** `create_company_invite` (`src/application/use_cases/company_invite.rs:93-113`) accepts
  the owner's own address. Accepting the invite then runs `ON CONFLICT (company_id, user_id) DO
  UPDATE SET role = EXCLUDED.role` (`company_invite.rs:240-252`), so the owner row becomes
  `member`.
- **Effect:**
  - The owner loses notification visibility, because `notification_principal_can_view` checks
    `member.role = 'owner'` (`migrations/20260817000000_init_schema.sql:827`).
  - The owner is listed as a removable member, and can then be removed from their own company.
- **Root cause: ownership is two facts with nothing tying them together.**
  - `companies.user_id` is read by about 43 Rust call sites.
  - `company_members.role = 'owner'` is read by the SQL policy functions and by
    `workflow/decisions.rs:312`.
  - They agree only because company creation writes both in one transaction (`company.rs:108-140`).

### 3. Deleting an agent can delete what it wrote

- **The cascade chain:**
  - `principals_company_agent_fk` (`init_schema.sql:9578`) cascades from the agent to its principal.
  - From the principal, `messages_author_principal_fk` (`:9458`) cascades into the messages it
    wrote.
  - `participant_identities_principal_fk` (`:9562`) and `message_participants_identity_fk`
    (`:9441`) cascade into the To/Cc rows of customers' mail.
- **The delete:** `delete_agent` (`src/application/use_cases/agent.rs:535-555`) is a plain `DELETE
  FROM agents` (`src/adapters/persistence/agent.rs:361`).
- **Why it rarely bites today:** agents that have run survive only because unrelated plain FKs
  block the delete: `model_usage_calls` (`:10863`), `task_harness_runs` (`:9658`) and
  `internal_notes` (`:9330`).
- **Misleading error:** when those block it, the error mapping (`agent.rs:365-372`) reports every
  23503/23514 error as "active on an enabled channel".

### 4. An invite can be edited after it was accepted

- `update_invite` checks `status == "pending"` in the use case
  (`use_cases/company_invite.rs:158-170`).
- The `UPDATE` itself (`persistence/company_invite.rs:165-180`) matches on `id` alone.
- So an invite accepted between the read and the write gets its email and role rewritten.

### 5. The allowlist takes over grants it doesn't own

- Adding an address to a channel allowlist upserts with `DO UPDATE SET provenance =
  EXCLUDED.provenance` (`src/adapters/persistence/channel.rs:240-255`). That relabels a `manager` or
  `conversation_membership` grant as `configured_allowlist`.
- The next edit deletes every `configured_allowlist` grant (`channel.rs:474-479`), so that person
  loses their access.

## Decisions

- **Detach the old address; don't disable it.**
  - In each company, move the old account identity to a new `external` principal, with status
    `observed` and the provenance an observed outsider gets.
  - History stays attached to the address that actually sent or received it, and the address stops
    granting anything.
  - If someone later registers that address, the existing promotion in `create_person_principal_on`
    adopts it, as it adopts any outsider.
  - No new status is needed, and [08](08-schema-cleanup.md) then drops the unused `disabled`.
- **Register the new address the same way.** Upsert it as the account identity using the statement
  `create_person_principal_on` already runs, in every company where the user has a person
  principal. This applies to both the confirmed path and the bypass path, since company creation
  applies no other rule.
- **Keep two ownership columns, but enforce them as one fact.**
  - Add a deferrable composite FK `companies(id, user_id) → company_members(company_id, user_id)`.
  - Add a constraint trigger that rejects a commit where the owner member row and
    `companies.user_id` disagree.
  - Collapsing to one column would rewrite the ~43 Rust reads that
    [04](04-authorization-in-application.md) replaces anyway. Reconsider once 04 has merged and
    nothing authorizes by `company.user_id`.
- **Retire agents instead of cascading.**
  - Copy `remove_member` (`company_invite.rs:409-432`): in one transaction, demote the agent's
    principal to `external` with `agent_id = NULL`, then delete the agent.
  - Change the three cascades in (3) to `NO ACTION`, not `RESTRICT`. `RESTRICT` is checked row by
    row immediately, so it can fail a company delete that cascades through principals and messages
    in one statement. `NO ACTION` is checked at the end of the statement.

## Batches

### 1 — An email change moves trust

- Add `rekey_account_identity_on(tx, user_id, old, new)` in `participant.rs`. For each company where
  the user has a person principal, it detaches the old identity as decided and upserts the new one.
- Call it inside the transaction of both email writes. The bypass path (`user.rs:317`) runs on the
  pool today, so give it a transaction.
- DB-backed tests, `account_email_change_*`:
  - After a confirmed change, and after a bypass change, the old address resolves to an external
    principal with no membership in every company.
  - The new address resolves to the person, with their role.
  - The `message_participants` rows for the old address still exist.
  - A second user who registers the old address and accepts an invite adopts the outsider.

### 2 — Ownership becomes one fact

- **Init schema:** add the composite FK and the owner-agreement constraint trigger.
- **Invite acceptance:**
  - Change the upsert to `DO UPDATE ... WHERE company_members.role <> 'owner'`.
  - `create_company_invite` refuses the owner's own address with `BadRequest`. Its malformed-address
    error also becomes `BadRequest` instead of `Internal`.
- **`update_invite`:**
  - Add `AND status = 'pending' AND company_id = $n` to the `UPDATE`.
  - When no row matches, return the existing answer: "Only a pending invitation can be changed."
- **Tests, `company_owner_*` and `company_invite_*`:**
  - Accepting an invite to one's own company leaves the role `owner`.
  - A hand-written `UPDATE company_members SET role = 'member'` on the owner row fails at commit.
  - Changing `companies.user_id` alone fails at commit.
  - Editing an invite that was accepted after the read is refused, and the accepted row is
    unchanged.

### 3 — Agents are retired

- **Init schema:** change the three FKs to `NO ACTION`.
- **`delete` in `persistence/agent.rs`:**
  - In one transaction, demote the principal, handle its grants the way `remove_member` handles a
    member's, then delete the agent.
  - Map FK errors by constraint name, so "has run history" and "bound to a channel" each say what
    they are.
- **Tests, `agent_retire_*`:**
  - Deleting an agent that wrote a message and has no usage rows keeps the message, its
    participants and its thread rows. The author principal becomes `external`.
  - Deleting an agent with usage rows returns the run-history message.
  - Deleting the company still removes everything. Use `own_database`, because this asserts
    whole-table state.

### 4 — The allowlist owns only its grants

- Change `channel.rs:244-246` to `ON CONFLICT ... DO NOTHING`.
- Test, `channel_allowlist_*`: a `manager` grant survives its address being added to the allowlist
  and then removed.

## Focused checks

Run each batch on a disposable database:
- Run `./scripts/reset-db.sh --all` after each init-schema edit.
- Run `cargo sqlx prepare -- --all-targets` if a macro-checked query in `user.rs`,
  `company_invite.rs` or `agent.rs` changed.

```sh
createdb mail_agents_fix_app_01
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_fix_app_01" \
  cargo test --locked --offline --lib account_email_change_
# repeat with company_owner_, company_invite_, agent_retire_, channel_allowlist_
dropdb mail_agents_fix_app_01
```

Confirm a nonzero test count for each filter. After batch 2, also rerun the team-page tests
(`cargo test --locked --offline --lib ui_team`).

## Final gate

Run `./scripts/reset-db.sh --all`, then `scripts/check.sh` once. Record the commands and results,
and name any gate that could not run.
