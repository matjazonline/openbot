# Reviewer notes — fix-app p04 (Authorization in the application layer)

Persistent review-round log for phase p04 (workflow `implement-opus-reviewed-2_2`). Entries are
appended; originals are preserved. Findings detail: [agent-review-findings.md](agent-review-findings.md).
Task record: [TASK.md](TASK.md). Review record: [REVIEW.md](REVIEW.md).

---

## FIXAPP-P04-G1-R1

### Identity

- Date 2026-11-01; workflow `implement-opus-reviewed-2_2`; stage: architecture gate **G1**, decision
  revision **r1** (D1–D7), covering WB1 (plan batches 1–4) and WB2 (plan batches 5–6).
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort as configured by the spawner (TASK states
  `xhigh`). Phase `p04`; worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p04`, branch
  `fix-app/p04`.
- Phase start = review base = HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; digest
  `c325eea2f3e71d55bbb4f3c5acdfb0df01515171e063db16d2366218d4aa3462`, exclusion
  `plan/fix-app/phases/p04/docs` (checked with `scripts/snapshot.sh` before the review and again
  before returning — unchanged; no code changes exist).
- Decision under review: `logs/G1-r1-decisions.md` (byte-identical to TASK.md at review start).
- Coordination: authority `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md`, revision 4,
  byte-identical to `logs/COORDINATION-r4.md` at review start and at return. Applicable:
  fix-app-I001 r1, I002 r1, I003 r1, I004 r1 (plan-wide), C002 r1, C003 r1 (target p04). C001,
  C004, C005 target other phases (not applicable). Freshness rechecked before returning: unchanged.

### Coverage and evidence

- Requirements derived from `plan/fix-app/04-authorization-in-application.md` (Outcome, response
  table, "What happens today", Decisions, Batches 1–6, Focused checks) and `plan/fix-app/README.md`
  (dependencies 01→04→07, concurrency and verification rules), plus `AGENTS.md`, `src/AGENTS.md`,
  `src/application/AGENTS.md`, `src/adapters/http/AGENTS.md`, `src/adapters/http/pages/AGENTS.md`,
  `src/adapters/smtp/AGENTS.md`, `src/adapters/persistence/AGENTS.md` (sections 1–7 and guards).
- Code read at HEAD (inspection, not tests): `use_cases/company.rs` (trait defaults, free functions,
  use cases, mock), `adapters/persistence/company.rs` (all impls), `entities/company_member.rs`,
  init-schema `company_members` keys/CHECK and the owner-agreement trigger/FK (C002), every
  `CompanyUseCases::get_company` adapter site (23 at HEAD; the plan's 26 includes three scoped
  `SkillUseCases::get_company` calls), `routes/{agent,channel,company,company_invite,onboarding,
  ui,ui_dashboard,ui_team,ui_companies,ui_agents,attention,notifications,monitoring,agent_library,
  skill_library}.rs` relevant handlers, `auth.rs` extractors, `infra/config.rs::is_operator`,
  `use_cases/{agent,skill,channel,company_invite}.rs` guards, `app_error.rs` and every exhaustive
  `AppError` match (`app_error_impl.rs`, `inbound_event_worker::classify_commit_error`) plus the
  `_`-armed classifiers (`harness/rig/workflow.rs`, `SmtpAnswer::for_error`, `channel_edit.rs`),
  `workflow/provisioning.rs` (`ProvisioningKey`, `provision_guarded`, `provision_on`,
  `AssistantChannelService`), `workflow/authority.rs::authorize_company`,
  `persistence/channel.rs` library copy (`prepare_library_agent_on`), `persistence/agent.rs` delete,
  `workflow_provisioning_commands` schema, `smtp/server.rs::sender_needs_spam_scan` and its mock,
  `thread/mod.rs` accessors, `thread/ingest/routing.rs::resolve_addresses`, `thread/support.rs::
  DirectoryCache`, `pages/layout.rs` htmx-config, `pages/mailbox.rs` request-error toast.
- Assumptions challenged and outcome:
  - Response policy per surface: company-level reads (D2) sound; update/delete company bypasses it
    with a persistence 500 (F1); list-scan selection helpers and `ui_team` undecided (F3);
    simulation surfaces' level unstated (F4); htmx fragments on `base_layout` lose visible failure
    feedback (F12); workflow-authority surfaces keep 404 (F13, optional).
  - "Extractor decides once, use cases check": holds for library agents (D4); skill library,
    dashboard rollup and monitoring still decide in HTTP without a recorded disposition (F6).
  - Contract change of the free functions to 403: `list_company_team_members` depends on the old
    `NotFound` (F2). Non-HTTP consumers of the affected use cases: none (only `infra/setup.rs` and
    routes), so the `_`-armed worker/harness classifiers are unaffected.
  - Onboarding: D6's trace is correct (`prepare_library_agent_on`, `insert_agent_on`,
    `create_agent_principal_on` take the transaction; `provision_guarded` savepoints refusals; lock
    order company → member/principal → agents/library rows → provisioning matches
    `create_assistant_channel`). Defects: idempotency key conflates retry with re-onboarding and
    replays stale command results (F7), no competing-claimants test / unspecified fault injection
    (F8), weaker in-tx re-check (F9), unbounded selection in one long transaction (F10), discarded
    provider call on retry and an error collapse kept in the factored helper (F11).
  - D1: one-row query is correct given `company_members_company_user_key` and C002; removing the
    list default for `list_accessible_by_user_id` is consistent; mock hoisting is feasible (six
    `impl CompanyPersistence` doubles confirmed; route tests call handlers with `State(..)`, so a
    failing double can be injected).
  - D3: one rule (viewer read + `thread.channel_id == channel.id`) is sound for the mailbox; entry
    point list incomplete for simulation (F4).
  - D5: policy sound (member TeamWork/summary → 403); list helper naming (F15, optional).
  - D7: preserves "unparseable → scan" and "lookup error → logged stranger", now also logging
    company/channel lookup errors; reusing `DirectoryCache`/`matches_slug` answers the plan's
    duplicate-loading complaint. No defect found.
- Checks run: none of cargo/DB (architecture gate, no code). Read-only greps recorded in
  [logs/G1-R1-reviewer-evidence.log](logs/G1-R1-reviewer-evidence.log). `snapshot.sh` run twice.
- Reviewed lightly (named so this round is not read as uniform): D5's metric recording and
  `attention_events` periodic re-check internals; D7's `DirectoryCache` identity resolution
  ("resolve without observing") beyond confirming it is the ingest path's helper; the thread-test
  mock's by-address heuristic (feasibility of the single `TeamFixture` enum was not traced).

### Outcome

- **Changes required** for G1 r1 (IDs `FIXAPP-P04-G1-F<n>` in [agent-review-findings.md](agent-review-findings.md)). Actionable: FIXAPP-P04-G1-F1 (high), F2 (medium), F3 (medium), F4 (medium), F5 (medium), F6 (medium), F7 (high), F8 (medium),
  F9 (low), F10 (low), F11 (low), F12 (low). Optional: F13–F16.
- No unresolved questions beyond the findings. A correction recheck should cover the revised
  decision text (r2) for D1/D2/D3/D4/D6 and the TASK verification plan; D5 and D7 evidence is
  reusable unless their text changes.

### Process and instruction feedback

- Batch split (G1 over WB1+WB2, then two batch reviews) was appropriate; G1 was necessary
  (authorization model, response policy and an idempotency/transaction protocol are costly to
  unwind). One review of all seven decisions stayed thorough.
- Missing gated content: no decision for `ui_team` or for the list-scan company helpers; no
  per-handler level table. The author's records were otherwise precise (decisions carry failure
  modes and test intent; D6's trace was verifiable line by line).
- Candidate instructions (for later analysis; not adopted):
  1. Scope: authorization plans/gates. "Derive the handler list from routes that accept a tenant id
     and list each handler's current and new level; do not inherit one method's call-site count."
     Evidence: F1, F3, F4.
  2. Scope: `src/application/`. "When a shared guard's error variant changes, grep for matches on
     the old variant and list them in the decision." Evidence: F2.
  3. Scope: `src/application/AGENTS.md` idempotency rule. "State which later, distinct operations
     map to the same idempotency key and what they observe (deleted/renamed resources, ownership
     transfer)." Evidence: F7.

---

## FIXAPP-P04-G1-R2

### Identity

- Date 2026-11-01; workflow `implement-opus-reviewed-2_2`; stage: architecture gate **G1
  correction recheck**, decision revision **r2** (D1–D7; `logs/G1-r2-decisions.md`, byte-identical
  to TASK.md G1 r2 at review start and at return; r1 preserved in `logs/G1-r1-decisions.md`).
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), same session as R1, effort as configured by the
  spawner. Phase `p04`; worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p04`, branch
  `fix-app/p04`; review base = HEAD `48626a1`.
