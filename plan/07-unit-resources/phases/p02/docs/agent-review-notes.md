# 07UR-P02 agent review notes

## 07UR-P02-G1-R1 (2026-11-21)

### Identity

- Round 07UR-P02-G1-R1. Workflow implement-opus-reviewed-2_4. Task [TASK.md](TASK.md), review
  [REVIEW.md](REVIEW.md), findings [agent-review-findings.md](agent-review-findings.md).
- Stage: architecture gate G1, decision revision r1 (TASK.md "Architecture gate G1 (decision
  revision r1, 2026-11-21)": D1–D9, the batch split B1 = 02A / B2 = 02B / B3 = 02C, "Acceptance
  mapping (planned)", "Contract impact (planned)", and the "Investigation" section). No
  implementation exists.
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5). Effort was set by root at spawn (TASK records
  Opus 5.5/xhigh) and cannot be observed from inside the run. Phase 07UR-P02, worktree
  /Users/mac02/orca/workspaces/openbot/plan06, branch plan07.
- Phase start = review base = HEAD = 107fd58867f7600bfaca56f39a4c790bde41c013. Artifact digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1, exclusions
  plan/07-unit-resources/phases/p02/docs and plan/07-unit-resources/COORDINATION.md. Confirmed at
  start and again before returning (unchanged). The preserved untracked files (three
  `.trail/learnings/*-mac02.md`, three Plan06 snapshot tars) were ignored as instructed.
- Coordination: plan/07-unit-resources/COORDINATION.md revision 8 (sha256 f0516f36…5cfa at
  return), no cross-phase notices. User decisions applied: (a) unit managers configure, (c) caps
  set by the unit's own manager, (d) agent resources follow the agent's units, (f) replaces (e),
  (g) one model connection per provider, owned by a unit, (h) unpriced = 0 and flagged, (i) UTC
  day/month. TASK.md sha256 ba6d1e86…c426 at return.
- Retained evidence (fixture-isolated, created by B):
  [logs/07UR-P02-G1-R1-fk-set-null-vs-immutable-guard.txt](logs/07UR-P02-G1-R1-fk-set-null-vs-immutable-guard.txt)
  (sha256 73c50a55…bd00; throwaway database `openbot_p07_p02g_fk` on 127.0.0.1:55407, created and
  dropped) and [logs/07UR-P02-G1-R1-period-start.txt](logs/07UR-P02-G1-R1-period-start.txt)
  (sha256 e06545f1…aac; read-only SELECTs on `postgres`). Both on PostgreSQL 18.7.

### Coverage and evidence

- Requirements read from the originals: README decisions 1, 3, 4, 5, 7 and "What does not
  change"; doc 02 (02A.1–3, 02B.1–3, 02C.1–2, focused checks); root AGENTS.md; src/AGENTS.md
  headings; src/adapters/persistence/AGENTS.md (invariants, guards, performance, migration, SQL
  style); the p01 review record for precedent.
- Inspected (inspection only, no project tests run):
  - Model connections: schema (init_schema.sql:3842-3854, PK 6403, default index 8170), the
    replace writer (persistence/company.rs:330-451), `model_connections_on`/`model_api_key_on`,
    the use-case validation (use_cases/company.rs:281-312, owner-only list), every non-test
    `company_model_connections` and `is_default` reader (trail grep, 70 hits), publication capture
    (agent_publication/capture.rs:44-117), admission availability (workflow/agent_authority.rs:
    67-103), `native_mcp_parent_agent_live` (no `is_default` read).
  - Agent creation: `resolve_model_on`/`insert_company_agent_identity_on`/`CompanyAgentService`
    (company_agent.rs:39-360), `AgentCreationPlacement` variants, `prepare_new_agent_placement_on`,
    the Builder (provisioning_builder.rs, provisioning.rs `agent_write`), the library copy
    (`prepare_library_write`, `create_agent_from_library`), onboarding `prepare`/`custom_write`,
    `generate_system_prompt`, the Builder directory, and the agent settings model selector. The
    only `insert_agent_on` company callers are `insert_company_agent_identity_on` and a
    `#[cfg(test)]` create, so D2's single resolver covers every production creation path.
  - p01 machinery: `unit_resources.rs` (authority, move, `uses_on`, `owned_resources_on`,
    `check_agent_reach_on` and its four callers), `unit_reaches`/`agent_reaches`/
    `unit_management_role`/`any_unit_management_role`/`resource_default_unit`
    (init_schema.sql:15776-15910), `ResourceOutsideUnit` and its HTTP body, `UnitResourceKind`.
  - Budgets and usage: `model_usage_calls` and its guards (init_schema.sql:11455-11535),
    `model_prices`, `model_usage_history` and its guards (12076-12124), `reserve_on`/`settle_on`
    (model_usage/mod.rs), the JEV reserve, the archive (storage_retention.rs:122-130),
    metrics.sql usage, the usage report (report.rs, 10,000-call bound, shared `authorize` for
    report and price writes), Rust aggregation (aggregate.rs), routes/model_usage.rs, dashboard
    scope resolution (ui_dashboard.rs:183-260).
  - Admission: admission.rs (READ COMMITTED; claimed-admission timeouts; error and refusal
    paths), admission_write.rs (`create`, root budgets), `authorize_person_with_locks`
    (`FOR NO KEY UPDATE` on companies), run scope capture (run_scopes.rs:42-212; one base write
    row at most; empty chain → no write unit), scopes.rs, all `insert_run` producers (admission,
    children, native MCP, a test fixture).
  - Departments: deletion guard and delete (department/lifecycle.rs:200-342), re-parenting
    (:170), `delete_company_tree`.
  - Refusal consumers: message_admission classify, inbound_event_worker `classify_commit_error`,
    local_agents workflow fallback, http app_error_impl.
