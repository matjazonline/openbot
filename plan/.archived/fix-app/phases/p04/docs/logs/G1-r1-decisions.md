# TASK — fix-app p04 (Authorization in the application layer)

## Identity

- Phase ID: `p04`; workflow: `implement-opus-reviewed-2_2`
  (skill dir `/Users/mac03/dev/openbot/.claude/skills/implement-opus-reviewed-2_2`).
- Record root: `/Users/mac03/dev/openbot/.worktrees/fix-app-p04/plan/fix-app/phases/p04/docs/`.
- Root = Author (A): phase root agent spawned by the manager, Opus 5.5 at `high` effort (stated
  by the manager at spawn). Reviewer (B): Opus 5.5 / `xhigh`, general-purpose subagent, agent ID
  recorded in HANDOFF once spawned.
- Worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p04`, branch `fix-app/p04`, starting commit
  `48626a1` (main with p01, p02, p05 merged). Preserved pre-existing changes: none (clean tree;
  `COORDINATION.md` lives only in the primary checkout and is not committed).
- Resources (fix-app-I002): private Postgres 18 cluster `127.0.0.1:55404` (trust auth, owned by the
  manager), dev DB `mail_agents`, tests derive `mail_agents_test`. `CARGO_TARGET_DIR` = worktree
  `target/`; every cargo command prefixed `CARGO_INCREMENTAL=0`.
- Context: `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75` is set in
  `/Users/mac03/dev/openbot/.claude/settings.local.json` and present in this session's environment.
- Snapshot exclusion (fixed for A, B and root): `plan/fix-app/phases/p04/docs`.

## Coordination

- Authority: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md`, writer = manager root session.
  Last checked revision: 4 (copy `logs/COORDINATION-r4.md`).
- fix-app-I001 r1 (repo and plan rules; init_schema edits only in place) — acknowledged. p04 needs
  no schema change (see D1); no init_schema statement is edited.
- fix-app-I002 r1 (isolation and resources) — acknowledged; resources above.
- fix-app-I003 r1 (commit the accepted phase on `fix-app/p04`; no push; never commit
  COORDINATION.md) — acknowledged.
- fix-app-I004 r1 (targeted tests only, nonzero counts per filter; no full suite, no
  `scripts/check.sh`; `cargo fmt --check` and compiling affected targets allowed; regenerate
  `.sqlx/` if macro queries change) — acknowledged; supersedes the plan's "Final gate".
- fix-app-C002 r1 (ownership is one fact; fixtures use `insert_company_with_owner`,
  `transfer_company_ownership`, `revoke_membership(_now)`; injected statements `expect` success) —
  acknowledged. New DB fixtures in p04 create companies through `CompanyPersistence::create` (which
  writes the owner row) and add admins/members as `company_members` rows only; no ownership edits.
- fix-app-C003 r1 (p04 precondition met; `is_company_owner_address`, scoped `update_invite`) —
  acknowledged. D1 relies on it: the owner `company_members` row *is* ownership, so the one-row
  query reads membership from `company_members.role` alone.
- Pending cross-phase requests: see "Cross-phase notices requested" (filled as work proceeds).

## Original requirements

- `plan/fix-app/README.md`, `plan/fix-app/04-authorization-in-application.md` (spans as of
  `4e92edd`; re-checked against `48626a1`, differences noted per decision).
- `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`, `src/adapters/http/AGENTS.md`,
  `src/adapters/http/pages/AGENTS.md` (Rust only; no styling), `src/adapters/smtp/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md`.

### Acceptance criteria (from the plan's Outcome and batches)

1. Every authorization decision is made once, in an application use case, by a method that takes
   the caller. HTTP handlers and the SMTP adapter only translate.
2. No adapter reads a tenant row by bare id (`CompanyUseCases::get_company`, `ThreadUseCases::
   get_thread` and the `ThreadUseCases` persistence accessors are gone from adapters).
3. Responses: unknown or someone else's → 404 revealing nothing; visible but not allowed → 403;
   database error → an error (5xx), never a decision.
4. `company_access` is a required one-row query; the other defaulted correctness methods
   (`update_for_user`, `delete_for_user`, `has_login_method`, `activate_password`,
   `latest_replyable_email_context`) are required; the six `MockCompanyPersistence` copies are one
   shared double whose behaviour is stated, with Admin and Member paths reachable.
5. One thread-in-channel read replaces the three copies; another channel's or tenant's thread gets
   the same 404 as an unknown id.
6. `ui_team` role from membership, non-owner write → 403; operator is a caller property checked by
   the library-agent use cases; attention gating in a use case.
7. Onboarding is one use case over a purpose-specific port: channel error leaves no agents; a
   provisioning refusal keeps the agent and shows the refusal; a retry with the same selection is
   idempotent.
