# Plan coordination: fix-app

- Authority: /Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md (restored primary checkout, main branch)
- Writer: current Codex /root session; local recovery setup owner; worktree /Users/mac02/dev/openbot-recovery
- Revision: 21

## Plan-wide instructions

### fix-app-I001 / r1 — Repository and plan rules
- `AGENTS.md` (repo root) and scoped guides under `src/` apply. No backward compatibility (reset DB instead).
- `plan/fix-app/README.md` "Rules for running plans concurrently" and "Verification" apply:
  edit only the named statements of `migrations/20260817000000_init_schema.sql` in place; per-plan final
  gate is `./scripts/reset-db.sh --all` then `scripts/check.sh`.

### fix-app-I002 / r1 — Isolation and resources
- Each phase runs in its own worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-pNN`, branch `fix-app/pNN`.
- Each phase has a private Postgres 18 cluster (trust auth) at `127.0.0.1:554NN`, data dir
  `/Users/mac03/dev/openbot/.worktrees/pg-pNN` (owned by the manager; do not stop/delete it).
  Worktree `.env`: `DATABASE_URL=postgres://mac03@127.0.0.1:554NN/mail_agents` (tests derive `mail_agents_test`).
  psql/createdb: `/opt/homebrew/opt/postgresql@18/bin` (not on PATH).
- Use `CARGO_TARGET_DIR` inside the worktree (default `target/`); never share another phase's target dir.
- Disk is tight: prefix cargo with `CARGO_INCREMENTAL=0`; delete only your own `target/debug/incremental` when needed.
- Never touch another phase's worktree, cluster, branch or records.

### fix-app-I003 / r1 — Commits and merges
- User authorized: each phase root commits its accepted phase on its own branch (`fix-app/pNN`), including
  code, migrations, `.sqlx/` and its own records under `plan/fix-app/phases/pNN/docs/`.
  Do not commit `plan/fix-app/COORDINATION.md`. No push.
- Only the manager merges phase branches into local `main`, then resets DB and reruns focused tests.
  Dependent phases start from the merged `main` commit.

### fix-app-I004 / r1 — Verification scope (user instruction, supersedes the README per-plan final gate)
- Phases do NOT run the full test suite or `scripts/check.sh`. Run only targeted tests for the code the
  phase affects (changed behavior, callers, shared contracts), with nonzero test counts per filter, after
  `./scripts/reset-db.sh --all` when the schema changed. `cargo fmt --check` and compiling the affected
  targets are fine. Regenerate `.sqlx/` if macro-checked queries changed. Record what ran.

### fix-app-I006 / r1 — Independent authentication signing key (user decision)
- User selected “Add AUTH_ATTEMPT_SECRET (recommended; independent key)” on2026-11-04.
- p07 shall add/read/document AUTH_ATTEMPT_SECRET as the stable authentication-attempt and confirmation-code
  signing key, independent of JWT_SECRET. This explicitly supersedes p07 batch3's no-new-secret assumption.
  Pending codes must survive JWT key rotation; hash kinds remain domain separated using the existing HMAC helper.
- No implicit JWT fallback or compatibility path. Implementation/startup semantics and focused tests must be
  reviewed in p07; this instruction changes no p06 storage or retention contract.

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths | State |
| --- | --- | --- | --- | --- |
| p01 | phase root agent | (removed) | main: plan/fix-app/phases/p01/docs/ | integrated in main 9d68a6b (commit 4d940da) |
| p02 | phase root agent | (removed) | main: plan/fix-app/phases/p02/docs/ | integrated in main fbf56e9 (commit 7ca2fb5) |
| p05 | phase root agent | (removed) | main: plan/fix-app/phases/p05/docs/ | integrated in main 48626a1 (commit 2f16a07) |
| p03 | root/A current Codex; B /root/p03_reviewer (idle) | /Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03 | /Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03/plan/fix-app/phases/p03/docs/ | acceptedc42ae04; INTEGRATED at6ffe52a after WAVE2-INT-R1-MAC02 PASS; affected1104/0failed/1ignored |
| p04 | current root/A Codex; B /root/p04_reviewer (idle) | /Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p04 | /Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p04/plan/fix-app/phases/p04/docs/ | acceptedced9cd8; INTEGRATED at6ffe52a after WAVE2-INT-R1-MAC02 PASS; affected523/0failed/4ignored |
| p06 | current root/A Codex; B /root/p06_reviewer | /Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p06 | /Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p06/plan/fix-app/phases/p06/docs/ | G1r3 architecture accepted; WB1 correction in progress (cardinality, retry race, native checkpoints), no code acceptance |
| p07 | — | — | — | ready after acceptedp04 integration; queued afterp06 |
| p08 | — | — | — | prerequisites integrated; queued, usage nullability follows acceptedp06 |

