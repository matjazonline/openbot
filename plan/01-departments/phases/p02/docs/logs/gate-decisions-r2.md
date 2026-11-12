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

## Architecture gate — decisions (revision r2, 2026-11-10)

Gate needed: schema/FK change on `channels`, a transaction/lock protocol joining department
writes with channel gates, a derived-authorization rule (grants), a change to the grant table's
key, and a change to a correctness trigger written in p01. r1 is preserved in
`logs/gate-decisions-r1.md`; r2 answers `01DEP-P02-G1-F1..F8`.

### DEP-P02-D1 Schema (r2)

- `channels.department_id uuid NULL` and `channels.members_only boolean NOT NULL DEFAULT false`
  are added inline to the `channels` table (init schema ~line 898). `channels_members_only_owned
  CHECK (NOT members_only OR department_id IS NOT NULL)` and `channels_members_only_allowlist
  CHECK (NOT members_only OR access_mode = 'allowlist')` inline. The FK
  `channels_department_fk (company_id, department_id) → departments(company_id, id) ON DELETE
  RESTRICT` is added in the p01 "Departments" block (`ALTER TABLE channels ADD CONSTRAINT`),
  because `channels` precedes `departments` in the file. `channels_company_id_id_key` already
  exists. A test proves a company teardown with an owned desk still succeeds and that deleting the
  department alone is refused.
- Index `channels_department_idx ON channels (company_id, department_id) WHERE department_id IS
  NOT NULL` for the desk-set reads of D4.
- **r2 (F1): the grant key includes provenance.** `channel_principal_grants_pkey` becomes
  `PRIMARY KEY (company_id, channel_id, principal_id, capability, provenance)`, so a principal may
  hold the same capability on a desk from two provenances, each owned by its writer. A form save
  still deletes only `configured_allowlist` rows; D4 deletes only `department_membership` rows; so
  neither can remove access the other gave, and a member who is also allowlisted keeps both.
  Every writer's conflict target gains `provenance`: `insert_email_allowlist_grants`
  (`channel.rs`), the identity merge (`account_identity.rs` ~L235) and D4. Readers are unaffected
  in meaning: `Channel::has_capability`, `notification_principal_can_view`, `association.rs`
  and `decisions.rs` all test existence of a row for (principal, capability); `association.rs`
  uses `fetch_optional` + `FOR SHARE OF access_grant`, which tolerates two rows.

### DEP-P02-D2 Channel write shape and the members-only rule (r2)

- `Channel` gains `department_id: Option<DepartmentId>` and `members_only: bool`; `ChannelDb`,
  `CHANNEL_SELECT`, `TryFrom` carry them (fields appended, binds re-read).
- `ChannelWrite` gains **`ownership: Option<ChannelOwnershipWrite>`** where
  `ChannelOwnershipWrite { department_id: Option<DepartmentId>, members_only: bool }`. `None` means
  "leave ownership as it is" on update and "no department, not members-only" on create. Reason:
  `ChannelWrite` is replaced wholesale by every existing form save; the HTML forms only gain the
  department fields in phase 04. (74 of 82 `ChannelWrite` literals use `..Default`; the 8 that list
  fields get `ownership: None`.) **The JSON channel payload is an explicit exception to the PUT's
  replace semantics (F7):** `department_id` (present-null clears) and `members_only` are
  keep-when-absent; either present builds `Some(ChannelOwnershipWrite)` merged over the current
  values. Documented on the payload type and in the route test.
- **r2 (F2): every settings write applies the members-only rule** against the ownership the row
  will have after the write (the current row's when `ownership` is `None`, read under the row lock
  `write_channel_settings_on` already runs under): if members-only, the stored `access_mode` is
  `allowlist` whatever the email list says (an empty list included); `@public` on a members-only
  desk is `BadRequest` ("A members-only desk cannot be open to @public"); members-only without a
  department is `BadRequest`; a department of another company or missing is `NotFound` (composite
  FK backstop). When the desk is not members-only, `access_mode` is derived from the email list as
  today (`team` / `allowlist` / `public`).
- Channel delete is unchanged (02A.4).

### DEP-P02-D3 Lock protocol (r2)

Plan-wide order (p01 D3): account → company → departments → principals (`FOR KEY SHARE`) → channel
gates (one sorted `acquire_channel_gates_on`, `ChangeSettings`) → channel rows.

- **Department writes** (p01 `_on` functions) call `sync_department_grants_on` (D4) after their
  row change: read the affected desk set and eligible principals (stable: every writer that can
  change them holds the company lock), lock those principals `FOR KEY SHARE` sorted and drop any no
  longer person/agent, take the desks' gates, then the rows `FOR UPDATE` (rows deleted meanwhile
  are skipped), then rewrite grants and move `settings_revision`.
