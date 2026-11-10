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

---

# Architecture gate B2 — round `01DEP-P03-G2-R1`

Common context for the G2 findings below:

- Date 2026-11-10; batch B2 (plan 03B: the department desk and re-leading); stage: architecture
  gate G2; round `01DEP-P03-G2-R1`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, HEAD `523732b7` (B1 committed). The
  working tree has only the TASK edit and the pinned gate copy.
- Reviewed: decision revision **r1** (DEP-P03-D7..D9 + "Verification plan B2"), immutable copy
  `logs/gate-B2-r1.md` sha256 `3ff959634457bc807196c3e7219b189a23ceb6b1cd0b8e28435f0177483891ce`.
- Coordination revision 3 (`880cac54…04b8`).
- Recheck round `01DEP-P03-G2-R2` (2026-11-10) reviewed decision revision **r3**, as the coordinator
  corrected the round from r2. `logs/gate-B2-r3.md` sha256
  `bb6dcc95378586701cbe978277a65906f6893efec7460e613b3d533636dda745` appears verbatim in `TASK.md`.
  `gate-B2-r2.md` (`d309c78e…dbdf`) and `gate-B2-r1.md` (`3ff95963…91ce`) are unchanged. r3
  differs from r2 only in D9 "Where". HEAD `523732b7`.
- Recheck round `01DEP-P03-G2-R3` (2026-11-10) reviewed decision revision **r4**:
  `logs/gate-B2-r4.md` sha256 `51d4dee8d53b30ff8033d8373b06f07024bd26fcbcdf193c61062f9e74fd8553`,
  verbatim in `TASK.md`; r1–r3 copies unchanged. The r3 → r4 diff touches only the preface, the
  D9 "Where" lock list, D9 "What" (non-agent lead), "Re-point" (source comparison),
  "Run-as" (the residual) and three verification rows.

---

## 01DEP-P03-G2-F1 — Re-leading takes other users' locks after the desk gates, which can deadlock with an account deletion

- **Classification:** actionable defect · **Medium** · lock protocol. D9 (with D7).
- **Evidence:**
  - D9 runs "after the grant sync", which means after `write_placement_on` has taken, in order:
    - the department row `FOR UPDATE`;
    - the involved principals `FOR KEY SHARE`;
    - in `rederive_on`, the eligible principals, then the gates of the reached members-only desks
      (exclusive), then those desks' rows `FOR UPDATE` (`department_desks.rs:230-262`).
  - D9 then reuses the user-facing binding, reply and publication writers. Their authorization
    locks other users' rows:
    - `save_binding_on` → `check_run_as` → `authorize_company(run_as)` takes `company_members` and
      `principals` `FOR SHARE OF member, principal` for the run-as (`authority.rs:45-68`,
      `bindings.rs:181-200`). For an allowlist desk, `authorize_association` also locks the
      run-as account (`association.rs:37-46`).
    - D9 keeps "same params/run_as". The run-as is whoever provisioned the desk: the department's
      creator for a D8 desk. When another admin re-leads, that is a different user.
    - Re-selecting "the same contract and approval policy" re-validates reviewers
      (`validate_reviewers` → `authorize_reviewer_on`: `company_members`/`principals FOR SHARE`).
      Factoring `configure_reply_on` out of `configure_channel_reply` would also bring
      `lock_settings_accounts` (`users … FOR SHARE`) after the company lock.
  - Account deletion locks in the opposite order (`user_lifecycle.rs:14-38`): `users FOR UPDATE`,
    then `company_members FOR UPDATE`, then `UPDATE principals SET kind='external'`. That update
    fires `department_principal_demoted`, which deletes department rows and grants and updates
    `channels.settings_revision` (schema 15498-15519).
  - **Concrete cycle.** Let R be the stored run-as (or a reviewer) and a member of a department in
    the role's reach.
    - The re-lead already holds R's principal `FOR KEY SHARE` from `rederive_on`, and holds the
      reached desks' rows. It then waits for `company_members(R)` `FOR SHARE`.
    - Deleting R's account holds `company_members(R)` `FOR UPDATE` and waits for R's principal.
      `kind` is a key column, so `FOR KEY SHARE` blocks the update (`department.rs:217-219`).
  - D9 also puts workflow compilation, publication, binding and reply writes after the gates. That
    is the gate-hold latency G1-F1 removed from B1.
  - The gate states no lock sequence for D9. The one it states for D8 stops at the role write.
