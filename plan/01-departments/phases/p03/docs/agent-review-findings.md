# p03 agent review findings

Shared findings log for phase `p03` of `plan/01-departments/` (workflow `implement-opus-reviewed-2_2`).
Records: `TASK.md`, `REVIEW.md`, `agent-review-notes.md` (this directory).

Common context for the G1 findings below:

- Date 2026-11-10; batch B1 (03A + 03C.0); stage: architecture gate G1; round `01DEP-P03-G1-R1`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, HEAD/start `9291fcd5`.
- Reviewed: decision revision **r1** (DEP-P03-D1..D6). Immutable copy `logs/gate-B1-r1.md`, sha256
  `25d7ee4d26dcfa8c87158e7446454cb5cec5e5c9cce6ad154763cd2bb21672a1`.
- Coordination revision 3 (`logs/coordination-r3.md`, sha256 `880cac54…04b8`).
- Recheck round `01DEP-P03-G1-R2` (2026-11-10) reviewed decision revision **r2**: `logs/gate-B1-r2.md`
  sha256 `ab506aa02116ccf96f3b5c25556168015deead36276e734efb33c087ead502e8`, byte-identical to
  `TASK.md` (`cmp`); r1 copy unchanged; coordination r3 unchanged; HEAD `9291fcd5`.
- Recheck round `01DEP-P03-G1-R3` (2026-11-10) reviewed decision revision **r3** (F4 only, plus the
  two observation edits): `logs/gate-B1-r3.md` sha256
  `14f1ffe03da2ce8c43fe3bf4c822c3309ba66597adff85bd8548ba3ef88d5aef`, byte-identical to `TASK.md`;
  r1/r2 copies unchanged; coordination r3 unchanged; HEAD `9291fcd5`, source unchanged.

---

## 01DEP-P03-G1-F1 — Placement is done before the agent is published and its desk exists, which blocks B2 and holds desk gates too long

- **Classification:** actionable defect · **Medium** · architecture / cross-batch contract. Gated
  decision D1.
- **Evidence:**
  - D1 orders the steps as: 3. placement (`add_department_member_on`, then
    `assign_department_role_on`); 4. `publish_initial_on`; 5. `assistant_channel_on`.
  - Since p02, both placement functions end in `sync_department_grants_on`
    (`department.rs:494-495` assign, `:547` add). That sync takes `ChangeSettings` gates and the rows of the
    department's members-only desks (`department_desks.rs` `rederive_on`).
  - Doing placement at step 3 has two consequences:
    1. **B2 is blocked for create-as-lead.** 03B.3 makes `assign_role_on` the place where taking
       the `lead` role re-points the department desk's `agent.run`, through the publication path
       `apply` uses. An agent created with `role: lead` would reach that re-pointing at step 3,
       before it has a published revision (step 4) or its default desk (step 5). B2 would then
       have to special-case "lead not yet published", or the creation would fail.
    2. **Gates are held longer, and out of order.** The members-only desks' gates are held through
       template compile, workflow publication and desk provisioning, so inbound admissions to
       those desks wait longer. The plan-wide order (p01 D3, p02 D3) puts gates and channel rows
       last, but D1 takes more locks after them: the agent row `FOR UPDATE`, the actor's
       re-authorization, and provisioning inserts. Nothing I found makes this a deadlock today:
       the later locks are on new rows or are re-entrant. But the gate gives no lock-sequence
       argument for it.
  - The plan's wording ("apply placement … then call `assistant_channel_on`") describes what is
    in the transaction, not a required lock order. The commit stays atomic whatever the order.
- **Required:**
  - Order D1 as model → agent and principal → publication → default desk → placement, or justify
    the current order.
  - State the full lock sequence of D1, and show that it composes with p02's department writers
    and with inbound commits.
  - Say what B2 may assume when placement assigns `lead`: the agent is published and has its
    default desk.
- **Instruction lesson:** (observation) when a step calls a function that a later batch will
  extend, its position must satisfy that extension's preconditions. Candidate: gates for staged
  batches list each forward contract's preconditions.
