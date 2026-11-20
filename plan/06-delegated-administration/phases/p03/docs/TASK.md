# 06DA-P03: Integration, documentation and final verification

## Identity and authority
- Workflow implement-opus-reviewed-2_4; phase p03 (Plan06 doc 03: 03A docs, 03B final gate).
- Criteria: ../../../03-integration-and-final-verification.md; AGENTS.md (full verification suite
  once before completion; record unavailable checks; no completion claim while one is missing).
- Root/A Claude Code (Opus 5.5/xhigh, session b36d9dc4-495e-4131-9804-05ebcc381915); B Opus 5.5/xhigh
  (same reviewer agent, continued). Worktree plan06, base a93cd388 (p01+p02 committed).
- Coordination r10. Exclusions: plan/06-delegated-administration/phases/p03/docs,
  plan/06-delegated-administration/phases/p02/docs, plan/06-delegated-administration/phases/p01/docs,
  plan/06-delegated-administration/COORDINATION.md.
- Resources: owned UTF-8 PG18 cluster 127.0.0.1:55407 (data
  /Users/mac02/Library/Caches/openbot/plan06-utf8-postgres, pg_stat_statements preloaded,
  max_connections 300); old C-locale cluster 55406 retained. Docker daemon unavailable. Node:
  default 26.13.1/npm 11.20.0; /opt/homebrew/opt/node@24 available for pinned-runtime gates.

## B1 = 03A docs + 03B final gate
03A: docs/departments.md gains an Administration section; authentication-operations.md and
workflow-operations.md replace "owner or admin" where the unit rule applies, naming only behavior
the code has. 03B: fresh-schema SQLx regeneration, scripts/check.sh on a disposable DB of the UTF-8
cluster, CI comparison, unavailable gates recorded as unavailable, graph refresh (trail build).

## 03A docs (done, uncommitted)
- docs/departments.md: intro and roles paragraphs point to a new **Administration** section
  (predicate; agents leading administer nothing; no conversation access; table of who changes
  what — department fields/move/child/members/roles, desks/bindings/schedules, agents, workflow
  definitions, knowledge, company administration incl. root-lead note; admin member invites;
  lists/pickers; 404 vs 403; Recent administration card and acting roles on audit rows);
  visibility of filters adds managed departments; knowledge read/upload/move/delete cover the
  managed subtree; JSON API writes follow Administration.
- docs/authentication-operations.md: company membership paragraph states root roles/people are
  the owner's, admins invite members, unit rule for unit-owned configuration, company-level
  surfaces stay owner/admin, refusals 404.
- docs/workflow-operations.md: desk response selection requires management of the desk's
  department (code: channel_reply::authorize_settings → management::authorize_association_on);
  private-memory note says "a person who manually runs"; reviewer rule (company admins) and
  run/history inspection (company-wide views) unchanged because the code is unchanged.
- scripts/check-integration-docs.py PASS after the edit.

## 03B final gate (run complete; accepted in phase with B1)
- Fresh UTF-8 schema SQLx regeneration (SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets on
  openbot_p06_final_sqlx @55407): PASS, .sqlx unchanged (logs/final-sqlx-prepare.txt).
- Prerequisites installed (locked, no source change): npm ci (root, plugin, attribution browser),
  Playwright chromium ×2, attribution contract venv (logs/final-prereq-install.txt). Node 24.22.3/
  npm 11.19.1 used vs pinned 24.21.0/11.19.0 (nearest available; recorded).
- Known unavailable before the run: Docker daemon (image gates), pgvector extension not installed
  for this PostgreSQL (memory service gates that need it).
- 2026-11-19, at the user's request during the run (while check.sh was at the browser protocol
  gates, before any memory gate): `brew install pgvector` → pgvector 0.8.7 bottle (extension files
  under /opt/homebrew/{share,lib}/postgresql@18). Probe on @55407: CREATE EXTENSION vector →
  extversion 0.8.7, `'[1,2,3]'::vector <-> '[1,2,4]'::vector` = 1; probe DB dropped. The memory
  gates read MEMORY_TEST_DATABASE_URL/MEMORY_INTEGRATION_ADMIN_URL = @55407/postgres, so this run
  uses it. pgvector is no longer an unavailable prerequisite; Docker still is.
