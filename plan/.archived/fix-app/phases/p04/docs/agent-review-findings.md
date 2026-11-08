# Reviewer findings — fix-app p04 (Authorization in the application layer)

Shared findings log for phase p04 (workflow `implement-opus-reviewed-2_2`). Owned by reviewer B.
Task record: [TASK.md](TASK.md). Review record: [REVIEW.md](REVIEW.md). Notes:
[agent-review-notes.md](agent-review-notes.md).

Common context for every G1 r1 finding below (unless a finding says otherwise):

- Date 2026-11-01; workflow `implement-opus-reviewed-2_2`; stage: architecture gate **G1**,
  decision revision **r1** (immutable copy `logs/G1-r1-decisions.md`, identical to TASK.md G1 at
  review time); covers WB1 (plan batches 1–4) and WB2 (plan batches 5–6).
- Review round: [FIXAPP-P04-G1-R1](agent-review-notes.md#fixapp-p04-g1-r1).
- Reviewer: B, Opus 5.5 (`claude-opus-5-5`), effort as configured by the spawner (TASK states
  `xhigh`; not independently observable by B).
- Phase `p04`, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p04`, branch `fix-app/p04`,
  phase start = review base = HEAD `48626a1`, digest
  `c325eea2f3e71d55bbb4f3c5acdfb0df01515171e063db16d2366218d4aa3462` (exclusion
  `plan/fix-app/phases/p04/docs`); no code changes yet.
- Supporting grep evidence: [logs/G1-R1-reviewer-evidence.log](logs/G1-R1-reviewer-evidence.log).

---

## FIXAPP-P04-G1-F1 — Company update/delete keep a persistence-level owner check that answers 500

- **Classification:** actionable defect; severity **high**; category requirements / security
  (response policy). Originates in gated decisions D1 and D2 (scope omission), not implementation.
- **Evidence:**
  - D1 states the Postgres impls of `update_for_user` / `delete_for_user` "already implement" and
    only test doubles change. Those impls answer an unknown, foreign or non-owned company with
    `AppError::Internal("Company not found.")` (`src/adapters/persistence/company.rs:262`, `:268`),
    which `app_error_impl.rs` maps to **500**.
  - `CompanyUseCases::update_company_for_user` / `delete_company_for_user`
    (`src/application/use_cases/company.rs:395-409`) do no access check of their own; the decision
    is the SQL `WHERE ... user_id = $6`, i.e. in persistence, not in a use case taking the caller.
  - Adapter callers with no prior scoped read, so D2's `get_company` replacement list never reaches
    them: `PUT/DELETE /api/companies/{id}` (`routes/company.rs:224-253`, `?` → 500 for a stranger,
    an admin or a member), `PUT /companies/{id}` (`company.rs:162-181`, renders "Update failed:
    Internal error…" with 200), `DELETE /companies/{id}` (`company.rs:185-194`, `let _ =` swallows
    every error including a DB outage and answers an empty 200), `DELETE /ui/companies/{id}`
    (`ui_companies.rs:742-748`, 500 for a member/admin/stranger).
  - Required (plan Outcome table, TASK acceptance criterion 3): unknown/someone else's → 404,
    visible but not allowed (admin/member) → 403, DB error → an error, never a decision; and the
    decision is made once in a use case (Outcome 1).
- **Impact:** after p04 claims criterion 3, the JSON API and the companies workspace still answer
  500 for ordinary authorization denials, and the HTMX delete hides failures as success.
- **Required change to the decision:** D1/D2 must cover these two use cases: run the `owned_company`
  policy in the use case (404/403) before the write, map a persistence no-row (race) to `NotFound`
  rather than `Internal`, and make the HTMX delete propagate. Add route tests (stranger 404, member
  403, failing double 500) for both update and delete.
- **Instruction lesson:** observed — the plan enumerated `get_company` call sites, and the decision
  inherited that enumeration instead of tracing every adapter entry into an authorization decision
  ("every caller-supplied entity id", `src/adapters/http/AGENTS.md`). Candidate (scope: plans and
  gated decisions on authorization): "derive the handler list from the routes that accept a tenant
  id, not from one method's call sites."
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D1: Postgres `update_for_user`/`delete_for_user` answer `company_not_found()` on no match; D2: both use cases call `owned_company` first (404/403/5xx), SQL scope kept as defence in depth; D2 table lists the HTML, JSON and `/ui/companies` delete handlers; D2 child-read rule turns the company delete's `let _ =` into `?`. Residual clarification for `PUT /companies/{id}` (inline "Update failed" alert) logged as optional FIXAPP-P04-G1-F18. Implementation to be verified in the WB1 review.

## FIXAPP-P04-G1-F2 — D2's 403 policy breaks the member team list that relies on `NotFound`

- **Classification:** actionable defect; severity **medium**; category correctness (contract change
  with an unlisted dependent). Originates in D2.
- **Evidence:** D2 changes the free function `owned_company` so an admin or member gets `Forbidden`
  instead of `NotFound`. `CompanyInviteUseCases::list_company_team_members`
  (`src/application/use_cases/company_invite.rs:284-296`) falls back to "a member may list the team"
  only on `Err(AppError::NotFound(_))`; every other error is returned. Under D2 a member or admin
  receives `Forbidden`, so the team tab (`ui_team::TeamView::members`, embedded by
  `ui_companies::team_pane` for members) becomes a 403 for everyone but the owner. D2 lists no
  callers that match on the old `NotFound`.
- **Required change:** D2 must enumerate callers that branch on the free functions' error variant
  and rewrite them over `company_access` (here: owner → members + invites; admin/member → members;
  none → `NotFound`). Add a use-case test for member and admin listing the team.
- **Instruction lesson:** observed — changing an error variant of a shared guard was treated as a
  signature-preserving change. Candidate (scope: `src/application/`): "when a shared guard's error
  variant changes, grep for matches on the old variant and list them in the decision."
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D2: `list_company_team_members` reads `company_access` once (none → `NotFound`, owner → owner listing, admin/member → member listing) and every other `Err(NotFound)` branch on the guards is audited via `graft callers --depth all`. Verified at HEAD that `company_invite.rs:284` is the only such use-case branch on the company.rs guards (workflow routes match their own authority's `NotFound`).

## FIXAPP-P04-G1-F3 — List-scan company selection helpers stay HTTP authorization over capped lists

- **Classification:** actionable defect; severity **medium**; category requirements coverage.
  Originates in D2 (and D4 for `ui_team`, which has no decision text).
- **Evidence:**
  - `load_managed_company`, `load_scoped_company`, `load_readable_company`
    (`src/adapters/http/routes/ui.rs:165-217`) decide "may this caller use requested company X" in
    HTTP by scanning `list_managed_companies` / `list_user_companies` /
    `list_accessible_companies`, each capped at 200 rows. About 30 call sites
    (`ui_agents`, `ui_channels`, `ui_schedules`, `ui_deliveries`, `ui_run_counts`,
    `workflows/ui/mod.rs`, `ui_dashboard`, `ui_team`, `attention`, `notifications`, …; see the
    evidence log) use them with an explicit `Some(company_id)`, e.g. `ui_agents::Workspace::
    scoped_company` (`ui_agents.rs:149-152`) and `workflows::ui::Ui::company`
    (`workflows/ui/mod.rs:65-69`).
  - Consequences the plan names as defects: beyond 200 companies the oldest is "not found" (the
    batch-1 rationale for `company_access`); a member requesting a managed page gets 404 or a 200
    "no company" page (`ui_agents.rs:176-183`) instead of the table's 403; `ui_team` resolves its
    company through `load_scoped_company` (owned-only list, `ui_team.rs:198-202`), so a non-owner's
    write is 404, not the 403 batch 4 requires, and `TeamView::require_owner` answers
    `AppError::Internal` (`ui_team.rs:549-556`). D1–D7 contain no decision about these helpers or
    about `ui_team` (criterion 6 appears only in the acceptance list and verification filters).
- **Required change:** decide in G1 that an explicitly requested company is resolved through the
  scoped one-row reads (`readable_company` / `managed_company` / `owned_company`, 404/403), with
  the lists used only to populate switchers and default selection; give `ui_team` its decision
  (role from `CompanyAccess`, `require_owner` → `Forbidden`, company via `readable_company`). Or
  record an explicit, justified scope disposition for any helper left as is.
- **Instruction lesson:** observed — the same "authorize by scanning a capped list" pattern was fixed
  in the trait default (batch 1) but not at its HTTP call sites. Cause of the omission unknown
  (the plan does not name the helpers).
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision**, with one new interaction logged as FIXAPP-P04-G1-F19 (operator dashboard). D2: `load_scoped_company`/`load_managed_company`/`load_readable_company` resolve `Some(id)` through `owned_company`/`managed_company`/`readable_company`; capped lists only for switchers/default; `managed_company_membership` deleted; `ui_team` decision added (`readable_company`, membership on `TeamView`, `require_owner` → `Forbidden`).

## FIXAPP-P04-G1-F4 — Owner-only simulation surfaces have no stated level; D3 misses entry points

- **Classification:** actionable defect; severity **medium**; category security / requirements.
  Originates in D2 (level mapping) and D3 (handler list).
- **Evidence:**
  - Today the simulation surfaces are owner-only (`company.user_id == user.id`):
    `simulate_channel_page` (`channel.rs:560-563`), `simulate_channel_handler` (`:717-720`),
    `simulation_stream` (`:906-910`), `open_simulated_thread_logic` (`:1060-1068`, reached by
    `open_simulated_thread_get` and `_post`). D2's mapping says "agent/channel management pages →
    `managed_company`", which, applied to these handlers, silently extends simulation — including
    `SimulationMode::Run`, a real outbound send — to company admins. D2 does not classify them.
  - D3 applies the viewer *read* rule to the simulation thread read; an admin (if admitted by D2)
    could submit into an allowlist channel without a View grant but never open the resulting thread.
  - D3 says "those two handlers (simulate page, simulation stream) switch to `Viewer`", but
    `load_simulation_thread_fragment` is reached from five handlers (page, POST, stream, open-thread
    GET and POST; `channel.rs:584`, `:855`, `:937`, `:1078`).
- **Required change:** add a per-handler level table to D2 that preserves each handler's current
  level (owner-only stays `owned_company`) unless a change is recorded with its reason; make D3
  name every entry point of the thread read and state which rule (viewer read vs. owner) governs the
  simulation thread.
- **Instruction lesson:** observed — the level mapping was written per page family rather than per
  handler. Candidate (scope: authorization plans): "a response/level policy change lists each
  handler with its current and new level."
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D2 per-handler table keeps simulation owner-only (`owned_company`) and preserves every other handler's level; D3 names all five simulation entry points and the nine `load_channel_thread` callers, and explains why the viewer-read rule adds no restriction for an owner (`Channel::viewer_access`: owner → true, verified `domain/entities/channel.rs:179-181`).

## FIXAPP-P04-G1-F5 — Child-entity reads in the rewritten handlers still turn DB errors into decisions

- **Classification:** actionable defect (fix or record an explicit scope boundary); severity
  **medium**; category requirements (Outcome table: "Database error → an error, never a
  decision"). Originates in D2 (scope).
- **Evidence:** D2 rewrites the company-level read in `routes/{agent,channel,company,company_invite}`
  but leaves, in the same handlers: `if let Ok(Some(ag)) = get_company_agent(..) else "Agent not
  found"` (`agent.rs:362`, `:385`); `if let Ok(Some(wf)) = get_company_channel(..)`
  (`channel.rs:407`, `:436`); `_ => "Channel not found" / "User not found"` (`channel.rs:569`,
  `:573`, `:726`, `:818`, `:1075`); `if let Ok(Some(invite))` (`company_invite.rs:185`, `:202`);
  `.unwrap_or_default()` on authorized lists after create/update (`agent.rs:298`, `:331`, `:339`;
  `channel.rs:299`, `:384`; `company_invite.rs:129`, `:134`, `:160`, `:168`). Each answers a DB
  outage as "not found" or an empty list. `src/AGENTS.md` ("Don't collapse errors into defaults on
  authorization paths") applies to the scoped reads (`get_company_agent`, `get_company_channel`
  authorize the caller).
- **Required change:** state in D2 that authorization-bearing reads in every handler D2 rewrites
  propagate with `?` (or an explicit logged `match`), or record which are deliberately left and why.
- **Instruction lesson:** cause unknown; the plan lists only company-level instances.
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D2 child-read rule: authorization-bearing `if let Ok(Some(..))`, `_ => "… not found"` and `.unwrap_or_default()` reads in the rewritten handlers become `?` + explicit `NotFound`; `let _ =` deletes become `?`; only business-validation alerts stay inline.

## FIXAPP-P04-G1-F6 — Skill-library operator authorization stays in HTTP

- **Classification:** actionable defect (extend D4 or record a justified disposition); severity
  **medium**; category requirements (Outcome 1; Decision "use cases that need it check it").
  Originates in D4.
- **Evidence:** `SkillUseCases::{list_library_page, get_library, create_library, update_library,
  delete_library}` (`src/application/use_cases/skill.rs:311-337`) check nothing; eleven
  `skill_library.rs` handlers gate them with `operator(&user, &users, &config)` (`:595`, `:630`,
  `:645`, `:660`, `:671`, `:684`, `:711`, `:729`, `:741`, `:824`, `:844`). D4 moves only the
  `AgentUseCases` library methods behind `&Caller`, and says the skill-library handlers "take the
  `Caller` extractor" — i.e. they would keep deciding in HTTP. The dashboard's global-rollup gate
  (`ui_dashboard.rs:202-207`, `DashboardSnapshotService::snapshot(None)`) and `monitoring.rs` also
  remain handler checks; that is acceptable only if recorded as the plan's "no use case exists"
  case.
- **Required change:** give `SkillUseCases` library management the same `&Caller` check and
  non-operator test as the library-agent use cases, and record the disposition for monitoring and
  the dashboard rollup (handler checks `caller.require_operator()` because no use case exists).
- **Instruction lesson:** observed — the plan's "library-agent use cases" wording was read
  literally although the skill library is the identical pattern. No instruction change supported.
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D4: `SkillUseCases` library create/update/delete and the operator-only library reads take `&Caller`; non-operator test extended to a library skill write; monitoring and the dashboard rollup recorded as handler checks of the caller property because no use case owns those reads (scope boundary toward 07); `skill_library::operator` deleted.

## FIXAPP-P04-G1-F7 — Onboarding idempotency key conflates a retry with a later re-onboarding

- **Classification:** actionable defect; severity **high**; category correctness / architecture
  (idempotency key, costly to unwind once agents carry derived ids). Originates in D6.
- **Evidence:**
  - D6 derives each agent id from `(company, library id | custom slug)` and the channel command key
    from the agent id (`onboarding:{agent_id}`), and replays the stored command result.
  - `workflow_provisioning_commands` rows are keyed `(company_id, command_key)` with no foreign key
    to the agent or channel (`init_schema.sql:5440-5450`), so they outlive both. Agent deletion
    removes the row (`adapters/persistence/agent.rs:363-401`), so a later onboarding of the same
    selection re-inserts an agent with the same id, finds the old command row with a matching
    digest, and **replays a result naming a channel that no longer exists**: no channel is created
    and the completion page shows a dead address. Today's random ids cannot hit this.
  - The digest includes `caller` and `agent_slug` (`provisioning.rs:917-929`). After an ownership
    transfer, or after the reused agent's slug was renamed, the replay is
    `Err(Conflict("This command key was already used for a different request"))`; D6 treats a
    command-step error as aborting the whole transaction, so that selection can never be onboarded
    again by the new owner.
  - `src/application/AGENTS.md`: the idempotency key is "derived from the logical operation, not
    the attempt". A retry of one submission and a new onboarding weeks later are different logical
    operations; D6 gives them the same key.
- **Required change:** define the logical operation and its key (e.g. a per-submission onboarding
  token minted when the channel step renders and carried in the form, with ids and command keys
  derived from it; or an onboarding record per company/selection that is retired when its agents
  are deleted; or a replay that verifies the stored channel still exists for this agent and
  otherwise provisions anew), and state the behaviour for a deleted agent, a renamed agent and a
  transferred company. Add DB tests for the deleted-agent and renamed-agent cases.
- **Instruction lesson:** hypothesis — deterministic ids were chosen to satisfy the plan's
  "retry is idempotent" test without modelling what else maps to the same key. Candidate (scope:
  `src/application/AGENTS.md` idempotency rule): "state which later, distinct operations share the
  key and what they observe."
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D6: per-submission `onboarding_key` minted by the channel-step GET (hidden field, required, UUID-validated); agent ids `UUIDv8(SHA-256("p04.onboarding" ‖ company ‖ key ‖ role))`, role `library:{id}` / `custom`; a later visit mints a new key, so deleted/renamed/transferred cases never replay an old command row; a replay digest mismatch is a per-agent refusal, not an abort. Edge case under one key logged as optional FIXAPP-P04-G1-F20.

## FIXAPP-P04-G1-F8 — Onboarding's new concurrency protocol has no competing-claimants test

- **Classification:** actionable defect; severity **medium**; category verification. Originates in
  D6's verification plan.
- **Evidence:** root `AGENTS.md`: "When adding or changing a concurrency protocol, include a test
  with competing claimants rather than relying only on sequential mocks." D6 introduces one
  (company row `FOR NO KEY UPDATE` + deterministic ids + command replay as the idempotency
  protocol), but its tests are sequential only ("same selection twice"). The "channel error after
  two agents" test does not say how a provisioning *error* (not a refusal) is induced; fix-app-C002
  requires injected statements to `expect` success so a refusal cannot pass as the expected error.
- **Required change:** add a DB test running two `onboard` calls with the same selection
  concurrently (one agent set, one channel per agent, both calls succeed with the same page, no
  error), and specify the fault-injection mechanism for the channel-error test (e.g. an
  `own_database` trigger raising on the channel insert, asserting it was hit).
- **Instruction lesson:** observed — the repository rule exists and was not applied in the
  verification plan. No new instruction supported.
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D6 states the concurrency protocol (same key serializes on the company row; second sees committed agents and command rows), specifies fault injection (`BEFORE INSERT` trigger on `channels` raising `P0001`, `.expect()`ed, `own_database`; `provision_guarded` re-checks `taken_slug`, which stays `None`, so the error propagates), and adds a competing-claimants test.

## FIXAPP-P04-G1-F9 — In-transaction re-check is weaker than the onboarding gate

- **Classification:** actionable defect; severity **low**; category security / consistency.
  Originates in D6.
- **Evidence:** D6 gates with `owned_company` (owner only) outside the transaction, then re-checks
  inside with `authority::authorize_company`, which admits owner **or admin**
  (`workflow/authority.rs:68-70`). An ownership transfer between the two leaves the transaction
  authorizing a caller the gate would refuse. D6 also lists `NotFound` among "user-fixable"
  failures shown on the draft page; `authorize_company`'s `missing()` is a `NotFound`, so an
  in-transaction authorization loss would render as a form error instead of 404.
- **Required change:** re-check owner inside the transaction (or justify admin), and map the
  in-transaction authorization failure to the 404/403 policy, not the draft page.
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D6 port step 1: owner check under the company lock; non-owner → `Forbidden` returned as an error (403), not a draft-page message.

## FIXAPP-P04-G1-F10 — No bound on agents per onboarding transaction

- **Classification:** actionable defect; severity **low**; category robustness (`src/AGENTS.md`
  "Bound work at every external boundary"). Originates in D6.
- **Evidence:** the selection is bounded only by the library listing (`list_library` `LIMIT 200`,
  `adapters/persistence/agent.rs:345-353`). D6 runs every agent insert, skill copy and workflow
  provisioning (compile, publish, binding) in one transaction holding the company row
  `FOR NO KEY UPDATE`, which serializes authority changes, MCP configuration and provisioning for
  that company for the transaction's duration.
- **Required change:** state a cap on agents per onboarding (reject above it as a `Rejected`
  message) and note the lock-hold consequence.
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D6 bound: at most 10 library agents per submission, rejected as a draft-page message above it.

## FIXAPP-P04-G1-F11 — Retried custom agent pays for a discarded provider call; refactor keeps an error collapse

- **Classification:** actionable defect; severity **low**; category requirements
  (`src/application/AGENTS.md`, `src/AGENTS.md`). Originates in D6.
- **Evidence:** D6 says a retried custom agent "re-runs prompt generation before the transaction;
  the stored agent is kept as is" — a provider call whose output is discarded on every retry,
  contrary to "a retry [must not] pay for a second provider call whose output can no longer be
  stored". Separately, the model fallback D6 factors out of `create_agent_from_library` uses
  `validate_model_selection(..).await.is_err()` (`use_cases/agent.rs:366-372`), which reads a DB
  error as "model unavailable" and silently falls back.
- **Required change:** look up the derived agent id before generating a prompt (the transaction
  still re-checks), and make the factored `prepare_library_copy` propagate non-validation errors.
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D6 step 3 `existing_agents(company, ids)` before preparing writes (no second prompt generation for an existing custom agent); `prepare_library_copy` treats only `BadRequest` from `validate_model_selection` as "model unavailable" (verified that the disabled-model answer is `BadRequest`, `use_cases/agent.rs:266-273`).

## FIXAPP-P04-G1-F12 — Status-code responses on base_layout htmx fragments become silent no-ops

- **Classification:** actionable defect; severity **low**; category UX requirement
  (`src/adapters/http/pages/AGENTS.md`: "On an in-place failure, restore the controls, clear the
  busy state, and show an actionable error"). Originates in D2.
- **Evidence:** D2 turns the 200 "Company not found." alerts of the `/companies/…` htmx fragments
  (`agent.rs`, `channel.rs`, `company.rs`, `company_invite.rs`) into 404/403/5xx. Those pages render
  through `base_layout`; the shared `htmx-config` does not swap 4xx/5xx (`pages/layout.rs:76`), and
  only `ui_layout` carries the request-error toast and `htmx:responseError` handler
  (`pages/mailbox.rs:91`, `:695`). A legitimate owner hitting a DB error would see nothing.
- **Required change:** decide the visible failure path for base_layout fragments (e.g. share the
  request-error toast with `base_layout`) and cover it with a page test.
- **Resolution history:**
  - 2026-11-01 (G1 r1): open.
  - 2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): **fixed in decision.** D2: `base_layout` gets the shared `REQUEST_ERROR_TOAST` and `REQUEST_ERROR_SCRIPT` with a page test. Note for implementation: the toast's dismiss button relies on the mailbox's delegated `data-action` click handler (`pages/mailbox.rs:888-899`); base_layout needs that case too (auto-hide after 10 s works regardless) — folded into FIXAPP-P04-G1-F20.

---

## Optional suggestions (G1 r1; not acceptance-blocking)

### FIXAPP-P04-G1-F13 — Record the workflow-authority boundary of "responses are consistent"

- Optional suggestion; low; requirements clarity. Workflow routes keep their own authority and map
  `NotFound` to refusals (`workflows/ui/typesafe.rs:41`, `workflows/decisions.rs:210`,
  `workflows/bindings.rs:91`); D2 does not change them. State that boundary in TASK so the 403
  policy is not read as global. Status: open (2026-11-01).
  2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): Adopted: D2 records the workflow-engine authority boundary (`application/workflow/authorization.rs`, file verified). Closed.