- **Resolution history:** 2026-11-10 open (G1-R1). 2026-11-10 **resolved** (G1-R2, r2 D1): steps
  are model → agent + principal → `publish_initial_on` → `assistant_channel_on(Existing)` →
  placement last, each with its lock footprint, and "B2 may assume" states published + default
  desk. Verified against code: provisioning takes no channel gate (`acquire_channel_gates_on` is
  called only from `department_desks.rs`, `channel_edit.rs` and `thread/inbound.rs`), so the
  p02 sync in step 5 takes the only gates, after every other lock; publication's locks are the
  re-entrant account/company locks, the new agent row `FOR UPDATE` and shared reads
  (`agent_publication/mod.rs:50-115`, `capture.rs:60,147`). Wording nit, not a defect: capture
  reads skills `FOR SHARE`, not `FOR KEY SHARE`; both are shared and taken before any department
  lock.

---

## 01DEP-P03-G1-F2 — The Agent Builder path would lose its worded refusals and could poison the action transaction

- **Classification:** actionable defect · **Medium** · correctness / contract. Gated decision D5 (with D1).
- **Evidence:**
  - Today `ProvisioningEffects::apply` runs the Builder's creation through `provision_guarded`,
    which wraps it in a savepoint. A refusal becomes `refusal.tool_error()`, an `Ok` tool result
    the model sees, and the action receipt commits (`provisioning.rs:192-237, 489-512`; F046).
  - D5 replaces that with D1, which returns `AppResult` and turns refusals into `AppError`
    (Conflict/BadRequest/NotFound). D1 runs on the caller's transaction with no savepoint.
  - Inside the action transaction, any failing statement leaves the whole transaction unusable.
    Examples: `insert_agent_on` on a taken agent slug raises a unique violation, mapped to
    `Conflict` at `agent.rs:117-124`; an FK or check error. After that, no receipt can be recorded.
  - D1 does run its own desk step through `assistant_channel_on` and `provision_guarded`, but
    steps 1–4 are unguarded.
  - D5 only says "refuse the whole command (`Invalid`: …)". It does not say how D1's errors become
    tool results, or that the action transaction stays usable.
  - D5 also reuses the default desk at "same slug, autonomous, no custom steps". Plan 03A.4 says
    "same slug **and settings**", and the Builder can pass a `channel_name` that differs from the
    agent's name. Nothing says whether that is reused, renamed or refused.
- **Required:** state in r2 that the Builder runs D1 and any extra desk inside a savepoint, as
  `provision_guarded` does, and maps D1's refusals back to `ProvisioningRefusal` tool errors, so
  the receipt commits and nothing of the creation does. State the `channel_name` rule at the same
  slug. Add a verification row: a Builder create with a taken agent slug or desk address returns a
  tool error, leaves zero agent/desk/command rows, and the run continues.
- **Instruction lesson:** (observation) moving a path onto a shared `AppResult` helper can
  silently change a refusal contract that existed as data. Candidate: when a gate routes an
  existing path through a new helper, list that path's error and refusal contract and how it is
  preserved.
- **Resolution history:** 2026-11-10 open (G1-R1). 2026-11-10 **resolved** (G1-R2, r2 D5): one
  savepoint around D1 and any extra desk; `Err` rolls back to it and maps to
  `ProvisioningRefusal` (Conflict → Conflict, BadRequest/NotFound → Invalid, post-rollback
  taken-slug check → Conflict, other errors propagate) — the same contract as `provision_guarded`
  (`provisioning.rs:489-512`), so the tool error is an `Ok` result and the receipt commits. The
  same-slug rule now requires the name omitted or equal to the agent's; a different name is
  refused. Verification row "Builder refusal (F2)" added. Retries of one Builder command are
  answered by the action receipt (`provisioning_competing_claimants_create_one_assistant`), so
  D1 does not re-run on the deterministic `key.id("agent")`.

---

## 01DEP-P03-G1-F3 — Replaying onboarding with a department name would fail the whole onboarding

