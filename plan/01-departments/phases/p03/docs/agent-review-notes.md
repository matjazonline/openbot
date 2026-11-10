# p03 agent review notes

## 01DEP-P03-G1-R1 — architecture gate G1 (B1: 03A automatic agent desks + 03C.0 placement), decision revision r1

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Records: `TASK.md`, `REVIEW.md`,
  `agent-review-findings.md` (this directory).
- Batch B1; stage: architecture gate. Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`.
  Start, base and HEAD are all `9291fcd5ec6406518c7660d2dd673b8766241f86`.
- Gated decisions DEP-P03-D1..D6 **r1**. `logs/gate-B1-r1.md` sha256
  `25d7ee4d26dcfa8c87158e7446454cb5cec5e5c9cce6ad154763cd2bb21672a1`; `TASK.md` is byte-identical
  to it (`cmp`).
- Code snapshot (informational): `119f961ba525f278ad843deb602c440c68c421cc2fd67a68630350f76f6f1e4d`,
  with no changed files.
- Coordination **revision 3**, sha256 `880cac548be56d2a25ac5ec0a430d563705886c7555e82f5aef15dee6cea04b8`,
  equal to the pinned `logs/coordination-r3.md`. Compared with r2, the p02 row is marked accepted and
  C001/C002 show p02 → satisfied; p03 is pending on both. I001–I003 apply. I checked this at the start
  and before returning.

### Coverage and evidence

- **Requirements:** 03A.1–5, 03C.0, the focused checks `agent_default_desk_` and
  `agent_default_desk_competing_`, README decisions 8 and 12, and plan 05's provisioning-commands
  comment. I read 03B and 03C.1–6 only for what B1 must not block.
- **Code facts verified:**
  - the four production company-agent inserts:
    - `AgentPersistence::create` (`agent.rs:320-336`): no lock, no publication;
    - library copy (`channel.rs:505-534`): users `FOR SHARE` → `authorize_company`;
    - onboarding `Session` (`persistence/onboarding.rs`): users → companies `FOR NO KEY UPDATE` →
      member, with a 45 s `lock_timeout`;
    - Builder `provision_agent_on(New)` under `provision_guarded`;
  - `assistant_channel_on` (`provisioning.rs:812-881`): `Existing` needs provider and model; the
    digest includes the caller; the command row is replayed by key;
  - `provision_agent_on` publishes an `Existing` agent itself, so D1 step 4 is redundant but
    harmless;
  - `publish_initial_on`, `ready_models_on` (keyed connections, default first) and
    `pinned_default_model` (default with key);
  - `validate_model_selection` does not require a key; D3 adds that requirement in persistence;
  - onboarding replay through deterministic ids; the Builder's `apply` refusal contract (tool
    errors, savepoint);
  - the Builder locks the requester's account before the company row (`action_authority.rs:75`,
    `delegated_provisioning_locks_requester_before_company`). So D1's account-before-company
    precondition holds on every path, which corrects my p01 G1-F7 remark.
- **Lock order:** analysed by inspection (→ F1). No deadlock found: the locks after the gates are
  on new rows or are re-entrant. But D1 gives no lock-sequence argument, and its step order breaks
  B2's create-as-lead path.
- **Not executed:** no cargo or SQL. This is a gate, and the decisions are about ordering and
  contracts.
- **Reviewed lightly:** template compile and workflow publication internals inside provisioning
  (lock footprint inferred from `provision_on`/`provision_workflow_on` and their `FOR KEY SHARE`
  reads).

### Outcome

**Changes required** for decision revision r1. There is no PASS.

- Actionable:
  - 01DEP-P03-G1-F1 (Medium): D1 does placement before publication and the desk. That blocks B2's
    create-as-lead re-pointing and holds desk gates through provisioning;
  - 01DEP-P03-G1-F2 (Medium): the Builder path would lose its tool-error refusal contract and its
    savepoint (risking a poisoned action transaction); the `channel_name` rule at the same slug is
    unstated;
  - 01DEP-P03-G1-F3 (Medium): onboarding department creation is not idempotent on replay, and the
    no-keyed-model onboarding outcome is not stated;
  - 01DEP-P03-G1-F4 (Low): the no-bypass guard checks the wrong function, and the fate of the
    `LibraryAgentCopyPersistence` port is unstated.
- Optional: F5 (create-as-lead and per-path duplicate-slug verification rows).
- **Sound as written:**
  - D1's single transaction, `Existing` (no recursion), the `agent:{id}` key and the `Box::pin`
    seam;
  - D2's explicit refusal when no port is configured;
  - D3 pinning (plan-explicit consequence: no agent before a keyed model);
  - D4's rollback on refusal and the `agent:{id}` readback;
  - D5's same-slug/different-slug rule (apart from `channel_name`);
  - D6.
- No item needs the user.

### Process and instruction feedback

- The gate's "plan/code mismatches resolved here" preface and its code citations were accurate and
  made verification quick.
- Candidate instruction (scope: gates of staged batches): list the preconditions that later
  batches' extensions of a called function will need, and order steps to satisfy them
  (evidence F1).

## 01DEP-P03-G1-R2 — architecture gate G1 recheck (B1), decision revision r2

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B1; stage: architecture gate
  recheck. Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`,
  HEAD `9291fcd5ec6406518c7660d2dd673b8766241f86`; the only change is the untracked phase record
  directory `plan/01-departments/phases/p03/`.
- Gated decisions DEP-P03-D1..D6 **r2**: `logs/gate-B1-r2.md` sha256
  `ab506aa02116ccf96f3b5c25556168015deead36276e734efb33c087ead502e8`; `TASK.md` is byte-identical
  to it (`cmp`). `logs/gate-B1-r1.md` unchanged (`25d7ee4d…72a1`).
- Coordination **revision 3**, sha256 `880cac548be56d2a25ac5ec0a430d563705886c7555e82f5aef15dee6cea04b8`,
  equal to `logs/coordination-r3.md`; I001–I003, C001/C002 apply as in R1. Checked at the start and
  before returning.

### Coverage and evidence