- **Required:**
  - Run the desk effect before the grant sync. Better, before `lock_principals_on` (for example,
    first in `write_placement_on`, after the department row), so the gates stay last.
  - State D9's full lock sequence and show that it composes with account deletion and member
    removal (the demotion trigger), with agent publication and with inbound commits.
  - Use run-as = actor. That is what D7 says, and it makes `authorize_company` and
    `authorize_association` re-entrant (see G2-F2).
  - For any other account D9 must authorize (approval-policy reviewers), either lock it in the
    account phase at the start of the write, or do not re-validate it on a re-point.
- **Instruction lesson:** (observation) reusing a user-facing writer inside a transaction carries
  in its authorization locks on other users' rows. Candidate: a gate that reuses user-facing
  writers inside a bigger transaction lists the rows those writers' authorization locks.
- **Resolution history:** 2026-11-10 open (G2-R1). 2026-11-10 **resolved** (G2-R2, r3 D9 "Where"), with my r1 advice corrected:
  - The desk effect stays after the grant sync, and the full lock sequence is stated. r3 is right
    that my r1 "run it before the sync" is unsafe for a members-only department desk. The
    reply-settings write would lock that desk's channel row `FOR UPDATE` before the sync takes
    its gate. An admission holding the gate shared and then taking the row's FK lock, or a
    demotion updating the row, closes a cycle.
  - The "after" argument holds: inbound takes all gates before any row lock and never locks
    workflow bindings (its `binding_id`s are channel bindings; intents have no FK to
    `workflow_bindings`). Settings, binding and agent writers hold the company lock.
  - The cost, gates held through compile and publication, is stated.
  - The premise is "D9 authorizes only the actor". That premise is false on one path, tracked
    in G2-F2.

---

## 01DEP-P03-G2-F2 — A stored run-as, a reviewer or desk visibility can make an unrelated lead change fail with a confusing error

- **Classification:** actionable defect · **Medium** · contract / failure behaviour. D9 (vs D7).
- **Evidence:**
  - D7 says "the desk's binding runs as the actor who made the change". D9 case 1 keeps "same
    params/run_as/trigger". The two contradict each other.
  - `check_run_as` (`bindings.rs:181-215`) re-checks the stored run-as on every configuration and
    activation. It must be a current owner or admin who can see the desk. Otherwise:
    `BadRequest("The binding's run-as must be a current owner or admin who can see its channel")`.
  - So when the department's creator is demoted or leaves, every later agent-to-agent re-lead of
    that department is refused with a message about a binding the person never saw.
  - The same happens for approval-policy reviewers that are no longer admins: "Final reply
    reviewers must be company admins".
  - **Visibility.** Department writers are owners or admins (`begin_department_write` →
    `lock_company_on`). But if a user makes the department desk members-only (an ordinary desk
    setting), a non-owner admin who is not a member, lead or parent lead has no view grant.
    `authorize_association(actor)` in `save_binding_on` and in reply configuration then returns
    `NotFound` (`association.rs:33-51`). The route answers 404 to a role change on a department the
    admin can see.
  - **Resources.** `provisioned_binding` refuses resource slots (`lifecycle/provisioned.rs:46-50`).
    D9's `provisioned_reconfiguration` does not say whether it carries the binding's resources or
    refuses a workflow that declares them.
- **Required:**
  - Make run-as = actor for every binding D9 configures or activates, as D7 states. Write down that
    attribution moves to the admin who made the role change.
  - State the outcome, with a worded error, for an actor who cannot see a members-only department
    desk. Either refuse with a message that names the remedy, or define a trusted path.
  - State how the approval policy and resources are carried over or refused.
  - Add verification rows: re-lead by a second admin after the creator lost admin; re-lead of a
    members-only department desk by an admin without a grant.