- Database-backed suite (isolated network) in this run: 4883 passed, 1 failed, 30 ignored. The
  only failure is the deferred Doop assertion
  `doop_agent_current_preparation_reaches_ready_before_model_io` (agent_history_freeze_guard_tests.rs
  :435, left 8 vs right 7; 05SC-C001/04RU-C005, not relabelled). The C-locale auth pair passes on
  the UTF-8 cluster.
- Browser protocols using native markup, in check.sh: **FAIL (hang)**. scripts/tests/knowledge-browser.mjs
  sat idle for ~4.7 h (Node report: loop idle 17042 s; open Playwright pipes to a live
  headless_shell; one keep-alive connection to the test server; report kept outside the repo at
  scratchpad/knowledge-hang-report.json). Root killed only that run's headless_shell (pid 11402,
  child of the test file process 11383); the file then failed with "page.evaluate: Target page,
  context or browser has been closed" at knowledge-browser.mjs:89 (the requestSubmit() evaluate
  issued while the first native submission is held by the test server; page.evaluate has no
  timeout). Gate result in that run: 17/18 passed, 1 failed (the hang). check.sh continued.
  - Plan06 changed neither the test, assets/app.js nor the knowledge page; its knowledge changes are
    authorization only (routes/knowledge/mod.rs, persistence/knowledge/permissions.rs).
  - Same fixtures, knowledge-browser.mjs alone with --test-timeout=120000: PASS 1/1 (1.3 s)
    (logs/final-knowledge-rerun.txt, copied from the run's scratchpad output).
  - Same fixtures (copied from the run's CHECK_WORK), the whole gate's 12 files with
    --test-timeout=300000, concurrent with the stack budget suite: PASS 18/18 (3.7 s),
    logs/final-browser-rerun.txt.
  - Disposition (corrected per 06DA-P03-B1-F2): intermittent hang, not reproduced in two reruns;
    the test, app.js and knowledge page are unchanged by Plan06; cause not established. Recorded,
    not fixed here.
    Candidate improvement (not applied): the browser gate has no timeout, so a hang stalls
    check.sh instead of failing it.
- CI comparison: every step of .github/workflows/{ci,agent-history,memory-service,
  attribution-service}.yml has a check.sh gate (formatting, whitespace, boundaries, docs, fetch,
  offline check, clippy, migrations, query statistics, SQLx check, compile tests, conformance,
  browser protocols, isolated DB suite, stack/lease budgets, knowledge/memory/attribution images and
  suites, Doop). No CI gate is omitted by check.sh.
- scripts/check.sh with DATABASE_URL=postgres://mac02@127.0.0.1:55407/postgres: finished, EXIT 1
  (logs/final-check.txt; HEAD a93cd388 + the three docs edits, logs/final-check-head.txt).

### 03B gate record (every check.sh gate)
| Gate | check.sh | Disposition / follow-up evidence |
| --- | --- | --- |
| Runner test coverage | PASS | |
| Local-agent plugin tests and guards | PASS | Also PASS under pinned Node v24.21.0 on 2026-11-20 (Post-acceptance). |
| Reproducible plugin package and runtime install | FAIL 1s | **Unavailable** in this run: package.mjs asserts pinned Node v24.21.0; only 24.22.3 installed. Plan06 changes nothing under integrations/ or services/ (git diff 174c83b3..HEAD empty there). Rerun 2026-11-20 under the pinned runtime: **PASS** (Post-acceptance). |
| Generated CSS is current | PASS | |
| Frontend protocols | PASS | |
| Deployment script tests | PASS | |
| Knowledge artifact preparation tests | PASS | |
| Doop runner boundaries | PASS | |
| Doop probe protocols | PASS | |
| Formatting | PASS | |
| Patch whitespace | PASS | |
| Transport abstraction boundary | FAIL 0s | Tool gap: `rg` not installed. The self-test rejected it ("gate accepted rig in application"); the real check printed "passed" with every rg call "command not found", which is no evidence. Rerun with Orca's bundled ripgrep 15.0.0 on PATH: **PASS 9s** (logs/final-env-rerun.txt). |
| Workflow sample boundary tests / boundary | PASS / PASS | |
| Integration boundary tests / boundary / documentation | PASS / PASS / PASS | |
| Workflow evidence validator fixtures / Retained workflow evidence | PASS / PASS | |
| Fetch locked Rust dependencies, Offline compilation, Clippy | PASS, PASS, PASS | |
| Test database, Migrations (fresh), SQLx metadata matches migrated schema | PASS, PASS, PASS | |
| Compile tests, Query statistics prerequisites, Conformance families | PASS, PASS, PASS | |
| Database-backed test suite (isolated network) | FAIL 1973s | lib: 4883 passed, 1 failed (deferred Doop assertion above), 30 ignored. cargo stopped after the lib binary, so src/main.rs unittests and tests/source_modules.rs did not run there; run separately on fresh migrated openbot_p06_final_rest @55407 under scripts/test-network-isolation.sh: main.rs **6 passed**, source_modules (reachability) **8 passed** (logs/final-remaining-targets.txt). |
| Ignored browser fixture producers | PASS | |
| Browser protocols using native markup | FAIL 17129s | Hang above; whole-gate rerun **PASS 18/18** (logs/final-browser-rerun.txt). |
| Stack budget | FAIL 2101s | Suite at 2048 KiB: 4883 passed, 1 failed (same Doop assertion); no "has overflowed its stack"; script: "tests failed for a reason unrelated to the stack budget". Budget signal clean; gate red only on the deferral. |
| Lease budget | PASS | |
| Knowledge / Memory / Attribution production image | FAIL ×3 | **Unavailable**: no Docker daemon (/var/run/docker.sock absent). |
| Memory model preparation | FAIL 47s | **Unavailable upstream**: manifest pins onnxruntime v1.24.4 archives; all three URLs 404 and GitHub has no release tag v1.24.4 (API 404). SHA-256 pinned, so no substitute. Plan05's final run failed this gate too. After the pin moved to NuGet (16696d6d): **PASS** (Post-acceptance). |
| Memory extracted service suite | FAIL 5s | Tool gap `tomllib` (python3 3.9). Rerun with Python 3.13.18: formatting, poisoned-parent boundary, fetch, offline compile, clippy, owner-role migrations, SQLx, manifest pass; stops at "Model artifacts" (runtime/libonnxruntime.1.24.4.dylib, LICENSE, ThirdPartyNotices missing) → **unavailable** (same upstream 404). After the pin moved to NuGet (16696d6d): **PASS** (Post-acceptance). |
| Memory backup and restore | PASS | (pgvector installed during the run) |
| Memory standalone journey | FAIL 6s | Tool gap `tomllib`; rerun with 3.13 stops at "embedding model is unavailable" → **unavailable** (same artifact). After the pin moved to NuGet (16696d6d): **PASS** (Post-acceptance). |
| OpenBot and memory integration | FAIL 27s | **Unavailable**: embedding model runtime missing (same artifact). After the pin moved to NuGet (16696d6d): **PASS** (Post-acceptance). |
| Attribution extracted service and contracts | FAIL 31s | Tool gap `tomllib`; rerun with Python 3.13: **PASS 51s** (logs/final-env-rerun.txt). |
| Attribution browser integration | PASS | |
| Managed Doop service and private integration | FAIL 379s | **Deferred** (05SC-C001/04RU-C005, not relabelled): Doop's own suite 2 of 303 files / 1 of 2728 tests failed (tests/privateMediaStorage.test.ts; tests/privatePdfRendering.test.ts "renders real private images and an embedded font on both PDF pages…"). |
| Linux network-isolation scope | not exercised | macOS: check.sh prints "NOT EXERCISED locally; required in CI". The isolated-network guard itself ran ("loopback allowed; IPv4 and IPv6 external destinations rejected") for the DB suite, stack and lease budgets and the remaining targets. |

- Graph refresh: `trail build` (successor to graft build) exit 0 (logs/final-trail-build.txt).
- Startup plan paths: src/main.rs NO_BACKWARD_COMPATIBILITY_NOTES lists README and docs 01–03 of
  plan/06-delegated-administration (lines 294–297); main.rs unittests pass.
- Rerun tooling (scratchpad only, nothing installed): PATH shim
  /private/tmp/claude-501/-Users-mac02-orca-workspaces-openbot-plan06/b36d9dc4-495e-4131-9804-05ebcc381915/scratchpad/toolshim
  with rg → /Applications/Orca.app/Contents/Resources/ripgrep/darwin-arm64/rg and python3 →
  /opt/homebrew/bin/python3.13; script scratchpad/env-rerun.sh.
- Verification status: every plan-required item (03B.4) has evidence. The DB suite and stack budget
  are red only on the pre-existing deferred Doop assertion. Unavailable, not passed: 3 Docker image
  gates, Linux network-isolation scope. Passed afterwards (Post-acceptance): the plugin package
  under the pinned Node runtime, and the 4 memory gates once the ONNX Runtime pin moved to NuGet.
  Whole-suite/CI green is **not** claimed.

## B1-R1 corrections (for recheck 06DA-P03-B1-R2)
- 06DA-P03-B1-F1 (low, actionable), each item checked against code before editing:
  1. Root row: root name/description/charter are the owner's, a root admin's or a root lead's
     (use_cases/department.rs managed_parent → managed(None) → ManagementTarget::Root); slug and
     parent never change, no deletion (persistence/department/lifecycle.rs root guards).
  2. Members row: removing a person from the root leaves the company and is the owner's
     (department/root.rs remove_root_member → team.require_owner); removing a root agent takes
     the owner or a root admin (administers_company). Adding stays under "whoever manages".
  3. New owner-only row: company edit/delete, model connections, skills, onboarding
     (company.rs update/delete/list+replace_model_connections → owned_company; skill.rs
     authorize_write → owned_company; onboarding.rs owned_company). Owner+root-admin row keeps team
     levels, invitations, connections, memory, local agents, runs, deliveries, dashboard. Same
     split in authentication-operations.md.
  4. History card: lists created/edited/moved; a deletion is recorded but has no card.
  5. authentication-operations.md: 404 for what the caller does not manage, 403 inside their
     authority (matches departments.md and F10).
  6. Intro: a lead looks after what is inside their department; the department's own name,
     deletion and roles belong to whoever manages its parent.
  7./8. Lead-change binding runs as, and is refused when, "the person who changed the lead" cannot
     see the desk (follow_department_lead_on(…, actor, …)).
  9. Agent Builder list_company_agents answers whoever manages a department, for what they manage
     (provisioning authorize_requester AnyUnit; directory unit_management_role filter).
  - Whole-document role-word scan of the three docs (owner/admin): remaining statements are the
    ones R1 verified (TypeSafe connections, reviewers, capture, run inspection, escalation,
    filters, knowledge, view grant).
  - scripts/check-integration-docs.py: PASS (logs/B1-R2-integration-docs.txt).
- 06DA-P03-B1-F2 (optional, applied): completion record names the gates that passed only on rerun;
  TASK hang disposition reworded "cause not established"; single-file rerun output retained
  (logs/final-knowledge-rerun.txt).

## Acceptance
- B PASS 06DA-P03-B1-R2 (note 06DA-P03-B1-R2-N1) at f8bfaccd7dadb06a774f1bde3dffd5001be0b3fb5246f67f9804df5d73f2d160.
- Root re-digest identical (logs/B1-root-acceptance.snapshot.txt); R2 manifest validates; coordination
  r10 at review, r11 written after acceptance (status only, no requirement change).
- trail check: 3 hard matches on Plan06's own init-schema lines 879 (valid_acting_role comment),
  15766 (unit_management_role root default comment), 17224 (administration transition_kind CHECK)
  against learnings l-9091bbc9 (agent-tool workflow admission), l-3837ec57 (native/workflow
  teardown boundary), l-0bea6cf9 (knowledge owner recheck before cached continuation): keyword
  overlap only; none of those mechanisms is touched. Reason given in the commit message
  (logs/B1-trail-check.txt).
- Status: accepted in phase; committed locally (no push). Plan06 implementation is accepted in all
  three phases but NOT declared complete: required checks unavailable locally and the deferred
  Doop assertion remains red (see gate record).

## Post-acceptance (2026-11-20)
- Push: at the user's request ("push to main"), `git push origin plan06:main` fast-forwarded
  origin/main be1b504b..bca40c21 (no force; branch plan06 itself not pushed). trail check before
  the push: the same 3 init-schema keyword matches explained in bca40c21's message, nothing new.
  Status: **integrated** on origin/main. CI result not observed here (`gh` not installed).
- Plugin gates under the pinned runtime: official nodejs.org node-v24.21.0-darwin-arm64.tar.gz,
  SHA-256 checked against its SHASUMS256.txt, extracted into the session scratchpad (nothing
  installed); bundled npm 11.19.0 matches `packageManager`. With it first on PATH: plugin
  `npm ci`, build, check_plugin (typecheck; test 133 passed; test:guards 14 passed) and
  check_plugin_package (package.mjs pin asserts, build twice with identical bytes, sha256
  5cdb500a…c10d, extract, `npm ci --omit=dev`): all **PASS**; tracked files unchanged
  (logs/final-plugin-pinned-node.txt). The default Node 26.13.1/npm 11.20.0 and Homebrew
  node@24 24.22.3 cannot run the package gate: package.mjs requires exactly v24.21.0 and
  npm@11.19.0.
- These record updates and the 03 completion-record line are status and evidence only, written
  after B's PASS; they were not re-reviewed by B.
- ONNX Runtime pin (user request "update the onnxruntime pin to a release that exists"; commit
  16696d6d; pushed with 5bf6a669 at the user's request, fast-forward 46c53d01..5bf6a669, no
  force): github.com/microsoft/onnxruntime itself answers 404 (API and web; other
  repositories 200), so no GitHub release exists at any version. Kept 1.24.4 and moved both
  identical manifests to Microsoft's Microsoft.ML.OnnxRuntime 1.24.4 NuGet package (one zip for all
  three platforms, 125194303 B, sha256 4b978d50…). Linux x64/aarch64 libraries are byte-identical
  to the previous pins; LICENSE/ThirdPartyNotices differ only by CRLF; the macOS dylib differs and
  was re-pinned. Extracted paths unchanged. Both preparers read zip; the tar link check has no zip
  counterpart (members are stored bytes, checksummed) — a mutation confirmed it guarded nothing,
  and the symlink fixture stays as a never-followed check. Outside Plan06's scope and not reviewed
  by B.
- Checks for the pin (logs/final-ort-repin-gates.txt, logs/final-ort-repin-focused.txt), on
  @55407 with the toolshim Python 3.13: knowledge preparation osx-arm64 (package downloaded) and
  linux-x64/linux-aarch64 (extract and verify) PASS; `knowledge model-check` PASS (native load,
  384-d, norm 1.0000, retrieval 16/16, exact-first 3/3, 8192-chunk corpus, peak RSS 1.75 GiB);
  memory model preparation PASS; memory extracted service suite PASS (178 passed, 2 ignored by
  design, incl. memory_search_real_model_retrieval_acceptance); memory backup and restore PASS;
  memory standalone journey PASS (13 steps, stand-in provider); OpenBot and memory integration PASS
  (7 live tests); knowledge-model-prepare.py 2 tests, deploy.sh and the main-app embedding
  artifact unit test PASS; integration docs PASS; trail check clean.
- Still unavailable, not passed: 3 Docker image gates (their builds now fetch from api.nuget.org,
  unexercised here) and the Linux network-isolation scope; deferred Doop failures stay red. Plan06
  is NOT declared complete.
