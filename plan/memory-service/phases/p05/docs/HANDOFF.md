# p05 handoff

- Current state (2026-11-08 ~20:15Z): p05-B2 implementing by A gen8 (af38660db9d70c7bd, Opus/high);
  scope extended to the whole 05A remainder incl. agent tools and the recall/save race; one batch review
  follows (new Opus/xhigh reviewer). After B2 acceptance root (Opus/xhigh) authors p05-B3 (05B).
- Authoritative records: TASK.md (this dir); REVIEW.md / agent-review-notes.md / agent-review-findings.md
  (created by B on first review); logs/. Coordination r1 at plan/memory-service/COORDINATION.md.
- Prior records (read-only): tmp/memory-service-run/progress.md (old root record), reviews/ (P1–P7b),
  frozen/ (old diffs + tree ids), logs/ (all worker logs).
- Accepted: phases 01–04 (service) and p05 P7a gate + P7b (see TASK.md batches).
- Resources: private cluster `tmp/memory-service-run/pg-cluster.sh` (127.0.0.1:55432, trust, superuser
  postgres; destroy at task end); sqlx-cli 0.8.6 at tmp/memory-service-run/tools/bin; embedding model at
  services/memory/assets/model (gitignored); services/memory/target cleaned (rebuild needed at 06C).
- Authorizations: brew pgvector (done); cleaning own build output; no commits/pushes/deploys.
- 21:55Z: A gen8 returned B2a–c (B2d = agent tools moved to p05-B3, root authors). Frozen digest d20858a7…f314 (tree acbda41e); xhigh B abdd866fb428eea05 reviewing.
- ~23:30Z: B R1 CHANGES REQUIRED (findings MEM-P05-B2-F1..F13 in agent-review-findings.md); corrections sent to A gen8. Sibling session wrote plan/agent-workflow-call/ + src/main.rs notes (preserved).
- Next action (superseded line follows): on B's return, corrections by A gen8 (continuity; it auto-compacts) or root; then root authors p05-B3 (tools) and p05-B4 (05B).

- 2026-11-09 ~00:xxZ INCIDENT: disk full (ENOSPC) during A gen8's R2 mutation runs; two files deleted: src/adapters/persistence/memory/deliveries.rs, src/application/workflow/actions/failure.rs. Root deleted A's probe target tmp/memory-service-run/mut-r2/target (8.6 GB, task-created) → 9.5 GiB free, restored both files from frozen R1 tree acbda41e (git show), and resumed A gen8 to re-apply its recorded R2 edits. Mutation probes now require ≥20 GiB free, one at a time, deleted immediately.
- R2 corrections returned (incl. 3 retargeted base tests per root decision option 1; mutations A–F all caught). Frozen digest a369b44e6526a7e542e81d78108877c0874d3308185c17d9e3ffb62cff2cfc75 (tree 8e4baa51); manifest MEM-P05-B2-R2 valid (14 PASS/4 FAIL/2 NE). Recheck sent to B abdd866fb428eea05.
- R2 recheck (MEM-P05-B2-R2): F1–F9, F13, ordering FIXED; new F14 minor (lanes vs connection reserve), F15 optional docs, F11 nit → round 3 sent to A gen8.
- p05-B2 ACCEPTED in phase: B PASS MEM-P05-B2-R3 for digest 9c7929bf1e3b9fce84664264e0530133669cf078bc77ff45a8ad587dafd51abc (root recomputed, match). Sibling docs (plan/agent-workflow-call, agent-default-channel, departments + main.rs entries) present, not p05.
- Next action: root (Opus/xhigh) authors p05-B3 = agent memory tools (A gen8's 'B2d design sketch' in TASK.md; E5/E6; B's B3 notes: release arm re-justification if tools use ActionService, F12 settled, tool-level race on production entry points) + optional F16 (completion lock_timeout; check stop before claim). Reviewer abdd866fb428eea05 continues. A gen8 idle (available).
- 2026-11-09 ~11:30Z p05-B3 implemented by root (Opus/xhigh): four memory tools end to end, tool
  consumption refs + save provenance (D3), user scope requires a manual root (D4), publication-only
  enabled gate (D6), schema names (D7), MEMORY_SERVICE_URL origin reserved in MCP/HTTP policies (D8).
  Decisions + mapping in TASK.md "p05-B3". Focused run 424/1 → exactness count fixed (11/11);
  9 DB-backed tool tests pass; clippy -D warnings clean; fmt clean. Mutation A caught (race test:
  exact rise-first froze 4, want 6). Mutation B running in tmp/memory-service-run/mut-b3 (own
  CARGO_TARGET_DIR; delete after). Root deleted this worktree's target/debug/incremental (14 GiB,
  own build cache) to keep ≥20 GiB free. Sibling sessions added plan/01-departments/,
  plan/02-workflow-roles/, plan/03-principal-addresses/, plan/04-root-unit/ (not p05).