- Assumptions challenged, by decision:
  - D5 FK action vs the immutability triggers (F1, reproduced).
  - D5/D7 deletion and re-parenting semantics (F1).
  - D8 against the doc's `unit_budget_competing_` wording and the discriminating power of its
    tests (F2).
  - D7 boundedness under the company lock and claimed-admission timeouts (F3).
  - D7 arithmetic, rounding, current prices, and the page's 10,000-call sample (F4, overflow
    reproduced).
  - D9 predicate direction against the `unit_reaches` signature; shared report/price
    authorization; filter placement relative to the scan LIMIT (F5).
  - D1 identity in uuid-based p01 plumbing, the root-default invariant on moves, the "at least
    one" rule, replace serialization and upsert ownership (F6).
  - D2 Deferred placement and the library fallback under reach (F7).
  - The contract-impact inventory against `trail grep is_default` and the refusal consumers (F8).
  - Mapping coverage against doc 02 and decisions c/h/i (F9).
- Accepted on inspection:
  - D1's key stays `(company_id, provider)` per (g); partial unique index for one default per
    unit; RESTRICT unit FK and the department-guard listing as in p01; credential rotation
    untouched.
  - D1's reach check placed in `check_agent_reach_on`, which reaches publication, admission
    manifest and both native paths.
  - D2's single in-transaction resolver: placement known before `insert_agent_on`; onboarding
    agents are unplaced, so root = the only unit at onboarding; the Builder is explicit.
  - D3 reading ("later publications" = later creations; desks never resolve a default at
    publication; no runtime `is_default` reader).
  - D4 shape.
  - D5 source: the base write row is unique per run. Child runs copy scopes. Native MCP brokers
    compile only an MCP tool (application/workflow/native_mcp.rs:35-45), so they make no model
    calls and need no D7 check. The JEV reserve knows the run. The archive copies by root run,
    and the history guard's jsonb equality keeps working with the column on both tables.
  - D6 table shape, CASCADE with the unit, and authority matching decision (c). The period
    formula is correct under any session time zone (log; optional F10 on `now()`).
  - D7 placement inside the admission transaction after scope capture, so a refusal rolls
    everything back; W = base write unit or root, consistent with D5; root runs only. Plumbing
    matches p01's (HTTP, classify, schedule CHECK, renderers), except the inbound-event
    consumer (F8).
  - D9 grouping by the recorded `unit_id`.
- Not exercised: no project test or check was run. The gate accepts a plan, not execution. The
  dashboard SSE/snapshot cache path and the usage page HTML were read only lightly.
- Mappings checked: TASK "Acceptance mapping (planned)" and "Contract impact (planned)". Gaps are
  recorded as F8 and F9. No round evidence manifest applies to an architecture gate.

### Outcome

- **Changes required; no PASS for G1 r1.**
- Actionable: F1 (high), F2, F3, F4, F5, F6 (medium), F7, F8, F9 (low).
- Optional: F10.
- Unresolved question for the user (through root): F2, the reading of "admit at most what the
  cap allows".

### Process and instruction feedback

- The batch split and gate scope fit: the gated decisions (schema, FK semantics, the
  concurrency contract, authority) are the ones costly to unwind.
- Defects originate in the gated decisions, not in any implementation. Three trace to the
  decisions not reading existing guards or signatures:
  - the immutability triggers (F1);
  - the `unit_reaches` argument order (F5);
  - p01's uuid-typed plumbing (F6).
- Candidate instructions (see each finding): check row triggers before adding referential
  actions (persistence AGENTS); write gate predicates with the real function signature; name the
  bound behind any "bounded read"; send any re-reading of a focused check to the user.
- HANDOFF.md was stale at review time (F10). TASK's investigation section was accurate and
  useful: its spans matched the code where checked.

## 07UR-P02-G1-R2 (2026-11-21)

### Identity

- Round 07UR-P02-G1-R2 (gate recheck). Workflow implement-opus-reviewed-2_4. Task
  [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md).
- Stage: architecture gate G1, decision revision r2: TASK.md "Architecture gate G1 (decision
  revision r2, 2026-11-21)" (D1, D2, D5, D6, D7, D8 and D9 r2, plus contract-impact additions)
  and "Acceptance mapping additions (r2, F9)". r1 stands where r2 does not change it. TASK.md
  sha256 e9fb71f2…2a71 at return.
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5), the same agent as R1. Effort was set by root.
- HEAD = phase start = review base = 107fd58867f7600bfaca56f39a4c790bde41c013. Digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1, with unchanged exclusions.
  Verified at start and before returning.
- Coordination: revision 9 (sha256 d35241da…d124 at return), no notices. New user decisions:
  (j) per-unit running totals, an explicit deviation from "no new ledger", with deletes and moves
  keeping spend already counted; (k) soft caps, where "admit at most what the cap allows" means
  refusing an admission that orders after spend reached the cap. Applied as authoritative.
- Retained evidence (created by B):
  [logs/07UR-P02-G1-R2-settlement-vs-department-delete.txt](logs/07UR-P02-G1-R2-settlement-vs-department-delete.txt)
  (sha256 7f1e551e…c547; throwaway database `openbot_p07_p02g_spend`, created and dropped;
  PG 18.7). R1 evidence is reused where it still applies.

