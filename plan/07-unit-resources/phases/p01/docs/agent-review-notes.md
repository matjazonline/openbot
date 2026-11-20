# 07UR-P01 agent review notes

## 07UR-P01-G1-R1 (2026-11-20)

### Identity

- Round 07UR-P01-G1-R1; workflow implement-opus-reviewed-2_4; task [TASK.md](TASK.md), review
  [REVIEW.md](REVIEW.md), findings [agent-review-findings.md](agent-review-findings.md).
- Stage: architecture gate G1, decision revision r1 (TASK.md "Architecture gate G1 (decision
  revision r1)": G1-D1 … G1-D11, batch split B1/B2, planned acceptance mapping and contract
  impact). No implementation exists.
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5); effort as set by root at spawn (TASK records
  opus/xhigh; not independently observable from inside the run). Phase 07UR-P01, worktree
  /Users/mac02/orca/workspaces/openbot/plan06, branch plan07.
- Phase start = review base = HEAD = cc545881; artifact digest
  c2d338160c48d4d2eabaa84b5bd5fa5cd389b58d2a93db85478eb2e05fb9ac33 with exclusions
  plan/07-unit-resources/phases/p01/docs and plan/07-unit-resources/COORDINATION.md; confirmed at
  start and again before returning (unchanged). Preserved pre-existing untracked files (one trail
  learning, three Plan06 snapshot tars) ignored as instructed.
- Coordination: plan/07-unit-resources/COORDINATION.md revision 1 (sha256 7d00bd7b…88e9 at
  return), no cross-phase notices; user decisions (a) unit managers everywhere, (b) local machines
  out of scope, (c) budget caps (p02) applied. TASK.md sha256 at return 4041cc10…da9e.
- Retained evidence: none beyond source inspection at cc545881 (no checks executed; no logs).

### Coverage and evidence

- Requirements read from originals: README decisions 1–7 and "What does not change";
  01-ownership-and-selection.md 01A.1–3, 01B.1–4 and focused checks; 02/03 for context only;
  AGENTS.md (root) and src/AGENTS.md, src/application/AGENTS.md,
  src/adapters/persistence/AGENTS.md, src/adapters/http/AGENTS.md, pages/AGENTS.md; relevant
  trail learnings (Plan05 agent-tool edge preflight, preparation format SQL readers, Plan06
  unit-manager and active-binding management decisions).
- Inspected (inspection only, no tests run): schema for skills/company_mcp_connections/
  workflow_http_connections, integration columns, immutability triggers, departments and
  RESTRICT precedents, `unit_management_role`, `workflow_creating_department`,
  `workflow_definition_management_role`, `workflow_target_departments`, `companies_delete_tree`,
  agent capability/revision tables and `check_agent_revision_members`, `agent_binding_source`,
  `native_mcp_tool_source_live`, `schedule_runs_refusal_check`; Rust: agent publication capture
  and tools, `agent_capability`, `agent_binding_selection`, workflow HTTP/MCP publication capture,
  `http::registered_on` and both runtime paths, `authority.rs` closure, admission/admission_write,
  agent_manifest capture, `run_scopes` capture/native_on and `application::workflow::scopes`,
  native MCP admit path, department delete guard, MCP soft delete, integration bindings and
  integration authority snapshots, message/schedule refusal classification and recording,
  provisioning skill/tool lookups, library-agent copy, HTTP error mapping.
- Assumptions challenged: binding identity vs captured unit (F1); company-wide unslotted
  resolution under unit trust (F2); D9b equality vs the plan's refusal condition (F3); single
  admission insertion point vs all `insert_run` producers (F4); FK RESTRICT vs soft-deleted rows
  (F5); revision bump vs integration authority (F6); default-unit helper semantics (F7);
  selection surfaces beyond catalogs (F8); authoring union vs run read set (F9); mapping coverage
  of gated additions (F10).
