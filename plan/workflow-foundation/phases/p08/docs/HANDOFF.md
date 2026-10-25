# P08 handoff

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Updated 2026-10-24: B4c-1 accepted in phase (R025). **B4c-2 (channel agents + agent runtime config) implemented and frozen** at `97531db8…802d`; in review as WF-P08-R026.

## Identity and resources

- Phase `p08`, workflow `implement-opus-reviewed-2_2`; records here; authoritative detail in
  `TASK.md` (requirements, user decisions P08-Q1..Q5, acceptance criteria, gate history, B1 log).
- Root = Author A (this Claude Code session, Opus 5.5 / xhigh). Reviewer B: agent
  `af5ae9ab0d4e0a22f` (general-purpose, opus, xhigh, fresh context) — continue with `SendMessage`.
- Worktree `/Users/mac02/dev/openbot`, branch `workflows`, starting commit `da86871`. Nothing
  committed by p08; no authorization to commit, deploy, publish or reset anything outside the
  phase cluster.
- Phase cluster `127.0.0.1:55480` (`/private/tmp/workflow08-20261022`), databases `workflow08`
  (dev, stale schema, unused by tests) and `workflow08_test` (recreate after every init-schema
  edit). Never touch 5432 or p07's 55470.
- Coordination r10 (r9: WF-C003 p08 verification; r10: WF-C005 for p09); p08 is its writer. WF-C001 r2 (B1 composition, B2
  manual start/rerun done — accepted in phase), WF-C003 r1 (B3), WF-C004 r2 (p08 origin; F030 done).

## State

- Gate P08-D1: rev 3 PASS (R003); contract `logs/decision-D1-r3.md`.
- **B1 accepted in phase** (2026-10-23; not integrated, not committed): B PASS WF-P08-R006 at
  digest `b90a7f9c850b737062fc92bb971899f18756b8f086f2641649a39a657b49a468` (base `da86871`,
  exclusions `plan/workflow-foundation/phases/p08/docs plan/workflow-foundation/COORDINATION.md
  marketing website`; manifest `logs/b1-snapshot-r3.txt`). Rounds R004, R005, R006. Deviations
  D-1..D-5 in TASK.md. Open: F030 (Info, optional) → fixed in B2.