## Cross-phase notices
Phases request notices in their final report or by SendMessage to the manager.

### fix-app-C001 / r1
- State: active
- Origin: p02 (TASK.md "Cross-phase notes"), integrated 48626a1
- Targets: p03
- Instruction/change: the one continuation limit is SQL `workflow_job_continuation_limit()` (63), mirrored by Rust
  `WORKFLOW_JOB_CONTINUATION_LIMIT` (DB test asserts equality). The `effect_parked` branch of
  `check_workflow_fenced_completion` now also requires `OLD.continuation_count < limit`. p03's run-state transition
  table and guard-error classification must admit this park and the refusals it produces.
- Verification: p02 filters `message_park_continuation_`, `delivery_late_completion_`, `notification_stale_`,
  `schedule_claim_` still pass after p03.
- Responses: p03 -> pending

### fix-app-C002 / r1
- State: active
- Origin: p01 (N1), integrated 9d68a6b
- Targets: all current and future phases with database fixtures
- Instruction/change: `companies.user_id` and the owner `company_members` row are one fact (deferred
  `companies_owner_member_fk` + constraint triggers). Fixtures must not insert a company without its owner row,
  change `companies.user_id` alone, or demote/delete the owner row. Use `test_support::insert_company_with_owner`,
  `transfer_company_ownership`, `revoke_membership` / `revoke_membership_now`. Injected statements should `expect`
  success so a refusal cannot pass as the expected error. Deleting an agent retires it; three history FKs are NO
  ACTION (`principals_company_agent_fk`, `messages_author_principal_fk`, `message_participants_identity_fk`).
- Responses: p03/p04 -> satisfied in acceptedcombined6ffe52a (WAVE2-INT-R1-MAC02 PASS).

### fix-app-C003 / r1
- State: active
- Origin: p01 (N3), integrated 9d68a6b
- Targets: p04
- Instruction/change: p04's precondition is met: ownership is enforced as one fact. `CompanyInvitePersistence` changed:
  `is_company_owner_address` added; `update_invite(company_id, …)` matches only a pending invite of that company and
  returns `Option`. Invite acceptance never rewrites the owner row; inviting the owner's own address is refused.
- Responses: p04 -> pending

### fix-app-C004 / r1
- State: active
- Origin: p01 (N4), p02, integrated
- Targets: p08
- Instruction/change: identity status `disabled` remains unused (droppable). Stale notification events now
  dead-letter with `last_error_class` NULL (no stale class exists); add one if 08 copies error-class rules onto that
  queue. 05: `message_deliveries`/`message_delivery_parts.company_id` are NOT NULL.
- Responses: p08 -> pending (not started)