### Coverage and evidence

- Rechecked F1–F10 by ID against r2. Each R1 finding has a dated resolution entry in the
  findings file. All ten are resolved in the decision; residual nuances are optional F13.
- Settlement trigger (asked focus):
  - Production settle sites are `model_usage::settle_on` (model_usage/mod.rs:79) and the JEV
    `settle_on` (jev_runtime.rs:285). Both set `settled_at` from NULL with `WHERE settled_at IS
    NULL`, and no producer inserts a settled row. `model_usage_immutable` (init_schema.sql:
    11499-11510) forbids any later change to a settled row. So `AFTER UPDATE OF settled_at …
    WHEN (OLD.settled_at IS NULL AND NEW.settled_at IS NOT NULL)` fires exactly once per call,
    in the settlement transaction, and rolls back with it.
  - The trigger writes another table, so it does not conflict with the update guard.
  - Archiving is INSERT into history plus DELETE (no UPDATE), so it neither double-charges nor
    trips the history guard. Its jsonb equality holds with `unit_id` on both tables.
  - The token-policy triggers sit on `workflow_decisions` and checkpoints and only read usage,
    so they are unaffected.
  - Each settlement transaction charges one call (checkpoint_models.rs:242; jev_runtime.rs:285),
    so per-invocation unit-id order is a global order among settlements. It still conflicts with
    department deletion (F11) and makes the root rows a hot spot (F12).
- Cost (inspection): numeric, rounded up per call, saturated; price at settlement; unpriced →
  `unpriced_tokens`. Per-call cost ≤ about 4.3e15 micro-USD. The running sum's saturation is
  unstated (F13). Periods come from `requested_at` in UTC, matching the rollover case in the
  mapping.
- Admission (inspection): after scope capture and under the company lock, READ COMMITTED
  (admission.rs:23). At most 8 units × 2 periods via PK reads; refuses at the nearest unit, day
  before month. D8 r2 test (1) discriminates a pre-lock or stale read. Test (2) is safe with an
  additive `ON CONFLICT DO UPDATE`. Test (3) has no outcome (F11).
- D1 r2: the uuid identity fits `ResourceOutsideUnit`, moves and uses. The invariants are
  enforced under the company lock in every write path (replace, move; onboarding creates the
  company in its own transaction; rotation touches only keys). The conditional upsert and the
  unit-scoped update/delete close the overwrite race. The competing test is planned.
- D2 r2 and D9 r2 accepted. The predicate direction is correct against `unit_reaches(company,
  start, unit)` (init_schema.sql:15852-15865). Placement before the limit, root-only price
  writes, and totals-backed cap/remaining are all stated. Contract additions (F8) and mapping
  additions (F9) were checked against the consumers identified in R1.
- Executed: the race reproduction above (two cases, both failing as described in F11).
  Everything else is inspection. No project tests ran.

### Outcome

- **Changes required; no PASS for G1 r2.**
- Actionable: 07UR-P02-G1-F11 (medium), 07UR-P02-G1-F12 (low).
- Optional: 07UR-P02-G1-F13.
- F1–F10 are resolved in the decision.

### Process and instruction feedback

- r2 answered every R1 finding in one revision, with the user decisions recorded in
  coordination first. That worked well.
- The new defect is again a lock order between a new write path and an existing delete path,
  introduced by the fix for F1 and F3. The candidate in F11 (state and test the lock order when
  a trigger writes rows whose parent another path deletes) is supported by two consecutive
  rounds touching department deletion.

## 07UR-P02-G1-R3 (2026-11-21)

### Identity

- Round 07UR-P02-G1-R3 (gate recheck). Workflow implement-opus-reviewed-2_4. Task
  [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md).
- Stage: architecture gate G1, decision revision r3 (TASK.md "Architecture gate G1 (decision
  revision r3, 2026-11-21)": D7 r3 charging protocol, competing tests 3a/3b/4, F13
  dispositions). r1/r2 stand where unchanged. TASK.md sha256 eccacaab…c5e8 at return.
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5), the same agent as R1/R2.
- HEAD = phase start = review base = 107fd58867f7600bfaca56f39a4c790bde41c013. Digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1, exclusions unchanged.
  Verified at start and before returning.
- Coordination: revision 9, unchanged (sha256 d35241da…d124). Decisions (j) and (k) applied.
- Retained evidence (created by B, PG 18.7):
  - [logs/07UR-P02-G1-R3-r3-charging-protocol.txt](logs/07UR-P02-G1-R3-r3-charging-protocol.txt)
    (sha256 b57169da…8424): the r3 protocol on a reduced schema in throwaway database
    `openbot_p07_p02g_r3`, created and dropped. Its script is retained as
    [logs/07UR-P02-G1-R3-r3-charging-protocol.sh.txt](logs/07UR-P02-G1-R3-r3-charging-protocol.sh.txt);
    the R2 race script as
    [logs/07UR-P02-G1-R2-settlement-vs-department-delete.sh.txt](logs/07UR-P02-G1-R2-settlement-vs-department-delete.sh.txt).
  - [logs/07UR-P02-G1-R3-saturating-sum.txt](logs/07UR-P02-G1-R3-saturating-sum.txt)
    (sha256 31bd732d…b3).

### Coverage and evidence

- Executed against the r3 protocol, which I implemented as specified:
  - deferred constraint trigger with the once-per-call WHEN;
  - chain resolved, then locked `FOR KEY SHARE` ordered by id, with existence judged from the
    locked result;
  - root fallback for a missing unit;
  - upsert ordered by `(unit, period)` with the sum in numeric;
  - the same immutability guard shape as `model_usage_immutable`.
