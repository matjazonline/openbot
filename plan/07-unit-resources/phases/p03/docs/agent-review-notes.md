# 07UR-P03 agent review notes

## 07UR-P03-B1-R1 (2026-11-21)

### Identity

- Round 07UR-P03-B1-R1, batch review of p03 B1 = doc 03A (documentation, startup plan paths,
  carried optional tests). Workflow implement-opus-reviewed-2_4. Task [TASK.md](TASK.md) ("B1
  implementation"), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5), the same agent as p02. Phase 07UR-P03,
  worktree /Users/mac02/orca/workspaces/openbot/plan06, branch plan07.
- Phase start = review base = HEAD 47c8074cce78ab63e098f4b1370a98918516153d (p02 accepted and
  committed). B1 is `git diff HEAD`: four docs, main.rs, three test files, plus the coordination
  p03 row. Digest d30f98a13277aab7980f5bd3bddc8ac2f666becca082adc8f21182970dd2e62e, exclusions
  plan/07-unit-resources/phases/p03/docs and plan/07-unit-resources/COORDINATION.md. Verified at
  start and before returning. TASK.md sha256 cad361e2…e597; coordination sha256 1b141b55…46c7.
- Coordination revision 9; the only change is the p03 phase row. Decisions a, c, d and f–k
  apply.
- Evidence:
  - Author manifest `logs/07UR-P03-B1-R1.evidence.json` validated (exit 0: 2 PASS).
  - B's run: [logs/07UR-P03-B1-R1-reviewer-tests.txt](logs/07UR-P03-B1-R1-reviewer-tests.txt) —
    the three carried tests, 3 passed (pure tests, no database).
  - `git diff HEAD --check` clean.

### Coverage and evidence

- Checked every new or changed doc statement against the code. Accurate:
  - **Authority:** the administration row matches `unit_management_role`; root resources belong
    to the owner, root admins and root leads.
  - **Creation and offers:** the creation default is `resource_default_unit`; library items have
    no unit; existing agents are offered only what they reach.
  - **Reach codes:** publication, binding and admission codes are as documented; unslotted HTTP
    resolves within the desk's reach; decision f.
  - **Models:** one provider per company; per-unit defaults and the cascade; frozen pins; the
    root's exactly-one default.
  - **Moves:** the move paths for MCP/HTTP/model and the skill move form; the moved-default rule
    (`move_model_connection_on`).
  - **Spend caps:**
    - decisions h–k; attribution to the admitted run's unit and its ancestors;
    - `budget.unit_exhausted` naming the nearest exhausted unit;
    - the cap JSON route and `cap_micros`;
    - deletion keeps ancestors' counts (`forget_unit_spend`);
    - usage page and dashboard visibility, and price writes for root managers only.
  - **Delete and JSON refusals:** they now name skills and connections (lifecycle.rs guard).
  - **Builder:** `create_agent`'s reach and `list_company_agents`' model department.
  - **agent-authoring.md:** the cascade, explicit-model reach, and the library fallback.
  - **workflow-operations.md:** the root budget's code `workflow.root_budget_exhausted` (budget.rs,
    recovery.rs) beside admission-time unit caps with the recorded refusal codes.
  - Inaccurate: P03-B1-F2 (two model claims). Precision and completeness: optional P03-B1-F3.
- main.rs: the plan/07 entries were already present at HEAD; B1 duplicates them (P03-B1-F1).
- The carried tests discriminate:
  - The HTTP body test asserts code, unit, period and exactly 4 fields.
  - `prompt_model_uses_only_the_roots_default`: the old `find(|c| c.is_default)` returns the
    department's default when the root has none, but the test expects a refusal, so the old code
    fails it.
  - The edit-pane case puts a department default first. `.first().filter(is_default)` would mark
    it "(default)", and the test asserts no marker and the escaped department label.
- Not adopted, recorded by the author: B2-F2's production message refusal and cap-route tests
  beyond the UI card; p01 B3-F1. Acceptable: optional items with reasons recorded.

### Outcome

- **Changes required; no PASS for B1.**
- Actionable: P03-B1-F1, P03-B1-F2 (both low).
- Optional: P03-B1-F3.

### Process and instruction feedback

- Checking documentation claim by claim against call sites (here, the reach checks' callers) was
  what found P03-B1-F2. Candidate for documentation batches: "for each documented check, name its
  call sites in the author record".

## 07UR-P03-B1-R2 (2026-11-21)

### Identity

- Round 07UR-P03-B1-R2, correction recheck of p03 B1 (03A) by finding ID: P03-B1-F1, F2
  (actionable) and F3 (optional). Workflow implement-opus-reviewed-2_4. Task [TASK.md](TASK.md)
  ("B1-R1 corrections"; sha256 e30c9238…9d3a at return), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5).