- Accepted on inspection: D1 shape (composite FK, MATCH SIMPLE library exemption, init-schema edit
  with reset; company teardown order via `companies_delete_tree` plausibly unaffected, but
  untested, see F10); D2 authority list matches the write inventory (mcp.rs, workflow/http.rs,
  workflow/mcp.rs, integration_bindings.rs, mcp/discovery.rs, skill.rs; credential rotation
  unchanged); D4 two-sided management and company-lock ordering; D5 explicit Conflict (except
  F5); D6's disposition that workflow `ToolSnapshot` carries no connection to tag (justified:
  operation-snapshot equality, the connection is fixed by binding slot or resolution) except the
  unslotted part (F2); D8 single variant, HTTP mapping, classify arm and schedule CHECK addition
  (the schedule CHECK is a real closed list, init_schema:16456-16460); D9 placement in the same
  transaction with rollback on refusal; D10 serialization (admission already takes the company
  row via `guard_workflow_role_context`, move takes the company lock, publication reads FOR
  SHARE); batch split B1/B2 (B1 widens write authority before B2 enforces selection; acceptable
  only because commits stay local).
- Lightly reviewed: D2 UI/JSON route enumeration and page rendering; D11 beyond the readers named
  in F1; p02 interactions (model connections, budgets) not reviewed.
- Checks executed: none (architecture gate; no database used). Unavailable: none required.

### Outcome

- **Changes required** for G1 r1. Actionable: 07UR-P01-G1-F2 (high); F1, F3, F4, F5, F6, F9, F10
  (medium); F7, F8 (low). Optional: F11, F12. Details and dispositions in
  [agent-review-findings.md](agent-review-findings.md).
- Unresolved questions for A/root: whether F3's stricter rule or F9's refusal outcome needs a user
  decision; whether native MCP (F4) is to be dispositioned under user decision (b).
- Recheck should cover the revised decision text for each finding ID and the updated mapping;
  unaffected decisions (D1 except F5, D2, D5 except F5, D8, D10) can be reused if unchanged.

### Process and instruction feedback

- Gate scope was appropriate: all eleven decisions are costly to unwind (schema, persisted agent
  format, authorization, admission). The investigation section was accurate where checked and
  useful (file:line claims for publication, admission and readiness matched the code).
- Missing in the author's record: an inventory of run producers, of company-wide name/snapshot
  resolutions, of soft-deleted referencing rows, and of the SQL readers of the agent revision
  JSON; these are the roots of F1, F2, F4, F5, F8.
- Candidate instruction (plan/gate template, scope: architecture gates that narrow a trust
  boundary): "List every company-wide resolution by name or snapshot (including runtime
  re-resolution), every producer of the guarded rows, and every SQL function that rebuilds or
  compares a persisted JSON field being extended." Evidence: F1, F2, F4, F8.
- No defect in repository instructions identified; the persistence AGENTS.md composite-FK
  checklist could add retained/soft-deleted referencing rows (F5).

## 07UR-P01-G1-R2 (2026-11-20, correction recheck)

### Identity

- Round 07UR-P01-G1-R2; workflow implement-opus-reviewed-2_4; stage architecture gate G1,
  decision revision r2 (TASK.md "Architecture gate G1 (decision revision r2, 2026-11-20)" and
  "Acceptance mapping additions (r2)"; r1 text retained). Reviewer Claude Opus 5.5 (B), same
  worktree/branch. HEAD cc545881; digest
  c2d338160c48d4d2eabaa84b5bd5fa5cd389b58d2a93db85478eb2e05fb9ac33 (same exclusions) confirmed
  before and after; records-only change since R1.
- Coordination: revision 2 (sha256 ace267f1…b91e), new user decision (d) on R1's F9; no
  cross-phase notices. TASK.md sha256 1b701dcc…14d5 at review.
- Retained evidence: source inspection at cc545881 only; no checks executed, no logs.

### Coverage and evidence

- Rechecked by finding ID F1–F12 against r2 and the code (dispositions appended in
  [agent-review-findings.md](agent-review-findings.md)): F1, F2, F3, F5, F6, F7, F8, F10 fixed in
  decision; F9 resolved by user decision (d); F4 fixed for native tool calls with a residual
  (new F14); F11 dismissed with an accepted disposition; F12 adopted.