- Read r2 D1–D6, "Verification plan B1" and "Contract impact B1" in full and checked each of
  G1-F1..F5 against them and the code:
  - **F1:** step order and per-step locks against `agent_publication/mod.rs:50-115` and
    `capture.rs` (shared reads only), `department.rs` (`_on` functions: department `FOR UPDATE`,
    `lock_principals_on`, then `sync_department_grants_on`), and every `acquire_channel_gates_on`
    caller (none in provisioning). Placement's gates are the last and only gates.
  - **F2:** r2 D5's mapping against `provision_guarded`/`taken_slug` (`provisioning.rs:489-530`) and
    `ProvisioningEffects::apply` (`:192-237`). Builder retries are answered by the action receipt
    (fence tests in `provisioning_dispatch_tests.rs`), so D1's deterministic Builder id is never
    re-inserted by a retry.
  - **F3:** `agent_id`/`selection` (`use_cases/onboarding.rs:250-330`), `OnboardedAgent.channel`
    (`:74-78`), the page variants `NoModel`/`Refused`/`Missing` (`pages/onboarding.rs:150-160`), the
    route readback (`routes/onboarding.rs:258-330`) and the provisioning-commands table (no FK to
    channels, so the stored `agent:{id}` result survives a desk deletion).
  - **F4:** grep of every `insert_agent_on(` / `create_agent_principal_on(` call site; the
    `AgentPersistence` port (`use_cases/agent.rs:106-127`), its impls and its production and test
    callers (48 files, all unit tests or `#[path]`/`cfg(test)` fixture modules; `tests/` has none).
  - **F5:** the two new rows.
- **Not executed:** no cargo or SQL; the recheck is about decision text and its code premises.

### Outcome

**Changes required, narrowly**, for decision revision r2. There is no PASS yet.

- Resolved: 01DEP-P03-G1-F1, F2, F3 and optional F5.
- Still open: **01DEP-P03-G1-F4 (Low)**. The scan targets the right functions and the library-copy
  port is gone, but the allowlist admits all of `persistence/agent.rs`, which keeps a production
  company-agent insert on the `AgentPersistence` port (`create`). r2's backstop, a source test that
  `create` "has no non-test caller", is not decidable for a trait method named `create`. Required:
  make that insert test-only by construction (`#[cfg(test)]` on the port method and its two impls,
  or a test-only helper), so the compiler enforces it. A one-line gate note or r3 suffices.
- Observations, not findings:
  - capture reads skills `FOR SHARE`, not `FOR KEY SHARE` as D1 step 3 says; both are shared and
    precede every department lock;
  - a resubmission with the same key and an edited department name reuses the first department
    under its first name (the id excludes the name); that is acceptable, but worth a test
    assertion if intended.
- No item needs the user.

### Process and instruction feedback

- r2 answered each finding by ID, kept r1 pinned and added the requested rows, which made the
  recheck a direct comparison.
- Candidate instruction (scope: no-bypass guards in gates): when a guard is a source scan, say what
  makes each allowlisted file safe; for a symbol the scan cannot type-resolve (a trait method),
  prefer `cfg(test)` or visibility over a textual "no caller" assertion (evidence F4).

## 01DEP-P03-G1-R3 — architecture gate G1 recheck (B1), decision revision r3

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B1; stage: architecture gate
  recheck. Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, HEAD `9291fcd5ec6406518c7660d2dd673b8766241f86`;
  the only change is the untracked phase record directory; source unchanged.
- Gated decisions DEP-P03-D1..D6 **r3**: `logs/gate-B1-r3.md` sha256
  `14f1ffe03da2ce8c43fe3bf4c822c3309ba66597adff85bd8548ba3ef88d5aef`, byte-identical to `TASK.md`
  (`cmp`). `gate-B1-r1.md` (`25d7ee4d…72a1`) and `gate-B1-r2.md` (`ab506aa0…02e8`) unchanged.
- Coordination revision 3, `880cac548be56d2a25ac5ec0a430d563705886c7555e82f5aef15dee6cea04b8`, equal
  to the pinned copy. Checked at the start and before returning.

### Coverage and evidence

- `diff gate-B1-r2.md gate-B1-r3.md`: the changes are the revision heading/preface, the D1 step 3
  skills lock (`FOR SHARE`), D1 "No bypass (r3, F4)", the onboarding-replay verification row
  (edited name keeps the first department), the no-bypass verification row, and a Status line. No
  other decision text changed.
- **F4:** checked that `#[cfg(test)]` on `AgentPersistence::create` is feasible and decisive:
  - the only impls are `persistence/agent.rs:319` and the mock at `use_cases/agent_tests.rs:35`;
  - Cargo.toml has no features, and there are no benches or examples; `tests/` does not use the
    port;
  - every caller outside `use_cases/agent.rs:298` is in a `#[cfg(test)] mod tests` (`agent.rs`,
    `channel.rs`, `company.rs`, `routes/onboarding.rs`) or in a fixture loaded by `#[path]` from a
    test file or a `#[cfg(test)] mod`;
  - `use_cases/agent.rs:298` (`AgentUseCases::create_agent`) is the production call D2 replaces.
    A leftover call would fail the lib build, which offline compilation already exercises.
- **Observation edits:** `capture.rs:147` reads skills `FOR SHARE`, which matches the new wording.
  The onboarding-replay row now asserts the edited-name reuse. D4's prose is unchanged; its "a
  replay finds that id and reuses the department" already implies the reuse, so the row is the
  assertion.
- **Not executed:** no cargo or SQL. This is a decision recheck.

### Outcome

**PASS** for decision revision r3 (`gate-B1-r3.md` sha256
`14f1ffe03da2ce8c43fe3bf4c822c3309ba66597adff85bd8548ba3ef88d5aef`). 01DEP-P03-G1-F1..F5 are all
resolved, and no finding is open. No item needs the user.

### Process and instruction feedback

- A narrow r3 that changes only the named items, with a pinned diff-able r2, kept the recheck small.
  Diffing the pinned revisions is the quickest way to confirm "no other decision text changed".

## 01DEP-P03-B1-R1 — batch review B1 (03A automatic agent desks + 03C.0 placement)

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B1; stage: batch review. Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, branch `codex/departments-addressing-plans`,
  base/HEAD `9291fcd5`. The changes are uncommitted: 35 modified files and 4 new source files
  (`company_agent.rs`, `company_agent_tests.rs`, `department_agent_desks_tests.rs`,
  `workflow/provisioning_builder.rs`).
- Artifact: snapshot digest `2f77ca9fc17c9b5e30a5481686580f8cb71597058ff9cdaa99d6072007d04279`,
  recomputed with `snapshot.sh plan/01-departments/phases/p03/docs plan/01-departments/COORDINATION.md`
  at the start and before returning.
- Evidence manifest `logs/01DEP-P03-B1-R1.evidence.json` validates with `check-evidence.py
  --expect-artifact 2f77ca9f…4279` (declared PASS 3, FAIL 1; the FAIL is the first affected run,
  superseded by `B1-affected-3`).
- Gate r3 `logs/gate-B1-r3.md` sha256 `14f1ffe0…5aef`. Coordination revision 3 `880cac54…04b8`.

### Coverage and evidence