- Artifact: at round start `snapshot.sh` = `c325eea2f3e71d55bbb4f3c5acdfb0df01515171e063db16d2366218d4aa3462`
  (no files). **Mismatch during the round:** the author began WB1 edits
  (`src/adapters/persistence/company.rs`, `src/application/use_cases/company.rs`, mtime 10:49:20);
  digest at return `1da491129a47537d970a6f06d189f6d4b163dca6ae119e59af569828a7eafdee` (2 files).
  Those edits were not reviewed (FIXAPP-P04-G1-F17). Final observation before return: the edits were still in progress — 13 files (`app_error_impl.rs`, `persistence/company.rs`, `app_error.rs`, `inbound_event_worker.rs`, `agent_tests.rs`, new `use_cases/caller.rs`, `use_cases/company.rs`, new `use_cases/company_test_support.rs`, `use_cases/mod.rs`, `thread/mod.rs`, `thread/test_support.rs`, `use_cases/user.rs`, `domain/entities/user.rs`), digest `cef53723e33366a6aca07ba97c005a88651272aa0e8adb18cef0ca1d01cc822b`. This round reviews the decision text r2 only.
- Coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` revision 4, byte-identical
  to `logs/COORDINATION-r4.md` at start and at return; applicable I001–I004 r1, C002 r1, C003 r1.

### Coverage and evidence

- Recheck by finding ID against the r2 text and HEAD code: every F1–F16 disposition read against
  the code it relies on (Postgres update/delete scope, `company_invite.rs:284` NotFound branch,
  `load_*_company` callers incl. `ui_dashboard`, `ui_team`, simulation handlers and
  `Channel::viewer_access` owner exemption, skill library methods, `workflow_provisioning_commands`
  replay and digest, `provision_guarded` error/`taken_slug` path under the proposed trigger,
  `authorize_company` locks, `validate_model_selection`'s `BadRequest`, base_layout vs.
  `REQUEST_ERROR_TOAST`/`REQUEST_ERROR_SCRIPT` and the delegated dismiss handler,
  `application/workflow/authorization.rs` existence).
- Broadened where r2 changed contracts: D2's explicit-id rule was traced through every
  `load_*_company` caller; it breaks the operator company dashboard (F19). D6's per-submission key
  was analysed for retry, re-onboarding, ownership transfer, rename, forged/malformed keys (company
  in the hash; UUIDv8 vs. v4 ids cannot collide), concurrent same-key and different-key
  submissions (READ COMMITTED re-read after the company row lock), and the refusal-kept edge (F20).
- Reused from R1 without change: D5 and D7 evidence (text unchanged except the F15 helper name).
- Checks run: none of cargo/DB (decision gate); `snapshot.sh` ×3, `git status`, read-only greps.
  The author's in-progress diff was only identified (`git diff --stat`, first hunk), not reviewed.

### Outcome

- **Changes required (minor)** for G1 r2. F1–F12 fixed in decision; F13–F15 adopted; F16 partly
  adopted (closed as optional). New: FIXAPP-P04-G1-F19 (low, actionable: operator dashboard
  regression from the F3 rule), FIXAPP-P04-G1-F17 (low, actionable process/records: edits before
  the gate pass, artifact changed mid-recheck). Optional: F18 (HTML company update response),
  F20 (custom role, toast dismiss wiring, name clash).
- A recheck needs only: the D2/D4 dashboard sentence and test (F19), the TASK/HANDOFF record for
  F17, and a fresh freeze identifying the in-progress WB1 edits as unreviewed. All other r2
  decisions are accepted as written.

### Process and instruction feedback

- The correction round was thorough and well cited (every change carries its finding ID), which
  made the recheck efficient.
- Process miss: dependent edits started while the gate recheck was pending, so the artifact the
  reviewer was asked to pin changed mid-review. Candidate (scope: SKILL.md architecture gate):
  "while a gate recheck is pending, the author works only on gate-independent tasks; root
  re-freezes and names any changed files before requesting the recheck." Evidence: F17.
- A uniform correction (F3) collided with a caller that deliberately bypasses the helper (F19);
  supports R1 candidate 1 (list each handler's current and new behaviour).

---

## FIXAPP-P04-G1-R3

### Identity

- Date 2026-11-01; workflow `implement-opus-reviewed-2_2`; stage: architecture gate **G1
  correction recheck**, decision revision **r3** (`logs/G1-r3-decisions.md`, byte-identical to
  TASK.md at start and return; r1/r2 copies preserved).
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), same session as R1/R2, effort as configured by the
  spawner. Phase `p04`; worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p04`, branch
  `fix-app/p04`; review base = HEAD `48626a1`.
