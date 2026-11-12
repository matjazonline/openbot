# p02 Desks, ownership and access — TASK

## Identity

- Plan / phase: `plan/01-departments/`, phase `p02` (document `02-desks-ownership-and-access.md`)
- Workflow: `implement-opus-reviewed-2_2` (skill dir `/Users/mac02/.claude/skills/implement-opus-reviewed-2_2`)
- Record root: `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/phases/p02/docs/`
- Root = Author (A): session 68933786-cbe7-4b6b-9387-b5dcb29689a4, Opus 5.5 / xhigh (user confirmed)
- Reviewer (B): agent `ad409b2d3d149c2c6`, Opus 5.5 / xhigh (same independent reviewer as p01; it
  authored nothing; p02 records are separate)
- Worktree: `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`
- Phase starting commit: `57adfa7a` (p01 accepted and committed); no pre-existing changes
- Prerequisite: p01 accepted in phase, digest `ccc4ee26be865cf9806a1e192f05bdddb8753eb4993704e2b8f98fcc78f03880`
  (commit 57adfa7a). Stable contracts used: p01 D1 demotion trigger, D3 `_on` functions and lock order.
- Snapshot exclusions: `plan/01-departments/phases/p02/docs` `plan/01-departments/COORDINATION.md`
- Coordination: `/Users/mac02/.codex/worktrees/2050/openbot/plan/01-departments/COORDINATION.md`
  revision 2 (pinned `logs/coordination-r2.md`, sha256 d33f363c…d02f). Applicable: I001–I003/r1,
  **01DEP-C001/r1** (relays stay trusted — acknowledged), **01DEP-C002/r1** (one step both ways —
  acknowledged).
- Authorizations: local commit per batch (B1 behaviour, B2 desk wording separately). No push.
- Resources: test DB `openbot_dept_p02_test` (phase-owned); worktree `target/`.

## Requirements (original sources)

- `plan/01-departments/README.md` decisions 2, 6, 7, 9; `02-desks-ownership-and-access.md`
  (02A, 02B, 02C, focused checks); as amended by C001 and C002.
- Instructions: `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md`, `src/adapters/http/AGENTS.md`,
  `src/adapters/http/pages/AGENTS.md` (02C).

## Acceptance criteria

- AC1 (02A) `channels.department_id` with tenant composite FK `ON DELETE RESTRICT`; `Channel`,
  `ChannelDb`, `CHANNEL_SELECT`, `ChannelWrite`, `ChannelUpdate`, `write_channel_settings_on` carry
  it; assignment/clearing is a channel settings edit through `ChannelUseCases::update_channel`,
  moves `settings_revision`, same-company only; channel delete unchanged; department delete with
  an owned desk refused (`Conflict`).
- AC2 (02B) `channels.members_only` + `channels_members_only_owned` +
  `channels_members_only_allowlist`; derived `department_membership` grants (participate + view)
  for members ∪ role holders ∪ parent lead ∪ direct child leads (C002), rewritten in the same
  transaction as every membership/role/re-parent/ownership change, moving `settings_revision` on
  each touched desk; configured allowlist entries survive; form round trip neither shows nor
  deletes derived grants; removing a member revokes their notifications on a members-only desk;
  derived grants are written for agent members too.
- AC3 (02B.5 per C001) relays into a members-only desk behave as relays do today (trusted); the
  loop/hop refusals still hold; documented.
- AC4 (02C) product word "desk" in UI copy, email copy and agent-facing text; identifiers keep
  `channel`; `pages/AGENTS.md` line; rendering tests and a regression grep test.
- AC5 focused checks `department_desk_ownership_`, `department_desk_members_only_`,
  `department_desk_relay_`, the desk-wording rendering tests, each with a nonzero count.

## Architecture gate — decisions (revision r1, 2026-11-10)

Gate needed: schema/FK change on `channels`, a transaction/lock protocol joining department
writes with channel gates, a derived-authorization rule (grants), and a change to a correctness
trigger written in p01.

### DEP-P02-D1 Schema (r1)

- `channels.department_id uuid NULL` and `channels.members_only boolean NOT NULL DEFAULT false`
  are added inline to the `channels` table (init schema ~line 898). `channels_members_only_owned
  CHECK (NOT members_only OR department_id IS NOT NULL)` and `channels_members_only_allowlist
  CHECK (NOT members_only OR access_mode = 'allowlist')` inline. The FK
  `channels_department_fk (company_id, department_id) → departments(company_id, id) ON DELETE
  RESTRICT` is added in the p01 "Departments" block (`ALTER TABLE channels ADD CONSTRAINT`),
  because `channels` precedes `departments` in the file. `channels_company_id_id_key` already
  exists. A test proves a company teardown with an owned desk still succeeds (reviewer's G1-F6
  probe says RESTRICT does not block it on PG 18.6) and that deleting the department alone is
  refused.
