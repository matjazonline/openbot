# fix-app p05 — Reviewer (B) notes

## FIXAPP-P05-G1-R1 — Architecture gate, decisions D1-D5 revision r1

### Identity

- Date 2026-10-31. Workflow `implement-opus-reviewed-2_2`. Reviewer (B): Opus 5.5 / xhigh.
- Records: task `TASK.md`, review `REVIEW.md`, findings `agent-review-findings.md` (all in
  `plan/fix-app/phases/p05/docs/`).
- Batch: plan 05 batches 1-5 (one implementation batch); stage: **architecture gate**.
- Phase p05, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p05`, branch `fix-app/p05`.
- Phase start `e6b0a51`, review base `e6b0a51`, HEAD `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb`,
  digest `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66`, exclusions
  `plan/fix-app/phases/p05/docs` — matched at start and rechecked before returning.
- Gated decisions: D1 (incl. D1.8 member-removal protocol), D2, D3, D4, D5 — revision **r1**
  (`TASK.md` "Architecture gate — decisions (revision r1)"; `logs/gate-r1-TASK-snapshot.md` was
  byte-identical to `TASK.md` at review start).
- Coordination: `/Users/mac03/dev/openbot/plan/fix-app/COORDINATION.md` revision 1 at start;
  applicable notices fix-app-I001 r1, I002 r1, I003 r1; `logs/coordination-r1.md` byte-identical at
  start. No cross-phase notices exist. Revision 2 (adds fix-app-I004 r1) appeared before return —
  see the freshness recheck at the end of this entry.

### Coverage and evidence

Requirements read from the originals: `plan/fix-app/05-tenant-integrity.md`,
`plan/fix-app/README.md` (concurrency rules, known overlaps, verification), `AGENTS.md`,
`src/AGENTS.md` (skimmed for applicability), `src/adapters/persistence/AGENTS.md` (database
boundary, lease, queue, test rules), `src/application/AGENTS.md` (not exercised by a decision
beyond the `BounceInfo`/composer contracts).

Checked against the actual schema and code (inspection unless marked *tested*):

- **D1 producers.** All `compose_standalone` / `insert_standalone_delivery_on` /
  `enqueue_standalone_delivery` callers enumerated (decisions `insert_reviewers`, notification
  `apply_projection_on`, routing `send_system_reply`, thread `handle_bounce_dispatch`; test
  `InMemoryIngress`). Bounce company claim verified: `UndeliverableSlugs::unknown/disabled` are only
  recorded after `matched_company = Some` (`routing.rs:141-215`); thread-injection bounce uses
  `candidate.company`. Only two `INSERT INTO message_deliveries` exist (`delivery/enqueue.rs`).
- **D1 FK/cascade behaviour.** Cascade depths computed from `pg_constraint` on a scratch copy of
  the init schema; NO ACTION semantics *tested* (`logs/g1-r1-toy-noaction-depth.log`) and
  reproduced on the real schema — D1.3 reviewer FK breaks company deletion → **F1**
  (`logs/g1-r1-company-delete-reviewer-notice.*`). `notifications_email_delivery_id_fkey` SET NULL
  and parts CASCADE are depth-safe by analysis.
- **D1 readers.** Tenant delivery view inner-joins message/binding (`delivery/read.rs:42-59`),
  attention filters `channel_id = ANY` (`attention.rs:260-276`, `:491`), dashboard counts include
  notices (author noted). `DeliveryRecord::company_id()` drives the email sending account
  (`egress.rs:518` → `resend_api/accounts.rs:112-122`) → **F2**.
- **D1.8 protocol.** Lock analysis against `recipient_for_source` (`notification.rs:649-690`,
  `FOR SHARE OF principal, account, member, company, channel`), `workflow_decision_person_snapshot`
  (`FOR SHARE` on principal), `insert_reviewers` order (delivery then reviewer), today's
  `remove_member` (`company_invite.rs:409-440`), `cancel_locked_deliveries_on`
  (`delivery/cancellation.rs:152-215`). Race closure holds for both producers, but the order
  member→principal inverts the projection's principal→member → **F5**. Not run live (no
  implementation); inspection only.
- **D2.** Run FK depth-safe (runs depth 1, revisions depth 2). Child runs carry the parent's binding
  id and revision (`children.rs:145-185`); tool-call children read the saved binding
  (`workflow_tool_calls.rs:85-139`); no production deletes of bindings/revisions;
  `ChannelPersistence::delete` refuses channels with threads/bindings/schedules/runs
  (`channel.rs:395-437`). Intent-binding FK CASCADE and its existing key verified.
- **D3.** `channel_bindings_company_transport_key` referenced only by the delivery destination FK;
  composer takes source and destination from the request channel's own binding
  (`transport/compose.rs:131-243`); external-thread callers have the channel; workflow-binding
  association is written only on insert (`workflow/bindings.rs:147-153`). The `channel_schedules`
  two-FK design *tested* on PG 18.6 (`logs/g1-r1-ddl-validity.log`): valid DDL, accepts company and
  same-channel bindings, rejects cross-channel and mis-claimed kinds, cascades. Sound.
- **D4.** Per-kind claims verified (`message_runtime.rs:157-505`, `workflow/message.rs:289-440`):
  channel destination id = destination channel and the thread opens there; user destination id =
  user id, thread in the run channel; reply destination id = source message whose association is
  the reply thread. Generated `destination_id` *tested*: valid with `STORED NOT NULL`, rejects
  explicit writes, unique key on it works, BEFORE INSERT triggers see NULL (no trigger on the table
  reads it: `workflow_message_destination_in_app_guard`, `workflow_action_fact_immutable`
  checked). Company deletion *tested*: the thread_messages FK is safe, the channel FK is not →
  **F3**; today's schema already fails for destinations → **F4**
  (`logs/g1-r1-company-delete-destinations.*`, `logs/g1-r1-company-delete-user-destination.*`).
  Internal-notes FK *tested* (`logs/g1-r1-delete-internal-note.*`): passes, but by trigger order →
  **F7** (optional).
- **D5.** `inbound_events.company_id` is NOT NULL; no downstream global uniqueness on inbound mail
  (`email_message_metadata`, `messages`, `external_messages` keys checked); notification composite
  event FK consistent with `ON CONFLICT … DO UPDATE SET event_id`; member FK depth-safe;
  `VALID_HINT` claim verified (`fairness.rs:70-90`) but `clean_cursors` sweep becomes dead → **F8**
  (optional); reply-to NO ACTION safe for company deletion (intents and messages both round 1).
- **Scope check.** Every plan batch item is covered by a decision. Out-of-scope same-pattern table
  noted (`schedule_runs`) → **F9** (optional). Hygiene/failure-mode notes → **F10** (optional).

Checks actually run (all fixture-isolated, phase cluster 127.0.0.1:55405):

- `createdb p05_b_gate_scratch` + `psql -f migrations/20260817000000_init_schema.sql` (exit 0,
  `logs/g1-r1-scratch-schema-apply.log` empty); all reproductions run in rolled-back transactions.
- `SQLX_OFFLINE=true cargo test --locked --offline --lib --no-run` in the worktree `target/`
  (exit 0, 2m39s, `logs/g1-r1-build-test-binary.log`) — used to try seeding a real decision
  fixture; the decision fixture uses `own_database` and drops it, so seeding switched to SQL
  fixtures. One existing test was run against a scratch database
  (`workflow_decision_reviewer_notice_provenance_rejects_foreign_delivery`, 1 passed) — not
  evidence for any decision.
- Scratch databases `p05_b_gate_scratch` and `p05_b_gate_rust` dropped afterwards; `mail_agents*`
  untouched.

Gaps / lightly reviewed (do not read this pass as uniform):

- No focused suite was run (nothing is implemented); D1.8 race and deadlock reasoning is by
  inspection, not a live competing-transaction run.
- D5 composite notification-event FK, D2/D3 fixture rewrites and inbound-event worker behaviour for
  two companies were reviewed lightly (constraint and key inspection only).
- Rust readers of `destination_id` were not enumerated individually (the generated column keeps
  reads unchanged by construction); only SQL functions, keys and triggers were checked.
- Merge hazards with p01/p02 assessed from their plan text only, not their worktrees.

### Outcome

**Changes required** for decision revision r1.

- Actionable: FIXAPP-P05-G1-F1 (high), FIXAPP-P05-G1-F2 (high), FIXAPP-P05-G1-F3 (high),
  FIXAPP-P05-G1-F4 (medium, disposition required), FIXAPP-P05-G1-F5 (medium),
  FIXAPP-P05-G1-F6 (medium), FIXAPP-P05-G1-F11 (low, coordination revision 2 / I004 r1).
- Optional: FIXAPP-P05-G1-F7, F8, F9, F10 (low).
- Sound as written: D1.1/D1.2/D1.4/D1.5/D1.7, D2, D3 (including the `channel_schedules` design),
  D4's generated `destination_id`, per-kind columns, thread_messages FKs and reply FK, D5 except the
  optional notes.
- A recheck should cover the revised D1.3, D1.6, D1.8, D4 FK choices, the F4 disposition and the
  expanded verification plan; unaffected D2/D3/D5 evidence can be reused.

Merge hazards and cross-phase notice candidates (for root/manager, not p05 defects):

- p01 edits the same `remove_member` area (principal demotion), principal/message FKs and adds a
  deferrable `companies → company_members` FK; whichever merges second must rerun both
  member-removal and company-deletion tests. p01's plan text also relies on "`NO ACTION` is checked
  at the end of the statement" — true only per cascade round (F1 background); worth a plan-wide
  notice: "A non-deferrable NO ACTION FK fails a parent delete when the referencing row is removed by
  a deeper cascade path; prove each with a populated whole-company delete or use DEFERRABLE
  INITIALLY DEFERRED."
- p02 edits `delivery/queue.rs` (lease fence, renewal) while D1.6 reshapes `DeliveryRecord`,
  `DeliveryDb`, `PartDb` and the claim mapping in the same module; expect textual conflicts in
  `delivery/queue.rs`/`read.rs` and rerun both plans' `delivery` and `notification` suites.

### Process and instruction feedback

- The gate was warranted (schema, cascade, locking and contract decisions) and the batch was
  reviewable in one pass; TASK spans were accurate and the decisions were explicit enough to test.
- Defects originate in the gated decisions, not implementation. Two recurring blind spots:
  cascade-order semantics of new NO ACTION FKs (F1, F3, F4, also in plan 01/05 text) and not
  tracing every reader of a field whose meaning changes (F2). Candidate instructions are recorded
  with F1, F2, F4 and F5 in the findings log; none adopted here.
- Useful: the README's "known overlaps" list pointed straight at the delivery/member areas where
  the defects were.

### Freshness recheck before returning

- Digest recomputed: unchanged (`9d5bde92…4c66`, HEAD `e6b0a51`); `TASK.md` still identical to
  `logs/gate-r1-TASK-snapshot.md`.
- `COORDINATION.md` had moved to **revision 2** during the review (copy
  `logs/coordination-r2-seen-by-B-G1-R1.md`). The only change is new plan-wide notice
  **fix-app-I004 r1** (verification scope: no full suite / `scripts/check.sh`; targeted tests for
  affected behaviour with nonzero counts). It applies to p05. It does not change any gated D1-D5
  decision or finding above, but `TASK.md`'s acceptance line and "Verification plan" still name the
  superseded `scripts/check.sh` final gate → **FIXAPP-P05-G1-F11** (low). The outcome (changes
  required) is unchanged; a recheck must pin revision 2.

## FIXAPP-P05-G1-R2 — Architecture gate recheck, decision revision r2

### Identity

- Date 2026-10-31. Workflow `implement-opus-reviewed-2_2`. Reviewer (B): Opus 5.5 / xhigh.
- Records: `TASK.md` (sections "Architecture gate — amendments (revision r2, answering G1-R1)" and
  "Verification plan (r2, per fix-app-I004 r1)"), `REVIEW.md`, `agent-review-findings.md`.
- Batch: plan 05 batches 1-5; stage: **architecture gate recheck** (by finding ID).
- Phase p05, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p05`, branch `fix-app/p05`;
  review base `e6b0a51`, HEAD `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb`, digest
  `9d5bde9263086a6007787848a407759f6e8a3ca7b391addfaef755df2fee4c66` (exclusions
  `plan/fix-app/phases/p05/docs`) — code artifact unchanged since G1-R1 at start; it drifted
  during the round because the author began implementing (see the freshness recheck below).