- Test DB: own database open_bots_root_test on the private cluster (reset after the D7 schema edit).
- Next: mutation B result → final focused rerun on formatted code → snapshot → manifest
  MEM-P05-B3-R1 → batch review to B abdd866fb428eea05.
- ~12:40Z mutation B caught (read test "no result before the observation commits"; e2e level 0 ≠ 5);
  copy + target deleted. Final focused run 425/0 (1 pre-existing ignored); sqlx migrate + prepare
  --check pass. p05-B3 frozen: digest 0d0b4622ed5d78726877bfccc1604450e4a67eff42ddff189957a58090336c3d,
  freeze tree dfeeca6774cffca03ca7302a7e73b0d6a64dd270; manifest MEM-P05-B3-R1 valid (7 PASS / 1 NE).
  R1 batch review sent to B abdd866fb428eea05. Worktree frozen until B returns (records only).
- Next: on B's return, correct all actionable findings in one round (root authors) or accept on PASS
  (recompute digest); then p05-B4 (05B: automatic recall, frozen packs + context.validate, capture +
  generation intents, lifecycle races, phase-05 gate incl. live tests + browser check).
- 2026-11-09 B R1 (MEM-P05-B3-R1): CHANGES REQUIRED, F1 Major (user scope from foreign-authored
  manual runs), F2–F4 Minor, F5 optional. User rulings: U4 keep and document (F1), U5 document
  the unconfigured edge (F4); recorded in E12, COORDINATION r2, docs. R2 corrections by root
  (TASK.md "p05-B3 correction round R2"): eligibility as a pure rule + child test, adversarial tool
  tests, one permit per multi-scope search (`search_scopes`), catalog test. R2 digest
  50d052b7858ba6aab6cdad37ff2e0022d73d4a7f465bbe3d235146a802b23457 (tree d9d63035); recheck sent
  to B abdd866fb428eea05. Root memory note saved (document-admin-and-operator-edges).
- B R2 (MEM-P05-B3-R2): F1–F5 resolved/fixed; new F6 Minor (single-operation test did not
  discriminate; reviewer mutation D passed it), F7 optional. R3 by root: fake counts held calls,
  test waits for three held at once (mutation D now fails it); F7 attached-session DB test taken.
  R3 digest 8857f53867e70b2edb79a9b13a0d21d4b9f05f5729f129daec690f5744bea777 (tree a69f88c5);
  manifest MEM-P05-B3-R3 valid; recheck sent to B. Root cleared its own incremental cache (11 GiB)
  again for disk headroom.
- p05-B3 ACCEPTED in phase (2026-11-09): B PASS MEM-P05-B3-R3 for digest 8857f538…a777 (tree
  a69f88c5); root recomputed de7efd2c…3f3c = B's return digest, drift = sibling plan files +
  sibling NOTES lines in src/main.rs only. Next: p05-B4 (05B) authored by root; B continues.
- p05-B4 (05B.1–4 automatic recall) implemented by root; frozen digest a998bca3…eece1 (tree
  3749a60c); manifest MEM-P05-B4-R1 valid; R1 batch review sent to B abdd866fb428eea05. 05B split:
  B5 = capture/generation/lifecycle races + phase-05 gate. Root cleared its incremental cache (12 GiB)
  again for disk headroom. Next: B's review; meanwhile records-only B5 design reading.
- B R1 (MEM-P05-B4-R1): CHANGES REQUIRED, F1–F4 Minor, F5 optional. User ruling U6 (accept the
  8,192-token cap, fixed 64 KiB bound) → F4; recorded in E12, the E7 budget text, COORDINATION r3.