- Results:
  - T1: deferral and exactly-once per qualifying row at COMMIT; no charge before commit.
  - T2: the guard makes NEW the committed row.
  - T3: a deferred-trigger failure fails only that COMMIT and rolls back the settlement.
  - 3a: the delete waits for the committing charge, then succeeds; ancestors include the call.
  - 3b: the R2 deadlock shape no longer deadlocks; the settlement waits, then charges the root.
  - 3b': delete first, root charged.
  - 4: 16 concurrent sibling settlements all commit; root totals equal the sum.
  - Case 5: a root `FOR UPDATE` holder stalls a child settlement's COMMIT (F14).
  - Saturation behaves correctly only when the sum is computed in numeric (F15).
- Inspected:
  - WriteBound COMMIT classification: a server error at COMMIT is refused or not applied, not
    ambiguous; a client deadline during COMMIT is ambiguous and resolved (persistence_bound.rs).
  - Settlement attempt bounds (lease.rs:48-50).
  - Department writers' `FOR UPDATE` lock sites (trail callers of `lock_department_on`).
  - The leaf-only deletability argument (lifecycle.rs guard plus parent FK).
  - `SET CONSTRAINTS` uses (none on settlement paths).
  - Existing `commit_ms` and bound warnings as the F12 signal.
- The reduced schema stands in for `model_usage_calls`, `departments` and `unit_spend`. It does
  not exercise the real settlement transaction (checkpoint_models), its WriteBound timeouts, or
  the JEV path; the planned tests 3a/3b/4 must.
- Not run: project tests.

### Outcome

- **Changes required; no PASS for G1 r3.**
- F11, F12 and F13 resolved (dated entries).
- Actionable: 07UR-P02-G1-F14 (medium), 07UR-P02-G1-F15 (low).

### Process and instruction feedback

- Executing the protocol in a throwaway database settled the questions the coordinator asked
  (deferral, exactly-once, failure scope, delete ordering) more cheaply than argument. It is
  worth keeping for lock-protocol gates.
- This is the third consecutive round whose defect is a lock interaction between the new
  charging path and department administration. The candidate instruction recorded in F11/F14 is
  supported: a new lock on an existing row needs an inventory of that row's lockers and modes.

## 07UR-P02-G1-R4 (2026-11-21)

### Identity

- Round 07UR-P02-G1-R4 (gate recheck). Workflow implement-opus-reviewed-2_4. Task
  [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md).
- Stage: architecture gate G1, decision revision r4 (TASK.md "Architecture gate G1 (decision
  revision r4, 2026-11-21)": D6/D7 r4 with no totals FK and no department locks, the delete
  trigger, the enumerated races, F15's numeric sum, the F12 signal, tests 3a/3b/4/5). Earlier
  revisions stand where unchanged. TASK.md sha256 0f106d7c…6043 at return.
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5), the same agent as R1–R3.
- HEAD = phase start = review base = 107fd58867f7600bfaca56f39a4c790bde41c013. Digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1, exclusions unchanged.
  Verified at start and before returning.
- Coordination: revision 9, unchanged (sha256 d35241da…d124).
- Retained evidence (created by B, PG 18.7, throwaway databases `openbot_p07_p02g_r4` and
  `openbot_p07_p02g_r4b`, created and dropped):
  - [logs/07UR-P02-G1-R4-r4-charging-protocol.txt](logs/07UR-P02-G1-R4-r4-charging-protocol.txt)
    (sha256 a9d64ca7…5926), script `07UR-P02-G1-R4-r4-charging-protocol.sh.txt` (fc5c1051…3f19);
  - [logs/07UR-P02-G1-R4-delete-heap-order.txt](logs/07UR-P02-G1-R4-delete-heap-order.txt)
    (sha256 02ebb3cb…03c3), script `07UR-P02-G1-R4-delete-heap-order.sh.txt` (520dcde9…3856).

### Coverage and evidence

- Executed the r4 protocol as specified: a deferred constraint trigger resolving the chain from
  committed rows with no department locks; root fallback; ordered numeric saturating upsert; no
  totals FK; an `AFTER DELETE ON departments` trigger removing the unit's rows. Results:
  - 3a: charge then delete; ancestors keep the call, the unit's rows are gone.
  - 3b: delete then settle; the root is charged, no row for the deleted unit.
  - 3c: delete executed but uncommitted; the settlement waits about 0.6 s, then commits; two
    orphan rows (day, month) and ancestors charged.
  - 3d: charge holding the unit's rows; the delete waits, then removes them; no orphan.
  - Test 5: root, parent and unit held `FOR UPDATE`; the settlement commits in 4 ms, no wait.
  - Test 4: 16 concurrent sibling settlements commit with exact totals.
  - Saturation reaches the bigint maximum without error.
  - Company teardown leaves no totals.
  - Re-parent racing a commit charges the old or the new chain: by statement snapshot,
    inspection only.
  - Heap-order variant (sequential scan forced): a deadlock between the delete and a settlement
    when a second charge holds the unit's month row (F16).
- Accepted on inspection:
  - The deliberate absence of a tenant FK on `unit_spend.unit_id`. The only writer resolves
    units within `NEW.company_id`; `company_id` keeps its FK to companies; readers join live
    departments and budgets cannot name a deleted unit.
  - Orphan rows left to the recorded retention pass.
  - The F12 signal (existing `commit_ms` and bound warning).
