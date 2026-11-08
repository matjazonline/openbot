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

## G1 — Architecture decisions (revision r3)

r1 text is preserved in `logs/G1-r1-decisions.md`, r2 in `logs/G1-r2-decisions.md`; r2 answers
FIXAPP-P04-G1-F1…F16 (finding IDs noted per change); r3 adds the F19 dashboard rule and adopts
F18/F20 (marked "r3"). D5 and D7 are unchanged from r1 except the helper named in F15.

### D1 — `company_access` is the one membership read (plan batch 1)

- `CompanyPersistence::company_access(user_id, company_id) -> AppResult<Option<CompanyAccess>>`
  becomes a required method. Postgres: one row,
  `SELECT company.*, member.role FROM companies company JOIN company_members member
   ON member.company_id = company.id AND member.user_id = $1 WHERE company.id = $2`.
  Role `owner` → `Owner`, `admin` → `Admin`, `member` → `Member`; no row → `None`. Ownership comes
  from the owner member row alone (01/C003 makes it agree with `companies.user_id` at commit), so
  no `companies.user_id = $1` disjunct. An unknown role string is `AppError::Internal` (the CHECK
  constraint makes it unreachable), never a downgrade.
- `list_accessible_by_user_id` also loses its default. Its Postgres listing keeps its 200 cap (it
  only populates switchers and default selection now, see D2) and derives membership from the
  member row the same way. Nothing authorizes an explicitly requested company by scanning it.
- `update_for_user`, `delete_for_user` (CompanyPersistence), `has_login_method`,
  `activate_password` (UserPersistence), `latest_replyable_email_context` (ThreadPersistence) lose
  their defaults. Postgres already implements all five; test doubles state their behaviour.
  Postgres `update_for_user` / `delete_for_user` answer `company_not_found()` (404) instead of
  `AppError::Internal` when the scoped statement matches nothing (F1).