- **Classification:** actionable defect · **Medium** · correctness (idempotency). Gated decision D4.
- **Evidence:**
  - Onboarding is replay-safe for agents. `selection(company_id, submission, library)` gives
    deterministic agent ids, `existing_agents` skips the ones already created, and readback finds
    their desks (`use_cases/onboarding.rs:120-170`, `persistence/onboarding.rs:93-142`).
  - D4 adds: "when present, the first department is created in the same transaction (p01
    `create_department_on`, no lead) and the first agent is placed in it". A replay of a completed
    onboarding (double submit, back/resubmit) would call `create_department_on` again. The slug is
    taken, so it returns `Conflict`. `complete` turns that into `Rejected` and rolls back
    everything. The replay therefore fails where today it succeeds.
  - The plan's check "Onboarding and command replay add no second desk" implies replay must
    succeed.
  - D4 also does not say what onboarding shows when the company has no keyed model. Under D3 every
    agent creation is refused. The existing outcomes "No desk: there is no model connection to run
    it on." and "Connect a model in company settings, then create an assistant desk…"
    (`pages/onboarding.rs:189, 247`) become unreachable, and onboarding is instead `Rejected` with
    D3's message.
- **Required:**
  - Make the onboarding department idempotent: a deterministic id derived like the agent ids, or
    reuse of an existing department with that slug created by this onboarding. Placement must be
    idempotent too (`add_member` already is).
  - State the no-keyed-model onboarding outcome.
  - Add a verification row: replaying an onboarding with a department name commits once, then
    returns the same result.
- **Resolution history:** 2026-11-10 open (G1-R1). 2026-11-10 **resolved** (G1-R2, r2 D4): the
  department id is `agent_id(company, submission_key, "department")`, which cannot collide with
  the agent roles `custom:{slug}` / `library:{id}` (`use_cases/onboarding.rs:275-330`); a replay
  reuses it, otherwise `create_department_with_id_on` creates it in the same transaction; a slug
  held by another department → `Conflict` → `Rejected`. No keyed model → `Rejected` with D3's
  message, and `OnboardedAgent.channel` (today `Option<Provisioned<Value>>`,
  `use_cases/onboarding.rs:74-78`) becomes the always-present readback. Rows "onboarding replay
  with department" and "onboarding without keyed model" added. Observation (not a finding): an
  edited department name on a resubmission with the same key reuses the first department under
  its first name.

---

## 01DEP-P03-G1-F4 — The source test that should stop other code creating company agents checks the wrong function

- **Classification:** actionable defect · **Low** · verification / architecture. Gated decisions D1/D2.
- **Evidence:**
  - D1's guard is a source test that no production file calls `AgentPersistence::create`. But the
    production paths that insert company agents call `insert_agent_on` and
    `create_agent_principal_on` directly:
    - `persistence/onboarding.rs:156-169`;
    - `persistence/channel.rs:560-561` (`LibraryAgentCopyPersistence::create_agent_from_library`);
    - `workflow/provisioning_agent.rs:40-41`.

    `AgentPersistence::create` (`agent.rs:323-324`) is only one of four.
  - D2 replaces the library copy with `CompanyAgents::copy_from_library`. It does not say whether
    the `LibraryAgentCopyPersistence` port and its Postgres method are removed. Left in place, they
    are a second company-agent insert with no desk.
- **Required:** the guard should assert that `insert_agent_on(… Some(company) …)` and
  `create_agent_principal_on` have no production callers outside `company_agent.rs`. Allow the
  library-only `insert_agent_on(…, None, …)` and the existing lazy agent-principal creation in
  `thread/message.rs:337`, if that is intended. State the removal of `LibraryAgentCopyPersistence`
  (or its new role).