- Not exercised: the real settlement transaction and WriteBound timeouts, JEV, and the planner's
  actual choice on production-sized `unit_spend` tables. The reduced schema stands in for these;
  the planned tests must cover the real paths. Project tests were not run.

### Outcome

- **Changes required; no PASS for G1 r4.**
- 07UR-P02-G1-F14 and F15: resolved.
- Actionable: 07UR-P02-G1-F16 (low). A local fix: order the delete trigger's locks; correct two
  wording points; add a three-way test. A recheck can be limited to that delta.

### Process and instruction feedback

- r4 adopted the decoupling option and removed the cross-subsystem coupling. Test 5 confirms it.
- The residual issue is again lock order. Executing each enumerated race, plus one plan variant
  per set-based statement, found it quickly. A candidate check for lock-protocol gates: "for
  every statement that locks more than one row of a shared table, state its visiting order".

## 07UR-P02-G1-R5 (2026-11-21)

### Identity

- Round 07UR-P02-G1-R5 (gate recheck, limited to the F16 delta as agreed). Workflow
  implement-opus-reviewed-2_4. Task [TASK.md](TASK.md), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md).
- Stage: architecture gate G1, decision revision r5 (TASK.md "Architecture gate G1 (decision
  revision r5, 2026-11-21)"). r1–r4 stand where unchanged. TASK.md sha256 28fa4c90…a630 at
  return.
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5), the same agent as R1–R4.
- HEAD = phase start = review base = 107fd58867f7600bfaca56f39a4c790bde41c013. Digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1, exclusions unchanged.
  Verified at start and before returning.
- Coordination: revision 9, unchanged (sha256 d35241da…d124).
- Retained evidence (created by B, PG 18.7, throwaway database `openbot_p07_p02g_r5`, created
  and dropped):
  [logs/07UR-P02-G1-R5-delete-heap-order.txt](logs/07UR-P02-G1-R5-delete-heap-order.txt)
  (sha256 eb5d88ed…7114), script `07UR-P02-G1-R5-delete-heap-order.sh.txt` (cf5eb2dc…aca1).

### Coverage and evidence

- Read the r5 text: one `unit_spend` lock order `(unit_id, period, period_start)` for the charge
  and the delete trigger; the delete trigger's lock-then-delete; test (6); corrected wording.
- Executed the R4 heap-order three-way race against the r5 trigger, sequential scans forced. The
  lock statement's plan is `LockRows` over `Sort (period, period_start)` over `Seq Scan`, so row
  locks are taken in the sorted order regardless of heap order.
  - 6a, the R4 deadlock order: delete, settlement and second charge all complete; two orphan
    rows, as recorded; root charged.
  - 6b, settlement first: all complete; no orphan.
- Reused unchanged evidence from R3/R4: deferral and exactly-once, failure scope, test 5 (no
  wait on department locks), 3a/3b and the in-between outcomes, 16-way concurrency, saturation,
  teardown. r5 does not change those paths.
- Not exercised (unchanged from R4): the real settlement transaction and WriteBound timeouts,
  JEV, and production-sized plans. The planned tests 3a/3b/4/5/6 and the D1/D8/D9 competing and
  page cases must exercise them in B1–B3.

### Outcome

- **PASS for G1 decision revision r5** at digest
  67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1 (HEAD 107fd588,
  coordination revision 9).
- The pass covers the gated decisions:
  - D1 r2: model connection id, invariants under the company lock, unit-conditional writes.
  - D2 r2: cascade from the placement, library reach fallback.
  - D3/D4 (r1).
  - D5 r2: recorded unit id without FK.
  - D6 r2/r4: `unit_budgets`, and `unit_spend` without a department FK, with the delete trigger.
  - D7 r3/r4/r5: the deferred once-per-call charge without department locks, root fallback,
    numeric saturating sum, one lock order; the admission check bounded by depth.
  - D8 r2/r3: soft caps per (k), with the planned competing tests.
  - D9 r2: subtree predicate before the limit, price writes kept at root, totals-backed figures.
  - Contract impact and acceptance mapping as amended in r2–r5.
- An architecture-gate pass accepts the verification plan, not its execution.
- No unresolved actionable findings. Recorded limits, not blocking:
  - spend of a deleted unit settling after the delete reaches the root only;
  - reserved-but-never-settled calls are not counted;
  - orphan rows and `unit_spend` retention are left to a later retention pass;
  - F13's non-adopted items;
  - the optional disclosure facet of F6 (the Conflict naming the owning unit).

### Process and instruction feedback

- Limiting the recheck to the F16 delta was enough. The fix was local, and the earlier
  executions remained valid because r5 changes no other path.
- Across R1–R5, every post-R1 defect was a lock-order interaction between the new charging path
  and department administration, and each was found by executing the protocol on a reduced
  schema. Candidate instruction for lock-protocol gates (scope: skill or persistence AGENTS):
  - list every statement that locks rows of each shared table, with its visiting order and
    mode;
  - run each enumerated race once, including one alternate plan per set-based statement.

## 07UR-P02-B1-R1 (2026-11-21)

### Identity

- Round 07UR-P02-B1-R1, batch review of B1 = doc 02A (G1 D1–D4 with the r2 amendments to D1/D2).
  Workflow implement-opus-reviewed-2_4. Task [TASK.md](TASK.md) ("B1 plan", "B1 implementation"),
  review [REVIEW.md](REVIEW.md), findings [agent-review-findings.md](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5), the same agent as the G1 rounds.