- **Channel edit with `ownership: Some(..)`** (`update_channel_on`): after the statement bounds,
  takes the company row `FOR NO KEY UPDATE`, reads the desk's current ownership (stable under the
  company lock), resolves and validates the merged ownership, locks the target department
  `FOR SHARE`, computes and locks its eligible principals, **then** takes the gate and the row as
  today, writes settings (D2 rule) and re-derives this desk's grants (D4). No re-authorization
  under the company lock: channel persistence carries no actor and the existing channel edit does
  not re-authorize either (F7 considered; out of p02's scope).
- **Channel edit with `ownership: None`**: today's protocol (gate → row) plus the D2 members-only
  rule read under the row lock; derived grants are not touched (ownership cannot change).
- **Channel create with `ownership: Some(..)`** (JSON in p02; p03 department desks): company →
  department → principals before inserting the row. **p03 contract (F7):** a desk's ownership is
  set inside another transaction only through `set_desk_ownership_on(tx, company, channel,
  ChannelOwnershipWrite)` (adapter `pub(crate)`), which runs the same prefix (company lock already
  held by the caller), updates the row, applies D2 and re-derives the desk's grants; p03's
  `create_department_on` calls it for the department desk, never writing `department_id` by hand.
- **Budgets (F6):** department writes keep `begin_bounded` (1 s `lock_timeout`, 5 s
  `statement_timeout`); waiting on a desk's gate held by an inbound admission is normally
  milliseconds. A wait past the bound is `AppError::DatabaseTimeout`, which the HTTP layer renders
  as its existing "try again" response with nothing changed. Channel edits keep `EditBudget`.
- **Residual deadlocks (r2, F4), complete for the new writers:** PostgreSQL detects each and
  aborts one transaction (`40P01`, surfaced as a database error; no partial state):
  1. D5 demotion (principal `FOR UPDATE` → grant delete → `notification_recheck_channel_access`
     locks the desk row `FOR UPDATE`) against an inbound commit on that desk that references the
     same principal (desk `KEY SHARE` via `threads`/`message_deliveries` FKs → principal
     `KEY SHARE`): requires the person being removed to be writing to a desk where they hold a
     derived grant at that moment. The removal fails and is retried by the admin; the inbound
     commit fails and its transport/queue retries it.
  2. D5 demotion against a channel edit with `ownership: None` that writes a configured-allowlist
     grant for the same principal (desk row → principal `KEY SHARE`): requires the person being
     removed to be on that desk's email allowlist while the desk is being edited.
  3. Department sync and ownership-changing edits take principals before gates and rows, so they
     cannot form a cycle with D5, with each other (company lock), or with inbound commits (which
     take no department locks and only `KEY SHARE` on principals).
  Not restructured: the cycles need a concurrent demotion of the very principal involved; both
  sides are retryable.

### DEP-P02-D4 Derived grants (r2)

- `sync_department_grants_on(tx, company, departments: &[DepartmentId])` re-derives every
  members-only desk owned by any listed department; `sync_desk_grants_on(tx, company, channel)`
  re-derives one desk. Both: **delete every `department_membership` grant of the desk(s)
  unconditionally**, then, only for a desk that is members-only, insert `participate` and `view`
  for each eligible principal with provenance `GrantProvenance::DepartmentMembership.as_str()`,
  `ON CONFLICT (company_id, channel_id, principal_id, capability, provenance) DO NOTHING`, then
  move `settings_revision`. Eligible principals of a desk owned by D: members of D, role holders of
  D, the lead of D's parent, the lead of each department whose parent is D (C002); kind person or
  agent (checked under the `FOR KEY SHARE` lock). The role is bound as
  `DepartmentRole::Lead.as_str()`.
- **Invariant (r2, F3):** only members-only desks hold `department_membership` grants. Every
  ownership change of a desk (members-only turned on or off, department set, changed or cleared)
  runs `sync_desk_grants_on`, whose unconditional delete removes the old department's grants; the
  new `access_mode` follows D2 (members-only → allowlist; otherwise from the email list).
- Affected department sets, by caller: add/remove member of D → {D}; assign/clear a role of D →
  {D, parent(D), children(D)}; create D → {parent(D)}; update D with a parent change old → new →
  {D, old, new} (no parent change → nothing); delete D → {parent(D)}; desk ownership change → that
  desk; demotion → D5.

### DEP-P02-D5 Demotion trigger extension (r2)

- p01's `department_principal_demoted` additionally deletes the demoted principal's
  `department_membership` grants in its company and moves `settings_revision` on the desks they
  were on. Without this, a removed person whose email identity still resolves to the (now
  external) principal would keep members-only access, and re-inviting them (invite acceptance
  promotes the external principal back to person) would revive it. Configured-allowlist grants on
  the demoted principal are left alone, as today. The revision bump does not take the channel gate
  (the trigger holds the principal; taking a gate there would add a cycle with channel edits): an
  admission routed under the old grant that already holds the gate shared may still commit, the
  same outcome as a message arriving just before the removal. Residual deadlocks: D3 items 1–2.

### DEP-P02-D6 Relays, thread parties and hierarchy (r2)

- Relays (C001): no change to `authorize_relay`/`authorize_channel`. A test proves a relay into a
  members-only desk is admitted from a member agent's desk and from a non-member desk, and a desk
  reappearing in the trace is still refused (`LoopCycle`). `docs/inter_channel_agent_communication.md`
  gets one paragraph: members-only does not gate desk-to-desk relays.
- **Thread parties (r2, F8):** a members-only desk is an allowlist desk, so the existing
  thread-party rule (`routing.rs` `thread_access_for`) still admits a non-listed sender into a
  thread they are already a party to. Removing a member therefore stops new conversations and
  reads, but not replies within threads they were on. This is today's allowlist behaviour and the
  plan does not ask to change it; recorded, with a test asserting it so the behaviour is explicit.
- Hierarchy: D4 implements C002.

### DEP-P02-D7 Batches and the word "desk" (r2, unchanged)

- **B1** = 02A + 02B + D5 + JSON payload fields + relay/thread-party tests and doc paragraph.
- **B2** = 02C only, separate review and commit; identifiers, URLs, JSON fields, headers, CSS
  classes, ids, `hx-*` targets and hrefs keep `channel`.

## Contract impact (r2, F5)

| Changed contract/value | Producers / consumers / equivalents | Disposition | Checks |
| --- | --- | --- | --- |
| `Channel` gains `department_id`, `members_only` (serialized in JSON responses) | `ChannelDb`/`CHANNEL_SELECT`/`TryFrom`; 50 struct literals in 10 files (`pages/tests.rs`, `routes/channel.rs`, `routes/onboarding.rs`, `persistence/channel.rs`, `smtp/server.rs`, `use_cases/channel.rs`, `thread/ingest/commit.rs`, `thread/tests.rs`, `workflow/tests/authorization_cases`, `domain/channel.rs`); JSON channel responses | compiler-driven; responses additive | build; channel tests; route test asserts fields |
| `ChannelWrite.ownership` | 82 literals (8 explicit → `ownership: None`); HTML forms (None); JSON payload (keep-when-absent); `insert_channel_row_on` callers (create, library copy, provisioning) | `None` keeps / creates unowned | channel tests; form round-trip test |
| `ChannelPersistence` impls | Postgres, `smtp/server.rs`, `services/test_support.rs`, `use_cases/channel.rs` mock | signatures unchanged (write struct only) | build |
| grant key + conflict targets | `insert_email_allowlist_grants`, `account_identity.rs` merge, D4 sync; readers `CHANNEL_SELECT`, `has_capability`, `notification_principal_can_view`, `association.rs`, `decisions.rs` | conflict targets gain provenance; readers existence-based | allowlist tests, identity merge tests, D4 tests |
| `access_mode` writer | `write_channel_settings_on` (only writer), `insert_channel_row_on` (create) | D2 members-only rule | `department_desk_members_only_*` |
| notification trigger on grant writes | fires per derived grant insert/delete (locks the desk row `FOR UPDATE`) | consistent with D3 order (rows after gates) | notification test |
| p01 `_on` functions | gain D4 sync calls; `delete_department_on` also refuses owned desks (`Conflict`), FK RESTRICT maps to `Conflict` (existing mapping) | extended | `department_` regression run + ownership tests |
| p01 `department_principal_demoted` | demotion paths (`remove_member`, agent delete, account deletion) | also deletes derived grants + bumps revision (D5) | demotion caller filters + new test |
| `begin_bounded` budget | department writes now also wait on desk gates | unchanged; recorded (D3 F6) | competing tests |

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
| AC2 configured allowlist survives (F1 both orders) | allowlisted then joins and the email is removed by a form save (still a member → access kept); member then allowlisted then leaves (email shown, access kept) | grant rows per provenance; `participant_emails` shows the email | `department_desk_members_only_*` |
| D2 form save on a members-only desk (F2) | HTML-equivalent `ChannelWrite` with `ownership: None`, empty list; with `@public` | stays `allowlist`; `BadRequest` | `department_desk_members_only_*` |
| D4 invariant (F3) | members-only turned off; department cleared; desk moved D1 → D2 | derived grants removed / replaced by D2's; mode from emails | `department_desk_ownership_*` |
| D6 thread parties (F8) | removed member replying in a thread they are a party to | admitted (allowlist thread rule), new thread refused | `department_desk_members_only_*` |
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
- 2026-11-10: B2 (02C wording) delegated to an author helper subagent (agent a77724bc09910bbe6,
  Opus 5.5 / xhigh, co-author under root; not the reviewer), DB openbot_dept_p02c_test, running in
  parallel with the B1 gate review. Order: B2 implement → B2 review → B2 commit → B1 implement.
- 2026-11-10: G1-R1 changes required (F1 High, F2/F3 Medium, F4/F5 Low; F6–F8 optional). r2 written
  answering all (grant key gains provenance; members-only rule on every settings write;
  unconditional derived delete; complete residual set; contract impact table; budgets; p03
  `set_desk_ownership_on`; thread parties). Awaiting B recheck.