### FIXAPP-P04-G1-F14 — Notifications still call persistence from HTTP

- Optional suggestion; low; layering. `routes/notifications.rs` calls `NotificationPersistence`
  with HTTP-assembled scope (`:53-66`, `:79-96`, `:137-185`), the pattern D5 removes for attention. Route it
  through a use case or record why not. Status: open (2026-11-01).
  2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): Adopted as a recorded scope boundary in D5 (notifications' own persistence calls deferred to 07 or a later plan). Closed (disposition: deferred, reason recorded).

### FIXAPP-P04-G1-F15 — D5 needs a readable-channel *list* helper, not D3's single-channel one

- Optional suggestion; low; design detail. `read_context` uses `list_readable_channels`; D3's free
  function is per channel. Name the list helper D5 uses. Status: open (2026-11-01).
  2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): Adopted: D3 names `readable_channels(…)` serving `list_readable_channels` and D5. Closed.

### FIXAPP-P04-G1-F16 — Reconcile "26 sites" and widen thread-read verification

- Optional suggestion; low; records/verification. HEAD has 23 `CompanyUseCases::get_company`
  adapter sites; the plan's 26 evidently counted three scoped `SkillUseCases::get_company` calls
  (`skill_library.rs:355`, `:391`, `ui_agents.rs:581`) — record it. Add the other
  `load_channel_thread` callers to the WB1 filters (`ui_attachments`, `ui_internal_notes`,
  `ui_message_diagnostics`), list the plan's named tests (201 companies; owner/admin/member/none;
  404 body without company name; member 403; failing double 500), and per
  `src/adapters/http/AGENTS.md` a foreign-tenant id test for each changed route. Domain-separate the
  deterministic id inputs (`library:` / `custom:`) if F7 keeps derived ids. Status: open
  (2026-11-01).
  2026-11-01 (G1 r2, round FIXAPP-P04-G1-R2, decision copy `logs/G1-r2-decisions.md`): Partly adopted: thread-read filters widened (`ui_attachments`, `ui_internal_notes`, `ui_message_diagnostics`), plan route tests named, a foreign-tenant 404 test per handler family, `library:`/`custom` role separation. Not recorded: the 26-vs-23 reconciliation and an explicit mention of batch 1's `company_access_*` DB tests (201 companies; owner/admin/member/none) — still optional here; the WB1 review will require those plan tests regardless. Closed as optional.
  2026-11-01 (G1 r3, round FIXAPP-P04-G1-R3, decision copy `logs/G1-r3-decisions.md`, freeze digest `4f458bd25090bae457d310e858875fba1211f2f7eafa94510b16728d51e1e7ed`): Remainder adopted: Verification plan names batch 1's `company_access_*` DB tests (201 companies; owner/admin/member/no-relation) and records the 23 + 3 site count. Fully closed.