- **Resolution history:** 2026-11-10 open (G2-R1). 2026-11-10 **partly resolved, still open** (G2-R2, r3):
  - Done: run-as = actor on reconfiguration; worded refusals for a missing `draft` step,
    resource slots, an approval policy and an invisible desk; the attribution move is stated.
  - Remaining: when D9 activates or selects the owned binding B *without* re-pointing it, B
    keeps its stored run-as. That happens when B is paused and already runs the new lead (agent
    A → person → A), or B is active, runs A, and a user cleared the selection.
  - On that path:
    - `validate_change` requires an Activate change to keep the configuration (`bindings.rs:93-112`),
      and `check_run_as` re-authorizes the stored run-as (`:181-200`);
    - selection runs `selected_on` → `authorize_company(configuration.run_as())`
      (`channel_reply.rs:233-251`);
    - a configure factored from `configure_channel_reply` adds `lock_settings_accounts` on the
      run-as (`:166-185`).
  - So another user's `company_members`/`principals`/`users` rows are locked after the gates
    and rows. That reopens the G2-F1 cycle against account deletion, and refuses the change if
    that user is no longer an admin.
  - **Required:** before activating or selecting B, reconfigure it (configuration revision + 1,
    same version, run-as = actor) whenever its stored run-as differs from the actor. Then every
    D9 path authorizes only the actor. Add a row: A → person (admin X) → A by admin Y, after X
    lost admin.
  2026-11-10 **resolved** (G2-R3, r4): before any activation or selection, B is reconfigured to run-as = actor
  whenever its stored run-as differs: configuration revision + 1, same or re-pointed version, same
  params, trigger and active flag. This checks out against the code:
  - `validate_change` accepts that Configuration change, since the active flag is unchanged.
  - `check_run_as` passes with run-as = actor and no owner check. The following Activate keeps
    run-as = actor (`chosen` is false).
  - `selected_on` and `lock_settings_accounts` then name only the actor.
  - Deactivate skips `check_run_as`, and clearing the selection locks only the actor.
  - A Deactivate still runs `authorize_association(actor)`, so an invisible members-only desk is
    refused with D9's worded message there too. That is consistent.
  - The new row covers A (X) → person → A (Y) after X lost admin.

---

## 01DEP-P03-G2-F3 — Re-pointing publishes the workflow's current draft, including unpublished edits

- **Classification:** actionable defect · **Medium** · correctness. D9 case 1.
- **Evidence:**
  - D9 case 1: "read the workflow's current draft; `retarget_agent(...)`; save the draft …
    at revision+1; `compiled` … ; `provisioned_publication` + `publish_on`".
  - A workflow keeps one revisioned draft, separate from its published versions
    (`definitions.rs:130-161`). `publish_on` publishes exactly the current draft (`:183-207`).
  - So a role change would publish whatever unpublished edits a user left in the desk workflow's
    draft, and bind the desk to them, without review.
  - If the draft is invalid (saved mid-edit), the compile fails and the role change is refused for
    an unrelated reason.
  - The alternative, retargeting the bound version's source, would overwrite the user's draft. The
    gate picks neither deliberately.
- **Required:**
  - Retarget the source of the version the responder binding is bound to.
  - When the current draft differs from that version (unpublished edits), either refuse with a
    worded error, or keep the draft untouched (if the lifecycle allows publishing from another
    source). State which.
  - Add verification rows: a re-lead with an unpublished draft edit, and with an invalid draft.
- **Resolution history:** 2026-11-10 open (G2-R1). 2026-11-10 **resolved** (G2-R2, r2/r3 D9 "Re-point"): the source re-targeted is the
  bound version's, and unpublished edits refuse with a message. Refinement in G2-F8.

---

## 01DEP-P03-G2-F4 — D9 acts on any selected responder, including one a user set up, and the slug-equality fallback can pick the wrong desk

- **Classification:** actionable defect · **Medium** · correctness / scope. D8 identity, D9 cases 1
  and 3.
