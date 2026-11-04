# 01 — Access level type and member levels

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Every person the app reasons about has exactly one access level for a company, and that level is
resolved in the same place as their membership. This covers decisions D1, D2, D5 and P1 in the
[README](README.md).

## Batch 01A — One type, one SQL domain

- Add `src/domain/entities/access_level.rs` with an
  `AccessLevel` enum (`L0` … `L9`, `#[repr(u8)]`), deriving `Ord` so that "may see" is `<=`.
  Provide:
  - constants `PUBLIC` (`L0`) and `MAX` (`L9`), plus `ALL`;
  - `try_from(i16)` and `as_i16()` for SQL, and `label()` returning text such as "Level 5";
  - serde as an integer, rejecting out-of-range values.

  It is an enum rather than a `u8` newtype so that every `match` is exhaustive and a value of 10
  cannot be constructed (D2).
- Add one pure domain helper, `AccessLevel::lowest(iter) -> AccessLevel`. An empty audience yields
  `PUBLIC` (invariant 3).
- In `migrations/20260817000000_init_schema.sql`, which is edited in place, add
  `CREATE DOMAIN public.access_level AS smallint CHECK (VALUE BETWEEN 0 AND 9)`. Every level
  column in this plan uses the domain, so the range is defined once in SQL.

## Batch 01B — Member and invite levels

- Add `company_members.access_level public.access_level NOT NULL DEFAULT 0` with
  `CHECK (role <> 'owner' OR access_level = 9)`. Owner creation writes 9 (P1). The table is at
  `migrations/20260817000000_init_schema.sql:839`.
- Add `company_invites.access_level public.access_level NOT NULL DEFAULT 0` (`:3526`). Accepting an
  invite copies the level to the member row in the same transaction that creates the member.
- Outside contacts have no stored level. They resolve to `PUBLIC` (D1).

## Batch 01C — Reader-level resolution

- Add `access_level: AccessLevel` to `PrincipalAccessContext`
  (`src/domain/entities/participant.rs:264`). That struct already documents itself as "all actor
  facts needed by channel policy", and the level is one more such fact.
  `PrincipalAccessContext::external` (`:279`) sets `PUBLIC`.
- Fill the level in the same queries that resolve membership today:
  `access_context_for_identity` and `access_context_for_user`
  (`src/adapters/persistence/participant.rs:492-577`).
  - `CompanyMembership::None` → `PUBLIC`.
  - Owner → `MAX`.
  - Admin or Member → the stored level.
  - A disabled identity, or a principal whose account is no longer a member → `PUBLIC`.
- Do not add a separate level lookup. Membership and level must come from one read so they cannot
  disagree.
- Email sender authenticity is already enforced at ingress: a remote message without a DMARC pass
  is rejected by `guard_ingress` (`src/application/use_cases/thread/ingest/policy.rs:153`). A forged
  `From:` therefore never resolves to a member's level. Keep a regression test that pins this.
- Agent and System principals are not readers and never count towards an audience (used in 03).

## Batch 01D — Managing levels

- Add one pure rule, `may_assign_level(actor, target_current, requested) -> Result<(), LevelRefusal>`:
  - the owner may assign any level to anyone except themselves (fixed at 9);
  - an admin may act only when `target_current <= actor.level` and `requested <= actor.level`;
  - a member may not assign levels.

  The member-management route and the invite route both call it. An invite level is capped at the
  inviter's level by the same rule.
- Member list and invite form: a level select showing only the levels the actor may assign.
- Lowering someone's level takes effect at their next read. Mail already sent cannot be recalled;
  say so in the UI help text.

## Focused checks

Prefix `access_level_`:

- range parsing and serde rejection of 10 and -1;
- ordering, and `lowest` of an empty input;
- the database rejects 10 and rejects an owner below 9;
- an invite copies its level on accept;
- the assignment rule's full matrix;
- an external principal, a disabled identity and a former member each resolve to 0;
- a DMARC-failing message from a member's address is rejected (regression).

Run with `cargo test --locked --offline --lib access_level_` against a disposable database, plus
the existing `participant` and `channel` access tests.