---

## Findings from the G1 r2 recheck

Context as above, except: review round [FIXAPP-P04-G1-R2](agent-review-notes.md), decision
revision **r2** (`logs/G1-r2-decisions.md`, byte-identical to TASK.md at review start). The
artifact digest was `c325eea2…3462` at the start of the round and **changed during it** (see F17).

### FIXAPP-P04-G1-F17 — Implementation edits started before the G1 pass; artifact changed mid-recheck

- **Classification:** actionable (process/records); severity **low**; category process. Not a
  defect in the decisions.
- **Evidence:** at round start `snapshot.sh` gave `c325eea2f3e71d55bbb4f3c5acdfb0df01515171e063db16d2366218d4aa3462`
  with no files; during the recheck `git status` showed `M src/adapters/persistence/company.rs` and
  `M src/application/use_cases/company.rs` (mtime 10:49:20), and `snapshot.sh` now gives
  `1da491129a47537d970a6f06d189f6d4b163dca6ae119e59af569828a7eafdee` (2 files, +77/−73: D1's
  trait-default removal). Final observation before return: the edits were still in progress — 13 files (`app_error_impl.rs`, `persistence/company.rs`, `app_error.rs`, `inbound_event_worker.rs`, `agent_tests.rs`, new `use_cases/caller.rs`, `use_cases/company.rs`, new `use_cases/company_test_support.rs`, `use_cases/mod.rs`, `thread/mod.rs`, `thread/test_support.rs`, `use_cases/user.rs`, `domain/entities/user.rs`), digest `cef53723e33366a6aca07ba97c005a88651272aa0e8adb18cef0ca1d01cc822b`. SKILL.md: "obtain B's explicit pass for the current decision revision
  before dependent edits"; the assignment pinned the r2 recheck to the unchanged artifact.