- Index `channels_department_idx ON channels (company_id, department_id) WHERE department_id IS
  NOT NULL` for the desk-set reads of D4.

### DEP-P02-D2 Channel write shape (r1)

- `Channel` gains `department_id: Option<DepartmentId>` and `members_only: bool`; `ChannelDb`,
  `CHANNEL_SELECT`, `TryFrom` carry them (fields appended, binds re-read).
- `ChannelWrite` gains **`ownership: Option<ChannelOwnershipWrite>`** where
  `ChannelOwnershipWrite { department_id: Option<DepartmentId>, members_only: bool }`. `None` means
  "leave ownership as it is" on update and "no department, not members-only" on create. Reason:
  `ChannelWrite` is replaced wholesale by every existing form save; the HTML forms only gain the
  department fields in phase 04, so plain fields would silently detach every desk on its next
  save. (74 of 82 `ChannelWrite` literals use `..Default`; the 8 that list fields get `ownership:
  None`.) The JSON channel payload gains `department_id` (present-null clears) and `members_only`,
  building `Some(ChannelOwnershipWrite)` when either is present, merged over the current values.
- Validation after merge, under the locks of D3: `members_only` without a department →
  `BadRequest`; `members_only` with `@public` in the participant list → `BadRequest`;
  `members_only` forces `access_mode = allowlist` even with an empty email list (02B.3); a
  department of another company or missing → `NotFound` (composite FK is the backstop).
- Channel delete is unchanged (02A.4).

### DEP-P02-D3 Lock protocol for desk ownership and derived grants (r1)

Follows p01 D3's plan-wide order: account → company → departments → principals (`FOR KEY SHARE`)
→ channel gates (one sorted `acquire_channel_gates_on`, `ChangeSettings`) → channel rows.

- **Department writes** (p01 `_on` functions) call `sync_department_grants_on` (D4) after their
  row change; it reads the affected desk set and derived principals (stable: every writer that
  can change them holds the company lock), locks those principals `FOR KEY SHARE` sorted and
  drops any no longer person/agent, then takes the desks' gates, then their rows `FOR UPDATE`
  (rows deleted meanwhile are skipped), then rewrites grants and moves `settings_revision`.
- **Channel edit with `ownership: Some(..)`** (`update_channel_on`): after the statement bounds,
  takes the company row `FOR NO KEY UPDATE` (no re-authorization — channel persistence has no
  actor; the use case authorizes, as today), reads the desk's current ownership (stable under the
  company lock), resolves and validates the merged ownership, locks the target department
  `FOR SHARE`, computes and locks its derived principals, **then** takes the gate and the row as
  today, writes settings, and rewrites this desk's derived grants. A channel edit with
  `ownership: None` keeps today's protocol exactly (gate → row), and never touches derived grants.
- **Channel create with `ownership: Some(..)`** (JSON only in p02): same company → department →
  principals prefix before inserting the row; a new desk has no gate contention.
- **Known residual**: a channel edit with `ownership: None` still inserts configured-allowlist
  grants after its row lock (channel → principal `KEY SHARE`), and the p02 demotion trigger (D5)
  deletes grants (principal → channel via `notification_recheck_channel_access`, which locks the
  channel row). They can deadlock only when the same person is both on that desk's email
  allowlist and a derived holder there while being removed from the company during the edit.
  PostgreSQL detects it and aborts one transaction (a retryable error; no partial state). Not
  redesigned here; recorded.

### DEP-P02-D4 Derived grants (r1)

- `sync_department_grants_on(tx, company, departments: &[DepartmentId])` recomputes every
  members-only desk owned by any listed department. Eligible principals of a desk owned by D:
  members of D, role holders of D, the `lead` of D's parent, the `lead` of each department whose
  parent is D (C002). The role is bound as `DepartmentRole::Lead.as_str()`, never a literal.
  Each gets `participate` and `view` with provenance `department_membership`
  (`GrantProvenance::DepartmentMembership.as_str()`), `ON CONFLICT DO NOTHING` so a grant another
  provenance holds stays that provenance's; the desk's previous `department_membership` grants
  are deleted first. `settings_revision` moves on every desk in the set.