- Re-verified in code for r2: no SQL key-set validator over agent tool/skill JSON (F1);
  child runs copy scope rows (`copy_child_on`, children.rs:376) for runtime reach (F2); the
  only MCP `deleted_at` writer is mcp.rs:291 (F5); ModelOnly uses expose neither tools nor skills
  (publication/capabilities.rs:52-63) (F12); run producers enumerated (admission_write.rs:14,
  children.rs:337, native_mcp/mod.rs:111) (F4); `check_readiness` has five callers, two of which
  run before scope rows exist (F13).
- Affected decisions re-examined: D6 (capture location), D7/D9 (two reaches under decision (d)),
  D4 (no revision bump; management re-checked on the current unit under FOR UPDATE), D3
  (`resource_default_unit`), D1 (MCP NULL-when-deleted CHECK; teardown FK mode decided by test),
  D10 (still holds for moves out of agent reach). Unchanged and reused from R1: D2, D5, D8, D11.
- Decision (d) cross-check: agent-tool workflows and slot/unslotted connections are
  workflow-level and use run reach; an agent's own HTTP/MCP tools and skills use agent reach —
  matches r2. Native bundle delivery is the remaining place an agent's skills reach a run
  without a check (F14).

### Outcome

- **Changes required** for G1 r2, limited to two low-severity actionable findings:
  07UR-P01-G1-F13 (stale r1 mapping/D10 rows contradict r2; no reach context named for the
  `check_readiness` callers that run before scopes exist) and 07UR-P01-G1-F14 (native bundle
  skills/tools not checked under decision (d), or an explicit (b) disposition). No high or
  medium defect remains; all R1 findings are closed or dispositioned.
- A recheck can be limited to the F13 record edits and the F14 decision text; everything else in
  r2 is accepted on inspection.

### Process and instruction feedback

- The correction round was efficient: one decision revision answered all twelve findings, and
  the user decision was captured in coordination with its trigger (F9).
- "r1 stands except where changed" without striking changed mapping rows produced F13; candidate
  instruction recorded there.

## 07UR-P01-G1-R3 (2026-11-20, correction recheck)

### Identity

- Round 07UR-P01-G1-R3; workflow implement-opus-reviewed-2_4; stage architecture gate G1,
  decision revision r3 (TASK.md "Architecture gate G1 (decision revision r3, 2026-11-20)", with
  the corrected r1 mapping rows, superseded markers and contract-impact rows). Reviewer Claude
  Opus 5.5 (B), same worktree/branch. HEAD cc545881; digest
  c2d338160c48d4d2eabaa84b5bd5fa5cd389b58d2a93db85478eb2e05fb9ac33 (same exclusions) confirmed
  before and after; records-only change.
- Coordination revision 2 (sha256 ace267f1…b91e, unchanged since R2), decisions (a)–(d), no
  notices.
- Retained evidence: source inspection at cc545881 only; no checks executed, no logs.

### Coverage and evidence

- F13: corrected rows verified in TASK.md (01A.1 default, 01B.4 move, 01B.3 admission),
  superseded markers on r1 D10 and the risk note, new contract-impact rows for `registered_on`
  reach and native bundle selection. Re-traced the `check_readiness` callers against the
  admission order (admission_write.rs:12/14/66): the rule stated in r3 classifies all of them
  correctly; two callers are not named (optional F15).
- F14: verified that `native::selected_with_locks_on` is reached from session creation, command
  dispatch, bundle read, native MCP authority and frozen-authority validation, so the r3 check
  covers conversation-started and workflow native sessions before any bundle is sent.
- Reused from R1/R2 unchanged: all other G1 decisions (D1–D11 as amended by r2), the r2 mapping
  additions, and the F1–F12 dispositions.

### Outcome

- **PASS for G1 decision revision r3** at digest
  c2d338160c48d4d2eabaa84b5bd5fa5cd389b58d2a93db85478eb2e05fb9ac33 with coordination revision 2.
  F13 and F14 fixed; no actionable finding remains open. Optional: 07UR-P01-G1-F15.