- Phase start = review base = HEAD 107fd58867f7600bfaca56f39a4c790bde41c013. B1 is `git diff HEAD`
  (34 files, +1397/−285) with no new untracked source files. Digest
  bbfb5e1c2cbba8700411b000a30a30bdc40506c4cd22830826b721b4558c03b1, exclusions unchanged.
  Verified at start and before returning. Preserved non-phase untracked files were ignored.
- Coordination: revision 9 (sha256 d35241da…d124), unchanged. Decisions (a) and (g) govern B1.
  TASK.md sha256 5e893ed4…f5176 at return.
- Evidence:
  - Author manifest `logs/07UR-P02-B1-R1.evidence.json` validated with `check-evidence.py
    --expect-artifact bbfb5e1c…` (exit 0: 4 PASS, 2 FAIL). The two FAILs are the pre-revert full
    suite and the focused rerun, whose only failure is the inherited Doop deferral
    (`doop_agent_current_preparation_reaches_ready_before_model_io`).
  - B's run:
    [logs/07UR-P02-B1-R1-reviewer-unit-model-default.txt](logs/07UR-P02-B1-R1-reviewer-unit-model-default.txt)
    (sha256 4e1119c2…0965): `cargo test --locked --offline --lib -- unit_model_default` with
    DATABASE_URL on B's `openbot_p07_p02r_b1` (dropped afterwards); 4 passed. The test binary
    was current; no rebuild was needed.
  - The fixtures create and drop per-test `open_bots_own_*` databases. Eleven such databases
    remain on the cluster with no connections. They cannot be attributed to B's run, so B did
    not drop them.

### Coverage and evidence

- Read the whole diff:
  - schema: id, unit FK RESTRICT, per-unit default index, `unit_default_model`;
  - per-unit replace: locks, `unit_management_role`, unit-conditional upsert and update,
    Conflict, unit-scoped default reset and delete, invariants, the unchanged company-wide
    stranding check;
  - `model_connections_on` ordering, the agent-reach id list, the move via `move_resource_on`
    and `move_model_connection_on`, the uses listing, the department delete listing;
  - `check_agent_model_reach_on` at publication capture and at admission manifest capture (every
    use mode);
  - `resolve_model_on` with the placement (Complete and Deferred) or the root, and
    `model_in_reach_on`;
  - the library copy reach fallback in `prepare_library_agent_on`; app-layer pinning removed;
    the Builder's reach refusal;
  - the use cases (root and unit replace, list by management, unit list, move, reach-filtered
    authoring list);
  - routes (department models POST, JSON move) and pages (department card, shared form,
    settings form on the root's own rows, agent form label and marker);
  - the adapted tests and raw-SQL fixtures.
- Callers checked:
  - every production agent creation reaches `insert_company_agent_identity_on →
    resolve_model_on` (UI/API create, library copy, onboarding, Builder; the persistence
    `create` is `#[cfg(test)]`);
  - onboarding agents are unplaced and placed afterwards, so they pin the root's default, the
    only one at onboarding;
  - every `company_model_connections` reader (credential rotation unchanged and unit-agnostic;
    availability `FOR SHARE` reads unchanged);
  - replace, move, admission and department delete all serialize on the company lock.
- Accepted:
  - schema and FK, the cascade function, and unit-conditional writes. The conflict-never-
    overwrites property is shown by the competing test, which discriminates even sequentially.
  - Root-default invariant on move and replace.
  - The department delete listing.
  - Reach at publication (tested) and admission (inspection only, B1-F1).
  - Explicit-model reach at UI/API create (tested).
  - Pins frozen (tested).
  - Decision (a) widening: root managers manage the root's rows through persistence and the root
    department page; the settings form stays owner-only (`editable = is_owner`), see B1-F2.
- Light or not reviewed:
  - native session paths (local agents) for model reach: inspection suggests native harnesses
    do not use the hosted connection;
  - page HTML beyond the diff;
  - the JSON move route's serialization (reuses p01's `moved_json`).
- Mappings checked: TASK B1 records four tests against the G1 `unit_model_default_*` mapping;
  gaps are listed in B1-F1. Contract rows from G1 r2 (F8) were checked against the diff; gaps are
  in B1-F4.

### Outcome

- **Changes required; no PASS for B1.**
- Actionable: B1-F1 (medium), B1-F2, B1-F3, B1-F4 (low).
- Optional: B1-F5, B1-F6.

### Process and instruction feedback

- The batch size was reviewable in one round.
- The author recorded the invariant revert, which made B1-F2 quick to find, but labeled the
  relaxed rule as "the gate's rule". A deviation from a passed gate should be named as one.
- TASK's B1 test line names four tests, not a case-by-case mapping, so the mapping gaps (B1-F1)
  were not visible to the author. Candidate: after implementation, rewrite the planned mapping
  rows with test names, one per case.

## 07UR-P02-B1-R2 (2026-11-21)

### Identity

- Round 07UR-P02-B1-R2, correction recheck of batch B1 (02A) by finding ID (07UR-P02-B1-F1 … F6).
  Workflow implement-opus-reviewed-2_4. Task [TASK.md](TASK.md) ("B1-R1 corrections"), review
  [REVIEW.md](REVIEW.md), findings [agent-review-findings.md](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5), the same agent as B1-R1.
- HEAD 107fd58867f7600bfaca56f39a4c790bde41c013 plus the working tree (42 files, +1892/−301), digest
  5a938fa0571d0863f5c823653901ca94f59b6cb7effd63222ca4ebd317494359, exclusions unchanged.
  Verified at start and before returning. Coordination revision 9, unchanged. TASK.md sha256
  82c58dac…8a61 at return.
