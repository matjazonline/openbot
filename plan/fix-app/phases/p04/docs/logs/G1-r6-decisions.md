# FIXAPP-P04-G1 decision revision r6 — 2026-11-04

Original requirements: plan/fix-app/04-authorization-in-application.md and README.md.
Corrects FIXAPP-P04-G1-F21 from independent round FIXAPP-P04-G1-R5; F11's retained
preparation ownership remains fixed. Historical r1–r5 evidence remains retained.
D1–D5/D7 unchanged; D6 specifies all configuration reads on the retained connection,
pure preparation, provider-only invocation, and a one-connection production-path test.
Phase start/review base/HEAD 48626a1240f032ca3b99372bd3f5a05eb08c0c66.
Frozen source digest 510dba5e4bd0cdf496b59c6fcacbc424a02e04721fe9fbd826e30cb3d8aaaef4,
manifest logs/G1-r5-source-snapshot.txt; no source edits since r5. WIP/unaccepted source.
Only exclusion: plan/fix-app/phases/p04/docs.
Coordination authority /Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md, writer
thr_gwccjxwzjt, revision 6; immutable logs/COORDINATION-r6.md SHA256
d3c5b83ecf08b42314e1194bb371ebee030216d8c875cc7f76cf5b0f20322371.
Applicable I001–I004 r1, C002/C003/C007 r1; C008 r1 proposed p07 output.
No schema change, no full suite/scripts/check.sh, no main import/rebase.

## G1 — Architecture decisions (revision r6; preparation uses the retained connection)

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

- Domain type `entities::user::Caller { user_id, operator: bool }` with `is_operator()`.
  `use_cases::caller::require_operator(&Caller) -> AppResult<()>` enforces `NotFound`
  (operator surfaces stay invisible). This r4 clarification keeps the domain free of
  application error imports; it matches the preserved WB1 implementation note.
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

### D6 — Onboarding as one use case over one owned transaction (plan batch 5)

- Trace result: the library-copy helpers `prepare_library_agent_on`, `insert_agent_on`,
  `create_agent_principal_on` accept a transaction, as does `provision_guarded` (which
  savepoints `provision_on`). No schema change is needed.
- Logical operation/key remains r3: one channel-step GET mints a UUID `submission_key`,
  carried as a required hidden POST field (missing/malformed → `BadRequest`). Agent IDs
  are UUIDv8 of SHA-256 over domain `p04.onboarding`, company UUID, submission UUID,
  and role `library:{library_agent_id}` or `custom:{normalized slug}`. Same key and role
  retries reuse IDs; an edited custom slug is a new agent; a new step visit mints a new
  operation, so later onboarding after deletion/rename/transfer never replays old results.
- Application `use_cases::onboarding::OnboardingUseCases::onboard(user_id, company_id,
  submission)` owns validation/preparation and response policy:
  1. `owned_company` (404/403); validate library IDs against the library, nonempty
     selection, custom name/instructions together, at most **10 library agents + 1 custom**.
     Invalid selection → `Rejected(message)` for the draft page.
  2. Begin an opaque purpose-specific session with
     `OnboardingPersistence::begin(user_id, company_id) -> AppResult<Box<dyn OnboardingSession>>`.
     This is the preparation ownership point: the adapter begins the sole transaction,
     locks the company `FOR NO KEY UPDATE` then the caller's member row (the existing
     `authorize_company` lock order), and rechecks membership **Owner**. Missing/no
     membership → `NotFound`; admin/member → `Forbidden`, always propagated as an
     authorization error, never a draft message. No provider call occurs before this point.
  3. `OnboardingSession::existing_agents(&mut self, ids)` reads the derived IDs in that
     company under the retained lock. Existing agents require no preparation. Prepare
     only missing writes in the application while retaining the session. **R6/F21:**
     load model connections using required `OnboardingSession::model_connections(&mut self)`
     and, only when a missing custom agent needs it, the selected provider's secret using
     `OnboardingSession::model_api_key(&mut self, provider)`. Both use the session's same
     retained transaction connection, including credential decryption in the adapter;
     they never call a pool-backed CompanyPersistence method. Connection/model rows can
     be locked/read in the existing company→membership→configuration order.
     Library fallback/pinning uses shared pure preparation/validation helpers over the
     loaded connection slice. Refactor the existing asynchronous model-validation path
     to load then call the same pure validator, preserving BadRequest-only fallback and
     propagation of every loading/internal error. Custom provider/model selection is also
     a shared pure helper over the loaded connections. Resolve the narrow prompt request
     with that provider/model and its separately loaded API key (no Debug/Serialize on
     secret-carrying types).
     Invoke application-owned `OnboardingPromptGenerator` with **fully resolved inputs**;
     the production path uses the configured TextClassifier and existing common prompt
     response handling only. It must NOT call the DB-reading `generate_system_prompt`,
     verify_company_manager, list_model_connections, model_api_key, or any helper that
     reacquires from the pool. Those entry-point checks/reads have been performed with
     the session. `AgentUseCases` may supply this provider-only method using its configured
     classifier, but it performs no persistence I/O. The provider future is boxed at
     that seam to avoid growing the unoptimized async stack chain.
  4. `OnboardingSession::complete(self: Box<Self>, command)` consumes the session and
     performs all library copies, inserts/principals and channel commands, then commits
     once and returns agents plus per-agent channel outcomes. The command carries the
     selected derived IDs and prepared missing writes; the session retains company/caller
     scope rather than accepting a second authority in the completion call.