- **Resolution history:** 2026-11-10 open (G1-R1). 2026-11-10 **partly resolved, still open**
  (G1-R2, r2 D1 "No bypass"): the scan now targets `insert_agent_on(` and
  `create_agent_principal_on(`, the allowlist matches today's call sites (grep: `agent.rs:249,323,
  341`, `thread/message.rs:337`, plus the three paths moving into D1), and
  `LibraryAgentCopyPersistence` is removed. Remaining: the allowlist admits the whole of
  `persistence/agent.rs`, which keeps a company-agent insert (`AgentPersistence::create`,
  `insert_agent_on(…, Some(company_id), …)` at `agent.rs:323`) on the production port
  (`use_cases/agent.rs:107`), reachable through any `dyn AgentPersistence`. r2's backstop, "the
  test also asserts it has no non-test caller", cannot be decided by a source scan: the call is a
  trait method named `create` (today `self.agent_persistence.create(company_id, write)`,
  `use_cases/agent.rs:298`), and `.create(` has other production callers (`use_cases/channel.rs:218`,
  `use_cases/company.rs:344`). **Required (r3 or a gate note):** make the fixture insert
  unreachable from production by construction — e.g. `#[cfg(test)]` on `AgentPersistence::create`
  and its two impls (`agent.rs:319`, `agent_tests.rs:35`; every caller is a unit test or a
  `cfg(test)` fixture module under `src/`, and `tests/` has none), or move it to a test-only
  helper — so the compiler is the guard and the `agent.rs` allowlist entry covers only the
  `company_id = None` library inserts.
  2026-11-10 **resolved** (G1-R3, r3 D1 "No bypass (r3, F4)"): `AgentPersistence::create` and both
  impls become `#[cfg(test)]`; the `agent.rs` allowlist entry is now `create_library`
  (`company_id = None`) plus the `cfg(test)` `create`; the verification row checks that the lib
  build has no `create` to call, so offline compilation is the guard. Verified feasible: the only
  impls are `agent.rs:319` and `agent_tests.rs:35`; no Cargo features, benches, examples or
  `tests/` use the port; every other caller sits in a `#[cfg(test)] mod tests` or a `#[path]`
  fixture loaded from a test file; the one production caller, `use_cases/agent.rs:298`
  (`AgentUseCases::create_agent`), is the call D2 replaces with `CompanyAgents::create`, so leaving
  it would fail the lib build.

---

## 01DEP-P03-G1-F5 — Add verification rows for creating an agent as lead, and for duplicate-slug errors on every path

- **Classification:** optional suggestion · Low · verification.
- **Evidence and suggestions:**
  - No row creates an agent with `role: lead` and asserts the p02 derived grants. Those are the
    agent's own department desks and, as a child lead, the parent's members-only desks (C002).
  - The competing row covers two creates of one slug, but no row checks the error each path
    returns. The UI and JSON paths should return `Conflict`. The Builder should return a tool error
    (G1-F2). Onboarding should return `Rejected`.
  - Checked and fine: the Builder already locks the requester's account before the company row
    (`action_authority.rs:75`; `provisioning_requester_lock_tests.rs`
    `delegated_provisioning_locks_requester_before_company`), so D1's "account → company"
    precondition holds on every path. That corrects my p01 G1-F7 remark about the Builder.
- **Resolution history:** 2026-11-10 open (optional). 2026-11-10 **resolved** (G1-R2): rows "lead
  at creation (F5, C002)" (own, parent's and children's members-only desks) and "duplicate slug
  per path (F5)" (`Conflict` / tool error / `Rejected` / `Conflict`, nothing written) added.

---

# Batch B1 review — round `01DEP-P03-B1-R1`

Common context for the B1 findings below:

- Date 2026-11-10; batch B1 (03A automatic agent desks + 03C.0 placement), implemented against gate
  r3 (`logs/gate-B1-r3.md` sha256 `14f1ffe0…5aef`); stage: batch review; round `01DEP-P03-B1-R1`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, base/HEAD `9291fcd5`, uncommitted changes.
  Snapshot digest `2f77ca9fc17c9b5e30a5481686580f8cb71597058ff9cdaa99d6072007d04279` (recomputed;
  exclusions `plan/01-departments/phases/p03/docs`, `plan/01-departments/COORDINATION.md`).
- Coordination revision 3 (`880cac54…04b8`), C001/C002, I001–I003.
- Recheck round `01DEP-P03-B1-R2` (2026-11-10): artifact digest
  `a75c40e7b888d42b415fe7408a319812242e96157e642c594a91ab87c7d55798` (recomputed), base/HEAD
  `9291fcd5`, evidence `logs/01DEP-P03-B1-R2.evidence.json` valid.

---

## 01DEP-P03-B1-F1 — Creating an agent as lead takes the desk gates in two calls, which can deadlock with an inbound message

- **Classification:** actionable defect · **Medium** · concurrency / lock protocol. Affects D1 step 5
  on every path that passes a role (JSON create, Builder `role: lead`).
- **Evidence:**
  - `create_company_agent_on` places the agent with two writer calls in one transaction
    (`company_agent.rs:106-110`): `add_department_member_on`, then `assign_department_role_on`.
  - Each call ends with its own grant sync. `add_department_member_on` syncs `[department]`
    (`department.rs:558`); `assign_department_role_on` syncs `role_reach_on`, which is the
    department, its parent and its children (`department.rs:506`, `:591-610`).
  - Each sync runs `rederive_on` (`department_desks.rs:230-262`): principals `FOR KEY SHARE`, then
    `acquire_channel_gates_on(…, ChangeSettings)` (exclusive), then channel rows `FOR UPDATE`.
  - So the transaction first holds the gates and rows of D's members-only desks. It then asks, in
    sorted order, for the gates and rows of D ∪ parent ∪ children, after already holding part of
    that set. It also locks the newly reached principals after holding gates.
  - `channel_gate.rs:58-61` forbids exactly this: "A multi-channel writer must pass all of its keys
    in one call: taking them one at a time … is how two such writers deadlock each other." The
    plan-wide order (TASK D1 step 5; p02 D3) is "channel gates (one sorted
    `acquire_channel_gates_on`)".
  - **Concrete cycle.** Let `d` be a members-only desk of D and `p` one of its parent's, with
    key(p) < key(d).
    - An inbound commit for one message routed to both desks takes shared gates in one sorted call
      (`thread/inbound.rs:147`, `hold_channel_settings`): it holds `p` and waits for `d`. Inbound
      takes no company lock, so the creator's company lock does not serialize it.
    - The creator holds `d` exclusively from the member sync, and its role sync asks for `p` first
      and waits.
    - PostgreSQL then aborts one side after `deadlock_timeout` or `lock_timeout` (both 1 s here):
      a 500 or 503 creation, or an inbound retry. Children's desks give the same cycle.
  - Nothing equivalent exists in p02: each p02 writer does one sync. The two-step composition is new
    in B1. `agent_default_desk_places_the_agent_and_its_lead_role_with_grants` runs this exact path
    with parent and child members-only desks, but sequentially.