- **Requirements read:** plan 03A.1–5 and 03C item 0; gate r3 D1–D6; TASK "B1 implementation",
  "Verification mapping B1 (actual)" and "Checks B1"; AGENTS.md concurrency and docs rules.
- **Code read in full:**
  - `company_agent.rs`, `workflow/provisioning_builder.rs` and `company_agent_tests.rs`;
  - the `department_agent_desks_tests.rs` helpers and the lead, refusal and competing tests;
  - diffs of `persistence/onboarding.rs`, `use_cases/onboarding.rs`, `use_cases/agent.rs`,
    `model_preparation.rs`, `routes/agent.rs` (`json_placement`), `routes/ui_agents.rs`,
    `routes/onboarding.rs`, `pages/onboarding.rs`, `persistence/workflow/provisioning*.rs`,
    `persistence/channel.rs`, `persistence/department.rs`, `persistence/agent.rs`,
    `infra/workflow/services.rs`, `infra/setup/accounts.rs`, `application/workflow/provisioning.rs`
    and the migration comment.
- **Callers and related code checked:**
  - `department.rs` `_on` writers and `role_reach_on`;
  - `department_desks.rs` `sync_department_grants_on` / `rederive_on`;
  - `channel_gate.rs`;
  - `thread/inbound.rs` `hold_channel_settings`;
  - `agent_publication` locks and `begin_bounded`;
  - the `AppError` mapping of 55P03 / 57014, and its HTTP 503;
  - the model-connection schema (`api_key NOT NULL`; at most one default, with "exactly one
    default" enforced by the use case);
  - `AssistantChannelService` (plain `begin`);
  - the built-in Agent Builder instructions;
  - `docs/agent-authoring.md`, `docs/rig.md`, `docs/agent-builder-context.md` and
    `docs/custom_tools.md`.
- **Executed** (phase DB `openbot_dept_p03_test`, `SQLX_OFFLINE=true`, `--test-threads=4`, with
  the coordinator's go-ahead that nothing else was running):
  - `cargo test --locked --offline --lib -- agent_default_desk`: 21 passed
    (`logs/B1-R1-review-agent-default-desk.txt`);
  - `agent_default_desk_competing_creators` eight times in a row: 8/8 pass, about 0.77 s each
    including fixtures;
  - the author's affected filter set (`onboarding provisioning use_cases::agent routes::agent
    routes::ui_agents skill_tests department assistant_channel desk_wording authorization
    routes::workflows agent_retire agent_constraint persistence::agent:: agent_default_desk`):
    322 passed, 2 ignored (`logs/B1-R1-review-affected.txt`);
  - `cargo check --locked --offline --lib --bins`: clean, so no production caller of the
    `#[cfg(test)]` `AgentPersistence::create` exists;
  - `cargo fmt --all -- --check`: clean.
- **Static measurement:** I replicated the no-bypass scan's truncation rule over `src/`: 41
  production files and 1,813 production lines are unscanned (B1-F3).
- **Not run:** the full repository gate (I003; phase 05), `scripts/stack-budget.sh`, clippy (the
  author reports it clean) and migrations from scratch. A competing deadlock reproduction for
  B1-F1 was not run because it needs a new test; the cycle is shown from code, with line
  citations.

### Answers to the coordinator's challenge list

1. **D1 lock order.**
   - Placement is last within D1, the desk is boxed, and the Builder savepoint nests over
     `provision_guarded`'s savepoint correctly.
   - Defect: placement with a role takes gates in two calls (B1-F1, Medium).
   - Optional: the Builder's extra desk is provisioned after placement (B1-F5).
   - `begin_bounded`'s 1 s lock_timeout is acceptable. D1 runs well under it locally, a timeout
     maps to 503 "The database is busy. Try again shortly.", and department and publication
     writers on the same lock use the same bound. Not a finding.
2. **Builder error mapping.**
   - The mapping matches gate r2 D5 and `provision_guarded`; retries are answered by the action
     receipt.
   - The default-desk rule is correct.
   - `placement_on` counting members ∪ role holders for "belongs to exactly one department" is
     acceptable.
   - The built-in Builder instructions were not updated (B1-F2, Medium).
3. **Behaviour changes.**
   - The library-copy refusal follows 03A.2 ("unresolved publication prerequisite"). Its message
     names the tool ("requires tool alias support.lookup").
   - A model connection is now required everywhere (D3).
   - "Keyed" means any connection: confirmed, since `company_model_connections.api_key` is NOT NULL
     (schema line 3758). The `api_key IS NOT NULL` filter is redundant but harmless.
   - The docs still describe drafts (B1-F4, Low).
4. **Onboarding.**
   - Create-or-reuse by deterministic id, `Rejected` on refusal and the `agent:{id}` readback are
     all correct.
   - Removing `OnboardingChannel::{Refused, NoModel}` is justified, because both became
     unreachable.
   - Edge case: a resubmission that first names a department creates it empty (B1-F6, optional).
5. **Removed and renamed tests.** The four removals in `logs/B1-test-names.txt` each have a
   replacement that asserts the intentionally changed behaviour. Accepted.
6. **JSON `Json<Value>`.** Returning 400 instead of 422 for unknown fields is acceptable: there is
   no backward compatibility, and serde's message is kept. Sibling endpoints still answer 422.
   Not a finding.
7. **No-bypass scan.** It has real blind spots (B1-F3, Low).

### Outcome

**Changes required** for artifact `2f77ca9f…4279`. There is no PASS.

- Actionable:
  - 01DEP-P03-B1-F1 (Medium): two-call gate acquisition when creating as lead;
  - 01DEP-P03-B1-F2 (Medium): the built-in Builder instructions contradict D5;
  - 01DEP-P03-B1-F3 (Low): no-bypass scan coverage;
  - 01DEP-P03-B1-F4 (Low): stale docs.
- Optional: B1-F5 (extra desk after placement), B1-F6 (empty department on a first-named
  resubmission).
- **Sound as written:**
  - D1's transaction contents and rollback;
  - D2's port, its explicit not-configured refusal, and the single production construction
    (`infra/setup/accounts.rs:38`);
  - D3 pinning and the explicit-model check on `tx`;
  - D4's department id, readback and rejection;
  - D5's mapping and same-address rule;
  - D6's command row and comment;
  - the `#[cfg(test)]` create;
  - the test inventory.
- No item needs the user.

### Process and instruction feedback

- The gate decisions pinned per revision and the TASK verification mapping made the review direct.
  The "behaviour changes to report" list saved time.
- Reviewer self-correction: at G1-R2/R3 I checked each placement writer's locks but not their
  composition (B1-F1). Candidate rule: when a decision calls several existing writers in one
  transaction, the gate lists the combined lock acquisitions.
- Candidate rule: contract-impact tables include model-facing consumers (built-in agent
  instructions) of a changed tool (B1-F2).

## 01DEP-P03-B1-R2 — batch B1 correction recheck (B1-F1..F6)

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B1; stage: correction recheck.
  Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, base/HEAD `9291fcd5`, uncommitted.
  Compared with R1, the changed set adds `docs/agent-authoring.md`, `docs/rig.md` and
  `use_cases/builtin_agent_library.rs`.
- Artifact digest `a75c40e7b888d42b415fe7408a319812242e96157e642c594a91ab87c7d55798`, recomputed at
  the start and before returning. Evidence `logs/01DEP-P03-B1-R2.evidence.json` validates with
  `--expect-artifact` (PASS 3, FAIL 2; the two FAILs are the intended mutation runs).
- Gate r3 `14f1ffe0…5aef`; coordination revision 3 `880cac54…04b8`.

### Coverage and evidence

- **Read:** TASK "B1 corrections for 01DEP-P03-B1-R1".
- **F1:** `department.rs` `place_in_department_on` / `write_placement_on` and the rewritten
  `add_department_member_on` / `assign_department_role_on`; every caller of the three; D1's call.
  The competing test reads correctly: it sorts the desk ids so the parent's key is lower and
  asserts that the department's gate is free while the creator waits. The mutation log fails it as
  intended.
- **F2:** the full built-in prompt diff and both prompt tests.
- **F3:** the `syn` walker in full: `test_only`, the visitor and module resolution with
  `#[path]`. I also read the mutation log.
- **F4:** both doc diffs.
- **F5:** `builder_on` in full.
- **F6:** `Session::complete` / `place` and the new onboarding test's assertions.
- **Executed** (`openbot_dept_p03_test`, `SQLX_OFFLINE=true`, `--test-threads=4`):
  - `--list`: 23 `agent_default_desk` tests, 3 of them `competing_`;
  - `agent_default_desk`: 23 passed (`logs/B1-R2-review-agent-default-desk.txt`);
  - `agent_default_desk_competing` six times in a row: 6/6, about 1.4 s each;
  - the affected set plus `builtin_agent_library` and `response_schema`: 329 passed, 2 ignored
    (`logs/B1-R2-review-affected.txt`);
  - `cargo check --locked --offline --lib --bins` and `cargo fmt --all -- --check`: both clean.
- **Not run:** the full gate (I003, phase 05), `scripts/stack-budget.sh`, clippy (the author
  reports it clean) and migrations from scratch. I did no mutations of my own, because I may not
  edit project files; I rely on the author's two mutation logs plus my reading of the code.

### Outcome

**PASS** for batch B1 at artifact `a75c40e7b888d42b415fe7408a319812242e96157e642c594a91ab87c7d55798`.
01DEP-P03-B1-F1..F6 are all resolved, and none is open. No item needs the user.

Residual notes, not findings:

- The `syn` scan cannot see a call through a renamed import, a function pointer or a macro body.
- `docs/agent-authoring.md` lists "a taken address" but not a taken agent slug among the refusals.
- An onboarding replay now re-adds the first agent to the department even if a person removed it
  there in the meantime. That is acceptable: the replay restores the original submission.

### Process and instruction feedback

- Mutation evidence for both new guards (the gate-order test and the scan) made the corrections
  quick to accept. Candidate practice: a new guard ships with a recorded mutation that it catches.

## 01DEP-P03-G2-R1 — architecture gate G2 (B2: plan 03B department desk and re-leading), decision revision r1

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B2; stage: architecture gate.
  Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, HEAD `523732b7` (B1 committed). The only
  changes are `TASK.md` and the new `logs/gate-B2-r1.md`.
- Gated decisions DEP-P03-D7..D9 **r1** and "Verification plan B2": `logs/gate-B2-r1.md` sha256
  `3ff959634457bc807196c3e7219b189a23ceb6b1cd0b8e28435f0177483891ce`. I compared the TASK section
  against it.
- Coordination revision 3 `880cac54…04b8`, unchanged. Checked at the start and before returning.

### Coverage and evidence

- **Requirements:** plan 03B.1–4; README decisions 4, 5, 7, 8, 9, 12.
- **Code read:**
  - `provisioning_native.rs` in full (`provision_on`, `provision_workflow_on`,
    `provision_binding_on`);
  - `channel_reply.rs` (`settings_on`, `bootstrap_native_on`, `configure_channel_reply`,
    `lock_settings_accounts`, `authorize_settings` with its `SET LOCAL` timeouts, `selected_on`
    requiring an active binding);
  - `bindings.rs` (`save_binding_on`, `validate_change`, `check_run_as`, the message-binding cap of
    8 per channel);
  - `lifecycle/provisioned.rs` (`provisioned_binding` refuses resource slots, runs as the target
    actor);
  - `compiler/rebase.rs` `retarget_agent` (a literal-agent pointer only);
  - `definitions.rs` (`save_draft`, `publish_on` publishing the current draft);
  - `association.rs` (an allowlist desk needs a view grant for non-owners);
  - `authority.rs` (`authorize_company` and `authorize_reviewer_on` lock other users'
    member/principal rows);
  - `department.rs` (`write_placement_on`, `update_department_on` slug change,
    `begin_department_write`);
  - `department_desks.rs` (`prelock_ownership_on`, `rederive_on`, `set_desk_ownership_on`);
  - the demotion trigger (schema 15498-15519) and its writers (`user_lifecycle.rs`,
    `company_invite.rs:566`, `agent.rs:405-440`);
  - `channel.rs` owned-desk create sequence;
  - the existence of `channel_reply_binding_inactive_before_ingress_has_visible_refusal_until_selection_cleared`.
- **Checked and fine:**
  - `validate_change` requires the active flag unchanged across a configuration change. D9's
    sequence (reconfigure, then activate, then select) satisfies it and `selected_on`'s
    active-only rule.
  - Agent demotion cannot drop an agent lead silently: published agents cannot be deleted
    (`agent_delete_error`). A demoted person lead needs no desk change.
  - D8's early gate on the new desk in `set_desk_ownership_on` is uncontended, as the gate argues.
  - Inside B1 creation-as-lead, D9 sees a published agent with its desk, as B1 guarantees. Errors
    map through the Builder savepoint and the 400/409 routes as described.
- **Not executed:** no cargo or SQL. This is a gate. The lock cycle in G2-F1 is derived from code,
  with line citations.

### Outcome

**Changes required** for decision revision r1. There is no PASS.

- Actionable:
  - 01DEP-P03-G2-F1 (Medium): D9 runs after the gates and takes other users' authorization locks
    after them, which can deadlock with account deletion; it has no stated lock sequence;
  - 01DEP-P03-G2-F2 (Medium): run-as contradicts D7; a stale run-as, reviewers or members-only
    visibility refuse an unrelated role change; resources are unstated;
  - 01DEP-P03-G2-F3 (Medium): re-point publishes the current draft;
  - 01DEP-P03-G2-F4 (Medium): D9 touches user-configured responders, and the slug-equality
    identity fallback is fragile;
  - 01DEP-P03-G2-F5 (Low): no contract-impact table, and the plan/code mismatches are unrecorded.
- Optional: G2-F6 (reuse a paused binding instead of accumulating them), G2-F7 (verification
  additions).
- **Sound as written:**
  - D7's wrapper and threading;
  - D8's two creation paths, refusal mapping and lock order;
  - D8's desk identity via the command row;
  - the delete rule;
  - D9's case structure and the pause semantics (clearing the selection);
  - the refusal for a workflow with no reply step;
  - the competing and delete verification rows.
- **Possible user item:** G2-F4's durable identity for the plain desk would deviate from plan
  03B.2's wording. If A prefers it over documenting the limitation, it needs confirmation.

### Process and instruction feedback

- Candidate rule (gates that reuse user-facing writers inside a larger transaction): list those
  writers' authorization locks, including other users' rows, and place them in the lock sequence
  (G2-F1).
- Candidate rule: a gate whose effect acts on shared desk state (selected responders) states
  whose state it may change (G2-F4).

## 01DEP-P03-G2-R2 — architecture gate G2 recheck (B2), decision revision r3

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B2; stage: architecture gate
  recheck. Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Round input: the coordinator corrected it from r2 to **r3** mid-review. `logs/gate-B2-r3.md`
  sha256 `bb6dcc95378586701cbe978277a65906f6893efec7460e613b3d533636dda745` appears verbatim in
  `TASK.md`. `gate-B2-r2.md` (`d309c78e…dbdf`) and `gate-B2-r1.md` (`3ff95963…91ce`) are
  unchanged. `diff r2 r3` shows only the heading, the preface and D9 "Where".
- HEAD `523732b7`; only records changed. Coordination revision 3 `880cac54…04b8`. Checked at the
  start and before returning.

### Coverage and evidence

- I read all of r2 and the r3 diff, and checked each of G2-F1..F7.
- **r3's "after the sync" argument.** I tried to break it against the code:
  - The inbound commit takes every gate before any row lock (`thread/inbound.rs:93-128`). Its
    `binding_id`s are channel (transport) bindings, and `workflow_message_intents` has no FK to
    `workflow_bindings` (schema 10893-10925). So no admission holds a row D9 wants while waiting
    on a gate D9's transaction holds.
  - Demotion (`user_lifecycle.rs`) waits on principals that the sync already holds.
  - Settings, binding, workflow and agent writers hold the company lock.
- **r3's "before fails" argument.** It is correct: a members-only department desk's row would be
  locked `FOR UPDATE` before the sync takes its gate. My r1 G2-F1 "run it before the sync" was
  wrong on that point.
- **The residual.** The premise "authorizes only the actor" fails when B is activated or selected
  without a reconfigure. `validate_change` keeps the configuration on Activate (`bindings.rs:93-112`).
  `check_run_as` re-authorizes the stored run-as. `selected_on` authorizes
  `configuration.run_as()` (`channel_reply.rs:251`). `lock_settings_accounts` adds the
  responder's run-as (`:166-185`).
- **F3 refinement.** No draft-discard operation exists (`editor.rs:196` only reloads the saved
  draft). `workflow_versions.draft_revision` exists (schema 5867).
- **F4.** Bindings are never deleted in production, so the owned B persists and case 2's fixed key
  is safe.
- **Not executed:** no cargo or SQL.

### Outcome

**Changes required, narrowly**, for decision revision r3. There is no PASS.

- Still open: **01DEP-P03-G2-F2 (Medium, residual)**. Reconfigure B to run-as = actor before any
  activation or selection that does not re-point it.
- New: **01DEP-P03-G2-F8 (Low)**. Use a source-based check for unpublished edits, with a remedy
  worded in operations that exist.
- Optional: G2-F9 (deactivate an active owned B under a person lead; state the lock bracket in
  procedure order).
- Resolved: G2-F1 (r3 placement accepted), F3, F4, F5, F6, F7.
- The coordinator's fallback, pre-acquiring the department desk's gate in the sync's sorted call,
  is not needed once G2-F2's residual is closed.
- No item needs the user. The F4 marker choice stays within 03B.2's "no new column or trigger",
  and is recorded.

### Process and instruction feedback

- Reviewer self-correction: my r1 Required for G2-F1 prescribed a placement ("before the sync")
  without checking the department desk's own row against its gate. A found the flaw. Candidate
  practice: a reviewer's suggested fix gets the same lock-order check as the author's design.
- Rechecks on a corrected revision go fastest when A supplies the pinned diff, as here (r2 → r3,
  one section).

## 01DEP-P03-G2-R3 — architecture gate G2 recheck (B2), decision revision r4

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B2; stage: architecture gate
  recheck. Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- `logs/gate-B2-r4.md` sha256 `51d4dee8d53b30ff8033d8373b06f07024bd26fcbcdf193c61062f9e74fd8553`
  appears verbatim in `TASK.md`. The r1–r3 copies are unchanged (`3ff95963…`, `d309c78e…`,
  `bb6dcc95…`). HEAD `523732b7`; only records changed. Coordination revision 3 `880cac54…04b8`.
  Checked at the start and before returning.

### Coverage and evidence

- `diff gate-B2-r3.md gate-B2-r4.md` shows the changes are limited to the preface, the D9 "Where"
  lock list, D9 "What" (non-agent lead), "Re-point" (source comparison), "Run-as" (the residual)
  and three verification rows.
- **G2-F2 residual.** I checked the reconfigure-before-activate/select rule against
  `validate_change` (`bindings.rs:93-112`: a Configuration change with the active flag unchanged
  is accepted), `check_run_as` (`:181-215`: run-as = actor needs no owner check; the following
  Activate keeps it), `selected_on` (`channel_reply.rs:233-251`) and `lock_settings_accounts`
  (`:166-185`). After the reconfigure, all of them name only the actor. Deactivate skips
  `check_run_as`, and clearing the selection locks only the actor.
- **"Only the actor's account" premise.** I walked every D9 step: `publish_initial_on`
  (`lock_company_on`), `save_draft_on` (`authorize_company`), compile/capture (shared reads, no
  accounts), `publish_on`, binding changes, `configure_reply_on`, `bootstrap_native_on`, and the
  allowlist `authorize_association` (the actor's own grant and account rows). With approval
  policies refused, the premise holds, so r3's "after the sync" argument stands.
- **G2-F8 and G2-F9.** I checked the new text and rows.
- **Not executed:** no cargo or SQL.

### Outcome

**PASS** for decision revision r4 (`logs/gate-B2-r4.md` sha256
`51d4dee8d53b30ff8033d8373b06f07024bd26fcbcdf193c61062f9e74fd8553`). All findings,
01DEP-P03-G2-F1..F9, are resolved, and none is open. No item needs the user.

Implementation notes for B2's batch review, not findings:

- Map only the association `NotFound` to the invisible-desk message. A `NotFound` from
  `verify_selection` (an archived workflow) is a different case.
- Update the provisioning-commands table comment: a `department:{id}` row's `result.binding_id`
  is updated by D9, not only replayed.

### Process and instruction feedback

- Four revisions converged because each recheck named a narrow residual, with file:line evidence
  for the exact authorization call. Candidate practice: a premise stated in a lock argument
  ("authorizes only the actor") gets checked step by step against the code path before the gate
  passes.

## 01DEP-P03-B2-R1 — batch review B2 (plan 03B: department desk and re-leading)

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B2; stage: batch review. Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, base/HEAD `523732b7`, uncommitted: 23
  modified files and 2 new ones (`workflow/provisioning_department.rs`,
  `department_lead_desk_tests.rs`).
- Artifact digest `8fd91d0452986625ad448afcdbe382b456c75bcaeaa9cddef117523947535c9b`, recomputed at
  the start and before returning. Evidence manifest valid (PASS 3, FAIL 1; the FAIL is the first
  run, with the expected consequences).
- Gate r4 `51d4dee8…8553`; coordination revision 3 `880cac54…04b8`.

### Coverage and evidence

- **Read in full:** `provisioning_department.rs`.
- **Read as diffs:**
  - `department.rs`: D7 wrapper, D8 call order, `follow_lead_on` after `write_placement_on` and
    after `clear_department_role_on`'s sync, and the record deletion in `delete_department_on`;
  - `channel_reply.rs` (`configure_reply_on`, `native_contract`);
  - `definitions.rs` (`save_draft_on`);
  - `lifecycle/provisioned.rs` (`provisioned_reconfiguration`, `provisioned_activation`);
  - `provisioning_native.rs` (`select`; the existing caller passes `true`);
  - the B1 callers, onboarding, setup and services, and the schema comment.
- **Callers that keep their behaviour:**
  - `configure_channel_reply` = begin → `configure_reply_on` → commit (the log line now precedes
    the commit);
  - `save_draft` = `authorize_company` → `save_draft_on`;
  - `provision_on` passes `select = true`.
- **D9 against gate r4:**
  - The case tree matches.
  - The no-change condition is active ∧ selected at B's current configuration revision ∧ runs A.
  - The order is re-point → reconfigure to the actor → activate → select.
  - `guard_on` refuses an approval policy, and maps only the explicit `authorize_association`'s
    `NotFound`.
  - `repoint_on` compares the draft source with the bound source.
- **Lock checks beyond the gate:**
  - Admission takes the company lock before it locks a binding (`admission.rs:22,213-215`).
  - Inbound never locks `workflow_bindings`.
  - A `NotFound` from `verify_selection` is not remapped.
- **Executed** (`openbot_dept_p03_test`, `SQLX_OFFLINE=true`, `--test-threads=4`):
  - `--list` for `department_desk_` + `agent_default_desk`: 53 tests;
  - those 53: passed (`logs/B2-R1-review-focused.txt`);
  - the competing filters five times in a row: 5/5, 7 tests each, which includes p02's because of
    the B2-F1 collision;
  - the author's full affected filter set: 686 passed, 1 ignored
    (`logs/B2-R1-review-affected.txt`);
  - `cargo check --lib --bins`, `fmt --check` and `--test source_modules`: all clean.
- **Not run:** the full gate (I003), `stack-budget.sh`, clippy (the author reports it clean) and
  migrations from scratch.

### Outcome

**Changes required** for artifact `8fd91d04…5c9b`. There is no PASS. All remaining items are Low
and concern tests and naming. The production code matches gate r4.

- Actionable:
  - 01DEP-P03-B2-F1 (Low): wrong test prefix, colliding with p02 (reported by A);
  - 01DEP-P03-B2-F2 (Low): no assertion that the lead agents' default desks are untouched
    (reported by A);
  - 01DEP-P03-B2-F3 (Low): the S ≠ B branch, which protects a user's own responder, is untested.
- Optional: B2-F4 (the pause path's order differs from the gate's list; harmless).
- **Accepted:**
  - the fixture change (a real company agent as `worker`, `drop_desk`);
  - the onboarding totals;
  - the no-`draft`-step and resources stand-ins, which are code-reviewed one-line guards;
  - the resources refusal applying only to a re-point, since reconfigure-to-actor carries the
    binding's resources unchanged;
  - the record deletion on department delete, which runs only after the desk is detached or
    deleted.
- No item needs the user.

### Process and instruction feedback

- Reviewer self-correction: I passed gate r4 without checking its verification prefix against
  the plan's focused-check names (B2-F1). Candidate rule: a gate's verification plan uses the
  plan's focused-check prefixes verbatim, and B checks for prefix collisions with earlier phases'
  tests.

## 01DEP-P03-B2-R2 — batch B2 correction recheck (B2-F1..F4)

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B2; stage: correction recheck.
  Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Artifact digest `31d169402e1d795115670b31b6399674904c1f0d41463afb0bdbe0c1378139ea`, recomputed at
  the start and before returning. Base/HEAD `523732b7`, uncommitted; the same file set as R1.
  Evidence `logs/01DEP-P03-B2-R2.evidence.json` validates (PASS 3).
- Gate r4 `51d4dee8…8553`; coordination revision 3 `880cac54…04b8`.

### Coverage and evidence

- **Read:** TASK "B2 corrections for 01DEP-P03-B2-R1"; the pause path in `provisioning_department.rs`
  (deactivate, then clear); the `own_desk` helper and its five uses; the new S ≠ B test in full; and
  a scan for leftover `department_desk_` names (only p02's remain).
- **Executed** (`openbot_dept_p03_test`, `SQLX_OFFLINE=true`, `--test-threads=4`):
  - `--list department_lead_desk`: 14 tests, 1 of them competing;
  - those 14: passed (`logs/B2-R2-review-lead-desk.txt`);
  - the competing filters four times in a row: 4/4, 4 tests each (B2's 1 + B1's 3);
  - affected `department onboarding provisioning channel_reply agent_default_desk
    department_lead_desk binding member_removal user_deletion desk_wording
    builtin_agent_library`: 515 passed (`logs/B2-R2-review-affected.txt`);
  - `cargo check --lib --bins` and `fmt --check`: clean.
- **Not run:** the full gate (I003, phase 05), `stack-budget.sh`, clippy (the author reports it
  clean) and migrations from scratch.

### Outcome

**PASS** for batch B2 at artifact `31d169402e1d795115670b31b6399674904c1f0d41463afb0bdbe0c1378139ea`.
01DEP-P03-B2-F1..F4 are all resolved, and none is open. The remaining code-review stand-ins (the
no-`draft`-step and resource-slot refusals) were accepted in R1. No item needs the user.

### Process and instruction feedback

- Asserting the user-configuration safety property with real lifecycle calls (`BindingService` +
  `configure_channel_reply`) rather than raw rows makes the test track the production rules.

## 01DEP-P03-G3-R1 — architecture gate G3 (B3: plan 03C under C003/C004), decision revision r1

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B3; stage: architecture gate.
  Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, HEAD `01608b1f`. The only changes are
  COORDINATION.md, TASK.md and the new logs `coordination-r4.md`, `coordination-r5.md` and
  `gate-B3-r1.md`.
- `logs/gate-B3-r1.md` sha256 `84f90db5a109344c0bec1b3bffa29259315251b7584bfe628cded9c0cbafda64`
  appears verbatim in `TASK.md`.
- Coordination **revision 5** `0a83f1c0…d586` = `logs/coordination-r5.md`. I diffed it against r3:
  it adds C003 and C004 (user decisions, 2026-11-10) and the p03 dispositions for C001/C002.
  Checked at the start and before returning.

### Coverage and evidence

- **Requirements:** plan 03C items 0–6 and "Focused checks"; README decisions 4, 5 and 10;
  C003 and C004.
- **C004's code facts, all verified:**
  - `DestinationKind` is only Channel, User or Reply (`message.rs:114-118`).
  - `plan_channel` writes a system message into a new thread, with no delivery
    (`message_planner.rs:288-326`).
  - `INSERT INTO workflow_message_intents` exists only in `thread/inbound.rs:737`, and
    `commit_inbound` is called only from ingest and the inbound event worker.
  - A reply continues its source's trace (`message_planner.rs:246-276`).
  - Routing refuses an internal origin whose trace contains the channel
    (`ingest/routing.rs:288-291`, `LoopCycle`).
- **D10:**
  - `list_on` (`provisioning.rs:365-410`) uses a byte budget with a truncation flag and cut names.
  - `PreparedTool::declaration` gives every non-knowledge tool the description
    "Workflow tool {name}" (`agent_context.rs:71-82`), so a provisioning-specific description is
    new but straightforward.
  - `AgentDirectoryEntry` is served only behind `lock_company_on`, so it is admin-only
    (`agent_authoring/reads.rs:59-97`). Adding charters causes no department-visibility leak.
- **D11:**
  - `candidate_on` / `build` / `PreparedAgent::new` produce the stored preamble.
  - `scope::Facts.channel_id` is available.
  - `restore_on` calls `with_frozen_history` before comparing, and `history_budget` depends on the
    preamble (G3-F1).
  - The frozen document's bound is 2 MiB, and a charter is at most 4096 bytes (schema 14273, 15379).
  - Charter and names are written by admins only, so treating them as trusted is consistent with
    agent instructions.
- **D12:** `fixtures::ALL` and `snapshots`. The preamble's untrusted-context rule
  (`agent.rs:380`). `TemplateCatalogue::shipped` (G3-F2).
- **D13:** the docs list covers the stale sentences (`agent-builder-context.md:17`,
  `custom_tools.md:37-41`) in files it already names.
- **Not executed:** no cargo or SQL. This is a gate.

### Outcome

**Changes required** for decision revision r1. There is no PASS.

- Actionable:
  - 01DEP-P03-G3-F1 (Medium): the frozen-restore rule must rebuild the preamble from the saved
    department before `with_frozen_history`;
  - 01DEP-P03-G3-F2 (Low): fixture role text in untrusted context, wording that contradicts C004,
    and the "authoring sample list" unresolved.
- Optional: G3-F3 (roster on public department desks), G3-F4 (docs say framing follows desk
  ownership).
- **Sound as written:**
  - C004's facts;
  - D10's filter, fields, desk resolution and bounds;
  - the `AgentDirectoryEntry` additions;
  - D11's read location, content, bounds, placement and trust;
  - the D13 list;
  - the contract-impact table.
- No item needs the user. G3-F3 stays inside the user's C003 decision.

### Process and instruction feedback

- Candidate rule (gates that add content to a frozen or replayed document): trace every derived
  field of that document (here `preamble`, and the history budget computed from it) through the
  restore path, not only the new field.

## 01DEP-P03-G3-R2 — architecture gate G3 recheck (B3), decision revision r2

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B3; stage: architecture gate
  recheck. Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- `logs/gate-B3-r2.md` sha256 `4aa241d522246d03781fe256eb8c2d815e7813c3550779cb167958657138357c`
  appears verbatim in `TASK.md`; `gate-B3-r1.md` (`84f90db5…da64`) is unchanged. HEAD `01608b1f`,
  with no source changes. Coordination revision 5 `0a83f1c0…d586`. Checked at the start and before
  returning.

### Coverage and evidence

- `diff gate-B3-r1.md gate-B3-r2.md` shows changes only in the preface, D11 (pure preamble, lead
  sentence, public desks, restore order), D12 (fixture context, header, mismatch), D13
  (agent-authoring scope) and two verification rows.
- I checked the restore order against `frozen.rs:15-36` and `agent_context.rs:18-53`. Once
  `with_department(saved)` has rebuilt the preamble, `with_frozen_history`'s budget check and the
  full-document compare both see the saved text. Both `freeze_on` and the ambiguous-resolution
  `proof_on` go through `restore_on`.
- **Not executed:** no cargo or SQL.

### Outcome

**PASS** for decision revision r2 (`logs/gate-B3-r2.md` sha256
`4aa241d522246d03781fe256eb8c2d815e7813c3550779cb167958657138357c`). 01DEP-P03-G3-F1..F4 are all
resolved, and none is open. No item needs the user.

Implementation note for B3's batch review, not a finding: keep the lead sentence a function of
the saved `DepartmentContext`, for example a stored lead flag. A public desk carries no roster, so
the sentence cannot be derived from roster membership there.

### Process and instruction feedback

- r2 named the derived field (`preamble`) and the budget dependency explicitly, which made the
  recheck a direct trace through two functions.

## 01DEP-P03-B3-R1 — batch review B3 (plan 03C under C003/C004)

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B3; stage: batch review. Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Worktree `/Users/mac02/.codex/worktrees/2050/openbot`, base/HEAD `01608b1f`, uncommitted: 22
  modified source files, 4 docs, the plan file, and 7 new files (`department_roster.rs` and its
  tests, `agent_department.rs` and its tests, `agent_history_department_tests.rs`,
  `provisioning_directory_tests.rs`, `fixtures/department-lead.yaml`).
- Artifact digest `241d6d8d955a1f236e384677c12f66cb37918ff980e2b3e1fef596b69c1524e9`, recomputed at
  the start and before returning. Evidence valid (PASS 3, FAIL 2: the first affected run and the
  intended mutation).
- Gate r2 `4aa241d5…357c`; coordination revision 5 `0a83f1c0…d586`.

### Coverage and evidence

- **Read in full:** `department_roster.rs` (roster and desk SQL), `agent_department.rs`
  (context and rendering), `fixtures/department-lead.yaml`.
- **Read as diffs:**
  - `preparation.rs`: `candidate_on` reads the context for every `agent.run` with a channel,
    then calls `with_department`;
  - `frozen.rs`: restore order saved department → `with_frozen_history` → compare;
  - `agent.rs`: `preamble(selection, department)`, and `with_department` refused after frozen
    history or for non-`agent.run` steps;
  - `agent_context.rs` (declaration), `provisioning.rs` (list input, filter, departments, desk,
    budget), `application/workflow/provisioning.rs` (schemas, description);
  - `agent_authoring/directory.rs` and `reads.rs`, `authoring_reads.rs`, the picker route,
    `mod.rs`/`infra/workflow.rs` (`app_domain`), `fixtures/mod.rs`;
  - the four docs and the plan's "Status after p03" note.
- **Desk identity checked.** An agent's desk is the `agent:{id}` record's channel while it
  exists, at its `is_primary` slug. A department's desk is the `department:{id}` record's channel,
  only while `department_id` matches. Both are consistent with B1/B2.
  `departments.slug` is citext, so the filter is case-insensitive.
- **`app_domain` checked.** Every production reader with desks gets the configured instance:
  `WorkflowServices.persistence` serves the picker, the JSON directory (`catalog.rs:118-133`)
  and agent preparation (`compose_handler`); `list_on` uses `templates.app_domain`. Local agents
  do not use `PreparedAgent` and have no diff, so the six `cloudroom_*` failures (Node peer build
  absent) are unrelated.
- **VERSION 5 checked.** `agent_history_preparation_valid` restricts only the keys inside
  `history`, not the top-level ones, so the additive `department` key is accepted. With the DB
  reset there are no pre-change documents.
- **Executed** (`openbot_dept_p03_test`, `SQLX_OFFLINE=true`, `--test-threads=4`):
  - the focused set (`department_directory_ department_context_ department_lead_fixture
    workflow_fixtures_ workflow_storage_ provisioning_lists refusals_and_results`): 38 listed,
    38 passed (`logs/B3-R1-review-focused.txt`);
  - the author's affected filters, with `agent_tests` narrowed to
    `application::workflow::agent_tests` to leave out the unavailable `cloudroom_*` tests:
    829 passed, 3 ignored (`logs/B3-R1-review-affected.txt`);
  - `cargo check --lib --bins`, `fmt --check` and `--test source_modules`: all clean.
- **Not run:** the full gate (I003), `stack-budget.sh`, clippy (the author reports it clean), the
  `cloudroom_*` e2e tests (Node peer build absent), and migrations from scratch (unchanged).

### Outcome

**Changes required, narrowly**, for artifact `241d6d8d…24e9`. There is no PASS.

- Actionable: **01DEP-P03-B3-F1 (Low, docs)**. The library-copy sentence in
  `agent-builder-context.md` is false since B1, and the "Later edits…" sentence in
  `agent-authoring.md` landed in the wrong paragraph.
- **Sound as written:**
  - the roster and desk SQL, with its bounds;
  - the preparation change and the restore order, with mutation evidence;
  - public desks (no roster);
  - trust placement: admin-written text in the preamble, before the skills and the untrusted
    notice;
  - the stored lead flag;
  - the picker route, which now reads the real entry and skips a vanished agent;
  - the listing filter, fields, budget and description;
  - the fixture (untrusted base context, header citing D11 and C004, not shipped);
  - the inter-channel doc and the plan status note.
- No item needs the user.

### Process and instruction feedback

- When a gate review names specific stale sentences for a later batch to fix, the batch's docs
  check should grep for those exact sentences (B3-F1).

## 01DEP-P03-B3-R2 — batch B3 correction recheck (B3-F1, docs only)

### Identity

- Date 2026-11-10. Workflow `implement-opus-reviewed-2_2`. Batch B3; stage: correction recheck.
  Phase `p03`.
- Reviewer B: Opus 5.5 (`claude-opus-5-5`), effort per TASK (xhigh), agent `ad409b2d3d149c2c6`.
- Artifact digest `0db985062e8c8701d12d2a0b89c4eca83220d0bccce46c2a0ff154298682dbf1`, recomputed at
  the start and before returning. Base/HEAD `01608b1f`. Evidence `logs/01DEP-P03-B3-R2.evidence.json`
  validates (PASS 2).
- Gate r2 `4aa241d5…357c` and coordination revision 5 `0a83f1c0…d586` are unchanged.

### Coverage and evidence

- **Read the corrected text:**
  - `docs/agent-builder-context.md:22`: the library copy gets its own autonomous desk. A grep
    finds no "creates only an agent" left in `docs/`.
  - `docs/agent-authoring.md:96-115`: "Later edits…" and "The update contract…" close the create
    paragraph again, and the department paragraph ends at the retry sentence.
    `git diff --stat 01608b1f` shows +11/−0.
  - `docs/custom_tools.md:47`: the quoted label now reads "Create assistant desk".
- **Not run:** the Rust and database suites, because this correction touches docs only (AGENTS.md:
  documentation-only changes do not require them). The source is the same as in B3-R1, where the
  focused set (38) and the affected set (829) passed.

### Outcome

**PASS** for batch B3 at artifact `0db985062e8c8701d12d2a0b89c4eca83220d0bccce46c2a0ff154298682dbf1`.
01DEP-P03-B3-F1 is resolved. No p03 finding is open. No item needs the user.

### Process and instruction feedback

- A docs-only correction with an add-only diff against the base made the recheck a short read.