- The pass accepts the decisions and the verification plan, not the execution of its tests;
  every mapping row remains a verification obligation for the B1/B2 batch reviews.
- Lightly covered (to re-examine at batch review): D2 route/UI enumeration, D11 exhaustive
  reader search, the D1 teardown FK mode decided by test.

### Process and instruction feedback

- Three rounds were needed; R2→R3 was record consistency only. The candidate from F13 (a
  decision revision rewrites the mapping rows it changes) is supported by this round.

## 07UR-P01-B1-R1 (2026-11-20, batch review of B1)

### Identity

- Round 07UR-P01-B1-R1; workflow implement-opus-reviewed-2_4; batch B1 = G1 D1–D5 with r2 F5
  (MCP soft delete releases the unit), F6 (move changes only `unit_id`), F7
  (`resource_default_unit`), under G1 r3 PASS (07UR-P01-G1-R3). B2 (D6–D11) not in scope.
- Reviewer Claude Opus 5.5 (B); worktree /Users/mac02/orca/workspaces/openbot/plan06, branch
  plan07. Phase start = review base = HEAD = cc545881; all B1 changes are uncommitted working-tree
  edits plus untracked files (ports/unit_resources.rs, persistence/unit_resources.rs,
  persistence/unit_resources_tests.rs, pages/unit_resources.rs, routes/unit_resources.rs).
  Digest 2c55a5762e07d9e7f2a7d6aaf9a7fffe06571ab1351f8c666a97f9a8a3dad81a (exclusions
  plan/07-unit-resources/phases/p01/docs, plan/07-unit-resources/COORDINATION.md) confirmed at
  start, after the rate-limit pause, and before returning. Preserved untracked files ignored.
- Coordination revision 2 (decisions a–d), no notices.
- Evidence: author manifest logs/07UR-P01-B1-R1.evidence.json (validated with check-evidence.py
  against the digest: 6 PASS, 2 FAIL as declared); reviewer re-run logs/B1-R1-reviewer-unit-resource.txt
  (sha256 0e4e7d15…7473).

### Coverage and evidence

- Read the full working-tree diff for schema, ports, persistence (unit_resources, mcp,
  mcp/discovery, workflow/mcp edit_on, workflow/http, skill, integration_bindings + source,
  company_agent, onboarding, channel library copy, department delete guard), use cases (mcp,
  skill, integration_binding), HTTP routes/pages (JSON create/move, UI pickers, move forms,
  notices), test support and the five new tests.
- Checked against code: every D2 write path named by the coordinator now authorizes the
  resource's unit — MCP update/delete/token (use case + unit predicate), level (in-tx), discovery
  (in-tx), MCP operations (in-tx), HTTP edit/enable/secret/operations/level (in-tx), Vault
  propose/confirm/inspect (in-tx), skill CRUD/copy (use case + unit predicate), library-agent
  copies (placement or root); 404 for unmanaged, 400 "Choose a unit" without a default,
  `may_assign_level` still gives 403; lists scoped to managed units; unit stability under a
  concurrent move (unit predicates; company row lock taken first by authorize/move, agent
  publication and MCP writes, so no lock-order cycle with connection FOR SHARE/FOR UPDATE);
  delete guard listing (≤10 + count, deleted MCP rows excluded); default unit SQL; F5 CHECK and
  delete clearing the unit; F6 move changing only `unit_id`; notices and pickers escaped.
- Checks executed by B: `cargo test --locked --offline --lib -- unit_resource` on a disposable
  database openbot_b1rev_28963 (+ `_test`) on 127.0.0.1:55407, Node 24 first on PATH,
  SQLX_OFFLINE=true: 5 passed (dropped afterwards; no leftover databases). Inspected only:
  focused existing suites (674 passed), clippy, sqlx check, broad lib suite (7 failures: 6
  cloudroom e2e environment-dependent under Node 26 and passing under Node 24; the inherited Doop
  8-vs-7 assertion, Plan05 deferral 05SC-C001, still red).