- **Evidence:**
  - D9 defines "Responder = D's channel reply settings' selected binding". It acts on that binding
    whoever created it:
    - Case 3 (person lead or no lead) deactivates it. A person-to-person lead change, or clearing
      a person lead, would therefore pause a responder a user configured on the department desk.
    - Case 1 re-points a user's own workflow if it has a literal `draft` agent step. Otherwise it
      refuses the whole role change.
  - **Identity fallback** (D8): without the `department:{id}` row, the department desk is "the
    channel with `department_id = id` whose primary slug equals the department's slug".
    - Department slugs are mutable (`update_department_on` sets `slug`, `department.rs:322-351`),
      and "Department rename does not rename the desk".
    - After a slug change, a person-led or unled department's plain desk is no longer found, so the
      next agent lead provisions nothing. That contradicts the verification row "renamed department
      keeps desk".
    - Another desk the department owns at the new slug becomes "the department desk". D9 would
      then provision on it, re-point it or pause it.
  - Plan 03B.2 prescribes slug equality for desks provisioned without `assistant_channel_on`, so a
    durable marker for the plain desk would be a plan deviation.
- **Required:**
  - Limit D9 to bindings that D8 or D9 provisioned. Record them, for example the binding in the
    `department:{id}` command result, and have case 2 write that row when none exists. Leave any
    other selected responder untouched. Decide whether a lead change then refuses or only reports.
  - Act only when the desired responder changes: agent A → agent B, agent → non-agent, or
    non-agent → agent. Person → person changes nothing.
  - For identity: either accept and document that a slug change detaches a plain desk's identity,
    with a verification row asserting that no desk is touched; or record identity for the plain
    desk too. The latter deviates from 03B.2's wording, so get coordinator/user confirmation first.
  - Add verification rows: a user-configured responder on the department desk survives a
    person-to-person change and a person-to-none change; another owned desk at the department's new
    slug is not touched.
- **Resolution history:** 2026-11-10 open (G2-R1). 2026-11-10 **resolved** (G2-R2, r2/r3 D8 "The record" + D9 "What"):
  - The `department:{id}` command row marks both desk kinds; the plain desk's row has
    `binding_id: null`. There is no slug fallback, and no new column or trigger.
  - D9 acts only on the owned binding B, and only when the desired state differs. A
    user-selected responder S ≠ B is never touched. Person → person and person → none change
    nothing.
  - The marker choice is recorded as a plan/code mismatch. I accept it as within 03B.2's "no new
    column or trigger". D9 now rewrites the command row's `result.binding_id`. The table comment
    should say a `department:{id}` result can change (not only be replayed).

---

## 01DEP-P03-G2-F5 — The B2 gate has no contract-impact table, and does not record the plan/code mismatches

- **Classification:** actionable defect · **Low** · process / contract coverage (skill:
  "records its criterion-to-assertion mapping and contract impact in TASK").
- **Evidence:**
  - Unlike B1 ("Contract impact B1"), r1 has no contract-impact table. B2 changes contracts with
    consumers:
    - Department create now also creates a desk. Affected: route and JSON responses, UI, onboarding
      (its first department now gets a desk at its slug, before the agents), and the p01/p02 tests
      and fixtures that count channels or create departments (`Departments::create`).
    - Department delete is now refused until the desk is detached or deleted, so every department
      is affected. Affected: the UI delete flow, and the p01/p02 delete tests.
    - The `_on` signatures gain `templates` and `actor`. Callers: `company_agent.rs`,
      `provisioning_builder.rs`, `persistence/onboarding.rs`, and the department routes and use
      cases.
    - `DepartmentPersistence` construction moves (setup, tests).
    - The provisioning-commands table comment gains `department:{id}`.
  - Plan/code mismatches the gate should record:
    - Plan 03B.3 says to reuse "the publication path the Agent Builder uses when it edits an
      existing agent channel's workflow (`apply`, `provisioning.rs:192`)". `apply` has no edit
      path, only `ListAgents` and `CreateAgentChannel`.
    - `REPLY_AGENT_STEP` is `"draft"` (`templates/catalogue.rs:14`), not `reply`.
    - `AssistantAgent::Existing` no longer exists after B1 (`AssistantChannelRequest.agent_slug`).