- **Required:**
  - Make placement take its gates, rows and principals once, sorted. For example, a department-side
    `place_on`: lock the department; lock the involved principals, including a replaced lead; write
    the member row and, with a role, the role row; then make one `sync_department_grants_on` over
    the role's reach (which contains D), or over `[D]` when there is no role.
  - Add a competing test, as AGENTS.md requires for a changed concurrency protocol. For example,
    hold the lowest-keyed reached desk's gate shared in another transaction, and assert that a
    create-as-lead holds no other reached gate while it waits. Alternatively, race an admission over
    two reached desks against the creation and assert both commit.
- **Instruction lesson:** (observation) when a decision composes existing writers in one
  transaction, check whether the composition still takes each lock class once and in order. I
  passed r2/r3 D1 step 5 without checking this. Candidate rule for gates: list the lock acquisitions
  of the composed calls, not only of each call alone.
- **Resolution history:** 2026-11-10 open (B1-R1). 2026-11-10 **resolved** (B1-R2, artifact `a75c40e7…5798`): `department.rs:470-549` `place_in_department_on` over one
  `write_placement_on`: department row, involved principals sorted, rows, then a single
  `sync_department_grants_on` over the role's reach (which contains the department) or over
  the department alone. `add_department_member_on`/`assign_department_role_on` share the writer
  (equivalent locks; add-member now also takes `lock_principals_on`, same `FOR KEY SHARE` mode).
  D1 calls it (`company_agent.rs:111-113`). Competing test
  `agent_default_desk_competing_lead_creation_takes_its_desk_gates_once` orders the parent desk
  key below the department's and proves the waiting creator holds no department gate; the
  author's mutation (old two calls) fails it with a lock timeout (`logs/B1-R2-gate-mutation.txt`).
  I ran the three `competing_` tests six times: 6/6 pass.

---

## 01DEP-P03-B1-F2 — The built-in Agent Builder's instructions contradict the new same-address rule