- Discrimination of tests: the new tests use non-root units and sibling/child leads, so a
  missing unit check would fail them; `test_root_unit`/`test_all_units` fixtures cannot mask a
  missing unit (NOT NULL/CHECK reject it) but, with owner actors, they cannot test authority —
  which is why the existing suites do not cover the Vault/discovery/register changes (F3).
- Not covered by B1 by design: agent/workflow selection and admission (B2); runtime action
  authority (`http_for_action`, `mcp_use`) and `ResourceDirectory::inspect` still require root
  management of the run actor (pre-existing; re-examine with B2's binding slot offers).

### Outcome

- **Changes required** for B1 at the digest above. Actionable: 07UR-P01-B1-F1 (medium, Skills tab
  still owner-only), 07UR-P01-B1-F2 (medium, agent skill picker narrowed to managed units),
  07UR-P01-B1-F3 (medium, missing discriminating tests for changed paths and planned cases).
  Optional: 07UR-P01-B1-F4 (management checks outside the transaction for MCP/skill writes),
  07UR-P01-B1-F5 (move response names unmanaged uses).
- A recheck should cover the F1/F2 code and tests, the F3 test additions with their logs, and
  re-run the unit_resource and affected UI/skill/vault filters; the schema, move, delete guard and
  default-unit code can be reused if unchanged.

### Process and instruction feedback

- Batch size was reviewable in one pass (≈1.8k lines, most in tests and signatures).
- The author's TASK B1 record was accurate about what changed; it omitted the second consumer of
  the skill readers (F2) and counted owner-fixture suites as coverage (F3). Candidate
  instructions recorded in the findings.

- **Follow-up 2026-11-20 (from 07UR-P01-B1-R2):** this round's coverage line "`may_assign_level`
  still gives 403" was wrong: the helper refuses non-admins first (`NotAdministrator`) and the
  refusal maps to 400. The resulting defect (unit leads cannot set any connection level) existed
  at this round's digest and is recorded as 07UR-P01-B1-F6. Cause: the helper's admin gate was
  not read, only its name in the gate text.

## 07UR-P01-B1-R2 (2026-11-20, correction recheck of B1)

### Identity

- Round 07UR-P01-B1-R2; workflow implement-opus-reviewed-2_4; recheck of B1 findings F1–F3 (and
  optional F5 applied, F4 not applied). Reviewer Claude Opus 5.5 (B); same worktree/branch; HEAD
  cc545881, all changes uncommitted. Digest
  c8c0b6d3b7f4634542ffc65305506e2e6d26f1ecaf163029e6156bf354e08cfd (same exclusions) confirmed
  before and after. Coordination revision 2, no notices.
- Evidence: author manifest logs/07UR-P01-B1-R2.evidence.json (validated: 5 PASS); reviewer
  re-run logs/B1-R2-reviewer-unit-resource.txt (sha256 de10f9b8…b4c0): `cargo test --locked
  --offline --lib -- unit_resource` on disposable openbot_b1rev2_19776 (+ `_test`) at
  127.0.0.1:55407, Node 24, SQLX_OFFLINE=true — 13 passed; databases dropped, none left.

### Coverage and evidence

- Read the R1→R2 changes named in TASK "B1-R1 corrections": skill use case (`manages_skills`,
  `list_authoring_page`), persistence `list_authoring_page`, ui_companies Skills tab gate and
  `skills_pane`, company_skills read-only text, ui_agents `skills()`, unit_resources `uses_on`/
  `listed` (F5), test helpers and the eight new tests.
- F1: traced the tab render path for a unit lead — `skills_pane` → `counts(editable=true)` →
  `list_company_channels` → `managed_company` (owner/admin) → not found; the test exercises only
  the use-case predicate. Still open.
- F2: fixed (authoring list company-wide with AnyUnit entry; configuration list managed).
- F3: tests added and passing; Vault, register, root lead, refresh, placed defaults, binding
  uses, unmanaged uses now discriminate; the library-agent copy test does not distinguish
  placement from creator default; the level-ceiling assertion is non-discriminating (F6).
- New: while checking the ceiling test, found that `may_assign_level` refuses every non-admin
  (`manages_levels` = owner/admin), so unit leads cannot set connection levels at all (F6) —
  present since R1 and missed there (follow-up appended to R1).
- Reused from R1 unchanged: schema, move, delete guard, default-unit SQL, Vault/discovery/
  operations authorization code, lock ordering.

### Outcome

- **Changes required** for B1 at digest c8c0b6d3…. Open actionable: 07UR-P01-B1-F1 (medium,
  Skills tab errors for unit/root leads), 07UR-P01-B1-F6 (medium, leads cannot set connection
  levels; non-discriminating test), 07UR-P01-B1-F3 (low remainder: library-agent copy test does
  not discriminate placement). Fixed: F2, F5. Optional open: F4.
- Next recheck: F1 route-level test and fix, F6 level-actor change with within/above-level tests
  for MCP and HTTP, F3 library-copy case; re-run unit_resource, connection_level, ui_companies,
  ui_mcp filters.

### Process and instruction feedback

- Both remaining defects passed tests that asserted at the wrong layer or on an error class shared
  by two checks; the candidate instructions are in B1-F1/B1-F6.

## 07UR-P01-B1-R3 (2026-11-20, correction recheck of B1)

### Identity

- Round 07UR-P01-B1-R3; workflow implement-opus-reviewed-2_4; recheck of B1-F1, B1-F6 and the
  B1-F3 remainder (B1-F4 optional, not applied). Reviewer Claude Opus 5.5 (B); same
  worktree/branch; HEAD cc545881, uncommitted. Digest
  2849d92a62feef47d12e7f73961195e804ef247edbe9801adb55770106493b42 (same exclusions) confirmed
  before and after. Coordination revision 2, no notices.
- Evidence: author manifest logs/07UR-P01-B1-R3.evidence.json (validated: 3 PASS); reviewer
  re-run logs/B1-R3-reviewer-focused.txt (sha256 12455099…5f94): `cargo test --locked --offline
  --lib -- unit_resource access_level connection_level ui_companies` on disposable
  openbot_b1rev3_30470 (+ `_test`) at 127.0.0.1:55407, Node 24, SQLX_OFFLINE=true — 80 passed,
  including the 14 `unit_resource` tests; databases dropped, none left.

### Coverage and evidence

- F1: read `skills_pane` and the new route test module; owner-only counts/settings decoupled from
  the skills section; route test renders the tab for owner, root admin, two unit leads and a
  member. Fixed.
- F6: read `may_assign_connection_level`, its uses in `set_mcp_level`/`set_http_level` (after the
  unit checks, 403) and the level form; other callers of the level writes are the UI/JSON routes
  and a Doop test fixture; team levels unchanged. Tests now distinguish the ceiling from
  management (accepted at own level, 403 above, sibling 404 elsewhere). Fixed.
- F3 remainder: owner placing into Sales distinguishes placement from creator default. Fixed.
- Reused unchanged from R1/R2: schema, move, delete guard, default-unit SQL, Vault/discovery/
  operation authorization, lists, lock ordering, F2/F5 fixes.

### Outcome

- **PASS for B1 at digest 2849d92a62feef47d12e7f73961195e804ef247edbe9801adb55770106493b42**
  (coordination revision 2; G1 r3). All actionable B1 findings fixed (F1, F2, F3, F6); F5
  optional applied; F4 optional open and non-blocking.
- Coverage relied on: R1 full inspection of the B1 diff, R2/R3 inspection of each correction,
  reviewer re-runs of the unit_resource (14), access_level, connection_level and ui_companies
  filters; the author's broad suites (R1: 674 focused existing, lib suite with the 6 Node-26
  cloudroom environment failures and the inherited Doop 8-vs-7 deferral; R3: 82 + 304 focused)
  are inspected, not re-run. B2 items noted for later: runtime action authority and
  `ResourceDirectory::inspect` still require root management of the run actor.

### Process and instruction feedback

- Three B1 rounds; the second exposed a defect missed in R1 (B1-F6, follow-up recorded on R1).
  Both late defects sat behind tests that asserted at the wrong layer or on a shared error class.