- **Required:**
  - Add "Contract impact B2", with each consumer's disposition and focused check.
  - Record the three mismatches.
  - Note the onboarding collision: a selected agent whose slug equals the new department's slug is
    now `Rejected`, because the address is taken.
- **Resolution history:** 2026-11-10 open (G2-R1). 2026-11-10 **resolved** (G2-R2): "Contract impact B2" is added, and the three plan/code
  mismatches plus the marker choice are recorded at the top.

---

## 01DEP-P03-G2-F6 — Each agent → person → agent cycle leaves another workflow and inactive binding behind

- **Classification:** optional suggestion · Low · design simplification. D9 cases 2 and 3.
- **Evidence:**
  - Case 3 pauses the binding and clears the selection. The next agent lead is case 2: a new
    workflow, version and binding under a fresh `department:{id}:lead:{uuid}` key.
  - So each cycle adds one inactive binding and one workflow. The desk then cannot be deleted
    until the user deletes them all, because desk deletion refuses a desk with bindings.
  - Case 1's machinery (re-point, activate if paused, reselect) already handles a paused binding.
- **Suggestion:** in case 2, when a D9-owned paused binding exists on the desk (G2-F4's record),
  re-point and reactivate it instead of provisioning a new one. Keep fresh provisioning for a desk
  with none.
- **Resolution history:** 2026-11-10 open (optional). 2026-11-10 **resolved** (G2-R2): a paused B is re-pointed if needed and reactivated, never
  re-provisioned. Case 2 provisions at most once per department, under the fixed key
  `department:{id}`. Bindings cannot be deleted in production, so B persists.

---

## 01DEP-P03-G2-F7 — Add verification rows for the onboarding desk and the plain-desk insert

- **Classification:** optional suggestion · Low · verification.
- **Suggestions:**
  - Onboarding: the first department gets its plain desk. A selected agent whose slug equals the
    department slug gives `Rejected` with the taken-address message, and nothing is committed.
    The replay creates no second desk.
  - D8 person path: mirror `ChannelPersistence::create` for an owned desk
    (`prelock_ownership_on` → `insert_channel_row_on` → `sync_desk_grants_on`, `channel.rs:349-363`),
    rather than inserting the ownership columns without the p02 writer. State it in D8.
  - e2e: after person → agent, a scripted message to the department desk is answered by the new
    lead. The plan names the e2e only for agent → agent.
- **Resolution history:** 2026-11-10 open (optional). 2026-11-10 **resolved** (G2-R2): rows added for the onboarding desk, slug collision and
  replay; the plain desk uses the owned-desk create sequence; and an e2e answer after person →
  agent.

---

## 01DEP-P03-G2-F8 — The "unpublished edits" check compares revision numbers, so its suggested fix doesn't clear it

- **Classification:** actionable defect · **Low** · failure behaviour. r2/r3 D9 "Re-point".
- **Evidence:**
  - D9 refuses when "the workflow's current draft revision ≠ the bound version's `draft_revision`",
    with "publish or discard them before changing the lead".
  - There is no discard operation for a saved workflow draft. The editor's "Reload saved draft"
    only drops unsaved browser changes (`pages/workflow_authoring/editor.rs:196`).
  - The only way to "discard" is to save the bound version's content again. That raises the draft
    revision, so the check still refuses.
  - Publishing alone does not satisfy the check either: B stays bound to the old version until
    someone reconfigures it. So a user who published a newer version without rebinding gets the
    "unpublished edits" message.
  - `workflow_versions.draft_revision` exists (schema 5863-5867), so the check is implementable. It
    is just stricter than its message suggests.
- **Required:**
  - Compare the current draft's *source* with the bound version's source, not the revision
    numbers. Then restoring the published content clears the refusal.
  - Word the remedy with operations that exist. For example: "…has edits that are not on its
    desk's bound version; publish them and bind the desk to the new version, or restore the bound
    version's content, before changing the lead."
  - Add a row: restore the content, then re-lead; it goes through.
- **Resolution history:** 2026-11-10 open (G2-R2).
  2026-11-10 **resolved** (G2-R3, r4): the check compares the draft source with the bound version's source; the
  remedy names existing operations (publish and bind, or restore); a row asserts that re-saving
  the bound content clears it.

---

## 01DEP-P03-G2-F9 — An owned binding left active under a person lead, and the bracketed lock order

- **Classification:** optional suggestion · Low · D9 "What" / "Where" wording.
- **Evidence and suggestions:**
  - A person lead or cleared role pauses B only when S = B and B is active. If a user cleared the
    selection (S none) or chose their own S while B stayed active, B keeps running the previous
    agent on every message, with its output unsent. Consider deactivating an active owned B
    whenever the desired state is "no agent", while still leaving S alone.
  - r3 lists the desk effect's locks as "[channel row, reply settings, binding rows, workflow rows,
    agent row]". The re-point procedure runs them in the opposite order: `publish_initial_on(A)`,
    then draft, publish, binding, and the channel row and settings last. That is the order every
    existing provisioning path uses. State the bracket in procedure order, so the implementation
    and the lock argument match.
