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