- HEAD 47c8074cce78ab63e098f4b1370a98918516153d plus the working tree, digest
  1cbaafd74a01ad78dc18d66bfea5864cb033e0833ff01de1673146ac06f9c66a, exclusions unchanged.
  Verified at start and before returning. Coordination revision 9.
- Evidence: author manifest `logs/07UR-P03-B1-R2.evidence.json` validated (exit 0: 1 PASS;
  `logs/B1-R2-bins.txt`, 6 passed). `git diff HEAD --check` clean. The carried tests are
  unchanged since R1 (B's R1 run stands).

### Coverage and evidence

- F1: `git diff HEAD -- src/main.rs` is empty; the plan/07 entries are listed once.
- F2 and F3: re-read every changed doc sentence against the code:
  - the move bodies (`UnitTarget` in skill, MCP and HTTP JSON handlers and the model route);
  - the decision f member requirement;
  - the ≥1-connection rule (use case for the root form, persistence for the department path);
  - caps deleted with their department (FK CASCADE);
  - token-allowance parking;
  - the delete refusal texts (lifecycle.rs);
  - the library fallback (`unit_default_model` from the placement).
  
  Native-selection wording is fixed in both docs.
- Residual: custom_tools.md still says authoring offers no out-of-reach resources, models
  included (P03-B1-F2).

### Outcome

- **Changes required; no PASS for B1.**
- P03-B1-F1 fixed. P03-B1-F2 still open: a single clause in docs/custom_tools.md:16-19.
- P03-B1-F3 (optional) mostly adopted, with one new optional nit (desk names in the delete
  refusal).

### Process and instruction feedback

- When a corrected claim appears in more than one document, the correction must reach every
  copy; here one of two was updated.

## 07UR-P03-B1-R3 (2026-11-21)

- Round 07UR-P03-B1-R3, correction recheck of p03 B1 (03A) for the P03-B1-F2 remainder and the
  optional F3 desk-refusal nit. Workflow implement-opus-reviewed-2_4. Reviewer Claude Opus 5.5 (B,
  claude-opus-5-5). Task [TASK.md](TASK.md) ("B1-R2 corrections"; sha256 38718ecb…1492 at
  return), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md).
- HEAD 47c8074cce78ab63e098f4b1370a98918516153d plus the working tree, digest
  43532e1f004ff6d7e87b3e69fe0619a1455eaec4c612d4e876886151a027721d, exclusions unchanged.
  Verified at start and before returning. Coordination revision 9. Docs only since R2;
  `git diff HEAD --check` clean.
- Coverage:
  - Read docs/custom_tools.md:16-21 and the departments.md Delete paragraph against
    `list_company_model_connections` (reach-filtered only for an existing agent),
    `resolve_model_on`'s explicit-model reach refusal, and the delete guard's messages
    (lifecycle.rs:237-260; knowledge documents via the FK "still referenced" conflict).
  - All other B1 content is unchanged since R1/R2 and those checks are reused. That covers the
    docs, src/main.rs (unchanged from HEAD) and the carried tests, which B ran in R1.
- **Outcome: PASS for p03 B1** at digest
  43532e1f004ff6d7e87b3e69fe0619a1455eaec4c612d4e876886151a027721d. P03-B1-F1 and F2 fixed;
  P03-B1-F3 (optional) closed, with messaging/JEV not adopted as recorded.
- Process: no issues.

## 07UR-P03-B2-R1 (2026-11-21)

### Identity

- Round 07UR-P03-B2-R1: batch review of p03 B2 = doc 03B final gate, plus the combined-integration
  review of Plan07 (p01, p02, p03 B1, all accepted and committed). Workflow
  implement-opus-reviewed-2_4. Task [TASK.md](TASK.md) ("B2 = 03B final gate"), review
  [REVIEW.md](REVIEW.md), findings [agent-review-findings.md](agent-review-findings.md).
- Reviewer: Claude Opus 5.5 (B, claude-opus-5-5).
- HEAD b154b3cc5bed59ed39894e12d236c4878f601bf9 plus the working tree (assets/app.css; TASK),
  digest f53289cf382cb4fc2d08a61d9de50a1336f4b04f93f826e0c9f7517ff8e1f564, exclusions
  plan/07-unit-resources/phases/p03/docs and plan/07-unit-resources/COORDINATION.md. Verified at
  start and before returning. Plan07 diff base 5bf6a669. Coordination revision 9.
- Evidence:
  - Author manifest `logs/07UR-P03-B2-R1.evidence.json` validated (exit 0: 6 PASS, 3 FAIL; the
    FAILs are the full check.sh run, the remaining-targets/attribution/Doop rerun, and the Doop
    rerun under TMPDIR=/tmp).
  - B spot-checked the retained logs rather than rerunning the multi-hour suites, as instructed:
    final-check.txt gate table and failure lines, final-rerun.txt, final-doop-suite-summary.txt,
    final-plugin-package-rerun.txt, final-doop-runner-py39.txt, final-sqlx-prepare.txt,
    final-trail-build.txt, final-css-rerun.txt, B2-css-focused.txt.