- Gated decisions: D1-D5 **revision r2** = r1 text plus the r2 amendments; immutable copy
  `logs/gate-r2-TASK-snapshot.md`, byte-identical to `TASK.md` at start and at return. Diff r1→r2
  (`logs/gate-r1-TASK-snapshot.md` vs `TASK.md`) contains only the coordination/acceptance updates,
  the reviewer agent ID, the amendments section, the r2 verification plan and status/notices.
- Coordination: `COORDINATION.md` **revision 2** (notices I001-I004 r1); author's copy
  `logs/coordination-r2.md` identical to the live file and to B's copy
  `logs/coordination-r2-seen-by-B-G1-R1.md`. I004 r1 acknowledged in `TASK.md`. Manager decision on
  F4 (option (a), scope extension, no notice) recorded in the amendments; I take it as reported by
  the author — it is not in `COORDINATION.md`.

### Coverage and evidence (by finding)

- **F1** — reviewer→notice FK `DEFERRABLE INITIALLY DEFERRED`: matches G1-R1 case C. Re-verified in
  the combined fixture (*tested*, `logs/g1-r2-company-delete-combined.*` case A) and a control that
  the deferred FK still rejects a foreign-company notice at commit (case C, guard trigger disabled to
  isolate the FK). Closed.
- **F2** — `sending_company() -> Option<Uuid>` from attribution only; egress keeps
  `transport_for(sending_company())`; tests named (accessor + egress seam). Matches the only reader
  (`egress.rs:518`). Fair share by tenancy is a stated, acceptable change. Closed at decision level;
  the batch review must see the egress test.