- 2026-11-09 ~20:50 CET R2 corrections by root (TASK.md "p05-B4 correction round R2"): structural
  recall guard on every agent runtime (F1), tool-item revalidation via fetch + E7 "As implemented"
  note (F2), deadline and concurrent three-scope recall tests (F3), F5 not taken (reason in TASK);
  B3 tool fixture prepares through a revalidating runtime (`ToolReadsOnly` stand-in); docs
  (workflow-operations "Agent memory tools", deploy U5). Focused 931 listed / 929 passed / 0 failed
  / 2 pre-existing ignored; 2 MiB stack 161 passed; clippy/fmt/diff-check clean; mutations H, I, J,
  K each fail exactly their target (copy mut-b4r2 deleted). R2 digest
  507e893896000e551a847db69b4179ba7b399d7683e4feb2e82fd9a8401f8f1b (tree ff1607fd); manifest
  MEM-P05-B4-R2 valid (6 PASS / 2 NE); recheck sent to B abdd866fb428eea05; the private cluster
  test DB is B's while it reviews. Root cleared its own stale build outputs (incremental + 11 GiB of
  superseded open_bots-* artifacts) for disk headroom (~34 GiB free).
- Next: on B's PASS, recompute the digest and accept p05-B4; then p05-B5 (capture policy
  `memory_agent_capture` + settings, generation intents at exchange acceptance `ob.g.<exchange>`,
  delivery, E9 lifecycle races, phase-05 gate: live-service tests, browser check, full suite +
  stack budget as applicable). On findings: correct all in one round (root authors).
- B R2 recheck (MEM-P05-B4-R2): F1–F5 resolved; F6 Minor (deleted tool item untested; B's mutation
  L passed every test), F7 optional (docs: disable then resume → revoked). R3 by root: deleted case
  added (mutation L now fails it on "deleted"), F7 docs taken. R3 digest
  8490e2e6c999eea62a7738e2e762b18c1fbda91cbec21f106612bc761d4f78ee (tree 2a0d2550); manifest
  MEM-P05-B4-R3 valid; recheck sent to B abdd866fb428eea05. Next unchanged: on PASS accept B4, then B5.
- p05-B4 ACCEPTED in phase (2026-11-09): B PASS MEM-P05-B4-R3 for digest 8490e2e6…78ee (tree
  2a0d2550); root recomputed the identical digest, HEAD a6bed48a, coordination r3, manifests R1–R3
  valid. Next: p05-B5 authored by root (E8 capture + generation intents, E9 lifecycle races, 05 gate).
- 2026-11-09 p05-B5 in progress (root authors; design in TASK.md "p05-B5"). Done so far:
  schema `memory_agent_capture` (+ allowlist; test DB `open_bots_root_test` reset after it);
  settings types/commands (`MemoryCaptureKind`, `CaptureLimits`, `MemoryCaptureChange`,
  `set_memory_capture`; persistence `memory/capture.rs`); capture freeze
  `deliveries::{enqueue_capture_on, freeze_capture_on}` (run → tenant → namespace → policy →
  intent); eligibility `agent_history/capture.rs` called from `accepted_on` (Completed only);
  `MemoryClient::request_generation` + adapter + fake route (`generations/request`, fake
  `generation_cap`); delivery `FrozenRequest`/`DeliveryReceipt` by kind, `generation_disabled`
  terminal; preparation provisioning `MemoryRecallPersistence::capture_targets` in
  `MemoryRecall::step`; UI capture section + POST `/ui/companies/{id}/memory/capture`.
  Tests passing: capture_intents (7, delivery_tests child), disable race extended with capture,
  e2e memory_tools::capture (3). Next: thread e2e (Deployment, agent scope, level 0), child-branch
  never captured, settings form test, docs (deploy/workflow-operations), mutation probes, then
  focused run, phase-05 gate (live-service, browser, full suite/stack budget), B5 review.