- **Classification:** actionable defect · **Medium** · contract consumer / product behaviour. D5.
- **Evidence:**
  - D5 (`provisioning_builder.rs:76-85`): a desk requested at the agent's own slug must be
    autonomous, with no steps and the agent's name. Otherwise the whole command is refused ("The
    agent's own address is its autonomous default desk; choose another address…").
  - `reply_mode` is a required tool input (`application/workflow/provisioning.rs:190`), and
    `channel_slug` defaults to the agent's slug. So a reviewed desk, or one with custom steps, must
    name another address.
  - The built-in Builder instructions (`use_cases/builtin_agent_library.rs:33-40`) tell the model
    to:
    - let the user choose autonomous or reviewed for "how its desk replies";
    - propose custom steps;
    - confirm a final specification that has no desk address ("name, slug, description, provider
      and model, reply mode, any custom steps, …");
    - "call create_agent_channel once with the agreed specification … never retry with a
      different design they did not agree to".
  - So every reviewed or custom-steps creation that follows the instructions is refused on the
    first call. Only the `channel_slug` description in the tool schema hints at the rule.
  - B1's "Contract impact" lists the tool schema's documentation consumers (`custom_tools.md`,
    `agent-builder-context.md`, both B3). It does not list the built-in Builder's own
    instructions, and neither B3 nor plan 04 schedules them.
- **Required:**
  - Update the built-in instructions so they agree with D5:
    - every agent gets its autonomous default desk at its slug;
    - a reviewed desk, or one with custom steps, is an additional desk whose address goes into the
      confirmed specification;
    - optionally, the `department` / `role` arguments.
  - Assert the new rule in `desk_wording_agent_builder_prompt_says_desk`, or in a sibling test.
  - Add the built-in instructions to the Contract impact table.
- **Instruction lesson:** (observation) a model-facing consumer of a tool contract is a
  production consumer. Candidate: contract-impact tables list built-in agent instructions that
  call a changed tool.
- **Resolution history:** 2026-11-10 open (B1-R1). 2026-11-10 **resolved** (B1-R2, artifact `a75c40e7…5798`): `builtin_agent_library.rs:33-41`: every agent gets its own
  autonomous desk at its slug; reviewed replies or custom steps are a second desk at another
  address that the confirmed specification carries; department and lead role are collected;
  the tool call and the report follow the rule. Asserted in both prompt tests (and the
  no-`channel` prose check still passes).

---

## 01DEP-P03-B1-F3 — The no-bypass source scan skips about 1,800 lines of production code

- **Classification:** actionable defect · **Low** · verification (gate r3 D1 "No bypass").
- **Evidence:**
  - `company_agent_tests.rs:29` scans only the text before the first `"#[cfg(test)]\nmod "`. Many
    production files declare a test module near the top with `#[cfg(test)]\nmod tests;` and then
    continue with production code.
  - Measured with the same rule: 41 non-test files, 1,813 non-comment production lines unscanned.
    These include `persistence/agent_publication/mod.rs` after line 17 (`publish_initial_on`,
    `LockedAgentPublication`), `persistence/workflow/mod.rs` after line 92, and
    `persistence/local_agents/management/mod.rs`. All of them can see the `pub(crate)` primitives.
  - `company_agent_tests.rs:24` also skips every path that contains `test`. Today all such files
    are test-only, but a future production file such as `latest_*.rs` or `attestation.rs` would be
    skipped silently.
- **Required:**
  - Exclude only test items. For example, parse with `syn`, already a dev-dependency
    (`Cargo.toml:70`, used by `tests/source_modules.rs`), and skip `#[cfg(test)]` items; or drop
    only `#[cfg(test)]` declaration lines and inline `mod … { … }` bodies.
  - Select test files by their test-only module declaration or by `_tests.rs` / `tests/` /
    `test_support` naming, not by any `test` substring.
  - Keep the "creation file has exactly the two calls" assertion.
- **Resolution history:** 2026-11-10 open (B1-R1). 2026-11-10 **resolved** (B1-R2, artifact `a75c40e7…5798`): `company_agent_tests.rs` walks the module tree from `src/lib.rs`
  with `syn`, follows `#[path]`, skips `#[cfg(test)]`/`#[test]` items with their contents, and
  asserts the exact (file, function, primitive) set; a missing module file panics and the walk
  must reach > 500 files. The author's mutation (a call appended to `agent_publication/mod.rs`,
  formerly unscanned) is caught (`logs/B1-R2-scan-mutation.txt`). Residual, not a finding: calls
  through a renamed import, a function pointer or inside a macro body are not seen.