- Coordination r7 published WF-C004 r1 (pool guard for every Postgres pool); r8 published
  WF-C004 r2 (one documented exemption: a test reading a pinned backend's last statement).
- B2 discovery done read-only: `logs/b2-inventory.md`.
- **B2 accepted in phase** (2026-10-23; not integrated, not committed): B PASS WF-P08-R008 at
  digest `e43d8c37b661f17963acebfcf685f9859aeba5d46b9702a3620c0b9aa974b94f`
  (`logs/b2-snapshot-r2.txt`, same exclusions); root recomputed it. Rounds R007, R008. F030–F034
  closed. Open: F035 (Info, optional; expired-occurrence sweep can lose a terminal failure's
  log/metric/`last_error`) → fixed at the start of B3a, reviewed with B3a.
- User decisions P08-Q4 (builder tool: agent + channel on the reply mode the Builder picks;
  channel never public → access mode `team`) and P08-Q5 (no built-in approval on the tool) are
  inputs to gate P08-D2.
- **Gate P08-D2:** rev 1 (`logs/decision-D2-r1.md`, `dfa1bf4d…0c07`) changes required in
  WF-P08-R009 (F036–F038 Medium, F039–F044 Low, F045 Info). **Rev 2**
  (`logs/decision-D2-r2.md`, `b88fe8ad…8afd`) **PASS** in WF-P08-R010; F046–F048 (Low) carried into the B3a review, F049 (Info) adopted. It splits B3 into B3a (provisioning tools, one-transaction assistant provisioning,
  `/run/deadline`, shipped catalogue, Agent Builder; gated) and B3b (JSON routes, decision page,
  WF-C003; ungated). It adds a requester owner/admin invariant for `create_agent_channel`; the
  user was told and did not object (widening to any member would reopen the gate).
- **B3a implemented** (TASK "B3a implementation log": F035 first, then D2 rev 2 + F046–F049,
  deviations DV-1..DV-6). Checks green (broad run 2773 passed; one page-class failure fixed and
  rerun). R011: F051 (Medium, address domain) and F050 (Low, listing bound) fixed;
  snapshot digest `22334ec4…77e1` (`logs/b3a-snapshot-r2.txt`). **B3a accepted in phase**
  (B PASS WF-P08-R012; root recomputed the digest; not integrated, not committed).
- B3b split (TASK "B3b plan"): **B3b-1** (every route except action reconciliation, including the
  decision page, the WF-C003 tool routes and agent grants) **implemented** (TASK "B3b-1 implementation log",
  DV-7..DV-10). Snapshot digest `22923f7f…2c20` (`logs/b3b1-snapshot.txt`, same exclusions).
  R013 CHANGES REQUIRED (F052–F061); all fixed in one round (TASK "B3b-1 corrections for
  WF-P08-R013"); corrected digest `b075aa52…11ef` (`logs/b3b1-snapshot-r2.txt`). The init schema
  gained three indexes; `workflow08_test` was recreated. R014: F062 (Low, an uncompiled CSS
  variant) fixed, F063/F064 (Info) recorded; digest `57566f92…dd33` (`logs/b3b1-snapshot-r3.txt`).
  **B3b-1 accepted in phase** (B PASS WF-P08-R015; root recomputed `57566f92…dd33`; not
  integrated, not committed). **B3b-2** (action-reconciliation route, which needs
  the production reconciliation composition) waits for gate P08-D3.
- B4 (removal, projection retargeting, bootstrap/docs, ToolId wording, memory deletion P08-Q1,
  task page deletion P08-Q3, phase gate): split into B4a–B4d (TASK "B4 plan"); B4a in review.

## Next action

- **B3b-2 is accepted in phase** (R018). F073 (Info) is deferred into B4.
- User decisions P08-Q6..Q8 and the B4 plan (B4a–B4d) are in TASK.
- **Gate P08-D4:** rev 1 got CHANGES REQUIRED in R019 (F074–F083). **Rev 2**
  (`logs/decision-D4-r2.md`, `d88aeb86…3bce`) **PASSED** in R020. F084 (Low) is carried into B4a.
- **B4a is implemented** (TASK "B4a progress", steps 1–7) and frozen for review: digest
  `5b27364aa4651526e4fec1b3b158e078f80fdfc171cdf0d687bc6d48573c2c16` (base/HEAD `da86871` +
  working tree; same exclusions; `logs/b4a-snapshot.txt`; B4a-only manifest
  `logs/b4a-files.txt` against the accepted B3b-2 snapshot). Coordination r10, no new notices.
  Checks: broad B4a set 2,071 passed (`logs/b4a-broad-1.log`), clippy clean, focused logs per
  step, index evidence `logs/b4a-index-explain.log`. Not run: `npm run check:css` (end, with the
  user's go-ahead); `npm run test:run-counts` unavailable (no Playwright).
- **For review:** two indexes not listed in D4 (`workflow_decisions_open`,
  `model_usage_calls_run`) and one D4.2 index dropped on evidence
  (`workflow_decision_reviewers_user`); the simulation page made owner-only on every route;
  the agent-level response contract's controls removed with the runtime settings (columns in B4c).
- **B4a accepted in phase** (2026-10-24): R021 CHANGES REQUIRED (F086 Low, fixed; F087 Info
  resolved), R022 **PASS** at `a54400486b9484c469752a35a48bfd207bdd020486e540f10c5591d4f4e720c8`
  (`logs/b4a-snapshot-r2.txt`); root recomputed it. Not integrated, not committed.
- **B4b implemented** (TASK "B4b progress"): the legacy runtime, its side areas and legacy task
  persistence are deleted (110 staged deletions). Deviations DV-B4b-1..8 and the items left for
  B4c/B4d are listed there. F087.5 closed with `use_cases/approval.rs`.
  - Frozen: digest `ac0a895808f9c091f86ca10cb74ebb7b9aba5524f3200d1d2aab0632eead63c7` (base/HEAD
    `da86871` plus the working tree and index; same exclusions; `logs/b4b-snapshot.txt`).
  - Delta against the accepted B4a artifact: `logs/b4b-files.txt`. Per-file hashes for the next
    delta: `logs/b4b-filehashes.txt`.
  - Checks: `cargo check --all-targets`, clippy `-D warnings` and fmt clean. Focused batch 1
    passed 507 (`logs/b4b-focused-1.log`) and batch 2 passed 972 (`logs/b4b-focused-2.log`).
  - Not run: `cargo sqlx prepare --check` (no sqlx-cli; the 18 stale `.sqlx` files were removed by
    exact query-text match). No schema edit, so the DB was not recreated. Coordination r10
    unchanged (`1df326af…d9ae`).
- **R023: CHANGES REQUIRED.**
  - Findings: F088 (Medium: an orphaned `store_tests` module), F089–F092 (Low) and F093 (Info).
  - All are fixed in one round (TASK "B4b corrections for WF-P08-R023").
  - Corrected digest: `c4b7eaa9c3625d3ac8e0d74a7afa802efcdf8a9dfee1f90a02de81107e63c896`
    (`logs/b4b-snapshot-r2.txt`). The delta against the R023 freeze is `logs/b4b-r2-delta.txt`.
  - Checks: clippy and fmt clean. Focused runs: 707 passed, then 777 passed with 1 intermittent
    timing failure (`lease_resolution` stall test), which passed 3/3 when rerun alone and is
    recorded as a gap.
- **B4b accepted in phase** (2026-10-24). R024 **PASS** at
  `c4b7eaa9c3625d3ac8e0d74a7afa802efcdf8a9dfee1f90a02de81107e63c896`, root recomputed. Not
  integrated, not committed.
  - Carried forward: F094 (Low; intermittent lease timing test) to B4d before the phase gate,
    plus F093.2 and F093.5.
  - B4b per-file hashes for the next delta: `logs/b4b-filehashes-r2.txt`.
- B4c is split into B4c-1 memory, B4c-2 channel agents + agent runtime config, B4c-3 legacy
  runtime schema (TASK "B4c plan"; inventory `logs/b4c-inventory.md`, verified per item).
- **B4c-1 implemented** (TASK "B4c-1 progress"): memory code, config, pages/routes, runtime-metric
  hydradb parts and the memory schema removed; init migration edited in place, `workflow08_test`
  recreated. Two new DB tests (memory schema gone, mutation-checked; company save round trip).
  - Frozen: `94139bfd7ef976af49f6ac6814c86e1b905521d9389a7cc140b72cbe5bec1e11`
    (`logs/b4c1-snapshot.txt`, same exclusions). Delta vs B4b: `logs/b4c1-delta.txt`; hashes for
    the next delta: `logs/b4c1-filehashes.txt`.
  - Checks: check/clippy `-D warnings`/fmt clean; focused 331 + 15 + 1 + 32 passed
    (`logs/b4c1-focused-2..5.log`); `.sqlx` exact-text check clean (25 files).
- **B4c-1 accepted in phase** (R025 PASS at `94139bfd…1e11`, root recomputed; one round; not
  integrated, not committed). Carried into B4c-2 step 0: F095 (Low; `git rm` two stale `.sqlx`
  files; `.sqlx` check is now `logs/r025-sqlx-macro-check.py`), F096.1 (`AGENT_RUN_TIMEOUT_SECS`
  docs), optional F096.2. F096.4 to B4d.
- **B4c-2 implemented** (TASK "B4c-2 work order" DV-B4c2-1..4 and "B4c-2 progress"): F095 fixed
  first; agent runtime config, sub-agents, channel agents/owner/response trigger, review/reply
  policies and default participants removed with their schema; the channel edit keeps the gate
  (`channel_edit.rs`, new competing test); relay accepts plain channels (new test).
  - Frozen: `97531db8994a61e52cb4fe227c5fc2961b7873690e87ce6304a6ae047340802d`
    (`logs/b4c2-snapshot.txt`). Delta vs B4c-1: `logs/b4c2-delta.txt`; hashes `logs/b4c2-filehashes.txt`.
  - Checks: clippy/fmt clean; `.sqlx` macro check clean; focused 833 passed
    (`logs/b4c2-focused-3.log`); node 7 passed; transport-boundary passed.
- **R026: CHANGES REQUIRED** (F097 Medium: the gate doc claimed an eligibility re-read the
  commit did not do; F098 Info ×6). Fixed in one round (TASK "B4c-2 corrections for
  WF-P08-R026", DV-B4c2-5):
  - `channels.settings_revision`, moved on by every settings edit;
  - routing carries it on each intent; the commit re-checks it under the shared gate and refuses
    as the new `AppError::Superseded` (SMTP 451; event worker `superseded` class, retried);
  - two competing tests on the real commit, both orders, mutation-checked;
  - F098.3 fixed (agent create pane previews `@handle`).
  - Frozen: `2d4ea381960e8134768631a06eacc425442310dd0ed06c7a4723ba880e8a7098`
    (`logs/b4c2-snapshot-r2.txt`). Delta: `logs/b4c2-r2-delta.txt` (24 files); hashes
    `logs/b4c2-filehashes-r2.txt`.
  - Checks: clippy and fmt clean; 881 focused tests passed (`logs/f097-focused-wide.log`); node
    7 passed; boundary passed; macro check 23/23.
- B4c-3 inventory, verified read-only against the tree: `logs/b4c3-inventory-verified.md`. The
  F083 competing test is now done (B4c-2); B4c-3 keeps the D4.8 catalog test.
- **R027:** F097 fixed and the protocol accepted. F099 (Low; the gate test's 2 s lock budget) is
  fixed: the spawned edit gets a 30 s `lock_timeout`. F100 (Info; binding status, membership and
  principal deletion sit outside the revision) is recorded. Frozen
  `95fe39183361bb49316fa8cb1181a5cedf45fa0834bfff5e694e4c094baacbe5`
  (`logs/b4c2-snapshot-r3.txt`); the delta is `thread/inbound_tests.rs` only.
- B4c-3 work order (split into 3a schema and 3b runtime metrics; DV-B4c3-1..5) is in TASK.
- **B4c-2 accepted in phase** (2026-10-24): R028 PASS at `95fe3918…acbe5`, root recomputed; rounds
  R026–R028; not integrated, not committed. Carried: F094, F096.2–4, F098.4, F100 binding note.
- **B4c-3a implemented** (TASK "B4c-3a progress"; DV-B4c3-1..7):
  - The legacy runtime schema is out of the init migration: 263 segments, plus the shared-table
    trims and function rewrites. The scratch DB applies it clean, the catalog scan is clean, and
    `workflow08_test` is recreated.
  - The enums are trimmed, the dashboard is retargeted to the 4 job statuses, the legacy tests
    are deleted or rewritten, and a new D4.8 catalog test checks itself with a canary.
  - Frozen: `fad11e2751bc298f501dbcbd1b6b8049c292ecfcf5d12eec25941eb75f1c1dde`
    (`logs/b4c3a-snapshot.txt`). Delta: `logs/b4c3a-delta.txt` (38 files). Hashes:
    `logs/b4c3a-filehashes.txt`.
  - Checks: clippy and fmt clean; 1807 focused tests passed (`logs/b4c3a-focused-2.log`); node 7
    passed; boundary scripts passed; macro check 23/23.
- **B4c-3a accepted in phase** (2026-10-24): R029 PASS at `fad11e27…1dde`, recomputed by root; one round. F101 (Info) .2 → B4c-3b step 0; .3/.4 → B4d.
- **B4c-3b implemented** (TASK "B4c-3b progress"):
  - F101.2: the catalog scan now covers full function definitions and the retired values.
  - `runtime_metric_samples` reports the workflow worker jobs and slots. The gauge is counted by
    `WorkflowWorker` while it runs a job (mutation-checked), and `TASK_WORKER_CONCURRENCY` is
    removed from code, config and docs.
  - Frozen: `845d5596b5945223b27143dc847eda42cc5cd620a84edc4d5262b9e39793e1d6`
    (`logs/b4c3b-snapshot.txt`). Delta: `logs/b4c3b-delta.txt` (22 files). Hashes:
    `logs/b4c3b-filehashes.txt`.
  - Checks: 2031 focused tests passed (`logs/b4c3b-focused-2.log`); clippy, fmt, node and the
    boundary scripts clean; macro check 23/23.
- **B4c-3b accepted in phase** (2026-10-24): R030 PASS at `845d5596…e1d6`, root recomputed; one round. F102 Info (optional extras → B4d). **B4c complete.**
- **B4d implemented** (TASK "B4d progress", DV-B4d-1..3):
  - F094 fixed.
  - Leftover search done: dead legacy helpers, icons and the dead Tasks link removed; the agent
    provenance arm dropped; usage check = 5.
  - Docs and guides rewritten against the code: README, deploy, rig, custom_tools,
    inter-channel and the `AGENTS.md` guides.
  - F073.2/.3 done.
  - Frozen: `4ec81c1a9bf670f8efb5ef46b9ebac7f8c44261a70fa538b19cecfa598140fc1`
    (`logs/b4d-snapshot.txt`). Delta: `logs/b4d-delta.txt` (27 files).
  - Checks: 2114 + 562 focused tests passed; clippy, fmt, node, boundary and macro checks clean.
- **B4d accepted in phase** (R032 PASS at `00ec454f…6829`, root recomputed). Rounds R031 and R032.
  **Every batch is accepted.** The phase acceptance review is done, with no blocking finding.
- **Next:**
  - (done) B review R031/R032.
  - Then the phase gate: ask the user to run the full suite (`cargo test --lib` +
    `stack-budget.sh`) and `npm run check:css`. `sqlx prepare --check` and `test:run-counts`
    are unavailable.

At the end of the whole task: ask the user whether to run the full suite
(`DATABASE_URL=… cargo test --lib`, plus `scripts/stack-budget.sh`); do not run it unasked.

**Status (2026-10-24):** every batch is accepted. At the phase gate the user chose to defer the
gate checks ("Not now"), so the phase is not yet phase-accepted.
- Owed: the full `cargo test --lib`, `scripts/stack-budget.sh` and `npm run check:css`.
- Unavailable: `cargo sqlx prepare --check` (no sqlx-cli; recreate `workflow08` first) and
  `npm run test:run-counts` (no Playwright).
- Nothing is committed.
- On resume: run the gate checks with the user's go-ahead, have B (`af5ae9ab0d4e0a22f`) review
  any fixes, then record phase acceptance.