- Freeze: digest `4f458bd25090bae457d310e858875fba1211f2f7eafa94510b16728d51e1e7ed` (exclusion
  `plan/fix-app/phases/p04/docs`), 14 in-progress WB1 files listed in TASK "G1 r3 freeze" and
  matching `snapshot.sh` exactly; identical at round start and at return (author held edits).
- Coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` revision 4, byte-identical
  to `logs/COORDINATION-r4.md` at start and return; applicable I001–I004 r1, C002 r1, C003 r1.

### Coverage and evidence

- Delta review of r2 → r3 by `diff logs/G1-r2-decisions.md logs/G1-r3-decisions.md`: F19 dashboard
  bullet (checked against `ui_dashboard.rs:159-207` — operator first, `operator_company` for a
  requested id, managed read otherwise, tests named), F18 `PUT /companies/{id}` bullet, F20
  (`custom:{normalized slug}` role, `submission_key` rename vs. existing `onboarding_key(agent)`
  helper, delegated dismiss handler for `base_layout`), F17 process record and freeze list, F16
  remainder in the Verification plan. Table row and bullets are consistent (no contradiction
  introduced). No other decision text changed, so R1/R2 evidence for D1, D3–D7 is reused.
- Checks run: `snapshot.sh` ×2, coordination and TASK/r3 byte comparisons, read-only reads. No
  cargo/DB (decision gate). The 14 in-progress files were not reviewed and are not covered.

### Outcome

- **PASS for G1 decision revision r3** (D1–D7 as written in `logs/G1-r3-decisions.md`). All
  actionable findings F1–F12, F17, F19 are resolved in decision/records; optional F13–F16, F18,
  F20 adopted or dispositioned (F14 deferred with reason). The pass covers the decision text, not
  the in-progress WB1 code, which is reviewed at the WB1 freeze (the batch review should also
  confirm the in-progress `use_cases/caller.rs` matches D4's domain `entities::user::Caller`).

### Process and instruction feedback

- Holding edits during this recheck made the freeze stable; the r3 records (process deviation and
  freeze list) are what F17 asked for. No further instruction candidates beyond R1/R2.


## FIXAPP-P04-G1-R4

### Identity

- Date 2026-11-04; workflow `implement-astra-reviewed-2-2`; fresh architecture gate after resume,
  G1 decision r4, D1–D7 for WB1/WB2. Task [TASK.md](TASK.md); [REVIEW.md](REVIEW.md).
- Independent reviewer B `/root/p04_reviewer`, requested `gpt-6-astra` / `high`; actual runtime
  model/effort not exposed. No inherited author conversation; no authoring or delegation.
  Worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p04`, branch `fix-app/p04`.
- Start/base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; preserved source digest
  `c5b583876682c317238f7a32d04fd2ba70ff220b40a87cc6b8c394daeea893bc`; sole exclusion
  `plan/fix-app/phases/p04/docs`. Decision [logs/G1-r4-decisions.md](logs/G1-r4-decisions.md),
  SHA256 `90714b8edd91eb82dbff5f9c401fb02d8562b51e7d535b0c01fdb19fc8f9a245`.
- Coordination authority `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md`, revision 6;
  pinned [logs/COORDINATION-r6.md](logs/COORDINATION-r6.md), SHA256
  `d3c5b83ecf08b42314e1194bb371ebee030216d8c875cc7f76cf5b0f20322371`. Applicable
  I001–I004/r1, C002/C003/r1, C007/r1; C008/r1 remains proposed p07 output.
  Authoritative file byte-compared with pin at review and return; no changed requirement.

### Coverage and evidence

- Requirements independently read from original p04 plan and fix-app README, root/src,
  application, HTTP/pages, persistence, SMTP and infra AGENTS. Historical G1 R1–R3 notes,
  findings and immutable r3-to-r4 decision diff read; historical pass not used as source acceptance.
- D1/D2: inspected preserved company query/guard changes and the shared-guard caller inventory.
  One-row membership query is compatible with integrated C002/C003 ownership contract; policy
  explicitly covers manager/owner/readable surfaces, explicit company IDs beyond capped lists,
  operator dashboard exception, and propagation of authorized-read errors.
- D3/D4/D5: inspected shared channel-reader extraction, Caller extractor, and attention use-case
  changes for applicability of the decisions. D4's application helper preserves the domain's
  dependency direction. Caller/operator use-case boundaries, team roles and response behavior
  are specified; monitoring/global-dashboard and notifications exceptions are expressly retained
  scope boundaries. These WIP inspections establish feasibility, not code acceptance.