- **F3** — `destination_channel_id → channels` deferred: *tested* in combined case A. Closed.
- **F4** — manager-approved scope extension: `workflow_message_destination_delivery` and
  `_principal` deferred. *Tested* with all three destination kinds (channel; user with email
  delivery and principal; reply with email delivery and replied message) plus D4's immediate
  thread_messages and reply FKs, the deferred D4 channel FK, the reviewer notice and an internal
  note: `DELETE FROM companies` succeeds at emulated commit and leaves 0 deliveries, destinations,
  reviewers, notes (case A). Control B: deleting a replied-to message is still refused (immediate
  reply FK). Closed. Residual: other pre-existing blockers on populated companies (outside these
  tables) can only surface in the F6 tests at implementation.
- **F5** — new order principal (demote) → member `FOR UPDATE` → notices → member delete. Checked by
  inspection against every locker of those rows: projection commit (`notification_events` row →
  failed channel delivery `FOR SHARE` → principal → users → member → company → channel), decision
  park (inserts the notice row, then principal `FOR SHARE` in the guard; its uncommitted
  notice/reviewer rows are invisible to removal's step 3, so removal never waits on them), delivery
  worker (locks only delivery rows). No inversion remains. The amendment's sentence "no producer
  locks a delivery and then a principal/member" is loose for the decision path (it inserts a delivery
  row first) but harmless for the reason above. Competing-claimant tests cover both orders and an
  in-flight notice. Closed (inspection; the live race test belongs to the batch).