- **Resolution history:** 2026-11-10 open (optional).
  2026-11-10 **resolved** (G2-R3, r4): with a non-agent lead, an active B is always deactivated, and the selection
  is cleared only when S = B; a row covers an active, unselected B. The "Where" lock list is in
  procedure order (agent → workflow → binding → channel row → settings), and says only the
  actor's account is locked or authorized. I verified that for every D9 step:
  `publish_initial_on`, `save_draft_on`, publish, binding changes, `configure_reply_on`,
  `bootstrap_native_on`/`selected_on`, and allowlist `authorize_association` (`FOR SHARE OF
  access_grant, account` — the actor's own rows).

---

# Batch B2 review — round `01DEP-P03-B2-R1`

Common context for the B2 findings below:

- Date 2026-11-10; batch B2 (plan 03B: the department desk and re-leading), implemented against
  gate r4 (`logs/gate-B2-r4.md` `51d4dee8…8553`); stage: batch review; round `01DEP-P03-B2-R1`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, base/HEAD `523732b7`, uncommitted. Artifact
  digest `8fd91d0452986625ad448afcdbe382b456c75bcaeaa9cddef117523947535c9b` (recomputed). Evidence
  `logs/01DEP-P03-B2-R1.evidence.json` is valid.
- Coordination revision 3 (`880cac54…04b8`).
- Recheck round `01DEP-P03-B2-R2` (2026-11-10): artifact digest
  `31d169402e1d795115670b31b6399674904c1f0d41463afb0bdbe0c1378139ea` (recomputed), base/HEAD
  `523732b7`; evidence `logs/01DEP-P03-B2-R2.evidence.json` is valid.
- B2-F1 and B2-F2 were reported by the coordinator (A) while preparing B3. I confirmed both
  against the plan and the code, and record them here so they go into the same correction round.

---

## 01DEP-P03-B2-F1 — B2's tests use the wrong prefix, which also selects p02's tests (reported by A)

- **Classification:** actionable defect · **Low** · verification naming.
- **Evidence:**
  - Plan 03 "Focused checks" names B2's check `department_lead_desk_`.
  - Gate r4's verification plan, and the 11 tests in `department_lead_desk_tests.rs`, use
    `department_desk_`. That prefix is already p02's (`department_desk_ownership_*`,
    `department_desk_members_only_*`, in `department_desks_tests.rs` and
    `channel_department_tests.rs`).
  - So TASK's "30 `department_desk_`" counts p02 tests. My
    `-- department_desk_competing agent_default_desk_competing` run selected 7 tests, which
    includes p02's competing tests.
  - I passed gate r4 without checking its prefix against the plan.
- **Required:** rename B2's tests to `department_lead_desk_` and `department_lead_desk_competing_`.
  Update TASK's verification mapping and inventory, and re-list the nonzero counts.