- `OnboardingSession` is an application port, not a SQLx abstraction: only these
  onboarding-specific operations are exposed (existing selected agents, scoped preparation
  inputs, and atomic completion). The Postgres implementation owns the
  `Transaction<'static, Postgres>` plus provisioning templates. Dropping the session,
  its future, or an uncompleted operation rolls back (SQLx transaction RAII). Application
  code imports no database/framework types; adapters contain no prompt-provider logic.
- Completion (same transaction): existing company agents are reused; missing library
  agents use `prepare_library_agent_on` + `insert_agent_on` + principal, missing custom
  agents use insert + principal. Per modelled agent, share the assistant-channel command
  step with `create_assistant_channel` (key `onboarding:{agent_id}`, unchanged digest/replay).
  Provisioning refusal rolls back only its savepoint and keeps the agent; digest mismatch
  becomes that agent's refusal; every other error rolls back all agents, channels,
  skills and command rows. Agent-creation `Conflict`/`BadRequest`/`NotFound` becomes a draft
  rejection after rollback; other errors propagate. Begin/authorization errors propagate.
  `create_assistant_channel` keeps its own refusal rollback; `complete` route stays unchanged.
- **F11 ownership correction (r5):** the transaction/company lock precedes the existing-ID
  check and all provider effects. Same-key contenders serialize there; the winner prepares
  and commits, the waiter then sees its agents and skips preparation. This applies across
  processes/instances and survives application task cancellation through DB lock release;
  no process-local mutex or unchecked existence preflight claims ownership.
- **F21 progress correction (r6):** after begin, inventory of all connection use is:
  existing-agent read, model-list read, selected secret read, inserts/library-skill copies,
  provisioning/replay writes and commit — **all on the retained transaction connection**.
  Provider-only preparation and pure model helpers acquire zero DB connections. No pooled
  persistence method runs until that session is committed/dropped. This gives progress
  even with a one-connection pool; no pool increase or reserved extra slot is the fix.
- Bounds and failure semantics: wrap the whole begin→prepare→complete operation in a
  **90-second timeout**, and each custom prompt future in a **30-second timeout**.
  Set transaction-local lock/statement timeouts to **45 seconds**, plus idle-in-transaction
  timeout **45 seconds**. Waiting/execution timeout → a 5xx error with retry guidance;
  dropping the timed-out future drops/rolls back the session and cancels the awaited
  provider future. No detached task may outlive the operation. Explicitly accept holding
  one pool connection and the company's authority lock across up to 30 seconds of prompt
  generation; this bounded tradeoff avoids a new preparation ledger/schema. At most one
  custom preparation runs per submission. A canceled/failed attempt can have paid for a
  provider response before rollback or process loss; a later retry may need a new call.
  Exactly-once billing across crashes is not promised; a successful competing retry never
  generates and discards a second result. Different keys remain distinct operations and
  may conflict on agent slugs (user-fixable rejection).
- Meaningful DB tests `onboarding_*`, own_database for whole-table assertions:
  - injected BEFORE INSERT channels trigger raises P0001 only for the second selected
    agent's channel, proving two agents were inserted before failure; expect successful
    fault installation, assert injected error, then zero agents/channels/command rows;
  - address-taken provisioning refusal keeps agent and returns refusal;
  - same key sequential retry returns same agents/channels with unchanged counts; new
    key after deletion yields fresh agents/channels; new key after rename/transfer avoids
    replaying old command rows (F7);
  - two same-key **custom** submissions compete: controlled counting prompt generator
    pauses the first invocation; start the second and prove its DB backend is waiting
    for the company lock before releasing the first (bounded wait helper, not a sleep).
    Assert **one provider invocation**, one agent/channel set, and identical results;
  - constrained-pool progress: use a **one-connection** Postgres pool, with the real
    production OnboardingUseCases preparation path and actual AgentUseCases configured
    with a controlled counting TextClassifier (not a replacement of the whole generator).
    Run custom + library preparation/provisioning successfully with bounded acquisition
    and an outer test deadline; assert one classifier call and expected agents/channels.
    Also run concurrent calls with the same constrained pool to prove the owning operation
    needs no second connection and the waiter progresses after release.
  - provider failure/timeout/cancellation rolls back and releases ownership; a later call
    can proceed, with no detached provider or DB task; test cancellation using controlled
    generator signals and a subsequent acquisition, without waiting 30/90 real seconds;
  - non-owner admin/member → 403, another tenant/unknown company → identical 404.

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