---

## 01DEP-P03-B1-F4 — Production docs still say agent creation and library copies produce drafts

- **Classification:** actionable defect · **Low** · documentation (AGENTS.md: docs must not promise
  behaviour the code lacks).
- **Evidence:**
  - Two sentences are now false:
    - `docs/agent-authoring.md:97`: "Create uses `POST /api/companies/{company}/agents` with flat
      agent metadata and creates a draft."
    - `docs/rig.md:16`: "Library copying creates a company draft for explicit
      validation/publication."
  - After B1, every company creation publishes the agent and gives it a default desk. A library
    copy that cannot publish is refused (the reported 03A.2 behaviour change).
  - The JSON create also takes `department_id` / `role` now.
  - Neither sentence is in B3's docs list (TASK: `agent-builder-context.md`, `custom_tools.md`,
    table comment) or in plan 04D (`agent-authoring.md` only for directory fields).
  - `docs/agent-builder-context.md:17` ("A library copy of Agent Builder itself creates only an
    agent") and `docs/custom_tools.md:37-41` are in B3's list. Make sure B3 covers those sentences
    too.
- **Required:** update the two sentences, and name the JSON placement fields, in B1. Or add them
  explicitly to B3's docs list in TASK.
- **Resolution history:** 2026-11-10 open (B1-R1). 2026-11-10 **resolved** (B1-R2, artifact `a75c40e7…5798`): `docs/agent-authoring.md:97-104` (publish + default desk, model
  rule, refusals, `department_id`/`role`, library copies) and `docs/rig.md:15-17` updated.

---

## 01DEP-P03-B1-F5 — The Builder's extra desk is provisioned while the placement gates are held

- **Classification:** optional suggestion · Low · lock-hold latency. D5 with D1.
- **Evidence:**
  - With a different `channel_slug`, `builder_on` calls D1, which ends in placement and its gates.
    It then calls `provision_on` for the extra desk (`provisioning_builder.rs:92-122`).
  - So the members-only desks' gates are held through the extra desk's template compile and
    workflow publication: the latency concern of G1-F1 (2), on this path only.
  - No deadlock found. After the gates, this path takes only shared reads, re-entrant
    account/company locks, its own new agent row and new rows; provisioning takes no gate.
  - `company_agent.rs:5-8` describes the gates as the last locks, which holds for D1 itself.
- **Suggestion:** provision the extra desk before placement, for example by passing
  `placement: None` to D1 and placing after `provision_on`. Or record the exception next to the
  lock-order note.
- **Resolution history:** 2026-11-10 open (optional). 2026-11-10 **resolved** (B1-R2, artifact `a75c40e7…5798`): the Builder passes `placement: None` to D1 and calls
  `place_in_department_on` after any extra desk (`provisioning_builder.rs:92-136`), so the
  placement gates are its last locks; B2's precondition (published, default desk exists) holds.

---

## 01DEP-P03-B1-F6 — An onboarding resubmission that first names a department creates it empty

- **Classification:** optional suggestion · Low · onboarding edge case. D4.
- **Evidence:**
  - `Session::complete` (`persistence/onboarding.rs:111-131`) creates or reuses the department by
    its deterministic id. It places the first selected agent only if that agent is inserted in this
    run (`Some(missing.id) == first`).
  - Take a first submission with no department name, followed by a same-key resubmission that
    names one. The agents already exist, so the department is created and nobody is placed in it.
    03C.0 says onboarding "creates the company's first department … and places the first agent
    there".
- **Suggestion:** when a department is created in this run, place the first selected agent even if
  that agent already exists (`add_department_member_on` is idempotent). Or create the department
  only together with the first agent's insert. Assert whichever is chosen.
- **Resolution history:** 2026-11-10 open (optional). 2026-11-10 **resolved** (B1-R2, artifact `a75c40e7…5798`): onboarding places the first selected agent after all inserts,
  idempotently (`persistence/onboarding.rs:127-136`, `place`), and D1 gets no placement there;
  `agent_default_desk_onboarding_names_the_department_on_a_resubmission` asserts the placement.