- Evidence:
  - Author manifest `logs/07UR-P02-B1-R2.evidence.json` validated with `--expect-artifact
    5a938fa0…` (exit 0: 2 PASS, 1 FAIL). The FAIL is the focused rerun taken before the directory
    schema fix: seven directory/Builder tests plus the inherited Doop deferral. All seven pass in
    `B1-R2-new-and-fixed.txt` (73 passed), checked by name.
  - B's run:
    [logs/07UR-P02-B1-R2-reviewer-focused.txt](logs/07UR-P02-B1-R2-reviewer-focused.txt)
    (sha256 1877befd…a637): `cargo test --locked --offline --lib -- unit_model_default
    department_directory` against B's `openbot_p07_p02r_b1r2` (dropped); 14 passed. The binary
    was current, with no rebuild.

### Coverage and evidence

- Rechecked each finding's correction in the code:
  - F3: the error mapping.
  - F2: use-case root detection and rules; the persistence count check on the named path.
  - F4:
    - the entity fields and their only producer (`model_connections_on`) and consumers
      (agent form, `prompt_model`); two internal literals in company_agent.rs and provisioning.rs
      use empty names;
    - the directory query and output schema, with callers: the schema validator, the listing
      tests, and the Builder runtime tests whose assertions now name the department;
    - the warning texts.
- Read the six new tests and their decisive assertions (detailed in B1-F1's resolution).
- Affected behavior re-run: all `unit_model_default_*` and `department_directory*` tests.
- Inspection only:
  - the broader focused rerun was taken before the directory fix; the fix touches only the
    listing output and schema, which the 73-test rerun covers;
  - F3's error propagation;
  - the F4 label and marker rendering (B1-F7, optional).
- B1-R1 evidence for unchanged code (schema, replace, move, reach, cascade) is reused; the
  corrections did not change those paths beyond the items above.

### Outcome

- **PASS for B1** at digest 5a938fa0571d0863f5c823653901ca94f59b6cb7effd63222ca4ebd317494359.
- B1-F1 … F4 fixed. B1-F5 and F6 dismissed (optional, not adopted, reasons recorded).
- Optional B1-F7 open; it does not block.
- Coverage relied on:
  - B1-R1's full-diff review;
  - this recheck of the corrections;
  - the author's evidence (manifest valid; the remaining failure is the inherited Doop
    deferral);
  - B's own run of the 14 affected tests.

### Process and instruction feedback

- Fixing every finding in one round worked. The focused rerun caught the directory schema
  break before the recheck, and the author recorded it with its fix. That shows the value of
  running the touched-area suite after a contract-visible output change.

## 07UR-P02-B2-R1 (2026-11-21)

### Identity

- Round 07UR-P02-B2-R1, batch review of B2 = doc 02B (G1 D5–D8 with r2–r5; decisions c, h, i, j,
  k). Workflow implement-opus-reviewed-2_4. Task [TASK.md](TASK.md) ("B2 plan", "B2
  implementation"), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5), the same agent as earlier rounds.
- Review base = HEAD 55f5f633f614fec3bef1682c2d0145c780299e24 (accepted B1, committed); phase start
  107fd588. B2 is `git diff HEAD` (17 source files plus TASK) and two untracked files
  (`persistence/unit_budgets.rs`, `workflow/unit_budget_tests.rs`). Digest
  80557b37f0f8cfabff6563c651842dff085ae5685de3c3882580ffba0e83cd8b, exclusions unchanged.
  Verified at start and before returning. Coordination revision 9, unchanged. TASK.md sha256
  5472729c…b1b at return.
- Evidence:
  - Author manifest `logs/07UR-P02-B2-R1.evidence.json` validated with `--expect-artifact` (exit
    0: 4 PASS, 1 FAIL). The FAIL is the full suite taken before the retention-test column fix;
    its other failure is the inherited Doop deferral.
  - B's run:
    [logs/07UR-P02-B2-R1-reviewer-unit-budget.txt](logs/07UR-P02-B2-R1-reviewer-unit-budget.txt)
    (sha256 6cfb1caa…a3ae1): `cargo test --locked --offline --lib -- unit_budget` after a
    rebuild, against B's `openbot_p07_p02r_b2` (dropped); 7 passed (six `unit_budget_tests` and
    the schedule production case).

### Coverage and evidence

- Read the whole delta:
  - schema: `unit_id` on live and history usage without FK, plus indexes; the
    `attribute_model_usage` BEFORE INSERT trigger; `unit_budgets`; `unit_spend`;
    `charge_unit_spend` and its deferred constraint trigger; `forget_unit_spend`; the schedule
    refusal CHECK;
  - `unit_budgets.rs`: period parsing, cap set/clear under person+company locks with
    `unit_management_role` on the unit (decision c), `check_admission_on`;
  - its placement after `run_scopes::capture_on` in `admission_write::create`;
  - the archive's column list;
  - the refusal plumbing: `AppError::BudgetExhausted`, HTTP mapping, message/schedule
    `classify`, inbound events, run controls, the publication failure class;
  - cap routes and use cases.