- **Resolution history:** 2026-11-10 open (B2-R1). 2026-11-10 **resolved** (B2-R2, artifact `31d16940…39ea`): B2's tests are renamed. `--list department_lead_desk` gives 14, one of
  them `department_lead_desk_competing_`, and the competing filter now selects 4 tests (1 + B1's 3)
  instead of 7. The remaining `department_desk_*` names belong to p02 (`department_desks_tests.rs`,
  `department_desk_relay_tests.rs`).

---

## 01DEP-P03-B2-F2 — No test checks that a lead agent's own default desk is left alone (reported by A)

- **Classification:** actionable defect · **Low** · verification coverage.
- **Evidence:**
  - Plan 03's `department_lead_desk_` check requires: "Provisioning or re-leading never replaces
    either lead agent's separate default desk."
  - No B2 test asserts it. In code, D9 only touches the department desk's owned binding B
    (`provisioning_department.rs:229-309`), so the property holds today, but nothing pins it.
- **Required:** after creation as lead, agent → agent, and agent → person, assert that each lead
  agent's `agent:{id}` desk and binding are unchanged. That means the same binding id, active, the
  same configuration revision, and still selected.
- **Resolution history:** 2026-11-10 open (B2-R1). 2026-11-10 **resolved** (B2-R2, artifact `31d16940…39ea`): the `own_desk` helper captures the `agent:{id}` desk, its binding state and
  the selection. It is asserted unchanged after creation as lead, after agent → agent (both leads)
  and after agent → person → agent, and an agent created as lead keeps its own desk active and
  selected.

---

## 01DEP-P03-B2-F3 — The branch that leaves a user-chosen responder alone is untested

- **Classification:** actionable defect · **Low** · verification coverage (gate r4 row "user's own
  responder").
- **Evidence:**
  - TASK lists "a user's own responder S ≠ B" as a stand-in gap, "covered by code review only".
  - The branch is the safety property G2-F4 introduced (`provisioning_department.rs:273-275` for
    an agent lead; the person path deactivates an active B but leaves S). It is the one place B2
    must not change user configuration, and gate r4 has a verification row for it.
  - A second responder is reachable with existing primitives: a published workflow bound to the
    department desk, activated, and selected through `configure_channel_reply`. Those are the
    same lifecycle calls that `channel_reply_binding_integrity_tests.rs` makes.
- **Required:** a `department_lead_desk_` test with a user's selected responder S ≠ B on the
  department desk. Assert that agent → agent leaves S selected and S and B unchanged, and that
  agent → person leaves S selected and unchanged (B deactivated if it was active, per r4).
- The other two stand-ins (no `draft` step, resource slots) are one-line guards
  (`provisioning_department.rs:369-373, 385-389`). Covering them by review is acceptable.
- **Resolution history:** 2026-11-10 open (B2-R1). 2026-11-10 **resolved** (B2-R2, artifact `31d16940…39ea`): `department_lead_desk_leaves_a_users_own_responder_alone`. A user's own
  binding is configured and activated through `BindingService` and selected through
  `configure_channel_reply`. It stays selected and unchanged through agent → agent and agent →
  person; the owned binding is unchanged by agent → agent and paused by agent → person, as r4
  requires.

---

## 01DEP-P03-B2-F4 — The pause path runs in a different order from the gate's lock list

- **Classification:** optional suggestion · Low · consistency.
- **Evidence:**
  - For a non-agent lead, `follow_department_lead_on` first clears the selection
    (`configure_reply_on`: desk row `FOR UPDATE` and settings), and only then deactivates B
    (`provisioning_department.rs:253-271`). r4's "Where" list says binding → channel row →
    settings.
  - This is harmless. Every transaction that locks `workflow_bindings` rows holds the company
    lock first. Admission's `authorize_company` takes `companies FOR NO KEY UPDATE`
    (`admission.rs:22`, `authority.rs:52-53`), and inbound commits never lock workflow bindings.
- **Suggestion:** deactivate first and then clear the selection, or say in the module doc that
  the pause path's order is free because all binding lockers hold the company lock.
- **Resolution history:** 2026-11-10 open (optional). 2026-11-10 **resolved** (B2-R2, artifact `31d16940…39ea`): the pause path now deactivates B first, then clears the selection
  (`provisioning_department.rs:253-272`), which is the gate's lock order.