- **Required:** record in TASK/HANDOFF that WB1 edits began before the G1 pass (start time,
  digest), keep them unreviewed until the WB1 batch review against a fresh freeze, and unwind any
  part a G1 correction changes. The edits were not reviewed in this round.
- **Instruction lesson:** observed. Candidate (scope: SKILL.md architecture gate): "while a gate
  recheck is pending, the author works only on gate-independent tasks; root re-freezes before
  requesting the recheck if anything changed."
- **Resolution history:**
  - 2026-11-01 (G1 r2): open.
  - 2026-11-01 (G1 r3, round FIXAPP-P04-G1-R3, decision copy `logs/G1-r3-decisions.md`, freeze digest `4f458bd25090bae457d310e858875fba1211f2f7eafa94510b16728d51e1e7ed`): **fixed (records).** TASK Status "Process deviation (F17)" records the start (~10:50), the mid-round digest change and that the edits stay unreviewed until the WB1 freeze and are unwound where a G1 correction changes them; the "G1 r3 freeze" section lists the 14 in-progress files, matching `snapshot.sh` exactly; the author committed to no code edits until this recheck returned (digest unchanged at start and return). Closed.

### FIXAPP-P04-G1-F19 — Explicit-id rule (F3) breaks the operator's company dashboard

- **Classification:** actionable defect; severity **low**; category correctness (interaction
  introduced by r2's D2 F3 rule with D4's dashboard boundary).