- Test doubles: the six `MockCompanyPersistence` copies (smtp/server.rs, agent_tests.rs,
  channel.rs, company.rs, company_invite_tests.rs, thread/tests.rs) become one
  `use_cases::company_test_support::MockCompanyPersistence` (`#[cfg(test)]`): companies, explicit
  `(user, company, membership)` rows (owner rows included — no "owner implied" shortcut), stated
  model connections/key, a `failing()` mode where every read returns `AppError::Database`, and one
  `TeamFixture` impl whose by-address policy is an explicit enum (`EveryAddressIsMember`, `Listed`,
  the thread tests' existing address heuristic). Methods a copy left `unimplemented!()` keep a loud
  failure for unconfigured calls rather than a silent default.

### D2 — Response policy and scoped company reads (plan batch 2)

- New `AppError::Forbidden(String)` → HTTP 403 with the message; every exhaustive
  `match AppError` gains an arm (the inbound worker classifies it as permanent, like `BadRequest`).
- Scope of "responses are consistent" (F13): company-scoped surfaces reached through
  `CompanyUseCases`/the use cases below. Workflow-engine routes keep their own authority and 404
  policy (`application/workflow/authorization.rs`), unchanged by this phase.
- `CompanyUseCases` scoped reads, all on `company_access` (ownership = membership `Owner`):
  - `readable_company(user_id, company_id) -> AppResult<CompanyAccess>`: any membership, else
    `NotFound` (identical for unknown and someone else's).
  - `managed_company(user_id, company_id) -> AppResult<CompanyAccess>`: no access → `NotFound`;
    `Member` → `Forbidden`; `Owner`/`Admin` → access. The free function `managed_company` (used by
    agent/channel/mcp/skill use cases) keeps returning `Company` with the same policy.
  - `owned_company(user_id, company_id) -> AppResult<Company>` (method and free function): no
    access → `NotFound`; `Admin`/`Member` → `Forbidden`; `Owner` → the company.
  - Doc comments rewritten (the "no visible-but-forbidden case" premise no longer holds).
  - Use-case code that branched on `Err(NotFound)` from these helpers is rewritten to branch on
    the membership instead (F2): `CompanyInviteUseCases::list_company_team_members` reads
    `company_access` once — no access → `NotFound`, owner → the owner listing, admin/member → the
    member listing it falls back to today. Every other such branch is found with
    `graft callers owned_company|managed_company --depth all` and handled the same way.
  - `update_company_for_user` / `delete_company_for_user` call `owned_company` first (F1), so an
    unknown/foreign company is 404, an admin/member 403, and a DB error 5xx; the SQL scope stays as
    defence in depth.
  - `operator_company(caller: &Caller, company_id) -> AppResult<Option<Company>>` for the operator
    dashboard; non-operator → `NotFound`.
  - `get_company(id)`: deleted if no internal caller remains, else `pub(crate)` with no adapter
    caller. `update_company(id, …)` / `delete_company(id)` (unscoped) likewise.
- Explicitly requested companies in `/ui` (F3): `ui::load_scoped_company`,
  `load_managed_company`, `load_readable_company` resolve a `Some(company_id)` through
  `owned_company` / `managed_company` / `readable_company` (404/403/5xx). The capped lists are used
  only for the switcher and for the default selection when no id is requested.
  `load_managed_company` returns the selection as `CompanyAccess`, and
  `managed_company_membership` (membership from `company.user_id`) is deleted; its callers take the
  membership from the access. `DashboardScope::membership` and `ui_companies::team_pane`'s
  `editable` likewise take it from `CompanyAccess`.
- `ui_team` (F3, plan batch 4): `Workspace::scoped_company` resolves through `readable_company`;
  `TeamView` carries the membership; `role()` is `Owner` iff membership `Owner`;
  `require_owner` → `Forbidden` (403). Read-only panes stay available to members and admins, as
  the embedded team tab already shows them.
- Per-handler levels (F4): every rewritten handler keeps today's level; only the response changes.

  | Handlers | Today | After |
  |---|---|---|
  | `agent.rs` list page, create, edit form, cancel, update, delete (HTML) | name via bare read; use case `managed_company` | `managed_company` (404/403/5xx) first |
  | `channel.rs` list page, create, edit form, cancel, update, delete, threads page/fragment | bare read; use case manager check | `managed_company` |
  | `channel.rs` simulate page, simulate POST, simulation stream, open-thread GET/POST | owner (`company.user_id == user.id`) | `owned_company` (owner-only stays) |
  | `company.rs` edit form, cancel, update, delete (HTML, JSON, `/ui/companies` delete) | owner via bare read / SQL | `owned_company` (via `update_/delete_company_for_user`) |
  | `company_invite.rs` invites page | owner via bare read | `owned_company` |
  | `onboarding.rs` channel step GET/POST, complete | owner, else redirect `/ui` | `owned_company` (404/403) |
  | `ui.rs::load_viewable_channel` | bare company read after channel guard | `readable_company` |
  | `ui_dashboard.rs` operator company | bare read for operators | `operator_company(&caller, id)` |

- Dashboard (r3, F19): `DashboardScope::resolve` checks the caller first. For an operator a
  requested company id is resolved with `operator_company(&caller, id)` (the operator need not
  manage it) and the managed list only fills the switcher; for everyone else the requested id goes
  through `managed_company` (member → 403, stranger → 404). Tests: operator + a company they do
  not manage → that company's dashboard; member → 403; stranger → 404.
- `PUT /companies/{id}` (r3, F18): the authorization step is `owned_company` before the form is
  processed, so a stranger gets 404 and a member/admin 403 rather than a 200 "Update failed" alert;
  only write failures after authorization stay inline.

- Child reads in those handlers (F5): `if let Ok(Some(..))`, `_ => "… not found"` and
  `.unwrap_or_default()` on authorized reads become `?` plus an explicit `NotFound` for `None`;
  `delete_agent_handler`'s and the company delete's `let _ =` become `?`. Business-validation
  alerts (form errors, "Update failed: …" from a write) stay inline.
- Visible failure for `base_layout` htmx fragments (F12): `base_layout` gets the shared
  `REQUEST_ERROR_TOAST` element, `REQUEST_ERROR_SCRIPT` and the delegated
  `dismiss-request-error` click handler (r3 F20) that `ui_layout` already carries, so a
  403/404/5xx on a legacy `/companies/…` fragment shows the error instead of a silent no-op;
  covered by a page test.
- `list_agents_page` / `create_agent_handler` authorize before rendering anything and no longer
  turn the agent-list error into an empty list.

### D3 — One thread-in-channel read (plan batch 3)

- `ThreadUseCases::readable_thread(viewer: &Viewer, company_id, channel_id, thread_id) ->
  AppResult<Option<Thread>>`: the channel must be readable by the viewer and the thread's
  `channel_id` must equal it. `None` alike for unknown thread, other channel, other tenant,
  unreadable channel. The channel rule is one free function in `use_cases/channel.rs`,
  `readable_channel(companies, channels, participants, viewer, company_id, channel_id)`, used by
  `ChannelUseCases::get_readable_channel` and by `readable_thread`; its sibling
  `readable_channels(…, viewer, company_id)` (list) serves `list_readable_channels` and D5 (F15).
- Entry points replaced (F4/F16): `ui::load_channel_thread` and all its callers (ui.rs ×5,
  ui_internal_notes, ui_message, ui_attachments, ui_message_diagnostics), and in `channel.rs`
  `load_simulation_thread_fragment`'s thread read reached from the simulate page, the simulate
  POST, the simulation stream and `open_simulated_thread_logic` (open-thread GET and POST) — the
  in-function `get_thread` + channel comparison in the page path (line ~648) and the stream
  (~923). The simulation surfaces stay owner-gated by `owned_company` (D2 table); the owner's
  viewer read admits every channel (`Channel::viewer_access`: owner → true), so `readable_thread`
  adds no new restriction there. "Thread does not belong to this channel" is gone.
- `ThreadUseCases::get_thread` is deleted.

### D4 — Operator is a caller property (plan batch 4)

- Domain type `entities::user::Caller { user_id, operator: bool }` with `is_operator()` and
  `require_operator() -> AppResult<()>` (`NotFound`: operator surfaces stay invisible).
- One constructor in the HTTP adapter, `auth::caller_for(account: &User, config: &AppConfig)`, is
  the only caller of `AppConfig::is_operator`; the `Caller` extractor loads the account once and
  uses it; `workspace_user` (display-only menu flag) uses the same constructor.
- Use cases that need the property check it themselves:
  - `AgentUseCases::{create_library_agent, update_library_agent, delete_library_agent,
    generate_library_system_prompt}` take `&Caller`; operator reads `library_agents_for(caller)` /
    `library_agent_for(caller, id)` serve the operator workspace and JSON API.
  - Library-skill management in `SkillUseCases` (F6): the library create/update/delete methods
    take `&Caller` and check it; the operator-only library reads used by `skill_library.rs`'s
    operator handlers get operator-checked variants the same way. Reads every company uses (the
    library listing behind agent/onboarding forms: `list_library_agents`, `get_library_agent` and
    the skill reads those forms use) stay unscoped.
- Handler-level checks with a recorded reason (F6): `monitoring.rs` (global process metrics from
  `MonitoringService`, no application use case owns them) and the dashboard's global rollup
  scope (`DashboardSnapshotService`, a service read with an `Option<company>` scope) check the
  `Caller` property in the handler. Moving those reads behind use cases is 07's application
  layering; recorded as a scope boundary, not a decision made in HTTP: the property itself is
  decided once in `caller_for`.
- `require_operator(user, users, config)` and `skill_library::operator` are deleted.
- Test: a non-operator `Caller` calling each library-agent management use case (and a library
  skill write) directly gets `NotFound` and nothing is written.

### D5 — Attention gating in a use case (plan batch 4)

- `application::attention::AttentionUseCases` (ports: `AttentionPersistence`,
  `CompanyPersistence`, `ParticipantPersistence`, `ChannelPersistence` via D3's
  `readable_channels`, `MonitoringService`) owns `read_context(viewer, company_id)`,
  `list(viewer, company_id, query)` and `operational_summary(viewer, company_id)`, including the
  metrics `read_page` records today.
- Policy: no membership / no person principal → `NotFound`; `TeamWork` view or operational summary
  for a non-manager member → `Forbidden` (today 404).
- `routes/attention.rs` and `routes/notifications.rs` (which reuses `read_context`) call it.
  Notifications' own persistence calls (mark read, list) are outside this plan (F14, scope
  boundary recorded; 07 or a later plan). `AppState` gains `attention_use_cases`.

### D6 — Onboarding as one use case over one transaction (plan batch 5)

- Trace result: `create_agent_from_library`'s write path is transaction-shaped
  (`prepare_library_agent_on`, `insert_agent_on`, `create_agent_principal_on` take the tx), so it
  runs on the same transaction as `provision_guarded` (which savepoints `provision_on`).
- Logical operation and key (F7): one onboarding *submission*. The channel step GET mints a random
  `submission_key` (UUID) carried as a hidden field; the POST requires it (missing/malformed →
  `BadRequest`). Agent ids derive from it: `UUIDv8(SHA-256("p04.onboarding" ‖ company ‖ key ‖
  role))` with role `library:{library_agent_id}` or `custom:{normalized slug}` (F16; r3 F20: a
  re-post of the same key with an edited custom name is a different agent, not a silent reuse).
  The form field is named `submission_key` (r3 F20), distinct from the `onboarding_key(agent)`
  command-key helper. Retrying the same submission
  (double submit, reload of the POST, network retry) reuses the same ids; a new visit to the step
  mints a new key, so a later re-onboarding — after the agents were deleted, renamed, or the company
  changed hands — is a new operation with new ids and never replays an old result.
- Application: `use_cases::onboarding::{OnboardingUseCases, OnboardingPersistence}`.
  `OnboardingUseCases::onboard(user_id, company_id, submission)`:
  1. `owned_company` (404/403);
  2. validates the selection as the handler does today (library ids still in the library, not
     empty, custom name and instructions together) plus a bound of at most 10 library agents per
     submission (F10) → `Rejected(message)` on the draft page;
  3. asks the port which derived agent ids already exist (`existing_agents(company, ids)`); for
     those, no write is prepared (F11: a retried custom agent does not pay for prompt generation
     again);
  4. prepares the remaining `AgentWrite`s outside the transaction (custom: prompt generation;
     library: the model fallback/pinning factored out of `create_agent_from_library` into a shared
     `prepare_library_copy`, where only a `BadRequest` from `validate_model_selection` means "model
     unavailable" and any other error propagates — F11);
  5. one port call `OnboardingPersistence::onboard(OnboardingCommand)`.
- Port impl (adapter, beside `AssistantChannelService`, same `PostgresPersistence` + templates),
  one transaction:
  1. the company row `FOR NO KEY UPDATE` and the requester's member row (`authorize_company`'s
     locks), then an owner check under the lock: not the owner → `Forbidden` returned as an error
     (403, not a form message) (F9);
  2. per agent: id present in the company → reused; else inserted (library copy through
     `prepare_library_agent_on` + `insert_agent_on` + `create_agent_principal_on`; custom through
     `insert_agent_on` + principal);
  3. per agent with a model: the assistant-channel command step shared with
     `create_assistant_channel` (factored out; key `onboarding:{agent_id}`, same digest/replay):
     success records the command row; a provisioning refusal rolls back only its savepoint and is
     returned for that agent (agent kept); a replay digest mismatch is returned as that agent's
     refusal too (agent kept, nothing else aborted); any other error propagates and the whole
     transaction rolls back (no agents, no channels, no command rows);
  4. commit once.
- Agent-creation failures roll everything back; user-fixable ones (`Conflict`, `BadRequest`,
  `NotFound`) are shown on the draft page as today, others propagate.
- Concurrency (F8): two submissions with the same key serialize on the company row lock; the
  second sees the first's agents (same ids) and its stored command rows, so both return the same
  agents and channels and nothing is duplicated. Different keys simply create separate agents
  (slug conflicts are the user-fixable error above).
- `create_assistant_channel` keeps its behaviour (it calls the shared step and still rolls back on
  its own refusal); `complete` is unchanged.
- Tests `onboarding_*` (DB-backed, `own_database` where they assert whole-table state):
  - channel error after two agents → no agents, channels or command rows. Fault injection: a
    `BEFORE INSERT` trigger on `channels` that raises (`P0001`) for the test company, installed
    with `.expect()`ed statements in its own database;
  - provisioning refusal (address already taken) → agent kept, refusal shown;
  - same key twice → same agents/channels, counts unchanged; a new key after deleting the agents →
    new agents and channels (F7);
  - competing claimants: two concurrent `onboard` calls with the same key → one set of agents and
    one channel each, identical results (F8);
  - non-owner admin → 403; another tenant's company → 404.

### D7 — SMTP sender trust in the application (plan batch 6)

- `ThreadUseCases::sender_trust(selector: &ChannelSelector, sender: &QualifiedIdentity) ->
  AppResult<SenderTrust>` (`Trusted` / `Stranger`): loads company, channel (by `matches_slug` over
  the company's channels — the lookup `resolve_addresses` uses, via `DirectoryCache`) and the
  sender's access context, and answers `channel.participant_access(context).trusted`. Unknown
  company/channel → `Stranger`; any lookup error → `Err`.
- `smtp/server.rs::sender_needs_spam_scan` parses the recipient selector and sender (adapter
  parsing; unparseable → scan, as today), calls `sender_trust`, and on `Err` logs a warning and
  scans. `ThreadUseCases::{channel,company,participant,thread}_persistence()` are deleted.
- Tests: the existing SMTP trust tests, plus a failing-lookup double → warning logged and scanned.

## Verification plan (fix-app-I004 scope)

- Batch 1 DB tests (F16): `company_access_*` — a user with 201 companies can access the oldest
  one; owner, admin, member and no-relation each return the right membership.
- Site count (F16): the plan's "26 adapter sites" for `get_company` is 23 `CompanyUseCases::
  get_company` calls at `48626a1` plus 3 scoped `SkillUseCases::get_company` calls (a different,
  already-scoped method, untouched).
- Route tests required by the plan (F16): another tenant's company id → 404 with no company name
  in the body; member on an owner action → 403; failing persistence → 500, not "not found". One
  foreign-tenant 404 test per rewritten handler family (agent pages, channel management pages,
  simulation, company pages, invites page, onboarding, mailbox thread read, team, attention).
- WB1 focused (nonzero counts recorded per filter): `company_access_` (DB), `routes::agent`,
  `routes::channel`, `routes::company`, `routes::company_invite`, `ui_team`, `attention`,
  `notifications`, `agent_library`, `skill_library`, `monitoring`, `ui_dashboard`,
  `ui_companies`, `ui_agents`, `ui_channels`, `ui_schedules`, `ui_deliveries`, `ui_run_counts`,
  `workflows::ui`, `ui_message`, `ui_stream`, `ui_internal_notes`, `ui_attachments`,
  `ui_message_diagnostics`, `use_cases::company`, `use_cases::channel`, `use_cases::agent`,
  `use_cases::company_invite`, `use_cases::thread`, `use_cases::user`, `use_cases::skill`,
  `use_cases::mcp`, `smtp::`, `pages::layout`; `cargo check --locked --offline --all-targets`
  right after the trait change.
- WB2 focused: `onboarding_`, `assistant_channel`, `provisioning`, `smtp::`, `use_cases::agent`.
- Final (I004): `cargo fmt --all -- --check`, `SQLX_OFFLINE=true cargo clippy --locked --offline
  --lib --tests -- -D warnings`, the union of the filters above; `.sqlx` regenerated only if a
  macro query changes (none planned). No schema change → no `reset-db` (recorded if that
  changes). Full suite / `scripts/check.sh` / stack budget not run, by instruction.

## Status

- G1 r1: changes required (FIXAPP-P04-G1-R1, F1–F16). G1 r2: changes required (FIXAPP-P04-G1-R2,
  F17 records, F19 dashboard; F18/F20 optional, adopted). G1 r3 written; awaiting recheck.
- Process deviation (F17): WB1 edits started at about 10:50 on 2026-11-01, during the r2 recheck
  and before any G1 pass, on parts B's R1 note called sound (D1 query, mock hoisting) and D2's
  helper policy; the artifact under review changed mid-round (B saw digest `cef53723…822b`). These
  edits are unreviewed until the WB1 freeze and will be unwound wherever a G1 correction changes
  them. Frozen for the r3 recheck: see "G1 r3 freeze" below.

## Cross-phase notices requested (for the manager to publish)

- (pending) p03: `AppError::Forbidden` is a new variant; any exhaustive `match AppError` p03 adds
  (guard-error classification) needs an arm at merge.
- (pending) p07: `ThreadUseCases` persistence accessors removed; new ports/use cases
  (`AttentionUseCases`, `OnboardingUseCases`/`OnboardingPersistence`, `Caller`) for 07's port move.

## Candidate instruction improvements

(none yet)

## G1 r3 freeze (for the G1 r3 recheck only)

- HEAD `48626a1`, digest `4f458bd25090bae457d310e858875fba1211f2f7eafa94510b16728d51e1e7ed`
  (exclusion `plan/fix-app/phases/p04/docs`). In-progress, unreviewed WB1 files (not part of the
  gate; reviewed at the WB1 freeze): `src/adapters/http/app_error_impl.rs`,
  `src/adapters/persistence/company.rs`, `src/application/app_error.rs`,
  `src/application/services/inbound_event_worker.rs`, `src/application/use_cases/{agent_tests,
  channel,company,mod,user,caller (new),company_test_support (new)}.rs`,
  `src/application/use_cases/thread/{mod,test_support}.rs`, `src/domain/entities/user.rs`.
  No code edits until the r3 recheck returns.