- **F6** — populated whole-company and raw-SQL channel delete tests listed, built from module
  fixtures; app-level refusal kept. Closed at plan level.
- **F7** — internal-notes FK deferred: *tested* in combined case A. Closed.
- **F8** — `clean_cursors` deleted-company statement dropped. Closed.
- **F9** — recorded as a follow-up candidate in `TASK.md`. Closed (deferred by scope, optional).
- **F10** — (a) left as is (optional, accepted); (b) failure mode named. Closed.
- **F11** — I004 r1 acknowledged; acceptance and verification plan use targeted tests, fmt, build,
  `.sqlx`; no full suite / `check.sh`. Closed.
- Reused unchanged G1-R1 evidence for D2, D3, D5 and the parts of D1/D4 not amended.

Checks run this round (I004 scope — fixture-isolated, targeted to the amended decisions):
`createdb p05_b_gate_r2_scratch` + init schema apply (exit 0, empty log
`logs/g1-r2-scratch-schema-apply.log`); `psql -f logs/g1-r2-company-delete-combined.sql` (output
`logs/g1-r2-company-delete-combined.log`); scratch database dropped. No Rust tests (nothing
implemented). A transient full-disk condition (`ENOSPC` on the temp volume) blocked shell commands
briefly mid-review; it cleared (20 GiB free) before any check ran, and no check was skipped because
of it.

### Outcome

**PASS for decision revision r2** (digest `9d5bde92…4c66`, coordination revision 2). All
actionable G1 findings (F1-F6, F11) are resolved at decision level; optional F7-F10 dispositions are
accepted. New optional note FIXAPP-P05-G1-F12 (low): deferred FKs move violations to `COMMIT`.

Carry to the batch review: the F2 egress test, the F5 competing-claimant tests run live, the F6
populated deletion tests (watch for other pre-existing blockers), and F12.

### Process and instruction feedback

