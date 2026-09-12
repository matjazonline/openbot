# Phase 1 — The audit trail survives a demotion; the trigger becomes a safety net

Read [`general_plan_instructions.md`](general_plan_instructions.md) first.

**Goal.** A demotion can no longer be blocked (or lose a row) by the append-only delegation audit
trail, and the release trigger is documented as the fallback it now is. No Rust behaviour changes.

**Files touched**

| File | Change |
|---|---|
| `migrations/20260817000000_init_schema.sql` | actor-kind check widened, actor FK gains `ON UPDATE CASCADE`, trigger comment |
| `src/adapters/persistence/company_invite.rs` | test asserting the audit row survives a removal |

---

## 1.1 `delegation_control_commands_actor_kind_check`

`delegation_control_commands` is a pure append-only record of who issued a past delegation command.
There is no "reassign who issued a historical command" concept, and directive #1 forbids losing the
row, so the fix is to let the row follow the principal's `kind` instead of blocking on it.

`ON UPDATE CASCADE` alone would write `actor_kind = 'external'` into a column whose check allows
only `person` and `agent`, turning an FK violation into a check violation. So the check is widened
first:

```sql
CONSTRAINT delegation_control_commands_actor_kind_check
    CHECK ((actor_kind = ANY (ARRAY['person'::text, 'agent'::text, 'external'::text]))),
```

`'external'` is only ever *arrived at* by the cascade — nothing inserts it, because `authorize`
(`controls.rs:91-141`) only ever returns `person` or `agent` as the actor kind. No production code
parses this column; the only readers are two assertions in
`src/adapters/persistence/task/tests.rs` (6292, 6436) that check what was written at insert time.

## 1.2 `delegation_control_commands_actor_fk`

```sql
ALTER TABLE ONLY public.delegation_control_commands
    ADD CONSTRAINT delegation_control_commands_actor_fk
    FOREIGN KEY (company_id, actor_principal_id, actor_kind)
    REFERENCES public.principals(company_id, id, kind) ON UPDATE CASCADE ON DELETE RESTRICT;
```

`ON DELETE RESTRICT` stays: the audit trail is precisely the reason a principal row is never
deleted. `actor_principal_id` — the identity the trail is read by — is untouched by the cascade;
only the denormalised `actor_kind` follows the principal.

## 1.3 The trigger's role, in the migration

Above `CREATE TRIGGER principals_release_owned_tasks_on_demotion` (init migration ~5920), a comment
saying the guided-removal flow is what resolves owned work now, and that this trigger is the
fallback for a path that reaches the demotion without it — so a future caller cannot turn a
forgotten pre-check into an FK error mid-transaction. The sibling `BEFORE DELETE` trigger keeps its
meaning unchanged.

## 1.4 Databases and metadata

```sh
./scripts/reset-db.sh --all
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx prepare -- --all-targets
```

## 1.5 Test

`src/adapters/persistence/company_invite.rs`, alongside
`removing_a_member_releases_the_live_work_they_still_own`: seed a
`delegation_control_commands` row whose actor is the member's own person principal (a real outreach
+ task + command row, written with raw SQL so the test does not depend on delegation authorisation),
remove the member, and assert the row is still there with `id`, `actor_principal_id`, `authority`,
`operation`, `reason`, `from_version`, `to_version`, `result` and `occurred_at` **unchanged**, and
`actor_kind` now `'external'` — the one field that is denormalised from the principal.

Scope every lookup to the ids the test created.