- D6: independently inspected transaction-taking library-copy helpers
  (`persistence/channel.rs:500–597`), `provision_guarded`/`taken_slug` and assistant command
  replay in `workflow/provisioning.rs`, authority company/member locking, and agent retirement.
  Reviewed refusal vs error rollback, per-submission IDs, reuse after changed ownership/names,
  same-key competing submissions, provider preparation and rollback test design. Durable row
  serialization is coherent, but preparation occurs before it: F11 is reopened for that race.
- D7: inspected SMTP trust baseline, DirectoryCache interface and the proposed policy using
  the existing participant trust decision. Explicit warning + scanning on lookup failure is
  consistent with the source instructions. No further costly decision defect found.
- Discovery: `graft map`, targeted `graft ask`, `graft skeleton`, `graft callers ... --depth all`
  and `graft grep owned_company`. Graft falls back to the primary checkout graph; ambiguous
  shared function names have no exact caller edges. Graph results were treated as locators and
  checked against local source/diffs, not as a complete current-WIP caller audit.
- Checks: branch/HEAD, immutable SHA256 checks, installed snapshot script, inspected local
  realpath wrapper (exact supported argument shape, preserves exclusion), byte comparison of
  authoritative coordination and pin. Final check log: [logs/G1-R4-reviewer-final-checks.log](logs/G1-R4-reviewer-final-checks.log).
  No cargo, migrations, DB or provider calls run: decision-only gate and heavy-check slot held by
  p03. No full suite or scripts/check.sh. Code is unfinished and unverified.
- Light/not covered: exhaustive current handler/caller audit, completed route tests, rendering
  behavior, detailed metric equivalence and mock implementations belong to WB1; WB2 code does
  not yet exist. Intervening main and p03 combined error/provisioning integration require the
  later independent integration gate (C007), and C008 is not yet a producer acceptance.

### Outcome