- Checked against code and callers:
  - **Attribution:** every `model_usage_calls` insert is the agent reserve, the JEV reserve or a
    test fixture; none names `unit_id`, so the trigger sets it from the run's base write row.
    That row is unique per run; child runs copy scopes; the root is the fallback. The
    `NOT NULL` check runs after the BEFORE trigger. The archive copies `unit_id`, and the
    history guard's whole-row equality keeps them aligned.
  - **Charge:**
    - exactly once per call (the only settle sites set `settled_at` from NULL; the immutability
      guard forbids later change; the WHEN clause);
    - at COMMIT; no department locks; upserts ordered `(unit, period, start)`;
    - numeric ceiling and saturating sums; price at settlement; unpriced tokens apart;
    - UTC period starts from `requested_at`; root fallback.
    This matches G1 r3–r5.
  - **Admission read:** ≤8 units × 2 periods via PKs; `clock_timestamp()` period starts; nearest
    unit, then day before month; READ COMMITTED after the company lock. Only root admissions pass
    through `create`; replays return earlier; native MCP brokers make no model calls.
  - **Refusal:** schedule occurrences fail with the code (production test). Message intents store
    the reason as free text (no closed list). Inbound events are definite. Authoring samples are
    in-memory and never admit.
  - **Caps:** `unit_management_role` on the unit itself (lead or above); the period is validated;
    cap > 0; upsert/delete.
- Tests: read all seven and the classify assertion.
  - Discriminating:
    - nearest/day-before-month and sibling isolation;
    - unpriced-only;
    - authority (unit lead, ancestor lead, child lead/sibling 404);
    - the blocked admission behind the company lock refused after the crossing settlement commits
      (would fail on a pre-lock read);
    - 16 concurrent settlements with exact totals (would fail on a lost update);
    - delete keeps ancestors; a call of a deleted unit charges the root;
    - no wait on department `FOR UPDATE`;
    - rollover by `requested_at`.
  - Not discriminating or missing: B2-F1.
- Not exercised: the real settlement transaction and JEV (B2-F1); production-size plans; the
  usage page and dashboard (B3).

### Outcome

- **Changes required; no PASS for B2.**
- Actionable: 07UR-P02-B2-F1 (medium).
- Optional: 07UR-P02-B2-F2.
- No code defect found in the delta. The implementation matches G1 r1–r5 on inspection.

### Process and instruction feedback

- The batch was cohesive and reviewable in one pass.
- The gate's explicit hand-off of real-path verification to the batches did not make it into the
  B2 plan's test list (B2-F1). See the candidate instruction in that finding.

## 07UR-P02-B2-R2 (2026-11-21)

### Identity

- Round 07UR-P02-B2-R2, correction recheck of batch B2 (02B) by finding ID: 07UR-P02-B2-F1
  (actionable) and F2 (optional). Workflow implement-opus-reviewed-2_4. Task [TASK.md](TASK.md)
  ("B2-R1 corrections"), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5), the same agent as B2-R1.
- Review base = HEAD 55f5f633f614fec3bef1682c2d0145c780299e24 plus the working tree, digest
  60e2c15f4935ce5371b485a604069eb4ada4f6add0ec3358466e97af4ea11929, exclusions unchanged.
  Verified at start and before returning. Coordination revision 9, unchanged. TASK.md sha256
  e09b8bcc…b9e at return.
- The production-file diff stat is identical to R1's, and `forget_unit_spend` still carries the
  ordered `PERFORM` (schema restored after the author's negative check). The changes are tests:
  autonomous_agent_admission_tests.rs, jev_runtime_tests.rs and the untracked
  unit_budget_tests.rs.
- Evidence:
  - Author manifest `logs/07UR-P02-B2-R2.evidence.json` validated with `--expect-artifact` (exit
    0: 2 PASS, 1 FAIL; the FAIL is the intended negative check, a deadlock with the unordered
    delete). Focused rerun: 133 passed. Clippy clean.
  - B's run:
    [logs/07UR-P02-B2-R2-reviewer-unit-budget.txt](logs/07UR-P02-B2-R2-reviewer-unit-budget.txt)
    (sha256 d0a35d4f…1bf8): `cargo test --locked --offline --lib -- unit_budget` against B's
    `openbot_p07_p02r_b2r2` (dropped); 11 passed.

### Coverage and evidence

- Read each new or replaced test against B2-F1's parts (a)–(e) and followed the production
  branches they drive:
  - `WorkflowService::admit`, then `claim_io`, then the agent runtime, then
    `persist_checkpoint_usage_on` in the checkpoint settlement transaction, then the COMMIT
    charge;
  - the JEV runtime's `reserve_on` and `settle_on`;
  - a child created by `child_support::spawn` with copied scopes, then the insert trigger.
- The decisive assertions, and the failure each one distinguishes:
  - the recorded `unit_id` versus a root default;
  - exact spend on the unit and the root versus none or double;
  - unpriced tokens versus spend;
  - the old and new parents' counts after re-parenting;
  - no deadlock with the ordered lock, deadlock without it (author's negative log, matching B's
    R4 reduced-schema result).
- B did not re-run the negative check: it requires editing the schema, which is outside B's
  write scope. It relies on the author's retained log plus B's own R4 reproduction.

### Outcome

- **PASS for B2** at digest 60e2c15f4935ce5371b485a604069eb4ada4f6add0ec3358466e97af4ea11929.
- B2-F1 fixed. B2-F2 (optional) deferred to p03, recorded.
- Coverage relied on:
  - B2-R1's full review of the production delta (unchanged since);
  - this recheck of the corrected tests;
  - the author's evidence (manifest valid);
  - B's run of all 11 `unit_budget_*` tests.

### Process and instruction feedback

- The author's negative check (removing the fix and showing the test fail) is the strongest
  evidence that a concurrency test discriminates. Worth recommending for lock-order fixes
  generally. Candidate: "for a concurrency fix, retain a log of the test failing with the fix
  reverted."