- **Evidence:** `DashboardScope::resolve` (`ui_dashboard.rs:161-195`) calls
  `load_managed_company(.., requested)` **before** consulting the operator flag, then, for an
  operator with `Some(company_id)`, reads that company directly because "an operator is not usually
  a member of the companies they are watching" (comment `:171-178`). r2 makes
  `load_managed_company(Some(id))` resolve through `managed_company` (404/403), so for an operator
  watching a company they do not manage the `?` returns 404/403 before `operator_company` is ever
  reached — today's operator company view stops working. No existing test covers
  operator + `company_id` (dashboard tests at `ui_dashboard.rs:522-620` exercise only
  `load_runtime_snapshot`, `load_query_health` and `authorized_company_id`).
- **Required change:** state in D2/D4 that the dashboard resolves a requested company as
  `operator_company(&caller, id)` for an operator and `managed_company` otherwise (the switcher
  list still from `list_managed_companies`), and add a test: operator, non-managed `company_id` →
  that company's dashboard; non-operator member → 403; stranger → 404.
- **Instruction lesson:** cause: a correction to one finding (F3) was applied uniformly to a helper
  with a caller that intentionally bypasses it. No new instruction beyond R1's candidate 1.
- **Resolution history:**
  - 2026-11-01 (G1 r2): open.
  - 2026-11-01 (G1 r3, round FIXAPP-P04-G1-R3, decision copy `logs/G1-r3-decisions.md`, freeze digest `4f458bd25090bae457d310e858875fba1211f2f7eafa94510b16728d51e1e7ed`): **fixed in decision.** D2 "Dashboard (r3, F19)": `DashboardScope::resolve` checks the caller first; operator → `operator_company(&caller, id)` (managed list only for the switcher); everyone else → `managed_company` (member 403, stranger 404); the three tests are named. Closed (implementation verified at WB1).