- **Changes required for G1 r4**, no PASS. One actionable issue: reopened
  [FIXAPP-P04-G1-F11](agent-review-findings.md#fixapp-p04-g1-f11--reopened-follow-up-competing-retries-still-discard-a-paid-prompt),
  medium, custom prompt preparation under concurrent same-key submissions. No new optional
  suggestions. D1–D5/D7 have no identified architecture blocker; source acceptance is not granted.
- Historical R2/R3 closure of F11 was incomplete for competing retries; retain original outcomes
  and this linked follow-up. Recheck needs the corrected D6 decision and its verification contract,
  plus any affected ownership/transaction/port changes, with a fresh identity and coordination pin.

### Process and instruction feedback

- Scope was appropriate for one resumed architecture gate; authorization and onboarding locks,
  idempotency and port boundaries warrant it. Source freeze held throughout review.
- The explicit historical finding links and immutable decision copy made independent validation
  practical. Earlier tests asserted final database state but omitted the provider effect count;
  see the narrow candidate in F11. No skills or AGENTS changes made.


## FIXAPP-P04-G1-R5

### Identity

- 2026-11-04; workflow `implement-astra-reviewed-2-2`; G1 r5 correction recheck for reopened
  F11 and affected D6 concurrency/resource/authorization contracts. Reviewer B
  `/root/p04_reviewer`, requested Astra/high; actual runtime model/effort not exposed.
  [Task](TASK.md), [review](REVIEW.md), [findings](agent-review-findings.md).
- Worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p04`, branch `fix-app/p04`;
  start/review-base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; frozen source digest
  `510dba5e4bd0cdf496b59c6fcacbc424a02e04721fe9fbd826e30cb3d8aaaef4`, sole exclusion
  `plan/fix-app/phases/p04/docs`. Decision [r5](logs/G1-r5-decisions.md) SHA256
  `176ddf0a29a0cf9ca5b41358f4ccdf7783af6ae21c031c3475737de52e5c06c2`.
- Coordination authority `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` revision 6,
  retained `logs/COORDINATION-r6.md`, SHA256
  `d3c5b83ecf08b42314e1194bb371ebee030216d8c875cc7f76cf5b0f20322371`.
  I001–I004/r1, C002/C003/C007/r1 apply; C008/r1 proposed. Bytes/freshness rechecked at return.

### Coverage and evidence

- Compared immutable r4/r5 decisions; only D6 and round identity changed. Reused R4 original
  requirement analysis and unaffected D1–D5/D7 evidence. The independently repaired WB1 source
  digest is accepted as a new review input identity, not accepted as reviewed code.
- F11 recheck: lock/session precedes lookup and paid effect, so successful same-key contenders
  do not both prepare. Reviewed cross-process ownership, rollback on consumed/dropped session,
  owner/member distinction, refusal savepoints, scope retention, 30/45/90s bounds, cancellation,
  and explicit inability to promise crash-proof exactly-once billing. Purpose-specific session
  has no SQLx/application dependency inversion. Tests name actual overlapping contenders and
  provider count, cancellation/reacquisition and rollback after the second channel fault.
- Traced the newly under-lock production helper via `graft ask`, `graft callers
  generate_system_prompt --direction out --depth all`, targeted skeletons and exact local reads:
  `AgentUseCases::generate_system_prompt` reads authorization, model connection and key through
  the ordinary shared pool; library copy preparation likewise performs ordinary pool reads.
  Inspected the query executors and production pool/acquire timeout configuration. This creates
  the nested-acquisition wait cycle described in new F21; the counting-generator test alone
  would hide it. No executable implementation or DB reproduction is claimed at this gate.
- Checks run: SHA256, source snapshot before/after using installed script and previously
  inspected wrapper, branch/HEAD and coordination byte equality. Evidence:
  [logs/G1-R5-reviewer-final-checks.log](logs/G1-R5-reviewer-final-checks.log).
  No cargo/DB/provider or full-suite checks; heavy slot remains owned by p03.

### Outcome and process feedback

- **Changes required for G1 r5.** F11 **fixed in decision**; new **FIXAPP-P04-G1-F21**, medium,
  retained transaction plus nested pool acquisition. No optional suggestions or other new
  architecture findings. Source remains WIP/unaccepted.
- Next recheck is D6's preparation-input/resource contract and constrained-pool test design;
  retain unaffected evidence. Correction scope stayed focused. Moving the ownership boundary
  required following helper calls across the new retained-resource lifetime; F21 carries the
  evidence-backed instruction candidate. No instructions/project code authored by B.


## FIXAPP-P04-G1-R6

### Identity

- 2026-11-04; `implement-astra-reviewed-2-2`; G1 r6 correction recheck for F21 and affected
  preparation/resource/security contracts. Independent B `/root/p04_reviewer`, requested
  Astra/high; actual runtime model/effort not exposed. [Task](TASK.md), [review](REVIEW.md).
- Worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p04`, branch `fix-app/p04`;
  start/base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; source digest unchanged
  `510dba5e4bd0cdf496b59c6fcacbc424a02e04721fe9fbd826e30cb3d8aaaef4`; sole exclusion
  `plan/fix-app/phases/p04/docs`. [Decision r6](logs/G1-r6-decisions.md) SHA256
  `c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b`.
- Coordination authority `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` r6;
  immutable `logs/COORDINATION-r6.md`, SHA256
  `d3c5b83ecf08b42314e1194bb371ebee030216d8c875cc7f76cf5b0f20322371`.
  I001–I004/r1, C002/C003/C007/r1 applicable; C008/r1 proposed. Byte equality and freshness
  confirmed at start and return.

### Coverage and evidence

- Compared immutable r5/r6; changes confined to D6's preparation input/connection protocol,
  verification and revision metadata. Reused R4/R5 original requirements and unchanged decisions.
- Reviewed every newly named preparation read: session-scoped connections and selected key,
  pure model selection/validation, provider-only callback, then transactional completion.
  No second pooled connection is permitted while retaining the session. Narrow application port
  preserves dependency direction; resolved secret inputs are not Debug/Serialize; no provider
  effect precedes the ownership point. Session retains company/caller authority. F11's lock and
  retry contract, refusal/error rollback, cancellation and bounded waits remain intact.
- Feasibility checked against previously located source: `agent.rs:151–204` existing pure
  `model_allowed`, `pinned_default_model`, library write derivation; `:660–684`
  `generate_prompt_with` contains only classifier invocation and response handling;
  `:735–753` resolves explicit provider/model/key without database I/O. Existing async database
  helpers remain entry-point conveniences, not the retained-session execution path.
- Verification design now exercises both custom and library preparation with a one-connection
  pool and production composition, replacing only TextClassifier. Concurrent same-key count,
  fault rollback and cancellation tests remain specified. These tests have not been run or
  implemented by B; batch acceptance must verify them and the actual connection inventory.
- Checks: snapshot and SHA256 before/after, branch/HEAD, authoritative coordination comparison.
  [Final checks](logs/G1-R6-reviewer-final-checks.log). No cargo, DB, migrations, provider calls
  or full suite (architecture gate; p03 retains heavy-check slot).

### Outcome and process feedback

- **PASS for G1 decision revision r6**, SHA256
  `c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b`, D1–D7 with
  unchanged evidence reused. F21 fixed in decision; F11 remains fixed. No new findings or
  optional suggestions; no unresolved architecture blockers. This is not code/test acceptance.
- Recheck remained focused and complete for its delta. Explicit connection inventory and a test
  that retains production composition directly answer F21; no additional instruction candidate
  beyond R5's evidence-backed retained-resource inventory rule. Source freeze held throughout.


## FIXAPP-P04-G1-R7

### Identity

- 2026-11-04; workflow `implement-astra-reviewed-2-2`; p04 G1 continuity validation of unchanged decision r6, D1–D7. Independent B `/root/p04_reviewer`; requested Astra/high, actual runtime settings not exposed. [Task](TASK.md), [review](REVIEW.md).
- Worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p04`, branch `fix-app/p04`; phase start/review base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; frozen source `a366d1d6eaba053c6a66ad52a798a073ec14e8a9cecd5903c3a22840d504e092`; sole exclusion `plan/fix-app/phases/p04/docs`. All existing WB1 source remains unaccepted; WB2 has not started.
- Immutable [decision r6](logs/G1-r6-decisions.md), SHA256 `c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b`. Its historical host, coordination revision and source digest remain historical identifiers, not claims about this resumed review.
- Current authority `/Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md`, revision 15; immutable [pin](logs/COORDINATION-r15-resumed.md), byte equality checked before and after review. Applicable I001/I003/I004 r1, I002 r2, C002/C003/C007/C009 r1 acknowledged. I005 future-only is not applicable; C008 is proposed output pending p04 acceptance/integration. R13 explicitly resumes implementation; r14/r15 change scheduling/status, not technical requirements.

### Coverage and evidence

- Read original README and phase 04 outcome, decisions, all six plan batches and tests; compared every D1–D7 contract with current notices and retained G1 R4–R6/F11/F21 evidence. Read applicable root/src/application/HTTP/persistence/SMTP/pages guidance. Verified p01 integration `9d68a6b` is an ancestor of the phase HEAD; C003's enforced ownership prerequisite remains present. p03 acceptance at `c42ae04` is not integration and supplies no new p04 dependency; C007 remains a combined-integration obligation.
- D1–D5 retain uncapped explicit-company authorization, required persistence behaviors, membership-derived owner/manager decisions, uniform foreign/unknown handling and surfaced database errors; thread scope includes both company and channel; operator property is established at the HTTP boundary with library enforcement in use cases; attention policy is application-owned. Prior explicit scope dispositions remain: workflow authority policy, operator service-only monitoring/dashboard checks, and notification persistence are not silently claimed migrated by this gate.
- Independently challenged D6 ownership before provider effects, same-key retry identity, company/member lock order, session authority retention, rollback versus provisioning refusal, bounded wait/provider/operation duration, cancellation, and nested pool acquisitions. The r6 retained-connection inventory plus provider-only callback addresses F21 without weakening F11; the one-connection production-composition test and actual competing custom requests remain required WB2 evidence. Existing connection-taking helpers and company→member locking were located via Graft; no broad code-batch review was performed. D7 retains application sender classification with the explicit logged error-to-scan adapter fallback required by the original plan.
- No schema change or compatibility path is introduced by r6. Resource relocation and fresh fixtures require current focused checks for code acceptance; old machine test results are not new execution evidence. I004 continues to supersede the plan/full-suite gate.
- Checks actually run: branch/HEAD, decision SHA256, ancestry, source snapshot before/after, current coordination/pin byte equality. Inspected the macOS wrapper and installed snapshot script: sole realpath shim, unchanged installed script and exact exclusion. [Final identity evidence](logs/G1-R7-reviewer-final-checks.log). No cargo, database, provider, migration or full-suite checks run by B. Implementation correctness and tests are unverified at this decision-only gate.

### Outcome and process feedback

- **PASS for unchanged G1 decision r6 and current applicability**, SHA256 `c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b`, D1–D7. No new concrete findings or optional suggestions; F11/F21 remain fixed in decision. Source digest unchanged before/after; this is not WB1/WB2 code acceptance.
- Continuity review is appropriately bounded: requirements and resource/authority changes were revalidated without repeating intermediate source review. WB1 whole-batch acceptance must precede WB2; reopen the gate only if a decision changes or a relied-on assumption fails. No new instruction change is supported beyond the preserved effect-ownership/connection-inventory lessons from R4/R5.


## FIXAPP-P04-WB1-R1-MAC02

### Identity

- 2026-11-04; workflow `implement-astra-reviewed-2-2`; p04 WB1 (plan batches 1–4); round [FIXAPP-P04-WB1-R1-MAC02](agent-review-notes.md#fixapp-p04-wb1-r1-mac02). Reviewer `/root/p04_reviewer`, requested Astra/high, actual runtime settings unavailable.
- Worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p04`, branch `fix-app/p04`; start/review base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; reviewed digest `c4983d97a745723104fcdf8a4a3d1804739e1863d72721fe14e03e901b55b3e3`, sole exclusion `plan/fix-app/phases/p04/docs`; unchanged G1 r6. [Task](TASK.md), [review](REVIEW.md).

- Whole WB1 review, not an intermediate step review; WB2 onboarding transaction/sender-trust migration remains unstarted and outside acceptance. Historical uncommitted WB1 work is included in full, not treated as pre-accepted.
- Current authority `/Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md` r15 and [immutable pin](logs/COORDINATION-r15-resumed.md) byte-identical before/after. Applicable I001/I003/I004 r1, I002 r2, C002/C003/C007/C009 r1; I005 not applicable, C008 proposed output. G1 r6 SHA256 `c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b`, continuity R7 retained.

### Coverage and evidence

- Reviewed original phase requirements and gated D1–D5; entire tracked dirty source diff and all 13 new source/test files from review base. HEAD equals the review base, so there is no separate committed delta. Read affected use-case/persistence queries, caller extraction, error mapping, HTTP company/agent/channel/invite/onboarding authorization changes, scoped thread and downstream message/attachment/note/diagnostic paths, all changed management selections, operator/library routes and attention/notification composition, shared mock consolidation, production wiring and error-toast integration.
- Independently traced helpers using Graft callers and exhaustive uses. `owned_company`/`managed_company` have ambiguous names, so graph callers explicitly undercounted; compensated with `graft grep` and exact caller reads. Retained [owned uses](logs/WB1-R1-owned-uses.txt), [managed uses](logs/WB1-R1-managed-uses.txt), [thread callers](logs/WB1-R1-callers-thread.txt). Confirmed only the already-scoped SkillUseCases `get_company` API remains, and production AppConfig operator classification is centralized in `caller_for`. Read the required existing Postgres password/login/reply-context implementations and the new explicit test behavior.
- Authorization analysis covered owner/admin/member/stranger, 201-company explicit lookup, cross-company/channel thread IDs, early library guards including direct use-case calls and provider spend, error propagation, and retained workflow scope exception. Reviewed the final Ui::company/frame correction and its page/pane database-500 regression without weakening existing workflow 404/empty-workspace behavior. Inspected unchanged downstream callers where helper behavior changed (including run-count streaming and team mutation handlers); F1 arises in that caller integration.
- Read all new authorization tests, shared fixtures, focused unit additions, and fixture deltas. Most tests call handlers directly with explicit extractor values; they verify response conversion but do not constitute browser/full-router end-to-end execution. Existing auth tests are in the affected union. Unchanged test bodies outside changed interfaces were reviewed only by their targeted execution evidence, not re-audited wholesale. No runtime/source edits or fixture-mutating commands by B.
- Author evidence inspected: initial union 459 passed/8 failed/4 ignored (retained historical correction input), workflow correction 73/73, final [union](logs/mac02-WB1-affected-r2.log) **468 passed / 0 failed / 4 ignored**, 472 selected, all 35 selectors nonzero in [counts](logs/mac02-WB1-executed-filter-counts-r2.json). The four ignored tests are not counted as exercised behavior. Fresh SQLx prepare/all-target compilation completed successfully per [log](logs/mac02-WB1-sqlx-prepare.log). [Clippy](logs/mac02-WB1-clippy.log) fails at the new layout assertion (F2); no unconditional final verification pass claimed. I004 excludes full suite/check.sh. No migrations changed in WB1.
- B checks: snapshot, branch/HEAD and coordination byte equality before/after; source remains `c4983d97a745723104fcdf8a4a3d1804739e1863d72721fe14e03e901b55b3e3`. [Start snapshot](logs/WB1-R1-reviewer-start-snapshot.txt), [final identity](logs/WB1-R1-reviewer-final-checks.log). Final build checks were author-owned to avoid overlapping shared fixtures; this review does not claim B independently reran them.

### Outcome and process feedback

- **CHANGES REQUIRED; no WB1 pass.** [F1 medium](agent-review-findings.md#fixapp-p04-wb1-f1-mac02--team-mutations-turn-authorization-errors-into-successful-fragments): team mutation errors become 200; [F2 low](agent-review-findings.md#fixapp-p04-wb1-f2-mac02--new-layout-test-fails-the-required-clippy-gate): required Clippy failure. No optional findings. All concrete findings saved together; WB2 remains blocked on WB1 acceptance.
- Batch size was cohesive and reviewable; no new architecture gate is needed for these corrections. Retain unaffected evidence on recheck, focusing on all team mutation error paths, response regression tests and the layout lint. F1 supports the scoped caller/error-mapper review instruction candidate above; no other instruction change supported. Architecture decision continuity pass remains decision-only and is not invalidated by these implementation defects.


## FIXAPP-P04-WB1-R2-MAC02

### Identity

- 2026-11-04; workflow `implement-astra-reviewed-2-2`; p04 WB1 correction recheck for F1/F2-MAC02 and affected caller behavior. Independent B `/root/p04_reviewer`, requested Astra/high; actual runtime settings unavailable. [Task](TASK.md), [review](REVIEW.md).
- Worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p04`, branch `fix-app/p04`; phase start/review base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; correction digest `c5debdeab77f80515c4a88ae2384af6503ae320e53cb48b4fc3dc3c87a2719da`; sole exclusion `plan/fix-app/phases/p04/docs`. Prior R1 digest `c4983d97a745723104fcdf8a4a3d1804739e1863d72721fe14e03e901b55b3e3` remains historical.
- Authority `/Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md` r15; immutable [pin](logs/COORDINATION-r15-resumed.md), byte equality checked before/after. I001/I003/I004 r1, I002 r2, C002/C003/C007/C009 r1 applicable; I005 not applicable, C008 proposed output. G1 r6 SHA256 `c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b` unchanged; continuity R7 retained.

### Coverage and evidence

- Reused whole-batch R1 inspection for unchanged authorization/scoped reads/Caller/attention and wiring. Rechecked correction in ui_team.rs, company_invite.rs, layout.rs and new team tests against F1/F2 and original phase error policy. Every affected team catch-all branch now restricts successful inline errors to BadRequest/Conflict; Forbidden/NotFound/Database/Internal propagate. Rechecked existing update/delete invite preauthorization and avatar branch. No architectural decision changed.
- Traced `update_company_team_member_role` and `remove_company_team_member` callers with Graft depth-all: UI, legacy HTML, JSON and direct application tests. The two self-owner conditions occur after owner authorization and are invalid requested mutations, so BadRequest accurately separates them from infrastructure failure. JSON/direct callers now get400; legacy validation fragments stay inline where their existing policy permits; UI owner pane remains404 because its list excludes the owner. Inspected the actual unchanged `list_members_by_company` SQL predicate and retained intermediate failed test; the corrected expected outcome follows existing behavior.
- New tests directly exercise create invite/change role/remove member for ordinary member and admin403, foreign/unknown404, and original typedDB500 after successful initial readable scope. They also verify valid owner's invalid invite create/update stays200, self-action refusal is BadRequest/400 and owner authority survives. The red runtime log independently establishes the previous error; no new source/test authoring or fixture mutation by B. Direct-handler tests do not claim browser end-to-end execution.
- Inspected [red log](logs/mac02-WB1-team-mutations-red-r2.log):0passed/2failed; retained intermediate compile and 23passed/1failed feedback-test attempts are not passed evidence. [Final affected union](logs/mac02-WB1-affected-r3.log): **472 passed, 0 failed, 4 ignored**, all35 filters nonzero in [counts](logs/mac02-WB1-executed-filter-counts-r3.json). [SQLx prepare/all-target offline compilation](logs/mac02-WB1-sqlx-prepare-r2.log) passes; [Clippy lib/tests -D warnings](logs/mac02-WB1-clippy-r2.log) passes; [final author summary](logs/mac02-WB1-correction-final-check-summary.txt) records formatting/whitespace PASS and unchanged digest. Four ignored tests remain unexercised, not counted as passes. No full suite/check.sh per I004; no schema change or migration check needed for this correction.
- B ran identity/coordination checks and source/caller inspection only, avoiding concurrent fixture runs. [Inspection checkpoint](logs/WB1-R2-reviewer-inspection.txt); [final identity checks](logs/WB1-R2-reviewer-final-checks.log). Source digest and current coordination pin match before/after.

### Outcome and process feedback

- **PASS for WB1 plan batches1–4**, digest `c5debdeab77f80515c4a88ae2384af6503ae320e53cb48b4fc3dc3c87a2719da`, combining unchanged R1 coverage with this correction recheck and completed final evidence. F1-MAC02 and F2-MAC02 **fixed**; no open acceptance-blocking WB1 findings, no new optional suggestions.
- This is WB1 acceptance evidence only. WB2 onboarding transaction and SMTP sender trust remain unstarted/unaccepted; p04 as a whole and combined integration are not complete. Root may accept WB1 and proceed within existing sequencing authority.
- Recheck scope stayed focused and covered every finding together. The red regressions show the caller/error-mapper lesson from R1; no additional instruction candidate is supported. Preserving failed owner-feedback evidence and checking SQL prevented an unrelated owner-list change merely to make a test pass.


## FIXAPP-P04-WB2-R1-MAC02

### Identity

- 2026-11-04; workflow `implement-astra-reviewed-2-2`; independent Reviewer B `/root/p04_reviewer`, requested Astra/high (actual runtime model/effort unavailable). Cohesive WB2 plan batches 5–6 and current whole-phase shared effects with accepted WB1. [Task](TASK.md), [review](REVIEW.md).
- Worktree `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p04`, branch `fix-app/p04`; phase start/review base/HEAD `48626a1240f032ca3b99372bd3f5a05eb08c0c66`; frozen artifact `9b1bfc76eb32f15cf6131b1ab55bebe375d49b2beb90b03d48a7dc71dcc29e91`; exact sole exclusion `plan/fix-app/phases/p04/docs`. [Start snapshot](logs/WB2-R1-reviewer-start-snapshot.txt), [final snapshot](logs/WB2-R1-reviewer-final-snapshot.txt), [identity and independent filter recount](logs/WB2-R1-reviewer-final-checks.json).
- Authority `/Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md` r16, [immutable pin](logs/COORDINATION-r16-WB2-review.md), byte-identical before/after. I001/I003/I004 r1, I002 r2 and C002/C003/C007/C009 r1 apply; I005 future-only, C008 proposed output. Authority SHA256 `32a4ca8e44f917e5507092052499501301b8d375df6de191c49b9ef84a50f615`. G1 r6 D1–D7 retained, decision SHA256 `c4ebf2010801326edc25951434c987451c803e5da8c39070955eb1f9aec8922b`, continuity R7 unchanged. WB1-R2 accepted digest `c5debdeab77f80515c4a88ae2384af6503ae320e53cb48b4fc3dc3c87a2719da` reused only for unaffected code.

### Coverage and evidence

- Rechecked the original phase requirements and normative D6/D7. Independently inspected new application onboarding and model-preparation modules, new persistence session, retained connection helpers, shared agent model-generation/library-copy changes, channel provisioning extraction and both production callers, HTTP form/response/key wiring, AppState/setup/WorkflowEngine wiring, and SMTP/application trust changes. Used Graft source/caller queries before exact source reads. The full new onboarding test module and new HTTP/SMTP tests were read; unaffected WB1 implementations retain the earlier complete review rather than a second line-by-line audit.
- Ownership and atomicity: the application authorizes the owner before preparation; the session rechecks company owner plus owner membership under company→member locks. Retained model connection/key reads, library skills/tool preparation, inserts/principals, channel provisioning and command storage use the same transaction. The provider callback resolves explicit inputs and calls the configured classifier without a second database checkout or detached task. Pure model selection is shared with ordinary AgentUseCases. Database model lookup failures now propagate rather than being interpreted as model mismatch. Channel refusals use existing savepoint isolation and preserve the agent; other errors roll back the enclosing transaction. Examined both normal assistant-channel service and onboarding callers of the extracted helper.
- Retry/recovery: GET issues a new submission UUID; POST requires it and rejected drafts retain it. Company/key/role-derived IDs plus locked existing-agent lookup prevent successful concurrent retries from repeating paid prompt generation. Fresh keys after deletion/rename/ownership transfer avoid old command replay; digest mismatch becomes a displayed refusal. Selection is bounded, custom fields paired, and authorization precedes provider effects. Operation/prompt timeouts are cancellation boundaries; dropped transaction/provider futures release locks and allow retry. The deliberate bounded company-lock/connection hold and possibility of repeated provider spend after crash remain explicit r6 tradeoffs, not newly discovered defects.
- Read the actual competing-custom test: it observes a database lock waiter before releasing the first classifier and asserts one call plus equal results. The one-connection test uses production AgentUseCases composition, actual model/key reads, custom and library preparation, then a competing retry; both complete with one provider call. Fault-trigger test verifies the second channel sees two inserted agents, then verifies zero agents/channels/command rows/copied skills after rollback. Provider failure, abort and both timeout paths verify no active callback/effects and successful retry. Denial tests cover direct port and application owner/admin/member/outsider, and route conversion; timeout maps to retryable503. Direct-handler tests are not browser/full-router end-to-end evidence.
- SMTP now parses transport input and delegates trust to ThreadUseCases, using the same DirectoryCache and channel alias matching as routing. Unknown company/channel/sender stays Stranger; typed lookup errors remain Err until the explicitly logged scan fallback. Raw persistence accessors are removed. New tests verify real directory owner/stranger/alias behavior and captured warning plus scan after lookup failure. Existing SMTP/routing tests remain in the affected union.
- Author evidence inspected: [affected command](logs/mac02-WB2-affected-command.json), [final affected log](logs/mac02-WB2-affected-r3.log), [counts](logs/mac02-WB2-executed-filter-counts-r3.json): **523 passed, 0 failed, 4 ignored**, all40 filters nonzero. Independently recounted each selector from actual log test names. [Onboarding final](logs/mac02-WB2-onboarding-r4.log) includes corrected503 timeout response; earlier failed attempts are retained historical evidence. [Offline all-target Clippy -D warnings](logs/mac02-WB2-clippy-r2.log) and [final formatting/whitespace summary](logs/mac02-WB2-final-check-summary-r2.json) pass. All-target Clippy provides final all-target offline compilation evidence; no schema or SQLx macro query change. Unchanged WB1 SQLx evidence remains applicable.
- B ran source/caller inspection and identity/coordination/filter-count checks, not cargo or fixture-mutating tests. Four ignored tests are not exercised coverage. No full suite/check.sh/stack-budget per I004; no claim of combined-main integration, migration reset, external-provider or browser verification. Source snapshot is byte-identical before/after, exact expected digest; authority pin and decision hash reconfirmed.

### Outcome and process feedback

- **PASS for WB2 plan batches5–6 and current whole-phase shared effects**, digest `9b1bfc76eb32f15cf6131b1ab55bebe375d49b2beb90b03d48a7dc71dcc29e91`, with unaffected accepted WB1 evidence reused. No new actionable or optional findings; F11/F21 are now satisfied in implementation and focused tests, and WB1 F1/F2 remain closed. [Disposition](agent-review-findings.md#fixapp-p04-wb2-r1-mac02--batch-disposition).
- This accepts the frozen p04 artifact for the assigned phase scope. It does not accept later rebases/merge resolutions or combined-main integration; those require identity and affected-interface review when available. Root remains responsible for phase closeout and coordination publication.
- Batch boundary was cohesive and permitted thorough review. The r6 resource inventory and production-composition one-connection test directly addressed the earlier gate failure; real observed contention made the duplicate-provider guarantee reviewable. No additional instruction change is supported beyond the already recorded ownership/connection-inventory lessons. No source or design authored by B.