- 2026-11-09 p05-B5 implemented by root and frozen: digest
  be48350f22f124546d81d9d9abc175039d0c971b8580c817ac32fe103629a5c3 (tree 8f5f0f97; base B4 tree
  2a0d2550; 29 files); manifest MEM-P05-B5-R1 valid (6 PASS / 2 NE); R1 batch review sent to B
  abdd866fb428eea05 (the private cluster is B's while it reviews). Phase-05 gate split into p05-B6
  (TASK batch table). Worktree frozen until B returns (records only). Next: on B's return correct
  all findings in one round (root authors) or accept on PASS (recompute digest); then p05-B6.
- B R1 (MEM-P05-B5-R1): CHANGES REQUIRED, F1/F2 Minor, F3/F4 optional; B6 split accepted (B6 must
  rerun all five prefixes on the final tree and review the script/CI/live tests as code). User ruling
  U7 (2026-11-10) for F2: skip capture while unconfigured (E12, COORDINATION r4; memory note
  updated). R2 by root: capture flag frozen at preparation by configured runtimes, docs, tests
  (U7, in-flight delete, configured thread deployment, child), mutations M6/M7 caught. R2 digest
  6041e394d913523eb06c8aaac8348c675134643e8947fe1281a6a20eca9e4f1e (tree 8e18b451); manifest
  MEM-P05-B5-R2 valid; recheck sent to B abdd866fb428eea05. Next: on PASS accept B5, then B6 gate.
- p05-B5 ACCEPTED in phase (2026-11-10): B PASS MEM-P05-B5-R2 for digest 6041e394…4f1e (tree
  8e18b451); root recomputed the identical digest, HEAD a6bed48a, coordination r4. Optional F5
  (memory_recall.rs `configured()` between `unconfigured`'s doc and fn) goes into B6. Next: p05-B6
  phase-05 gate by root (E11): `memory_integration_live_` #[ignore] tests + scripts/memory-integration.sh
  + CI step, all five prefixes on the final tree, scripts/stack-budget.sh, boundary scripts,
  credential rotation, sqlx prepare --check, browser check of the settings memory section.
- 2026-11-10 p05-B6 in progress (root): F5 fixed; scripts/memory-integration.sh +
  memory-integration-fake-messages.py + live_tests.rs (2 #[ignore] tests) pass against the real
  service locally (private cluster); live run found and fixed a B5 defect (capture events shared a
  source revision → 409), contract fake now enforces that rule; script trap fixed. CI job
  `memory-integration` + verify-job browser step added. Browser check
  (scripts/tests/memory-settings-browser.mjs + dump_memory_settings_browser_fixtures) passes;
  capture fields relaid as labelled blocks (screenshots looked at, both themes). Boundaries: all
  pass except pre-existing transport-boundary offender (channel_reply_continuation_views_support.rs:59,
  unchanged since base). Rotation script passes. Running: stack-budget (full suite, 2 MiB). Next:
  sqlx prepare --check (DB open_bots_mem_prepare), per-prefix list counts, snapshot, manifest
  MEM-P05-B6-R1, B review (script/CI/live tests as code; five prefixes rerun).
- 2026-11-10 p05-B6 frozen and sent: digest 88808f5ad06857e86e417fe9593fd6b72185bdc01b42ad52871054c77e81b0a1
  (tree 7388d1c2; base B5 tree 8e18b451; 11 files); manifest MEM-P05-B6-R1 valid (8 PASS / 2 NE):
  live 2/2, prefixes listed, stack-budget lib suite 3689 passed / 3 pre-existing, boundaries,
  rotation, prepare --check, browser, clippy all-targets. Gate review sent to B abdd866fb428eea05.
  Next: on PASS accept B6 and phase 05 (records: TASK status, REVIEW pointer, COORDINATION phase
  row); then phase 06 (06A ops/retention etc., 06B standalone e2e, 06C final gate incl. full
  suite); at the end destroy the private cluster and stop B.
- 2026-11-10 p05-B6 ACCEPTED and **phase 05 accepted in phase**: B PASS MEM-P05-B6-R1 for digest
  88808f5a…b0a1 (tree 7388d1c2), root recomputed identical, coordination r4. Optional B6-F1–F3
  carried to phase 06. Next: phase 06 under implement-opus-reviewed-2_2 (new records at
  plan/memory-service/phases/p06/docs/; read 06 plan + implementation-decisions 06 sections;
  coordination startup), reviewer B abdd866fb428eea05 continues.