### FIXAPP-P04-G1-F18 — Optional: say how `PUT /companies/{id}` answers an authorization failure

- Optional suggestion; low; requirements clarity. The D2 table routes the HTML update through
  `update_company_for_user` (which now authorizes), while the D2 child-read rule keeps
  "Update failed: …" write alerts inline (`routes/company.rs:177-180` renders every `Err` as a 200
  alert). Unless the handler calls `owned_company` first (as the edit form does) or maps
  `NotFound`/`Forbidden`/5xx to status, a stranger or member gets a 200 alert rather than 404/403.
  The WB1 review will treat a 200 authorization answer there as a defect. Status: open
  (2026-11-01).
  2026-11-01 (G1 r3, round FIXAPP-P04-G1-R3, decision copy `logs/G1-r3-decisions.md`, freeze digest `4f458bd25090bae457d310e858875fba1211f2f7eafa94510b16728d51e1e7ed`): Adopted in D2 "PUT /companies/{id} (r3, F18)": `owned_company` before the form is processed; only post-authorization write failures stay inline. Closed.

### FIXAPP-P04-G1-F20 — Optional: onboarding key edge and toast dismiss wiring

- Optional suggestion; low. (a) With role `custom` (not `custom:{slug}`), re-posting the same
  `onboarding_key` with an edited custom name after a refusal kept the agent silently reuses the
  earlier custom agent; including the custom slug in the role (or re-minting the key when the
  completion page renders) avoids it. (b) The shared toast's dismiss button depends on the
  mailbox's delegated `data-action` handler (`pages/mailbox.rs:888-899`); `base_layout` needs that
  case for F12's page test to cover dismissal. (c) The new `onboarding_key` form field shares its
  name with the existing `fn onboarding_key(agent)` command-key helper (`routes/onboarding.rs:376`,
  `:395`); rename one. Status: open (2026-11-01).
  2026-11-01 (G1 r3, round FIXAPP-P04-G1-R3, decision copy `logs/G1-r3-decisions.md`, freeze digest `4f458bd25090bae457d310e858875fba1211f2f7eafa94510b16728d51e1e7ed`): Adopted: role `custom:{normalized slug}`, form field renamed `submission_key`, delegated `dismiss-request-error` handler shared with `base_layout`. Closed.


## FIXAPP-P04-G1-F11 — Reopened follow-up: competing retries still discard a paid prompt

