# 07UR-P03: Integration and final verification

## Identity and authority
- Workflow implement-opus-reviewed-2_4; phase p03 = Plan07 doc 03 (03A docs, 03B final gate);
  record root plan/07-unit-resources/phases/p03/docs.
- Root/A: Claude Code Opus 5.5/xhigh, session 73d846ee-63bd-49f3-bbdf-7f3d92fcce54. B: Opus
  5.5/xhigh, agent a5e0b0f447b0f21fa (continued from p02).
- Worktree /Users/mac02/orca/workspaces/openbot/plan06, branch plan07, phase start 47c8074c (p01 and
  p02 complete in phase, committed locally). Coordination r9 (decisions a, c, d, f–k apply).
- Snapshot exclusions: plan/07-unit-resources/phases/p03/docs, plan/07-unit-resources/COORDINATION.md.
- Preserved untracked (not part of Plan07): the three .trail/learnings/*-mac02.md (user declined
  to commit) and plan/06-delegated-administration/phases/p01/docs/logs/*.snapshot.tar.
- Authorizations: commit locally after an accepted batch; push to main once p01–p03 are complete
  and accepted (user, 2026-11-20). No deployment or shared/production database reset.

## Requirements (doc 03)
- 03A: docs/departments.md gains a resources section; docs/custom_tools.md and
  docs/agent-authoring.md describe unit-scoped selection and the default model cascade;
  docs/workflow-operations.md describes unit budgets beside the root budget. Only behavior the code
  has. Startup plan paths: add plan/07-unit-resources to `NO_BACKWARD_COMPATIBILITY_NOTES`.
- 03B (once, after integration): fresh-schema `.sqlx` regeneration; `scripts/check.sh` on a
  disposable DB with every gate recorded; CI comparison; unavailable prerequisites recorded as
  unavailable; evidence for formatting, whitespace, locked offline all-target compile, clippy,
  fresh migrations, SQLx metadata, source-module reachability, DB suite, network isolation, stack
  and lease budgets; `trail build` (successor to graft build).
- Carried optional items (from p01/p02 reviews): B1-F7 (tests for the department label, the
  root-only "(default)" marker and `prompt_model` with no root default), B2-F2 (budget HTTP 400
  body, cap routes, production message refusal), p01 B3-F1 (an action run as a plain member and
  after that member's removal). Adopted where cheap in B1; the rest recorded.

## Plan
- B1 = 03A docs + startup plan paths + the cheap carried tests; B review.
- B2 = 03B final gate; B combined-integration review; accept; push to main (authorized).

## B1 implementation (03A + carried tests, 2026-11-21; uncommitted on 47c8074c)
- Docs (each claim checked against the code):
  - docs/departments.md: administration table row for unit resources and caps; new "Resources"
    section (ownership and creation default = `resource_default_unit`; reach for agents at
    publication/admission/native selection; workflows at publication, binding save and admission;
    unslotted HTTP resolution among the desk's reach; decision f; one provider per company, unit
    defaults and the cascade; move routes and the move's default rule) and "Spend caps" (decisions
    h–k, attribution to the admitted run's unit and ancestors, `budget.unit_exhausted`, JSON route
    and `cap_micros`, deletion keeps ancestors' counts = `forget_unit_spend` deletes only the
    unit's rows; usage page and dashboard visibility); Delete and JSON refusals name skills and
    connections (lifecycle.rs:241-260).
  - docs/custom_tools.md: unit ownership and reach codes; Builder `create_agent` reach and
    `list_company_agents` model department.
  - docs/agent-authoring.md: the default model cascade and explicit-model reach; library copies'
    fallback to the department default (channel.rs `prepare_library_agent_on`).
  - docs/workflow-operations.md: the root budget (`workflow.root_budget_exhausted`, in-run)
    beside unit caps (admission-time, `budget.unit_exhausted`, recorded on refused schedule
    occurrences and messages); the usage gauge row no longer says no monetary budget is enforced.
- Startup plan paths: src/main.rs `NO_BACKWARD_COMPATIBILITY_NOTES` lists README and docs 01–03 of
  plan/07-unit-resources (all four carry the note); main.rs unittests 6 passed.
- Carried tests adopted: `unit_budget_http_refusal_is_a_bounded_named_bad_request` (B2-F2 400 body),
  `prompt_model_uses_only_the_roots_default` (B1-F7), and in
  `agent_edit_pane_prefills_the_stored_agent_and_offers_delete` the department label (escaped) and
  no "(default)" for a department's own default (B1-F7). Not adopted (recorded): B2-F2's
  production message refusal path and cap route tests beyond the UI card, p01 B3-F1 (action as a
  plain member and after removal) — fixture-heavy, behavior covered at binding save/admission.
- Checks: logs/B1-focused.txt (bins 6, lib 18 passed), clippy clean (logs/B1-clippy.txt),
  `git diff --check` clean. Frozen digest in logs/B1-author-freeze.snapshot.txt; manifest
  logs/07UR-P03-B1-R1.evidence.json.

## B1-R1 corrections (for recheck 07UR-P03-B1-R2, 2026-11-21)
- F1: removed the duplicated plan/07 block from `NO_BACKWARD_COMPATIBILITY_NOTES` (the four paths
  were already listed at HEAD; now once each, main.rs:298-301). Bins tests 6 passed.
- F2: departments.md and custom_tools.md now say native selection checks tools and skills (the
  model only at publication and admission), and that editing an agent offers only what it reaches
  while the new-agent form lists every model connection with its department and refuses an
  out-of-reach one on submit.
- F3 (optional), adopted: move routes for all four kinds with `{"unit_id": …}` (UnitTarget) and
  forms for skills/MCP/HTTP only; library copy → nearest default up the tree; root budget token
  allowance may park for approval; knowledge documents' generic "still referenced" refusal;
  decision f's current-member requirement; at least one model connection; caps deleted with
  their department. Not adopted: messaging/JEV staying with root management (unchanged by Plan07,
  documented on their own pages).
- Frozen 1cbaafd74a01ad78dc18d66bfea5864cb033e0833ff01de1673146ac06f9c66a; manifest
  logs/07UR-P03-B1-R2.evidence.json.

## B1-R2 corrections (for recheck 07UR-P03-B1-R3, 2026-11-21)
- F2 remainder: custom_tools.md now says editing an agent offers no others and a new agent's form
  lists every model with its department and refuses one out of reach. Optional nit adopted: the
  Delete section says the desk refusal only says it still owns desks (lifecycle.rs:237); skills and
  connections are named. Docs only; no check rerun needed (git diff --check clean). Frozen digest
  in logs/B1-R3-author-freeze.snapshot.txt.

## Status
- p03 started 2026-11-21.
- **B1 accepted in phase** 2026-11-21: B PASS 07UR-P03-B1-R3 at
  43532e1f004ff6d7e87b3e69fe0619a1455eaec4c612d4e876886151a027721d (R1 F1–F3, R2 F2 remainder,
  R3 PASS); root re-digest identical (logs/B1-root-acceptance.snapshot.txt). Committed locally.
  Next: B2 = 03B final gate.

## B2 = 03B final gate (2026-11-21, on b154b3cc)
- Fresh-schema SQLx regeneration: fresh openbot_p07_final_sqlx @55407, `sqlx migrate run`,
  `SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets`: PASS, .sqlx unchanged
  (logs/final-sqlx-prepare.txt); database dropped.
- Tooling (scratchpad only, nothing installed): PATH shim rg → Orca's ripgrep 15.0.0, python3 →
  /opt/homebrew/bin/python3.13; pinned Node v24.21.0/npm 11.19.0 (Plan06's official tarball,
  SHA-256 re-checked against SHASUMS256.txt); attribution venv tmp/attribution-contract-env.
  Docker daemon absent (image gates unavailable). Runner scratchpad/final-check.sh.
- CI comparison: .github, scripts/check.sh, check-support.sh, stack/lease budget scripts and
  .cargo unchanged since Plan06's comparison (git diff 5bf6a669..HEAD empty), which found no CI
  gate omitted by check.sh.
- scripts/check.sh with DATABASE_URL=postgres://mac02@127.0.0.1:55407/postgres:
  16:48–18:12Z, EXIT 1 (logs/final-check.txt).

### Gate record (every check.sh gate)
| Gate | check.sh | Disposition / follow-up evidence |
| --- | --- | --- |
| Runner test coverage, Local-agent plugin tests and guards | PASS, PASS | |
| Reproducible plugin package and runtime install | FAIL 1s | Package built with the expected sha256 5cdb500a…c10d, then `npm ci --omit=dev` in the extracted package refused "Missing: plugin@0.2.0 from lock file". Rerun of `check_plugin_package` alone under the same pinned Node: **PASS**, same sha256 (logs/final-plugin-package-rerun.txt). Plan07 changes nothing under integrations/ or services/ (git diff 5bf6a669..HEAD empty there). Not reproduced; recorded as an environment-dependent failure in the full run. |
| Generated CSS is current | FAIL 0s | **Plan07 regression**: the B3 budget tables use `table-sm`, absent from assets/app.css. Rebuilt with `npm run build:css` (pinned Node): the only change adds the two `.table-sm` rules. Gate command rerun: **PASS** (logs/final-css-rerun.txt). Fix in B2. |
| Frontend protocols, Deployment script tests, Knowledge artifact preparation tests | PASS | |
| Doop runner boundaries | FAIL 7s | 1 of 46 errors: TLS handshake "Missing Authority Key Identifier", which Python 3.13's default strict X.509 verification rejects for the test CA; the shim made python3 3.13. Rerun with /usr/bin/python3 3.9.6 (the interpreter Plan06's run used for it): **PASS 46/46** (logs/final-doop-runner-py39.txt). Tool environment, not Plan07. |
| Doop probe protocols, Formatting, Patch whitespace | PASS | |
| Transport abstraction boundary | PASS 9s | With real ripgrep 15.0.0 on PATH (not the vacuous no-rg pass). |
| Workflow sample/Integration boundaries and documentation, evidence validator, Retained workflow evidence | PASS | |
| Fetch locked Rust dependencies, Offline compilation, Clippy | PASS | |
| Test database, Migrations (fresh), SQLx metadata matches migrated schema | PASS | |
| Compile tests, Query statistics prerequisites, Conformance families | PASS | |
| Database-backed test suite (isolated network) | FAIL 1965s | lib: **4948 passed, 1 failed, 30 ignored**; the only failure is the deferred Doop assertion `doop_agent_current_preparation_reaches_ready_before_model_io` (05SC-C001/04RU-C005, failing before Plan07, not relabelled). cargo stopped after lib; remaining targets run separately on fresh openbot_p07_final_rest under scripts/test-network-isolation.sh: main.rs **6 passed**, tests/source_modules (reachability) **8 passed** (logs/final-rerun.txt). |
| Ignored browser fixture producers | PASS | |
| Browser protocols using native markup | PASS 5s | 18/18 tests ran and passed (no hang this run). |
| Stack budget | FAIL 2082s | Suite at 2048 KiB: 4948 passed, 1 failed (same Doop assertion); no "has overflowed its stack"; script: "tests failed for a reason unrelated to the stack budget". Budget signal clean; threshold unchanged. |
| Lease budget | PASS 293s | |
| Knowledge / Memory / Attribution production image | FAIL ×3 | **Unavailable**: no Docker daemon. |
| Memory model preparation, extracted suite, backup and restore, standalone journey, OpenBot and memory integration | PASS ×5 | |
| Attribution extracted service and contracts | FAIL 32s | 1 of 120: `attribution_company_lifecycle_operator_signal_survives_restart_and_repair` (log did not contain "company status healthy"). Plan07 changes nothing in services/attribution. Rerun of the gate: **PASS 48s**, 120 passed (logs/final-rerun.txt). Timing-dependent log assertion; recorded. |
| Attribution browser integration | PASS | |
| Managed Doop service and private integration | FAIL 0s | Runner lacked `DOOP_TEST_UI_CHROME` (CI sets it). Reruns with system Chrome: first two failed before tests because tsx's IPC socket path under the macOS `$TMPDIR` work directory (113 bytes) exceeds the 104-byte Unix socket limit ("listen EINVAL … tsx-501/….pipe", reproduced by hand); with TMPDIR=/tmp the suite ran: **Test Files 4 failed, 298 passed; Tests 42 failed, 2660 passed, 26 skipped** (logs/final-doop-retry.txt, logs/final-doop-suite-summary.txt): elementResizeBrowser 39/39 (page script `deselect` undefined, sub-second), drawingBrowser 2, privatePdfRendering 1 (Plan06's deferred case), privateMediaStorage skipped (needs Docker). Doop's pinned upstream revision is unchanged by Plan07. **Deferred** (05SC-C001/04RU-C005), not passed; more browser failures than Plan06's run, attributed to the external suite and host Chrome, unverified. |
| Linux network-isolation scope | not exercised | macOS: "NOT EXERCISED locally; required in CI". The isolated-network guard ran for the DB suite, stack and lease budgets and the remaining targets. |

- Graph refresh: `trail build` (successor to graft build) exit 0 (logs/final-trail-build.txt).
- Startup plan paths: `NO_BACKWARD_COMPATIBILITY_NOTES` lists README and docs 01–03 of
  plan/07-unit-resources; main.rs unittests 6 passed.
- B2 source change: assets/app.css regenerated (CSS gate fix). Nothing else changed after the
  gate run; the CSS is embedded (routes/assets.rs `include_bytes!`), so the asset/layout tests were
  rerun against it (logs/B2-css-focused.txt); CSS gate rerun PASS.
- Integration note: origin/main advanced to fa58df8a (Plan10 planning docs + 8 startup-list lines
  in src/main.rs; no overlap with Plan07's files). Plan: rebase plan07 onto it after acceptance,
  rerun main.rs unittests and `git diff --check`, then fast-forward push (no force).

## B2-R1 corrections (for recheck 07UR-P03-B2-R2)
- F1: doc 03's completion record filled with actual statuses (passed / red on the deferral /
  unavailable / not exercised / deferred) and the status line "accepted in phase, not declared
  complete".
- F2: Plan07 is recorded as accepted, not declared complete. Push to main waits for the user's
  confirmation that the authorization covers that state (asked 2026-11-21).
- F3 (optional): CSS fix committed with B2; `trail check` before push; one-entry check of the
  plan/07 startup paths after the rebase; CI comparison of the managed Doop failures after push.
  Company-teardown test with p02 data not added (recorded).
- **B2 accepted in phase** 2026-11-21: B PASS 07UR-P03-B2-R2 at
  c2876c0d1fb91960e9ab63dd56cca47bfe8299bb981225f5671228165f20ec6e (R1 F1–F3, R2 PASS); root
  re-digest identical (logs/B2-root-acceptance.snapshot.txt). **Plan07: p01–p03 accepted in
  phase, not declared complete** (Docker image gates unavailable, Linux isolation scope not
  exercised, managed Doop deferred and red). User confirmed the push in that state (2026-11-21,
  "Push now"). Committed locally; then rebase onto origin/main fa58df8a and fast-forward push.