8. SMTP asks the application for sender trust; the accessors at `thread/mod.rs:397-411` are
   deleted; a lookup failure is logged and treated as a stranger.

## Batches

The plan's six batches are reviewed as two workflow batches, preceded by one architecture gate
**G1** covering every costly decision of both (authorization model, response policy, caller
property, transaction boundary of onboarding, SMTP port):

- **WB1** = plan batches 1–4 (persistence contract, scoped company reads, thread-in-channel read,
  owner/operator/attention). One review.
- **WB2** = plan batches 5–6 (onboarding transaction, SMTP sender trust). One review.

Split because WB1 touches ~25 HTTP/use-case files and WB2 adds a new transactional port; one
review of both could not stay thorough. WB2 builds on WB1's accepted contracts (`owned_company`
policy, shared mocks).

## G1 — Architecture decisions (revision r1)

### D1 — `company_access` is the one membership read (plan batch 1)

- `CompanyPersistence::company_access(user_id, company_id) -> AppResult<Option<CompanyAccess>>`
  becomes a required method. Postgres: one row,
  `SELECT company.*, member.role FROM companies company JOIN company_members member
   ON member.company_id = company.id AND member.user_id = $1 WHERE company.id = $2`.
  Role `owner` → `Owner`, `admin` → `Admin`, `member` → `Member`; no row → `None`. Ownership comes
  from the owner member row alone (01/C003 makes it agree with `companies.user_id` at commit), so
  no `companies.user_id = $1` disjunct. An unknown role string is `AppError::Internal` (the CHECK
  constraint makes it unreachable), never a downgrade.
- `list_accessible_by_user_id` also loses its default (owned-only "conservative half"); it is an
  authorization read the plan's rule covers. Its Postgres listing keeps its 200 cap (pagination is
  out of scope) but derives membership from the member row the same way. `company_access` never
  goes through the list again, so beyond 200 companies nothing is silently denied.
- `update_for_user`, `delete_for_user` (CompanyPersistence), `has_login_method`,
  `activate_password` (UserPersistence), `latest_replyable_email_context` (ThreadPersistence) lose
  their defaults. Postgres already implements all five; only test doubles change, each stating its
  behaviour explicitly (no `Ok(method == Password)`-style silent answer).