- **Date / identity:** 2026-11-04; workflow `implement-astra-reviewed-2-2`; phase p04,
  architecture gate G1 r4; review round [FIXAPP-P04-G1-R4](agent-review-notes.md#fixapp-p04-g1-r4).
  Reviewer B `/root/p04_reviewer`, requested Astra/high; actual runtime model/effort not exposed.
  Worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p04`, branch `fix-app/p04`.
  Start/base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; source digest
  `c5b583876682c317238f7a32d04fd2ba70ff220b40a87cc6b8c394daeea893bc`, sole exclusion
  `plan/fix-app/phases/p04/docs`. Decision [G1 r4](logs/G1-r4-decisions.md), SHA256
  `90714b8edd91eb82dbff5f9c401fb02d8562b51e7d535b0c01fdb19fc8f9a245`.
- **Classification:** actionable defect; severity **medium**; architecture / concurrency.
  Reopens the prompt-generation half of F11 only; the model-validation error-propagation
  correction remains accepted in decision. This is a defect in D6, not a claim about finished code.
- **Evidence:** D6 lines 210–217 check `existing_agents` and generate a missing custom agent's
  prompt before the port transaction. Serialization begins only at lines 220–225, and an
  existing id then reuses the winning agent. The allowed schedule is: A and B carry identical
  company/key/custom input; both read no existing agent; both invoke the prompt provider;
  A locks/inserts/commits; B locks and reuses A's agent, discarding B's generated prompt.
  The row-count/identical-result test at lines 248–249 still passes. Thus the statement at
  lines 210–212 that a retried custom agent does not pay for generation again holds only after
  the earlier request commits, not for the explicitly supported concurrent same-key retry.
  `src/application/AGENTS.md:17–25` requires that a retry not pay for a second provider call
  whose output cannot be stored. The existing agent lookup does not own preparation.
- **Impact:** double submits or overlapping network retries can make duplicate provider calls
  and discard billed results, despite the claimed onboarding retry contract. Database atomicity
  and agent/channel idempotency alone do not prevent this.
- **Required correction:** revise D6 to define preparation ownership/reuse for the same logical
  custom-agent operation before invoking the provider, with bounded waiting/cancellation and
  failure behavior, while retaining the agent/channel transaction's rollback guarantee. Specify
  a competing same-key custom-submission test using a controlled counting prompt provider that
  forces the overlap and asserts one preparation, one agent/channel set, and identical results.
  Any chosen schema/port/locking changes belong in the revised gate; this finding does not
  prescribe an implementation or authorize a weaker contract.
- **Instruction lesson:** the R2/R3 closure checked an existence preflight but did not cover the
  external effect before the transaction lock. Candidate, scope concurrency review: trace each
  irreversible/provider effect through the entire competing-claimant schedule, and assert the
  effect count in addition to final durable row counts.
- **Resolution history:** original F11 and historical closures above are preserved. Reopened
  2026-11-04 against G1 r4; unresolved. The fresh review invalidates no previously accepted code
  because WB1/WB2 code acceptance has not occurred.


### FIXAPP-P04-G1-F11 — R5 correction disposition

2026-11-04, round [FIXAPP-P04-G1-R5](agent-review-notes.md#fixapp-p04-g1-r5): **fixed in
 decision r5** (SHA256 `176ddf0a29a0cf9ca5b41358f4ccdf7783af6ae21c031c3475737de52e5c06c2`).
D6 begins and retains the company-locking session before the existing-agent lookup and provider
invocation; a committed winner therefore suppresses its same-key waiter's generation. Counting
provider/competing custom-submission and cancellation tests are specified. Bounded waits and
crash/cancellation billing limitations are explicit. No implementation pass is implied. The new
retained-session contract introduces F21 below, so G1 r5 as a whole remains changes required.

## FIXAPP-P04-G1-F21 — Retained onboarding transaction waits for a second pool connection

- **Identity:** 2026-11-04; workflow `implement-astra-reviewed-2-2`; p04 architecture gate G1 r5;
  round [FIXAPP-P04-G1-R5](agent-review-notes.md#fixapp-p04-g1-r5). Reviewer
  `/root/p04_reviewer`, requested Astra/high, actual runtime settings not exposed. Worktree
  `/Users/mac03/dev/openbot/.worktrees/fix-app-p04`, branch `fix-app/p04`; start/base/HEAD
  `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; digest
  `510dba5e4bd0cdf496b59c6fcacbc424a02e04721fe9fbd826e30cb3d8aaaef4`, sole exclusion
  `plan/fix-app/phases/p04/docs`. [Decision r5](logs/G1-r5-decisions.md), SHA256
  `176ddf0a29a0cf9ca5b41358f4ccdf7783af6ae21c031c3475737de52e5c06c2`.
- **Classification:** actionable defect; **medium**; architecture / concurrency / availability.
  Originates in the new D6 session contract, not completed implementation.
- **Evidence:** D6 lines 200–215 retain a pool-backed transaction while production prompt
  preparation delegates to `AgentUseCases::generate_system_prompt`. That method at
  `src/application/use_cases/agent.rs:590–640` calls `verify_company_manager`,
  `list_model_connections` and `model_api_key`; those Postgres methods acquire from `self.pool`
  (`src/adapters/persistence/company.rs:222–241`, `:299–344`). The library preparation to be
  extracted from `create_agent_from_library` also resolves/validates models through that pool
  (`agent.rs:348–412`, `:252–279`). These are additional acquisitions while the session already
  owns a connection. The stated one-connection tradeoff (D6:245–247) omits them.
- **Failure schedule:** N sessions consume an N-connection pool. For same-company contenders,
  one owns the company lock, the rest await that lock while holding their connections. The
  owner now calls the existing prompt method and awaits another pool connection. It cannot
  complete/release its lock, and its waiters cannot release theirs until a timeout breaks the
  cycle. Different-company sessions can similarly all wait for their second connection.
  This turns bounded ordinary contention into avoidable 5xx failures and stalls unrelated pool
  work. Existing acquisition timeout (10s default, `src/infra/db.rs:57–70`) bounds the stall;
  it does not give the protocol a progress path. A small isolated pool can reproduce it without
  saturating the machine or involving a provider.
- **Required correction:** define how all preparation inputs are obtained without acquiring a
  second connection from the shared pool while an onboarding session is retained. Cover both
  custom prompt and library model/fallback preparation while preserving under-lock authority,
  the F11 provider ownership ordering, and application/adapter dependency direction. Do not
  fix this only by enlarging the pool. Add a constrained-pool progress test covering the real
  production preparation path (a counting classifier/provider is fine), since replacing the
  entire generator with a DB-free mock hides the nested acquisition.
- **Instruction lesson:** `src/infra/AGENTS.md` requires accounting for dedicated connections
  when sizing bounded pools. Candidate scope, transaction/session gates: inventory every
  database acquisition made while retaining a transaction, including through application helper
  calls; test progress with constrained capacity, not just durable row-count idempotency.
- **Resolution history:** open against r5. F11's original race is fixed in decision separately;
  no requirement to repeat unchanged D1–D5/D7 review.


### FIXAPP-P04-G1-F21 — R6 correction disposition

2026-11-04, round [FIXAPP-P04-G1-R6](agent-review-notes.md#fixapp-p04-g1-r6): **fixed in
 decision r6**, SHA256 `c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b`.
D6 requires model configuration and selected secret reads on the retained session connection,
shared pure library/custom preparation helpers, and provider-only invocation from resolved inputs.
It explicitly forbids invoking the pool-reading `generate_system_prompt` or other pool-backed
helpers after begin. Completion/library-skill/provisioning reads and writes also use that same
connection. The new one-connection test exercises the production preparation path with only the
TextClassifier replaced and includes concurrent callers. This removes the nested-acquisition
wait cycle without weakening F11 ownership, rollback or authorization; existing 30/45/90s bounds
and explicit crash billing limits remain. Closed in decision; implementation and tests await WB2.
No unresolved acceptance-blocking architecture findings remain for r6.


### FIXAPP-P04-G1-R7 — Continuity disposition (no new findings)

2026-11-04, [round note](agent-review-notes.md#fixapp-p04-g1-r7): unchanged G1 r6 SHA256
`c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b` remains applicable under
coordination r15. F11 and F21 remain fixed **in decision**; original entries/dispositions are preserved.
No new actionable defects or optional suggestions. Current frozen source
`a366d1d6eaba053c6a66ad52a798a073ec14e8a9cecd5903c3a22840d504e092` remains WIP/unaccepted.
WB1/WB2 implementation and tests require their respective whole-batch reviews.


## FIXAPP-P04-WB1-F1-MAC02 — Team mutations turn authorization errors into successful fragments

- 2026-11-04; workflow `implement-astra-reviewed-2-2`; p04 WB1 (plan batches 1–4); round [FIXAPP-P04-WB1-R1-MAC02](agent-review-notes.md#fixapp-p04-wb1-r1-mac02). Reviewer `/root/p04_reviewer`, requested Astra/high, actual runtime settings unavailable.
- Worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p04`, branch `fix-app/p04`; start/review base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; reviewed digest `c4983d97a745723104fcdf8a4a3d1804739e1863d72721fe14e03e901b55b3e3`, sole exclusion `plan/fix-app/phases/p04/docs`; unchanged G1 r6. [Task](TASK.md), [review](REVIEW.md).

- **Classification:** actionable defect; **medium**; correctness / requirements / authorization response handling. Implementation defect, not a change needed to G1.
- **Evidence:** `src/adapters/http/routes/ui_team.rs:198–202` now admits any member through `readable_company`. `create_invite:390–421` then calls the owner-checked application mutation but its catch-all `Err(err)` returns `Ok(Html(...).into_response())` (HTTP 200). `update_member_role:478–508` and `remove_member:512–534` likewise catch the owner's `Forbidden` and render an ordinary member pane with 200 when the target is an existing teammate. No write is authorized, but the transport violates phase batch 4 / G1 D2's required 403 for a visible company and non-owner action. This affects both admins and ordinary members.
- **Reproduction by inspected control flow:** with the new authorization fixture, call `create_invite(workspace(&f, f.member.id), Path(f.company.id), Form(valid invite))`; company scope succeeds, `CompanyInviteUseCases::verify_company_owner`/`owned_company` yields `Forbidden`, and the handler returns a successful fragment. Change/remove an existing teammate follows the same schedule. The new `ui_team_authorization_tests.rs:16–49` tests only `invite_form_pane` (which explicitly calls `require_owner`), so the passing union does not exercise these mutation responses. No runtime reproduction by B is claimed.
- **Related error path:** these same catch-all mutation branches also hide a database/internal failure as 200 if the initial readable-company lookup succeeds (e.g. healthy CompanyUseCases but failing company port in CompanyInviteUseCases). `update_invite:425–459` has the same catch-all write branch after its initial owner-scoped read. Restrict inline validation/business refusals to their intended types and preserve authorization/missing/infrastructure errors as typed HTTP failures; do not only patch the form-opening check.
- **Required correction/verification:** make non-owner team mutations return 403, foreign/unknown scopes return their concealed 404, and database authorization errors propagate. Add focused mutation tests for admin/member, foreign scope, and a failure after initial readable scope; keep valid owner's business-validation feedback. Review all team mutation response branches together.
- **Instruction lesson:** existing plan and HTTP error requirements already specify the contract. Candidate for authorization reviews: when broadening a shared read guard from owner-only to member-visible, follow every caller through its mutation error mapper, and test a mutation as well as its form-opening route. Evidence is the changed `scoped_company` plus three reachable catch-all branches.
- **Resolution history:** open against WB1 digest above; acceptance-blocking until corrected and rechecked.

## FIXAPP-P04-WB1-F2-MAC02 — New layout test fails the required Clippy gate

- 2026-11-04; workflow `implement-astra-reviewed-2-2`; p04 WB1 (plan batches 1–4); round [FIXAPP-P04-WB1-R1-MAC02](agent-review-notes.md#fixapp-p04-wb1-r1-mac02). Reviewer `/root/p04_reviewer`, requested Astra/high, actual runtime settings unavailable.
- Worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p04`, branch `fix-app/p04`; start/review base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; reviewed digest `c4983d97a745723104fcdf8a4a3d1804739e1863d72721fe14e03e901b55b3e3`, sole exclusion `plan/fix-app/phases/p04/docs`; unchanged G1 r6. [Task](TASK.md), [review](REVIEW.md).

- **Classification:** actionable defect; **low**; verification/build.
- **Evidence:** `src/adapters/http/pages/layout.rs:695` adds `page.contains(&crate::adapters::http::routes::assets::app_js_url())`. Author's fresh [Clippy log](logs/mac02-WB1-clippy.log) reports `clippy::needless_borrows_for_generic_args` with `-D warnings`, failing lib-test compilation. This line belongs to the reviewed diff; failure is not inherited or unrelated.
- **Required correction/verification:** remove the needless borrow and rerun the required offline Clippy check. Preserve the substantive layout assertion; do not weaken lint policy.
- **Instruction lesson:** no new instruction supported; current required verification correctly catches this mechanical issue.
- **Resolution history:** open against WB1 digest above; required check remains failed.


### FIXAPP-P04-WB1-F1-MAC02 — R2 correction disposition

2026-11-04, [FIXAPP-P04-WB1-R2-MAC02](agent-review-notes.md#fixapp-p04-wb1-r2-mac02), digest
`c5debdeab77f80515c4a88ae2384af6503ae320e53cb48b4fc3dc3c87a2719da`: **FIXED**.
`inline_team_mutation_error` admits only BadRequest/Conflict; create/update invite, change role,
remove member and avatar error branches propagate all other variants before fallback rendering.
Direct ownership checks remain in application. The deliberate owner self-removal/demotion refusals
are now BadRequest after authorization; their HTML/JSON/direct callers were traced. No permission
or owner-list expansion. New tests exercise all three reported mutation paths for member/admin403,
foreign/unknown404 and typed DB500 after a healthy initial scope; reachable invalid invite
create/update keeps inline200 feedback. Existing owner-member pane404 and durable ownership remain
verified. [Red reproduction](logs/mac02-WB1-team-mutations-red-r2.log) failed both security/error
regressions before correction. [Final corrected union](logs/mac02-WB1-affected-r3.log) passes472,
fails0, ignores4. The intermediate owner-feedback test's mistaken expectation and its failure are
retained; correction matches the unchanged SQL `member.role <> 'owner'`, not a weakened auth assertion.

### FIXAPP-P04-WB1-F2-MAC02 — R2 correction disposition

2026-11-04, [FIXAPP-P04-WB1-R2-MAC02](agent-review-notes.md#fixapp-p04-wb1-r2-mac02), same correction
digest: **FIXED**. The layout test passes `app_js_url()` without a redundant borrow; substantive
assertions remain. [Offline Clippy -D warnings](logs/mac02-WB1-clippy-r2.log) passes for lib/tests.


### FIXAPP-P04-WB2-R1-MAC02 — Batch disposition

2026-11-04, independent B `/root/p04_reviewer`, requested Astra/high (actual runtime unavailable),
`implement-astra-reviewed-2-2`, [round note](agent-review-notes.md#fixapp-p04-wb2-r1-mac02).
Frozen source `9b1bfc76eb32f15cf6131b1ab55bebe375d49b2beb90b03d48a7dc71dcc29e91`,
start/base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`, same p04 worktree and sole docs
exclusion; coordination r16 and G1 r6 unchanged before/after.

**No new actionable defects or optional suggestions. PASS WB2 plan batches5–6 and current
whole-phase shared effects with accepted WB1.** G1-F11 and G1-F21 are now verified in
implementation: authorization and existing-ID detection occur under retained company locking
before provider work; all preparation database operations use that retained connection and the
provider-only callback does not reenter the pool. Actual lock-waiter concurrent retry and
one-connection production-composition tests pass, together with failure/abort/timeout rollback
and retry coverage. Historical decision findings and original dispositions remain preserved.
WB1-F1/F2-MAC02 remain fixed; affected union passes523/fails0/ignores4, all40 filters exercised;
offline all-target Clippy, formatting and whitespace pass. B inspected these author logs and
independently recounted filters, without rerunning fixture checks. No combined-main integration
or full-suite acceptance is implied.