- Affected department sets, by caller:
  - add/remove member of D → {D};
  - assign/clear a role of D (lead) → {D, parent(D), children(D)};
  - create D (with or without lead) → {parent(D)} (D has no desks yet; its lead is a child lead of
    the parent's desks);
  - update D with a parent change old → new → {D, old, new}; without a parent change → nothing;
  - delete D (no desks, no children by then) → {parent(D)};
  - channel edit/create changing ownership → that one desk (`sync_desk_grants_on`, the same
    statements restricted to one channel);
  - demotion → D5.
- Unit-level decision helper: none — the rule is SQL over rows; covered by DB tests.

### DEP-P02-D5 Demotion trigger extension (r1)

- p01's `department_principal_demoted` additionally deletes the demoted principal's
  `department_membership` grants in its company and moves `settings_revision` on the desks they
  were on. Without this, a removed person whose email identity still resolves to the (now
  external) principal would keep members-only access (allowlist `participant_access` ignores
  kind). Configured-allowlist grants on the demoted principal are left alone, as today.

### DEP-P02-D6 Relays and hierarchy (r1, from C001/C002)

- Relays: no change to `authorize_relay`/`authorize_channel`. A test proves a relay into a
  members-only desk is admitted from a member agent's desk and from a non-member desk, and a
  desk reappearing in the trace is still refused (`LoopCycle`). `docs/inter_channel_agent_communication.md`
  gets one paragraph: members-only does not gate desk-to-desk relays. (03's dispatch section
  carries the same statement.)
- Hierarchy: D4 implements C002.

### DEP-P02-D7 Batches and the word "desk" (r1)

- **B1** = 02A + 02B + D5 + JSON payload fields + the relay test and doc paragraph. One commit.
- **B2** = 02C only (UI copy in `src/adapters/http/pages/`, `format_help_email_body` and the bounce,
  tool descriptions / directory tool text, `pages/AGENTS.md` line, rendering tests, regression grep
  test). Separate review and commit (plan 05: "in a separate commit so the rename does not hide
  behavior changes in review"). Identifiers, URLs, JSON fields, headers, CSS classes, ids, `hx-*`
  targets and hrefs keep `channel`.

## Verification plan

| Criterion | Production path and case | Decisive assertion | Check |
| --- | --- | --- | --- |
| AC1 cross-company department | `ChannelUseCases::update_channel` with another company's department | `NotFound`; row unchanged | `department_desk_ownership_*` |
| AC1 delete dept with desk / after clearing | `DepartmentUseCases::delete_department` | `Conflict`, then success after `update_channel` clears | `department_desk_ownership_*` |
| AC1 revision + round trip | assign/clear via update_channel; form save without ownership | `settings_revision` moves; `ownership: None` keeps department and members_only | `department_desk_ownership_*` |
| D1 teardown | company delete with department + owned desk | succeeds | `department_desk_ownership_*` |
| AC2 member/non-member/owner | grants loaded on `Channel`; `participant_access` / `viewer_access` and `list_readable_channels` | member yes, teammate no, owner yes | `department_desk_members_only_*` |
| AC2 removal | `remove_member` use case | both capabilities gone, revision moved, in one commit | `department_desk_members_only_*` |
| AC2 hierarchy (C002) | parent lead, grandparent lead, child lead, grandchild lead | parent+child leads yes; grand* no | `department_desk_members_only_*` |
| AC2 re-parent / lead change | grants follow the new parent / new lead | exact grant sets | `department_desk_members_only_*` |
| AC2 configured allowlist survives | email allowlist + membership change | email stays in `participant_emails`, grant kept | `department_desk_members_only_*` |
| AC2 form round trip | `a_channel_form_round_trips_its_email_allowlist_through_principal_grants` extended | derived grants not in emails, not deleted by a save | existing test updated |
| AC2 notifications | active notification for member on members-only desk; remove member | notification `withdrawn` | `department_desk_members_only_*` |
| AC2 validation | members_only without dept; with @public; empty list | BadRequest; BadRequest; allowlist mode | `department_desk_members_only_*` |
| D5 demotion | `remove_member` of a derived holder | derived grants gone, revision moved; configured grant stays | `department_desk_members_only_*` |
| D3 competing | membership add racing a desk ownership edit; demotion racing a sync | grants consistent, no lost update | `department_desk_competing_*` |
| AC3 relays (C001) | thread ingest relay harness into a members-only allowlist desk | admitted from member and non-member desks; trace reappearance `LoopCycle` | `department_desk_relay_*` |
| AC4 wording | rendering tests; grep test | "desk" in nav/headings/flash; no `>Channel`/`Channel<` text nodes | B2 tests |

## Batches

- B1 focused checks: `department_desk_`, `department_` (p01 regressions, now syncing grants),
  `channel::tests::`, `channel_edit`, `channel_allowlist`, `use_cases::channel::`, notification
  recheck tests, relay tests, demotion callers (`remove_member`, `agent_retire`, `user_lifecycle`,
  `account_deletion`), `source_modules`, router mount, clippy, fmt, sqlx prepare on a fresh DB.
- B2 focused checks: pages rendering tests, email copy unit tests, the grep test, clippy, fmt.

## Status

- 2026-11-10: p02 started from 57adfa7a; coordination rev 2 (C001, C002) acknowledged; gate r1
  written, awaiting B.