### fix-app-C005 / r1
- State: active
- Origin: p05 BR1 review, integrated 48626a1
- Targets: p03, p06 (anyone editing workflow_runs / init_schema FKs)
- Instruction/change: a workflow binding with runs can no longer be hard-deleted (run -> binding revision FK).
  Several FKs in workflow message/thread tables are DEFERRABLE INITIALLY DEFERRED because a non-deferred NO ACTION
  key is checked per cascade round in Postgres (p01's schema comment saying "end of the statement" is inaccurate).
  p06 retention deleting terminal runs must respect these keys.
- Responses: p03 -> pending; p06 -> pending (not started)

## Manager log
- 2026-10-31: wave 1 started. Phase roots p01, p02, p05 spawned (Opus 5.5 / high, general-purpose subagents of the manager session) from main e6b0a51. Each spawns its own Opus 5.5 / xhigh reviewer.
- 2026-10-31: user instruction — do not start further phases (03, 04, 06, 07, 08) after wave 1 completes. Merges to main held pending user confirmation.
- 2026-11-01: revision 3 — status rows for p01 (4d940da) and p02 (7ca2fb5) updated in place under r2 without a bump; no instruction changed (I001–I004 unchanged).
- 2026-11-01: wave 1 complete; p01/p02/p05 accepted in phase, committed on branches, not merged. Merges await user.
- 2026-11-01: revision 4 — wave 1 merged into main (9d68a6b, fbf56e9, 48626a1; conflict in message_publish.rs resolved, two test fixtures adapted). Merged targeted union: 1586 passed, 1 failed (fixed: p02 test vs p05 generated column), then message_park_continuation_ 5/5, fairness 17/17; sqlx prepare --check passes. User resumed next phases: wave 2 p03, p04 started from 48626a1.

## Manager resume pointer (2026-11-01, paused by user)
- main = 48626a1 (wave 1 integrated, unpushed). Wave 2 p03/p04 paused mid-phase; read each phase's HANDOFF.md in
  its worktree before resuming. Phase roots and reviewers are stopped; a fresh manager starts fresh agents
  (do not trust old agent IDs) with the same roles (root/A Opus 5.5 high, B Opus 5.5 xhigh).
- Running resources (manager-owned): Postgres clusters .worktrees/pg-p03 (:55403), pg-p04 (:55404),
  pg-main (:55400, merge checks). Restart after reboot with
  `LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 /opt/homebrew/opt/postgresql@18/bin/pg_ctl -D <dir> -o "-p <port> -k <dir> -c listen_addresses=127.0.0.1 -c max_locks_per_transaction=256" -l <dir>/server.log start`.
- Phase prompt template: same as wave 2 (I001–I004, notices r4, CARGO_INCREMENTAL=0, targeted tests only,
  commit on own branch, manager merges). User authorized: merge accepted phases into local main and continue
  with next phases (06+08 after 03, 07 after 04). Pull origin/main if new before merging.

### Pending notice requests (publish on resume)
- p03 -> p06: workflow_runs BEFORE-trigger order; new terminal-execution CHECK (see p03 TASK "Cross-phase notes").
- p03 -> p08: retirement value `guard`, failure code `workflow.guard_violation`, handler codes `*.guard_violation`,
  run event kind `guard_violation:<guard>`; token_refusal value `cancelled` dropped.
- p04 -> p03: new `AppError::Forbidden` variant; text conflicts expected in app_error.rs, app_error_impl.rs,
  inbound_event_worker.rs with p03's `AppError::WorkflowGuard`. Merge second one resolves.
- p04 -> p07: ThreadUseCases persistence accessors removed; new AttentionUseCases, Caller, onboarding use case.
- 2026-11-01: revision 5 — p03 and p04 paused at safe points by user; reviewers stopped; handoffs written.

## Resume ownership (2026-11-04)
- Coordination writer transferred from the stopped manager to current manager thread thr_gwccjxwzjt in /Users/mac03/dev/openbot.
- User selected implement-astra-reviewed-2-2 and explicitly chose separate Sol 6.1/xhigh phase authors. Root stays coordinator (actual model/effort not exposed); independent reviewers are Astra/high, direct children of root, without inherited context. Historical Opus gate evidence remains historical; fresh reviewers validate it before dependent work that needs a gate.
- Current primary HEAD e4273e8; phase worktrees remain at 48626a1 with preserved uncommitted phase work. Preserve the unrelated primary modification plan/knowledge-access-levels/README.md. Authors finish on their existing baseline; integration must reconcile all intervening main changes and receive independent review before acceptance as integrated.
- Prior authorizations retained: accepted phase commits, local main merges, pull origin/main if new, dependent phases after prerequisite integration, no push. I004 targeted verification continues to supersede full-suite gates.
- Host exposes context usage but effective auto-compaction threshold is unknown; durable phase checkpoints required.

### fix-app-C006 / r1
- State: active
- Origin: p03 paused TASK Cross-phase notes; G1 r3 historical review
- Targets: p06, p08
- Instruction/change: p06 must account for workflow_runs BEFORE-trigger order and terminal-execution CHECK. p08 must account for retirement guard, workflow.guard_violation, handler *.guard_violation, event guard_violation:<guard>, and removal of token_refusal cancelled.
- Dependencies/order: blocked on accepted and integrated p03 artifact; consult final p03 decision and tests before affected edits.
- Evidence/review: p03 authoritative TASK/REVIEW in .worktrees/fix-app-p03/plan/fix-app/phases/p03/docs; acceptedp03c42ae04, integrated6ffe52a; wholephaseB1-R2-MAC02 and combinedWAVE2-INT-R1-MAC02 PASS.
- Responses: p06 -> pending; p08 -> pending

### fix-app-C007 / r1
- State: active
- Origin: p04 paused TASK Cross-phase notes
- Targets: p03, p04 integration
- Instruction/change: p04 adds AppError::Forbidden; p03 adds AppError::WorkflowGuard. Preserve both semantics in app_error.rs, app_error_impl.rs, inbound_event_worker.rs when merging; second merge resolves overlap and runs both phases' affected checks.
- Evidence/review: both historical G1 r3 decisions; final code and combined behavior require fresh review.
- Responses: p03/p04 -> satisfied in acceptedcombined6ffe52a (WAVE2-INT-R1-MAC02 PASS).

### fix-app-C008 / r1
- State: active
- Origin: p04 paused TASK Cross-phase notes
- Targets: p07
- Instruction/change: ThreadUseCases persistence accessors are removed; new AttentionUseCases, Caller, and onboarding ports/use case must be included in p07 layering work.
- Dependencies/order: blocked on accepted and integrated p04 artifact; consult final p04 interfaces before affected edits.
- Evidence/review: p04 authoritative TASK/REVIEW in .worktrees/fix-app-p04/plan/fix-app/phases/p04/docs; acceptedp04ced9cd8, integrated6ffe52a; WB1-R2/WB2-R1-MAC02 and combinedWAVE2-INT-R1-MAC02 PASS.
- Responses: p07 -> pending

### fix-app-I005 / r1 — User retention decision (2026-11-04)
- State: active
- Targets: p06, p08, integration
- User explicitly selected: delete terminal workflow trees after90 days once no live child, open decision or unsettled reconciliation references them; preserve usage and cost reporting.
- p06 owns the concrete storage/reporting preservation design and independent architecture gate. Do not delete usage/cost reporting information as a side effect of retention. p08 model_usage_calls nullability must follow accepted p06 decision/artifact.
- No authorization for a production reset or deployment is added. Use isolated disposable fixtures for verification.
- Responses: p06 -> pending; p08 -> pending; integration -> acknowledged, implementation pending

## User pause (2026-11-04)
- User explicitly requested: stop at safe checkpoint and write handoff. No new implementation/tests/reviews/commits/merges or dependent phases. Authors are saving current work and logs and stopping only their own commands; reviewer completed evidence remains preserved.
- Manager resume authority remains /Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md. Fresh resume must read primary plan/fix-app/integration/resumed-all/docs/HANDOFF.md and each authoritative phase HANDOFF in its worktree. Do not reuse stale worker handles from this session.
- Current authorization for future resume remains separate Sol6.1/xhigh authors and independent Astra/high reviewers, targeted tests only, accepted local commits/merges, no push. This pause must be explicitly resumed before further work.
- User retention decision I005/r1 remains90 days with usage/cost reporting preserved. p06/p07/p08 have not started.
- p03/p04 source remains uncommitted and unaccepted in this resumed session. Their architecture gates have fresh PASS (p03 G1r4, p04 G1r6); code batches and combined integration still require independent review.
- Manager-owned PG clusters pg-p03:55403, pg-p04:55404, pg-main:55400 remain preserved; do not clean other processes/caches. Final phase paths/digests/results are in phase handoffs.

## Final stop confirmations (revision9)
- Both authors and both independent reviewers confirmed all own commands stopped; no task source commits/merges/pushes occurred. Preserve worktrees/DBs.
- p03 final digest05defcdaaf223bb332f5123dcddda09b740a1d64b96501f9108892e25a1220e9, sole exclusion plan/fix-app/phases/p03/docs; phase HANDOFF and logs/PAUSE-checkpoint.md authoritative. All31 broad-run failures remain unresolved.
- p04 final digesta366d1d6eaba053c6a66ad52a798a073ec14e8a9cecd5903c3a22840d504e092, sole exclusion plan/fix-app/phases/p04/docs; phase HANDOFF authoritative. Broader union/clippy/sqlx/code review not started.
- Manager HANDOFF: /Users/mac03/dev/openbot/plan/fix-app/integration/resumed-all/docs/HANDOFF.md. Phase handoffs last reconciledr8; revision9 only updates final stop/status identities, no technical requirement changes.

## Portable checkpoint publication (revision10)
- User explicitly authorized committing and pushing this recovery checkpoint to main. This publishes paused source as patches/new-file archives plus records, not as accepted phase implementation; application work remains paused.
- Versioned recovery entry point: plan/fix-app/recovery/2026-11-04/README.md and restore.py. Git-based restore reuses original base48626a1 from repository history, creates fresh worktrees, restores phase records, and verifies source. No secrets, databases or build caches included.
- Both offline ZIP and Git-based restoration were exercised in temporary checkouts; all2937 source files and both historical workflow snapshot digests matched. The original phase worktrees were not changed.
- After pulling main on another computer, read versioned recovery README and restore into a NEW directory; do not resolve old machine-local absolute paths directly. Fresh manager must establish local coordination authority/resource paths before resumed implementation.


## Local recovery ownership — revision 11 (2026-11-04)

- User explicitly requested updating local coordination paths and recreating isolated databases.
  This authorizes recovery/environment preparation only. Application implementation, tests,
  reviews, commits, merges and downstream phases remain paused.
- Authority transferred from stopped historical manager `thr_gwccjxwzjt` on the unavailable
  `/Users/mac03/dev/openbot` host to current Codex `/root` in `/Users/mac02/dev/openbot-recovery`.
  Root settings are Sol 6.1/xhigh, confirmed by the user in this session. No workers are running.
- Historical paths, decisions, review evidence and source hashes remain unchanged. For current
  work map `/Users/mac03/dev/openbot` to `/Users/mac02/dev/openbot-recovery`. Phase authorities remain
  `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03/plan/fix-app/phases/p03/docs/` and
  `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p04/plan/fix-app/phases/p04/docs/`.
- Primary HEAD is `52f257a61baec3c1a3253f4e57a2b5eb96bea344`; both phase HEADs remain
  `48626a1240f032ca3b99372bd3f5a05eb08c0c66`. Uncommitted source work is preserved.
- Preparation status: COMPLETE (revision 12). All three clusters are running, six databases are migrated,
  and each migration checksum/cluster identity matches its owning checkout. No application tests ran.

### fix-app-I002 / r2 — Local isolation and resources

- Supersedes I002/r1 resource paths and role/DB URLs on this host; preserves the isolation contract.
- Phase worktrees: `/Users/mac02/dev/openbot-recovery/.worktrees/fix-app-p03` and `fix-app-p04`, on their original branches.
- Manager-owned disposable PostgreSQL 18 clusters, bound only to `127.0.0.1` with local trust auth:
  p03 `/Users/mac02/dev/openbot-recovery/.worktrees/pg-p03` port `55403`; p04 `/Users/mac02/dev/openbot-recovery/.worktrees/pg-p04` port `55404`;
  integration `/Users/mac02/dev/openbot-recovery/.worktrees/pg-main` port `55400`. Do not stop or remove sibling resources.
- Phase URLs use role `mac02`, databases `mail_agents` and `mail_agents_test` on each phase port.
  Primary integration URLs use `open_bots` and `open_bots_test` on port `55400`.
- Each worktree has its own ignored, mode-0600 `.env` containing only its database URLs.
  Tests require explicit exported DATABASE_URL/TEST_DATABASE_URL; cargo test does not load .env.
- PostgreSQL tools: `/opt/homebrew/opt/postgresql@18/bin`; `max_locks_per_transaction=256`.
  Each database must use its owning checkout's migrations, never another phase's schema.
- Keep `CARGO_INCREMENTAL=0`, separate target directories, and all existing no-cross-cleanup rules.
- I004/r1 targeted-test-only scope and I005/r1 90-day retention decision remain unchanged.

### fix-app-C009 / r1 — Restored local coordination authority

- State: active
- Origin: current user recovery-setup request; local ownership transfer above.
- Targets: p03, p04, integration, and all future phases on this host.
- Instruction/change: use `/Users/mac02/dev/openbot-recovery/plan/fix-app/COORDINATION.md` as the single authority. Historical absolute paths
  map to `/Users/mac02/dev/openbot-recovery`; historical worker handles are stopped and must not be reused.
- Dependencies/order: local resource migration verification must complete before affected work;
  application implementation still requires explicit user resume.
- Evidence/review: recovery manifest verified 2937 source files; original phase acceptance unchanged.
- Verification: local TASK/HANDOFF acknowledgement; own-cluster connection and migration checksum checks.
- Responses: p03 -> acknowledged by local recovery setup owner, see phase TASK/HANDOFF local setup;
  p04 -> acknowledged by local recovery setup owner, see phase TASK/HANDOFF local setup;
  integration -> satisfied for environment setup; future workers/phases -> acknowledge when assigned.
  These setup acknowledgements do not resume or accept either implementation batch.


## Local environment verification — revision 12 (2026-11-04)

- Completed the explicit recovery/environment request; implementation remains paused.
- PostgreSQL 18.6 clusters p03:55403, p04:55404 and integration:55400 are running under mac02.
  Each binds only 127.0.0.1, uses its own socket/data directory and max_locks_per_transaction=256.
- Six databases are fresh and migrated. Each worktree ran its existing
  `bash scripts/reset-db.sh --all --yes`, followed by
  `cargo sqlx migrate run --database-url <own TEST_DATABASE_URL>`.
  `cargo sqlx migrate info` passed for each development/test database. Ledger success and SHA384
  checksums match the owning worktree's exact migration bytes; SQL confirmed role, port, data
  directory, listener configuration and public tables. No application tests or code checks ran.
- All 2937 restored source files still match the recovery manifest; tracked src/migrations/Cargo
  diff digests are unchanged. Historical review digests and logs are retained, with no new acceptance.
- Evidence: `/Users/mac02/dev/openbot-recovery/plan/fix-app/integration/resumed-all/docs/logs/local-setup-20261104/resources.json`, per-resource `*-database-setup.log`,
  `source-before.json`, `source-after.json`, and immutable `COORDINATION-r12-local-ready.md`.
- Local .env files contain only DATABASE_URL and TEST_DATABASE_URL and are mode 0600, ignored by Git.
  No credentials were copied from the original or another checkout.
- Restart an owned stopped cluster using
  `/opt/homebrew/opt/postgresql@18/bin/pg_ctl -D <owned cluster dir> -l <owned cluster dir>/server.log -w start`.
  Port/listener/socket/lock settings are persisted in its own postgresql.conf.
- Next action on explicit implementation resume: fresh workers read this authority and the phase
  TASK/HANDOFF local setup entries, acknowledge I002/r2 and C009/r1, then rerun affected checks on
  the new fixtures. p03's 31 unresolved historical failures and p04's unfinished review/WB2 remain.


## Explicit implementation resume — revision 13 (2026-11-04)

- Current user explicitly requested continuing implementation with implement-astra-reviewed-2-2.
  This supersedes the historical user pause for the remaining fix-app implementation and
  integration; the setup-only pause text above remains historical.
- Current root /root is Author A (Sol 6.1/xhigh confirmed in this conversation), working
  sequentially from restored p03, then respecting the existing dependency order. Fresh independent
  Reviewer B is Astra/high, direct child without inherited context, assigned when first needed.
  No separate author is needed under the selected workflow's confirmed-root rule.
- Current batch: p03 B1 at its original baseline. Preserve all restored source/evidence and
  complete unresolved affected failures, focused verification, independent review and acceptance
  before commit/integration. p04 remains queued with its preserved work.
- I001/r1, I002/r2, I003/r1, I004/r1 and I005/r1 remain active; p03 acknowledges C001/C002/C005/
  C007/C009 r1, with C006 proposed output only. Root records the same in p03 TASK/HANDOFF.
- Prior local accepted-commit/merge authorization retained; no push/deployment/production reset.
  Targeted tests only, no full suite/check.sh. p06/p08 require p03 integration; p07 requires p04.
- Local PostgreSQL fixtures and authoritative paths remain those verified at revision 12.
  Context/auto-compaction threshold remains unavailable; durable checkpoints are required.


## p03 acceptance and commit — revision 14 (2026-11-04)

- Root accepted p03 B1 after independent FIXAPP-P03-B1-R2-MAC02 PASS for
  ba772625d27ac6dfa497e3798067e0665d09ebfff4081d9b0dd02ef724cf1440, sole phase-doc exclusion.
  Medium finding F1-MAC02 closed by B; G1 continuity R5 retained.
- Source/evidence committed locally on fix-app/p03 at `c42ae0432e3fc20eb8a03944d6c295797750ef53`; no push.
  Content fingerprint `5454182be91ee687b3fcabcd8d472536c669f98624edbae0ccb03db0346ccf28` across1465 files matches the accepted working source
  exactly after routine staging/commit; clean worktree. Raw evidence logs are committed intact,
  with phase-doc-only whitespace classification for captured console data.
- Final checks: affected1104 passed/0failed/1ignored, all filters nonzero; SQLx/all-target offline
  compile, clippy-Dwarnings, fmt/whitespace PASS. Full suite remains excluded by I004.
- Status: ACCEPTED IN PHASE, not integrated. C006 remains proposed until producer integration.
  Current primary52f257a overlaps five p03 source paths, recorded read-only in integration logs.
- Next authorized work: finish preserved p04 at its old baseline, then assemble accepted wave2
  artifacts against current main under an independent combined integration gate. This avoids
  repeating shared migration/contract integration while preserving dependency requirements.
  p06/p08 still await p03integration, p07 awaits p04integration.
- This revision changes execution status/ordering only; active technical instruction/notice
  revisions are unchanged. No new resource/reset/deployment/push authorization.


## p04 sequential resume — revision15 (2026-11-04)

- Root/A confirmed Sol6.1/xhigh now resumes preserved p04 in its existing worktree/base.
  Fresh independent Astra/high reviewer will validate unchanged G1r6 continuity and review
  WB1, then WB2. No author subagent; preserve all existing source/review history.
- Original phase batching retained: WB1 plan batches1-4 (authorization/scoped reads/caller/attention),
  accept before WB2 plan batches5-6 (owned onboarding transaction/SMTP trust).
- I001/I003/I004r1,I002r2 acknowledged; C002/C003/C007/C009r1 acknowledged. I005 future-only
  not applicable to p04; C008 proposed output remains blocked on accepted/integrated p04.
- Resource ownership unchanged: ownPG55404 mail_agents/mail_agents_test, own target;
  CARGO_INCREMENTAL=0. p03 acceptedc42ae04 remains clean/notintegrated; no sibling edits.
- Technical notice/decision requirements unchanged; this is execution status/assignment only.


## p04 WB1 accepted, WB2 starts — revision16 (2026-11-04)

- Root accepted WB1 after independent FIXAPP-P04-WB1-R2-MAC02 PASS atc5debdeab77f80515c4a88ae2384af6503ae320e53cb48b4fc3dc3c87a2719da,
  phase start/reviewbase/HEAD48626a1, solephase-doc exclusion. F1/F2closed;472/0/4ignored, all
  required affected metadata/offlinecompile/clippy/fmt/whitespace checksPASS. No p04commit yet.
- Root/A now implements WB2 plan5-6: purpose-specific owned onboarding session/provider-only
  preparation, atomiccompletion/refusalsavepoints/idempotency and SMTP applicationtrust.
  Normative G1r6 unchanged, freshcontinuityG1-R7PASS; apply F11/F21 safeguards and required
  real-path competing custom provider/one-connection tests.
- Same roles/resources/technicalnotice revisions asr15. I004targeted-only; p03 acceptedc42ae04
  remainsclean/notintegrated. Combined reviewedwave2integration follows p04wholephase acceptance.


## Phase04 acceptance / wave2 assembly — revision17 (2026-11-04)

- Root accepted p04 wholephase after independent FIXAPP-P04-WB2-R1-MAC02 PASS for9b1bfc76eb32f15cf6131b1ab55bebe375d49b2beb90b03d48a7dc71dcc29e91. WB1R2 findingsclosed, no newWB2 findings. Wholephase localcommitced9cd84be96960c6a9a45cf573bfee787499570 onfix-app/p04, cleanworktree; acceptedsourcefingerprintf9db48296bd464fc1e8c9fc3904badd39113eff29720f8df8506a881860b74ae across1477files unchanged aftercommit. Raw immutableevidence preserved with phase-doc-only whitespace attributes.
- p04 affected union523passed/0failed/4ignored/all40filters nonzero, timeoutcorrection16/16, alltargetofflineclippy/fmt/whitespace PASS. I004 still excludesfullsuite. p04independent review accepted unchangedcoordr16; this execution-status update changes no applicabletechnical instruction.
- Freshfetch origin/main stayed52f257a. Acceptedp03c42ae04 assembled into localmainc242a742b6a5566098f83a39e257fb2891a57470; newer-main retry exclusions/RepeatedToolCall preserved. Primary privateDB55400 open_bots/open_bots_test reset+migrated successfully to assembledschema; sibling resources untouched.
- Acceptedp04ced9cd8 merge is being assembled on primary. Explicitconflicts in skill_library route/usecase retain newer boundedtext skill model plus p04Caller authority; provisioning retains newer steps with p04 shared transaction command. Adaptedtwo p04 tests to currenttext skill shape without weakening assertions. Firstcombinedofflinealltarget check active.
- None of these source assemblies is accepted combinedintegration yet. C006/C008 outputs remain proposed; p06/p07/p08 await combinedaffectedverification and independentintegration review. No push/deployment/productionreset.


## Wave2 combined review freeze — revision18 (2026-11-04)

- Currentlocalmain6ffe52a9fbdaffcfbfa7a5b4ddc5778945647426 containsacceptedp03 andp04, conflictresolutionsretain newertextskillmodel/steps and p04authority/atomicity. Producerphase branches clean/unmodified. Reviewbase52f257a, freshorigin/mainunchanged.
- CombinedtargetedgatePASS1600passed/0failed/5ignored,1605selected/all52filters nonzero. SQLxprepare/livealltargetcompile/no metadatachanges, finalofflinealltargetclippy-Dwarnings, fmt/whitespace, exactprivateDBresource/migrationchecksum allPASS. Four `- should panic` finalstatuslines explainparser1601; read-onlyinventory independentlyconfirms1605selected.
- Rootfreezescombinedsource forindependentintegrationB under managerrecordrootplan/fix-app/integration/resumed-all/docs; exactsnapshotexclusions thatworkflowrecorddir plusplan/fix-app/COORDINATION.md. No source/deliverableexclusion. Technicalnoticerevisionsunchanged; C006/C008 stillproposed untilcombinedacceptance.
- Downstream06/07/08 remainblockedoncombinedacceptance. Fullsuite/check.sh/stack-budget remainexcludedI004; no push/deploy/productionreset.


## Accepted wave2 / downstream start — revision19 (2026-11-04)

- Independent FIXAPP-WAVE2-INT-R1-MAC02 PASS408e2bedb3d1d4e15f2b1fa8c5211c9728cc99eb0b587e2c02edcb00d9c32004 for combinedmain6ffe52a, no newfindings. Root recomputedidenticalartifact andunchangedr18pin. Bothproducerphasesare now accepted AND integrated. C007 satisfied; C006/C008 publishedactive withacceptedproduceridentities.
- Combinedverification1600passed/0failed/5ignored,1605selected/all52nonzerofilters, SQLx/livealltargets andofflineclippy/fmt/whitespace/migrationidentityPASS; no fullsuite/check.sh/stackbudget/externalproviderclaims. No push/deploy.
- Remainingauthorizedscope06/07/08 continuessequentially undercurrentroot/A Sol6.1/xhigh (userconfirmed), independentAstra/high B perphase, no separateauthor. Startp06 fromacceptedmain withfreshisolatedresources, oneconsequentialarchitecturegate forwholephase design aftertracingactualstorage/reportingcontracts. I00590days+preserveusage/cost isdecided; do notre-ask.

### fix-app-I002 / r3 — Downstream local resources

- Extends/supersedesr2 only fornewp06/p07/p08 resources; retainedp03/p04/primarypathsandDBs unchanged.
- Worktrees .worktrees/fix-app-p06,p07,p08 onfix-app/pNN branches fromtheiracceptedprerequisitemain. EachhasownPG18cluster .worktrees/pg-pNN on127.0.0.1:554NN, rolemac02, open_bots/open_bots_test DBs (newmain naming). Createonlyphasebeingstarted.
- PrivatePGclusterconfiguration: loopbackonly, trustauthdisposable, ownsocketdir, max_locks_per_transaction256; managerownsresources. Do not stop/delete siblings or sharetargets/DBs. Ownignoredmode0600.env contains onlyURLs; explicitlyexportforcargo. CARGO_INCREMENTAL0, no sharedCARGO_TARGET_DIR.
- Context/compactionthresholdunavailable; keepdurablephasecheckpoints. Sourcegraftbeforediscovery/edits, refreshafterlargechanges. I004targeted-only, I003acceptedlocalcommits/merges/noCOORDINATIONcommit/no push remainunchanged.


### fix-app-I005 / r2 — Application-wide retention scope (2026-11-04)

- State: active; supersedes I005/r1 bymakingglobalmaintenance scopeexplicit, retainingallpriorretention/reporting requirements. Targets p06,p08,integration.
- User asked whetherthewindow isforthewholeapplication andremovalcanrunacrosscompanies. Currentclarification: the90dayterminalworkflowhistory retentionwindow appliesapplication-wide; themaintenanceworker maydeleteeligibleterminalrun treesacrossallcompanies.
- Eligibilitystillrequiresno livechild, open decision or unsettledreconciliation reference; preserveusage/costreporting. P06ownstheconcrete schema/maintenance/reportingdesignandindependentarchitecturegate. Include crosscompanysweep coverage andboundedforwardprogress inverification.
- This scopes systemmaintenance ofworkflowhistory; no productionreset/deployment/pushauthorization added. Isolateddisposablefixtures onlyforverification. Responses p06 -> acknowledgeatstartup; p08 -> followacceptedp06design; integration -> acknowledged.

- 2026-11-04: revision21 — published I006/r1 from the user's explicit independent AUTH_ATTEMPT_SECRET choice for p07; p06 G1r3 is accepted and whole-code corrections continue. No p06 contract changed; no phase source accepted or pushed by this notice.