### Coverage and evidence

- **(1) Generated CSS.** The working-tree change adds only the two `.table-sm` rules (a
  byte-level diff of the minified file). `table-sm` is used by the B3 caps card and the
  dashboard panel, so this was a Plan07 regression that the gate caught. The gate rerun passes,
  and the asset/layout tests (19) pass against the embedded CSS. Accepted.
- **(2) Red gates:**
  - **Not attributable to Plan07** (it touches nothing under integrations/, services/, scripts/,
    .github or .cargo; `git diff 5bf6a669..HEAD` is empty there):
    - the plugin package: rerun passes with the same sha256;
    - the Doop runner: TLS under Python 3.13, 46/46 under 3.9 as in Plan06;
    - attribution: timing; rerun 120 passed.
  - **Unavailable:** the Docker image gates.
  - **Not exercised:** the Linux network-isolation scope.
  - **Red only on the deferred Doop assertion:** the DB suite (lib 4948 passed, 1 failed) and the
    stack budget (no overflow; "unrelated to the stack budget").
  - **Managed Doop:** deferred and red; the environment attribution is plausible but unverified
    (P03-B2-F3).
  - None needs to be treated as a Plan07 regression on this evidence. None may be reported as
    passed.
- **(3) Combined integration.** Checked across batches:
  - **Admission order:** binding readiness reach (p01), then the agent manifest with tool/skill
    and model reach (p01, p02 B1), then run scopes, then the unit-cap check (p02 B2). All are in
    one transaction under the company lock, so a refusal rolls everything back and the first
    failing check names the refusal.
  - **Lock discipline:** model-connection replace and move, cap writes and admission all
    serialize on the company row; the charge takes no department or company lock (G1 r4).
  - **Department delete:** names skills, MCP/HTTP and model connections; caps cascade; totals are
    forgotten in lock order.
  - **Company teardown:** inspection only (P03-B2-F3).
  - **Schema:** fresh migrations and SQLx metadata pass; .sqlx is unchanged.
  - **Docs:** they match the final code (p03 B1 PASS); nothing in B2 changed code or docs.
  - **Rebase:** the target fa58df8a overlaps no Plan07 file.
- **(4) Completeness.** Plan07 cannot be declared complete in doc 03's sense, and doc 03's
  completion record is still "Pending" (P03-B2-F1, P03-B2-F2).

### Outcome

- **Changes required; no PASS for B2.**
- Actionable:
  - P03-B2-F1 (low): fill doc 03's completion record with the true statuses;
  - P03-B2-F2 (medium, process for root): record "accepted, not declared complete" and confirm
    the push with the user.
- Optional: P03-B2-F3.
- The final-gate evidence and its dispositions are otherwise sound, and the CSS fix is accepted.
  A recheck can be limited to the completion record and the recorded status.

### Process and instruction feedback

- The CSS regression shows the value of running the full check.sh even after green focused
  suites: no focused test covered generated assets.

## 07UR-P03-B2-R2 (2026-11-21)

- Round 07UR-P03-B2-R2: correction recheck of p03 B2, records only (P03-B2-F1, F2; F3 optional).
  Workflow implement-opus-reviewed-2_4. Reviewer Claude Opus 5.5 (B, claude-opus-5-5). Task
  [TASK.md](TASK.md) ("B2-R1 corrections"), review [REVIEW.md](REVIEW.md), findings
  [agent-review-findings.md](agent-review-findings.md).
- HEAD b154b3cc5bed59ed39894e12d236c4878f601bf9 plus the working tree (assets/app.css and
  plan/07-unit-resources/03-integration-and-final-verification.md), digest
  c2876c0d1fb91960e9ab63dd56cca47bfe8299bb981225f5671228165f20ec6e, exclusions unchanged.
  Verified at start and before returning. Coordination revision 9.
- Coverage: read doc 03's completion-record diff row by row against the retained B2 evidence
  checked in R1 (final-check.txt gate table, final-rerun.txt, sqlx and trail logs) and TASK's
  status line. No code changed since R1; the CSS check and the gate dispositions from R1 are
  reused.
- **Outcome: PASS for p03 B2** at digest
  c2876c0d1fb91960e9ab63dd56cca47bfe8299bb981225f5671228165f20ec6e. P03-B2-F1 and F2 fixed;
  F3 (optional) closed.
- Plan07 overall: accepted in all phases (p01, p02, p03 B1 and B2), **not declared complete**.
  Docker image gates are unavailable, the Linux network-isolation scope is not exercised, and
  managed Doop plus the deferred Doop assertion stay red. The fast-forward push to main waits
  for the user's explicit confirmation; B sees no other reason against it.
- Process: none.