- Test doubles: the six `MockCompanyPersistence` copies (smtp/server.rs, agent_tests.rs,
  channel.rs, company.rs, company_invite_tests.rs, thread/tests.rs) become one
  `use_cases::company_test_support::MockCompanyPersistence` (`#[cfg(test)]`): companies, explicit
  `(user, company, membership)` rows (owner rows included — no "owner implied" shortcut), stated
  model connections/key, a `failing()` mode where every read returns `AppError::Database` (for the
  500-not-404 route tests), and one `TeamFixture` impl whose by-address policy is an explicit enum
  (`EveryAddressIsMember`, `Listed`, the thread tests' existing address heuristic). Copies that
  were `unimplemented!()` on a method keep a loud failure for unconfigured calls rather than a
  silent default.
- Failure modes: DB error propagates (`?`) everywhere `company_access` is used; no `.ok()`.

### D2 — Response policy and scoped company reads (plan batch 2)

- New `AppError::Forbidden(String)` → HTTP 403 with the message. (Shared contract: every
  exhaustive `match AppError` gains an arm; inbound worker classifies it like `BadRequest`,
  i.e. permanent.)
- `CompanyUseCases` scoped reads, all built on `company_access` (so ownership is membership
  `Owner`, never `company.user_id == user`):
  - `readable_company(user_id, company_id) -> AppResult<CompanyAccess>`: any membership; `None` →
    `NotFound` (same message for unknown and someone else's).
  - `managed_company(user_id, company_id) -> AppResult<Company>` (signature unchanged for its
    existing use-case callers); policy: no access → `NotFound`; `Member` → `Forbidden`;
    `Owner`/`Admin` → the company.
  - `owned_company(user_id, company_id) -> AppResult<Company>`: no access → `NotFound`;
    `Admin`/`Member` → `Forbidden`; `Owner` → the company.
  - The free functions `owned_company` / `managed_company` (used inside other use cases) follow
    the same policy, so e.g. a member calling `create_agent` gets 403 instead of today's 404.
    Their doc comments are rewritten (the "no visible-but-forbidden case" premise is false now that
    members see companies).
  - `operator_company(caller: &Caller, company_id) -> AppResult<Option<Company>>` for the operator
    dashboard (replaces its bare `get_company`); non-operator → `NotFound`.
- `get_company(id)` becomes `pub(crate)` with no adapter caller (kept only if an internal caller
  remains; otherwise deleted).
- Handlers in `routes/{agent,channel,company,company_invite,onboarding,ui,ui_dashboard}.rs` replace
  `get_company` + `c.user_id == user.id` + `_ => "not found"` with the scoped read at the level the
  action needs and `?` (handlers that returned `impl IntoResponse` with a 200 error alert become
  `AppResult<…>` for the authorization step; business-validation alerts stay inline). Levels:
  agent/channel management pages → `managed_company`; company edit/row/delete, invites →
  `owned_company`; onboarding → `owned_company` (today's owner-only redirect becomes 404/403);
  mailbox `load_viewable_channel` → `readable_company`.
- `list_agents_page` / `create_agent_handler` no longer render the company name before the
  permission check and no longer turn the agent-list error into an empty list.
- Page helpers that derived the caller's membership from `company.user_id`
  (`ui::managed_company_membership`, `DashboardScope::membership`, `ui_companies::team_pane`'s
  `editable`) take it from `CompanyAccess` instead.
- Failure modes: DB error → 5xx via `?`; unknown/foreign → 404 with no company name in the body;
  member on an owner/manager action → 403.

### D3 — One thread-in-channel read (plan batch 3)

- `ThreadUseCases::readable_thread(viewer: &Viewer, company_id, channel_id, thread_id) ->
  AppResult<Option<Thread>>`: the channel must be readable by the viewer (the same rule as
  `ChannelUseCases::get_readable_channel`, extracted into one free function
  `readable_channel(companies, channels, participants, viewer, company_id, channel_id)` in
  `use_cases/channel.rs` that both use cases call), and the thread's `channel_id` must equal that
  channel. `None` for unknown thread, other channel, other tenant, unreadable channel alike.
- `load_channel_thread` (ui.rs) and the two copies in `routes/channel.rs` (simulate page,
  simulation stream) are replaced by it; the simulate page no longer answers "Thread does not belong
  to this channel". Those two handlers switch from `AuthenticatedUser` to the `Viewer` extractor.
- `ThreadUseCases::get_thread` is deleted (no remaining caller).

### D4 — Operator is a caller property (plan batch 4)

- Domain type `entities::user::Caller { user_id, operator: bool }` (`is_operator()`,
  `require_operator() -> AppResult<()>` returning `NotFound` — operator surfaces stay invisible to
  everyone else, as today).
- One constructor in the HTTP adapter, `auth::caller_for(account: &User, config: &AppConfig)`, is
  the only caller of `AppConfig::is_operator`; the `Caller` extractor (`FromRequestParts<AppState>`)
  loads the account once and uses it. `workspace_user` (display-only menu flag) uses the same
  constructor so the menu and the authorization can never disagree.
- Library-agent management use cases take `&Caller` and check it themselves:
  `AgentUseCases::{create_library_agent, update_library_agent, delete_library_agent,
  generate_library_system_prompt}` plus new operator reads `library_agents_for(caller)` /
  `library_agent_for(caller, id)` for the operator workspace and JSON API. The public
  `list_library_agents()` / `get_library_agent()` stay unscoped because every company picks from
  the library during onboarding and agent creation.
- `monitoring.rs`, `agent_library{,_forms}.rs`, `skill_library.rs`'s operator handlers and
  `ui_dashboard.rs` take the `Caller` extractor; `require_operator(user, users, config)` is deleted.
- Test: a non-operator `Caller` calling each library-agent management use case directly gets
  `NotFound` and nothing is written.

### D5 — Attention gating in a use case (plan batch 4)

- `application::attention::AttentionUseCases` (ports: `AttentionPersistence`,
  `CompanyPersistence`, `ParticipantPersistence`, `ChannelPersistence` via the D3 free function,
  `MonitoringService`) owns `read_context(viewer, company_id)`, `list(viewer, company_id, query)`
  and `operational_summary(viewer, company_id)`, including the metrics `read_page` records today.
- Policy: no membership / no person principal → `NotFound`; `TeamWork` view or operational summary
  for a non-manager member → `Forbidden` (today 404; a member *sees* the company).
- `routes/attention.rs` and `routes/notifications.rs` (which reuses `read_context`) call it;
  neither touches `AttentionPersistence` or computes gating. `AppState` gains
  `attention_use_cases`.

### D6 — Onboarding as one use case over one transaction (plan batch 5)

- Trace result: `create_agent_from_library`'s write path is already transaction-shaped
  (`prepare_library_agent_on`, `insert_agent_on`, `create_agent_principal_on` take the tx), so it
  runs on the same transaction as `provision_guarded` (which savepoints `provision_on`).
- Application: `use_cases::onboarding::{OnboardingUseCases, OnboardingPersistence}`.
  `OnboardingUseCases::onboard(user_id, company_id, selection)`:
  1. `owned_company` (404/403);
  2. validates the selection exactly as the handler does today (library ids still in the library,
     not empty, custom name and instructions together) → `Rejected(message)` shown on the draft page;
  3. prepares every `AgentWrite` *outside* the transaction (custom: prompt generation, a provider
     call; library: the existing model fallback/pinning, factored out of
     `AgentUseCases::create_agent_from_library` into a shared `prepare_library_copy`);
  4. one port call `OnboardingPersistence::onboard(OnboardingCommand)`.
- Port impl (adapter, next to `AssistantChannelService`, same `PostgresPersistence` + templates):
  one transaction: `authorize_company` (company row `FOR NO KEY UPDATE` + owner/admin re-check);
  each agent gets a deterministic id `UUIDv8(SHA-256("p04.onboarding" ‖ company ‖ library id or
  custom slug))`; an agent with that id already in the company is reused, otherwise inserted
  (library copy through `prepare_library_agent_on`); then for every agent with a model the shared
  assistant-channel command step (factored out of `create_assistant_channel`, command key
  `onboarding:{agent_id}`, same digest/replay rules) runs on the same tx: success records the
  command row, a refusal rolls back only its savepoint and is returned for that agent (agent kept),
  an error propagates and the whole transaction rolls back (no agents). Commit once.
- Agent-creation failures (slug taken by another agent, library agent vanished) roll everything
  back; user-fixable ones (`Conflict`, `BadRequest`, `NotFound`) are shown on the draft page as
  today, others propagate.
- Idempotency: a retry with the same selection maps to the same agent ids and command keys, so it
  reuses the agents and replays the stored channels: no duplicates, same page. (A retried custom
  agent re-runs prompt generation before the transaction; the stored agent is kept as is.)
- `create_assistant_channel` keeps its behaviour (it now calls the shared step); the completion
  page (`complete`) is unchanged.
- Tests `onboarding_*` (DB-backed): channel error after two agents → no agents, no channels, no
  command rows; refusal (address taken) → agent kept, refusal shown; same selection twice → same
  agents/channels, counts unchanged.

### D7 — SMTP sender trust in the application (plan batch 6)

- `ThreadUseCases::sender_trust(selector: &ChannelSelector, sender: &QualifiedIdentity) ->
  AppResult<SenderTrust>` (`Trusted` / `Stranger`): loads company, channel (by `matches_slug`
  over the company's channels — the same lookup `resolve_addresses` uses, via `DirectoryCache`) and
  the sender's access context, and answers `channel.participant_access(context).trusted`. Unknown
  company/channel → `Stranger`; any lookup error → `Err`.
- `smtp/server.rs::sender_needs_spam_scan` parses the recipient selector and sender (adapter
  parsing; unparseable → scan, as today), calls `sender_trust`, and on `Err` logs a warning and
  scans (stranger), as today. `ThreadUseCases::{channel,company,participant,thread}_persistence()`
  accessors are deleted.
- Tests: the existing SMTP trust tests, plus a failing-lookup double → warning logged and scanned.

## Verification plan (fix-app-I004 scope)

- WB1 focused: `cargo test --locked --offline --lib company_access_` (DB), then
  `routes::agent`, `routes::channel`, `routes::company`, `ui_team`, `attention`, `agent_library`,
  `monitoring`, `use_cases::company`, `use_cases::channel`, `use_cases::agent`,
  `use_cases::company_invite`, `use_cases::thread`, `use_cases::user`, `smtp::` (mock hoisting),
  `ui_dashboard`, `ui_companies`, `notifications`, `ui_message`, `ui_stream` (thread read callers);
  `cargo check --locked --offline --all-targets` straight after the trait change; nonzero counts
  recorded per filter.
- WB2 focused: `onboarding_`, `assistant_channel`, `provisioning`, `smtp::`, `use_cases::agent`.
- Final (I004): `cargo fmt --all -- --check`, `SQLX_OFFLINE=true cargo clippy --locked --offline
  --lib --tests -- -D warnings`, the union of the filters above; `.sqlx` regenerated only if a
  macro query changes (none planned). No schema change → no `reset-db` needed (recorded if that
  changes). Full suite / `scripts/check.sh` / stack budget not run, by instruction.

## Status

- G1 r1 written; awaiting B's gate review.

## Cross-phase notices requested (for the manager to publish)

- (pending) p03: `AppError::Forbidden` is a new variant; any exhaustive `match AppError` p03 adds
  (guard-error classification) needs an arm at merge.
- (pending) p07: `ThreadUseCases` persistence accessors removed; new ports/use cases
  (`AttentionUseCases`, `OnboardingUseCases`/`OnboardingPersistence`, `Caller`) for 07's port move.

## Candidate instruction improvements

(none yet)