- One correction round resolved every finding with no new evidence needed beyond one combined
  fixture; reusing D2/D3/D5 evidence was sound because those decisions did not change.
- The manager's F4 approval reached B only through the author's message and `TASK.md`; a one-line
  manager-log entry in `COORDINATION.md` would make such scope extensions verifiable. Candidate
  (scope: concurrent-phases workflow reference): "Record manager-approved scope extensions in the
  coordination manager log, not only in the phase TASK."

### Freshness recheck before returning (G1-R2)

- `TASK.md` still byte-identical to `logs/gate-r2-TASK-snapshot.md`: the PASS applies to decision
  revision r2 as recorded.
- **Code digest drifted during this round**: `d318e4ba02cede498d018bb0fb88abeb1c45200a08563cdb38c14db559a9d10b`
  (HEAD still `e6b0a51`), with unstaged edits to `migrations/20260817000000_init_schema.sql`,
  `webhooks/resend_api.rs`, `persistence/inbound_event/mod.rs`, `persistence/thread/{external,
  inbound,message,tests}.rs`, `workflow/{fair_polling,schedules}.rs` — i.e. the author began
  implementing (D3/D5/F8 areas) before this pass was returned. Those edits were not reviewed here;
  they belong to the batch review. Process note: the workflow asks for B's gate pass before
  dependent edits; the touched areas were unchanged since r1 or only gained F8, so no decision was
  affected, but the batch review should start from a fresh digest.
- `COORDINATION.md` is still labelled revision 2 but was edited in place: I002 r1 gained "Disk is
  tight: prefix cargo with `CARGO_INCREMENTAL=0`; delete only your own `target/debug/incremental`
  when needed." (copy `logs/coordination-r2-amended-seen-by-B-G1-R2.md`). Operational only; it does
  not change any requirement under review. Candidate: in-place notice edits should bump the notice
  or file revision so pinned review inputs stay identifiable. B's G1-R1 `cargo test --no-run` build
  lives in the shared phase `target/` (3.0 GiB total now); left in place because the author uses
  the same directory.

## FIXAPP-P05-BR1 — Batch review, implementation batch 1 (plan batches 1-5)

### Identity

- Date 2026-11-01. Workflow `implement-opus-reviewed-2_2`. Reviewer (B): Opus 5.5 / xhigh.
- Records: `TASK.md` (gate r2 + "Implementation amendments (r3 ...)" + Check log), `REVIEW.md`,
  `agent-review-findings.md`. TASK copy as reviewed: `logs/br1-TASK-seen-by-B.md`
  (sha256 `9aecebb14fa1d5d19b7d60b75ae4fcc02507bc65d4751b635e744e0eb6dcb505`).
