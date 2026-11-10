# 02 Access modes

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A desk's audience is its owning department, one step up, plus explicit grants; `team`,
`allowlist` and `members_only` are gone. Continues in
[03](03-integration-and-final-verification.md).

## Batch 02A Schema and policy

1. `channels.department_id NOT NULL`; creation defaults it to the root. `channels_access_mode_check`
   becomes `members`, `public`. Drop `members_only` and its checks from 01-departments.
2. `ChannelAccessMode` (`channel.rs:85`) becomes `Members | Public`. `participant_access` (`:135`)
   and `viewer_access` (`:174`) keep their shape: `Public` authorizes anyone to write and trusts
   listed principals and members; `Members` authorizes listed principals and the owner. "Listed"
   still means a grant row. The `is_team()` arms disappear, because root membership is now a
   materialized `department_membership` grant on root desks like any other department.
3. Grant materialization (`sync_department_grants_on`, 01-departments batch 02B) runs for every
   desk, root desks included. Root membership changes rewrite grants on root desks; the write
   batches by channel and bumps `settings_revision` once per channel. Measure the cost on a company
   with 1,000 members and 200 root desks before accepting; if it is unacceptable, the fallback is a
   `department_membership` grant that names the department instead of each principal, resolved in
   the two policy functions, and that decision is recorded here.
4. `channel_access` (`src/adapters/persistence/channel.rs:193`) derives `Public` from `@public`
   in the email list and `Members` otherwise; named addresses become `configured_allowlist`
   grants on either mode.

## Batch 02B UI and docs

1. Desk settings show the owning department (required) and an "Open to anyone" checkbox for
   `public`. The participant email list stays as "Also allow these addresses".
2. `docs/departments.md` and `docs/transport_architecture.md` describe the single rule.

## Focused checks

- `access_mode_`: every case in `channel.rs` tests
  (`a_team_channel_is_readable_by_the_team_and_nobody_else` and siblings, `:312` onward) is
  rewritten for the two modes with the same expectations; a root desk behaves as `team` did; a
  sub-department desk behaves as `members_only` did.
- `access_mode_grants_`: adding a root member grants them every root desk in one commit; removing
  revokes; the measurement from 02A item 3 is recorded with its numbers.