- Batch: implementation batch 1 = plan 05 batches 1-5; stage **batch review**.
- Phase p05, worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p05`, branch `fix-app/p05`;
  base = HEAD = `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb` (all changes uncommitted); digest
  `2a3cb838df2f8983bb82f2adf75cb7c044d0d121fa6d1fb02309702a8c4b09c4` (exclusions
  `plan/fix-app/phases/p05/docs`), confirmed at start and before returning.
- Coordination: `COORDINATION.md` still labelled **revision 2** (I001-I004 r1, all acknowledged in
  TASK) but edited in place again since G1-R2: p02 row "accepted in phase, committed 7ca2fb5", and a
  manager-log line (user: no further phases after wave 1; merges held). Copy
  `logs/coordination-r2-seen-by-B-BR1.md`. No requirement change for p05.
- Additional input (author message, 2026-11-01): p01 committed `4d940da` on `fix-app/p01`; asked to
  judge runtime conflicts after merge. Read with `git show` only; p01 branch not checked out.

### Coverage

Read the whole diff (48 tracked files + 2 untracked test files + 1 `.sqlx` entry): schema hunks
against D1-D5 r2 and r3; `remove_member` + `cancel_member_notices_on` against D1.8 r2 (F5 order);
`insert_standalone_delivery_on` absorb filter (D1.5); `DeliveryRecord`/`DeliveryAttribution`/
`sending_company` and every reader (egress, fair_order, read entry, tests); `BounceInfo` and the
three bounce/help producers; decision notice `company` plumbing in three park paths; inbound-event
tenant key; `clean_cursors`; external-thread mapping (both callers); destination target columns
and `PlannedDestination.channel_id` for all four planners (reply thread channel = sender, re-checked
by `RouteCheck::Source`); schedule `binding_channel_id` writer (binding association is immutable,
`bindings.rs:88`, so the new keys cannot strand a binding update); every changed and new test.

Evidence checked by inspection (not re-run): every notice producer passes its company
(`grep compose_standalone/insert_standalone_delivery_on`); every notice source key is tenant-unique
(help reply embeds `company.id`, bounce hashes a `BounceInfo` that now contains `company_id`,
notification/decision keys embed UUIDs), so the company-filtered absorb can never turn a legitimate
replay into an Internal error; the member's notice set (notification emails, reviewer notices) is
standalone only, so removal never locks a channel delivery the projection holds `FOR SHARE`; no app
code deletes threads, messages, bindings or users (only tests), and the only FK-violation mapper
(`ChannelPersistence::delete`) is preceded by its own refusal, so the deferred keys (F12) surface
nowhere in app paths; no `.sqlx` entry touches a changed table (one new entry is complete).

### Checks run by B (I004 scope)

- `cargo test --lib -- notification_delivery_tenant_ run_binding_fk_ channel_path_fk_
  message_thread_fk_ tenant_fk_small_` with `DATABASE_URL` from `.env` (55405, test DB already at
  the frozen schema, verified via `pg_constraint`): **23 passed, 0 failed**
  (`logs/br1-b-plan-filters.log`). A first attempt without `DATABASE_URL` exported failed 22/23 at
  the harness's "DATABASE_URL is not set" guard -- environment, not code; overwritten by the rerun.
- `notification_delivery_tenant_` x5 (F5 race stability): 7/7 each; `company_invite`: 10 passed
  (`logs/br1-b-repeat-and-callers.log`).
- `cargo fmt --check`: clean (`logs/br1-b-fmt-check.log`, empty).
- Removal-first race, real lock shapes, two psql sessions (`logs/br1-removal-first-race.log`):
  session R runs `remove_member`'s statements and holds them 2 s; session P runs the projection's
  `recipient_for_source` lock shape. P blocks from 39.50 until R commits at 40.78, then returns
  **0 recipients**. The protocol is correct; only its test is missing (BR1-F1).
- Merged schema = p05 worktree init schema + `git show 4d940da -- migrations/...` applied with
  `patch` in `$TMPDIR` (clean apply; `logs/br1-merged-schema-apply.log` empty), scratch DB
  `p05_b_br1_merged`, dropped afterwards.
  - `logs/br1-merged-company-delete.{sql,log}`: one company with person/agent/system principals
    authoring messages, a participant identity on a message, an owner member row, plus p05's
    reviewer notice, notification email notice, all three destination kinds and an internal note.
    A. company delete at emulated commit: 0 rows left everywhere. B. p05's `remove_member`
    statements for the member: member principal `external`, messages and participants kept,
    notifications cascaded, reviewers/destinations kept; the deferred owner-agreement trigger passes.
  - `logs/br1-merged-fairness-fixture.{sql,log}`: the p05 fairness fixture shape (company INSERT and
    owner-member INSERT as two autocommit statements) fails on p01's deferred
    `companies_owner_member_fk`; the same two inserts in one transaction pass (BR1-F4).
- Author logs cross-checked: `a-run5` 7+5+4+4+3 = 23 passed; `a-run6` 1348 passed / 0 failed /
  3 ignored; `a-run7` prepare wrote the query data; `a-run8` offline build finished. Not re-run:
  the 1348-test targeted run and the offline build (author evidence accepted; fmt and the plan
  filters re-run independently).

### Judgements requested

- **r3 item 1 (`workflow_message_intent_reply_to` deferred):** correct. The thread-entry trigger
  `delete_orphan_message` deletes messages in a PL/pgSQL statement whose own AFTER-trigger queue
  fires at that statement's end, before the outer cascade reaches the intent -- the same mechanism
  as G1-F1/F7. Deferred NO ACTION keeps the guarantee at commit, and the test shows deleting the
  target alone is still refused. Accepted.
- **r3 item 2 (binding hard-delete refused):** asserting the refusal is right. `ON DELETE CASCADE`
  would erase run history with a binding row; `SET NULL` is impossible (`binding_id` is NOT NULL and
  part of `workflow_runs_company_id_id_binding_id_key`, which the intent-outcome FK uses). The app
  never deletes bindings (deactivate only; association immutable), NO ACTION mirrors the run ->
  channel FK, and company deletion cascades runs first (tested). The edited tests keep their
  archived-definition half. Accepted.
- **r3 items 3-5:** fixture adaptations sound (real binding revisions, real companies for demand,
  company-bound schedule for the raw channel delete with the company delete covering both kinds);
  notice company plumbing complete.
- **D1.8 r2 (`remove_member`, `cancel_member_notices_on`):** matches the r2 order (demote -> member
  `FOR UPDATE` -> lock and cancel notices `ORDER BY id` with `RecipientRevoked` -> delete member);
  owner/absent is a committed no-op; claimable -> `dead_letter` with parts dead, in-flight -> intent
  only (tested). Producer-first race is a live competing-claimant test; removal-first is not (F1).
- **BounceInfo:** `company_id` + required `company_slug`; `undeliverable_rejection(&Company)`; the
  unreachable no-company case falls through to `UnknownRecipient` (D1.6). Accepted.
- **F8 (`clean_cursors`):** deleted-company statement removed, the FK cascades those rows (tested by
  `tenant_fk_small_company_deletion_leaves_no_demand_or_poll_rows`). Accepted.
- **F2 / F6 / F12** (carried from G1-R2): F2 egress test present and correct; F6 populated deletions
  present in every listed module (with the BR1-F2 caveat); F12 closed, no app path affected.
- **p01 merge (author's added scope):** no runtime FK/cascade conflict found on the merged schema
  for company deletion or member removal. p01's three new non-deferred NO ACTION keys reference rows
  that the `companies` cascades delete in round 1 (agents, principals) or round 2 (identities via
  principals), and their referencing rows (messages, principals; participants via messages) go in
  the same or an earlier round, so each check, fired one round later, finds nothing. The comment
  p01 adds ("NO ACTION is checked at the end of the statement") is inaccurate (per-round, see G1
  background), but harmless for these three keys. `remove_member` (p05) already demotes first, and
  p01 left its body unchanged, so the textual merge is only at the end of `company_invite.rs`.
  `notifications_recipient_member_fk` only fires on non-owner member deletion, which the
  owner-agreement trigger allows. One merge-time break: BR1-F4.

### Outcome

**CHANGES REQUIRED** for digest `2a3cb838…c09c4`: one required (low) finding, FIXAPP-P05-BR1-F1.
Optional: BR1-F2, BR1-F3; merge watch (not blocking this digest): BR1-F4. G1-F12 closed.
Everything else in the batch is accepted as implemented.

### Process and instruction feedback

- The r2 verification plan named three competing-claimant cases; the batch delivered two live and
  one sequential without recording the change. Candidate rule (scope: implement-opus-reviewed-2_2
  author checklist): "When a gated verification item is implemented differently (e.g. sequential
  instead of concurrent), record the deviation and its reason in TASK before requesting review."
- Running tests requires `DATABASE_URL` from the worktree `.env`; the harness refuses to run
  without it. Candidate note (scope: phase TASK template, Resources): "export DATABASE_URL from
  `.env` before `cargo test`".
- `COORDINATION.md` keeps changing in place under the same revision label; reviewers must copy it
  each round to pin inputs (same candidate as G1-R2).

### Freshness at return (BR1)

- Digest `2a3cb838…c09c4` and HEAD `e6b0a51` unchanged; `TASK.md` byte-identical to
  `logs/br1-TASK-seen-by-B.md`.
- `COORDINATION.md` changed in place again during the round (still "Revision: 2"): the p01 row now
  reads "accepted in phase, committed 4d940da (not merged)", which matches the author's p01 input.
  Copy at return: `logs/coordination-r2-at-return-B-BR1.md`. No effect on the outcome.
- Scratch database `p05_b_br1_merged` dropped; `mail_agents_mergecheck` (not B's) left untouched.

## FIXAPP-P05-BR1-R2 — Batch review recheck by finding ID

### Identity

- Date 2026-11-01. Workflow `implement-opus-reviewed-2_2`. Reviewer (B): Opus 5.5 / xhigh.
- Batch: implementation batch 1 (plan batches 1-5), stage **batch review recheck**.
- Worktree `/Users/mac03/dev/openbot/.worktrees/fix-app-p05`, branch `fix-app/p05`; base = HEAD
  `e6b0a510ec27a636bf76c9aa9312605ca3e5c8cb`; digest
  `202392ee3dd968a2c9c80aeccf1b293b13ee79c904b6dd02cd67f18690d7da34` (exclusions
  `plan/fix-app/phases/p05/docs`), confirmed at start and before returning.
- Inputs: `TASK.md` "BR1 corrections" + Check log (copy `logs/br1r2-TASK-seen-by-B.md`);
  coordination **revision 3** (live file identical to the author's `logs/coordination-r3.md`; the
  only change from B's BR1 return copy is the revision bump and a manager-log line recording the
  p01/p02 status edits, no instruction change).
- Delta scope: files modified after B's BR1 return marker
  (`find -newer logs/coordination-r2-at-return-B-BR1.md`) are exactly five test files:
  `delivery/tenant_tests.rs`, `thread/tests.rs`, `workflow/message_thread_tests.rs`,
  `workflow/message_sql_tests.rs`, `workflow/fairness_tests.rs`. No production, schema or `.sqlx`
  change, consistent with the author's statement. BR1 evidence for everything else is reused.

### By finding

- **BR1-F1 (required): fixed.** `notification_delivery_tenant_removal_first_producer_finds_no_member`
  (`own_database`): a third transaction holds the member's pending notice `FOR UPDATE`;
  `remove_member` is spawned and can only block in `cancel_member_notices_on` (after it demoted the
  principal and locked the member); the producer's `FOR SHARE OF principal, member` select is spawned
  and the test waits for 2 blocked backends, which proves the producer is queued behind removal; then
  the notice is released. Assertions: removal Ok, producer sees no recipient, its notification insert
  fails on `notifications_recipient_member_fk` (constraint name, not `is_panic()`), notice
  `dead_letter` + `RecipientRevoked`, principal `external`. This matches the recipe and the psql
  reproduction (`logs/br1-removal-first-race.log`). The sequential `late_producer` test is gone.
- **BR1-F2: fixed.** Both destination company-deletion tests count `message_delivery_parts` joined
  to the user / reply destination's `delivery_id` and assert > 0 before deleting.
- **BR1-F3: fixed.** (a) "another thread of the same channel"; (b) "A second user (the destination's
  user key is to `users`)"; (c) the `DISABLE TRIGGER` note test uses the pre-existing
  `Fixture::isolated` (`thread/test_support.rs`, `own_database`); (d) the fairness test deletes its
  33 stale companies (demand rows cascade).
- **BR1-F4: fixed ahead of merge.** One data-modifying CTE statement writes each stale company and
  its owner row, valid on this schema and on p01's deferred owner key. Author's trial merge with
  p01 `4d940da`: 166 passed, 0 failed (`logs/a-mergecheck-p01-4d940da.log`, separate mergecheck
  worktree/DB of the author; accepted, not re-run). The comment names
  `insert_company_with_owner` for the merge.

### Checks run by B

- `DATABASE_URL` from `.env`; `cargo test --lib -- notification_delivery_tenant_ run_binding_fk_
  channel_path_fk_ message_thread_fk_ tenant_fk_small_ workflow_fairness_ message_tests::sql_tests
  message_tests::thread_tests`: **56 passed, 0 failed**. The changed tests are included: the
  internal-note FK test, both destination deletion tests, the fairness invalid-offer test and both
  race tests. Then `notification_delivery_tenant_removal_` x5: 2/2 each (6/6 runs including the first
  for each race test). Log `logs/br1r2-b-checks.log`.
- `cargo fmt --check`: clean (`logs/br1r2-b-fmt-check.log`, empty).
- Test database already at the frozen schema (no schema change since BR1; the author reset before
  `a-run9`).

### Outcome

**PASS** for digest `202392ee…7da34`, coordination revision 3. All BR1 findings are resolved (F1
required; F2-F4 optional, all done). No new findings. Carry to merge: F4 note (use
`insert_company_with_owner` once p01 is merged); the p01 per-round NO ACTION comment
(cross-phase notice already requested at G1).

### Process and instruction feedback

- The correction round stayed inside test code, and the author scoped the delta explicitly, which
  made a by-ID recheck cheap. The `find -newer <B's return marker>` check gives an independent
  confirmation of a "test-only" claim when every change is uncommitted. Candidate (scope: reviewer
  checklist in implement-opus-reviewed-2_2): "On rechecks of uncommitted work, confirm the claimed
  delta against files modified since the previous round's return marker."
- A verdict delivered through SubagentHandback reached the coordinator, not the author. Candidate:
  the workflow should state who receives B's handback when the author did not spawn B, so the author
  never waits on a verdict that went elsewhere.
