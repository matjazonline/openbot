# P08 agent review notes

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## WF-P08-R001

Architecture gate P08-D1 rev 1. **Outcome: CHANGES REQUIRED.**

### Identity

- **Round:** WF-P08-R001, 2026-10-22, workflow `implement-opus-reviewed-2_2`. Records:
  [`TASK.md`](TASK.md), [`REVIEW.md`](REVIEW.md), findings
  [`agent-review-findings.md`](agent-review-findings.md).
- **Batch and stage:** B1 (engine composition and message ingress), plus the D1.9 schedule
  contract implemented in B2 and the D1.0 batch split. Stage: pre-implementation architecture gate.
- **Reviewer:** B, Opus 5.5, effort xhigh, fresh context. Phase `p08`; worktree
  `/Users/mac02/dev/openbot`, branch `workflows`.
- **Commits:** phase start = review base = HEAD = `da868710d3b235d49856e3e481f8f3735f6b3e56`. No
  source edits exist.
- **Digest:** `e07e4b1d4a5f9491a6fd1115d251a416d56955383a564f249531e9ba565b47e9`, with exclusions
  `plan/workflow-foundation/phases/p08/docs plan/workflow-foundation/COORDINATION.md marketing website`.
  Checked at start and again before returning (see freshness).
- **Gated decision:** P08-D1 rev 1 = `logs/decision-D1.md`, sha256
  `453bf4a05cab5d9b5d358f1c6ea0b58b2b454d2ff65c7562e7375e45e946b7bb`, checked at start and end.
- **Coordination:** authority `plan/workflow-foundation/COORDINATION.md`, revision 5, sha256
  `38fd9afe19ea5970ebc485649d17ce6a135c06afcb5e0f8fd1862bb0abac5c74`. It is byte-identical to the
  retained `logs/coordination-r5.md`. Applicable notices: WF-C001 r2 and WF-C003 r1 (target p08).
  WF-C002 r1 targets p09 only and is unrelated, apart from optional note F012.4. There are no
  plan-wide instructions beyond the `AGENTS.md` files and the README.
- **Freshness check:** recomputed before returning. The digest, the decision hash and the
  coordination hash were all unchanged.

### Coverage and evidence

All coverage is by inspection; it is a gate and no checks were run.

**Requirements read in full:**

- `08-triggers-and-application-replacement.md`
- EXPANSION rows 08.1–08.4
- README
- NOT_PLANNED
- every REPLACEMENT-MAP row naming 08, plus its removal ledger
- RESUME "Phase-08 obligations (from 06C)" and the RESUME:17-19 additions
- TASK user decisions P08-Q1..Q3 and acceptance criteria 1–13
- WF-C001 r2 and WF-C003 r1
- `AGENTS.md`, `src/AGENTS.md`, `src/application/AGENTS.md`,
  `src/adapters/persistence/AGENTS.md`, `src/adapters/http/AGENTS.md`, `src/infra/AGENTS.md`,
  `src/adapters/smtp/AGENTS.md`

**Code read to challenge D1.1–D1.9:**

- Ingest:
  - `thread/ingest/commit.rs`, in full
  - `thread/ingest/routing.rs`: `prepare_channels`, `candidate_answers`, `replies_to_agent`,
    `authorize_thread_injection`, outreach correlation
  - `thread/ingest/policy.rs`: `fold_disposition`
  - `thread/ingest/mod.rs`: the commit result
  - `thread/mod.rs`: `InboundIngestResult`
- `adapters/persistence/thread/inbound.rs`, in full: `commit_on` order, `recognise_redelivery`,
  `create_tasks`, `create_handoffs`, `message_write` (audience from disposition)
- `channel_gate.rs`
- Workflow admission and authority:
  - `workflow/service.rs` (`admit`, `admissible_trigger`) and `contracts.rs`
    (`AdmitWorkflowRequest`, `PreparedAdmission`)
  - persistence `admission.rs` (`verify_current`), `admission_binding.rs` (`SourceKey`, the
    REPEATABLE READ selection), `admission_replay.rs`, `admission_write.rs`, `authority.rs`,
    `association.rs`, `action_authority.rs` (the run actor recheck)
  - `lifecycle/bindings.rs`
- Waits: `workflow/waits.rs` (port, `EventIngress`) and persistence `waits.rs` (`record_event_on`
  predicate)
- Registry: `registry/mod.rs` (event payload-schema rule, kinds) and `schemas.rs`
  (`message_result`)
- Message runtime:
  - `message_runtime.rs`: destination planning, user threads, relay
  - persistence `message.rs`: `reply_target_on`, `source_relay_on`
  - `message_publish.rs`: no `thread_principals`, destinations write
  - the `workflow_message_destinations` schema, CHECK and SQL aggregate
- `workflow/worker.rs` (cancellation token, handler `supports`, unsupported kinds) and the
  handlers' `supports`
- Test composition in `workflow_tool_support.rs` (how runtimes compose; the WF-C001 shape)
- Process wiring: `src/main.rs` `serve` (broadcast shutdown, `join_background`) and
  `src/infra/db.rs` (pool size 10)
- Schedules: `use_cases/schedule.rs` (run-as policy, run-now, materialization path), the
  `channel_schedules` / `schedule_runs` schema and state CHECK
- Fixtures: `autonomous-response.yaml`, `reviewed-support.yaml`
- `channel.rs` `ChannelResponseTrigger`, and the simulation route `channel.rs` (`delivery_for`,
  owner-only)

**Assumptions challenged:**

- That the commit-time fold no longer depends on channel settings. Not established: F001.
- That `record_event` "matched" means "will be consumed". False: F002.
- That ingress sender authorization is equivalent to legacy outreach correlation. False in both
  directions: F003.
- The crash-replay claim for reply events. It contradicts the stated order: F004.
- That `Conflict` is deterministic. False for `verify_current`: F005.
- That P08-Q2 replaces the existing run-as rules. It drops the admin-self-only rule and the
  stored-attribution rule: F006.
- D1.0 completeness against the 06C obligations. Gap: F007.
- Queue protocol versus the persistence fence/fairness rules: F008.
- That the in-app-only shape fits the schema: F009.
- Pool, shutdown and lifecycle rules for the new loops: F010.
- D1.9 authority and verification: F011.

**Confirmed sound (inspection only):**

- **WF-C001 r2:** child admission refused in `admit`; `ChildPublicationDirectory` outside
  `AgentPublicationDirectory`; `with_workflow_tools`; rerun deferred to B2. **WF-C003 r1:** routes
  deferred to B2.
- **One run per (binding, message):** holds via the `SourceKey` that excludes the trigger UUID,
  key `m:{intent}:{binding}` with a stable `trigger_id`, and replay (`admission_replay.rs`).
- **Redelivery:** writes no intents (`recognise_redelivery` is read-only apart from event
  completion).
- **Legacy scheduled runs:** they insert messages directly rather than through ingest, so no dual
  dispatch exists between B1 and B2.
- **D1.6 relay ownership:** keyed per (message, channel), which is a stricter key than the legacy
  primary-channel task row.
- **D1.8 publication refusal:** covers the unhandled `memory.*` kinds.
- **§04.10:** handlers compose only ActionService-backed runtimes.
- **Revocation:** reaches running work through `action_authority.rs:40-71`.
- **No backward compatibility:** the note is present in the decision and TASK.

**Light coverage:**

- The B2 route surface. Per D1.0 it is reviewed in the batch review, so only the D1.9 schema
  contract was gated.
- B3 removal scope, beyond requirement coverage.
- Agent runtime internals and stack depth: no measurement.

**Not run (unavailable):** checks, `graft` (not installed). Searches used rg/grep and targeted
reads.

### Outcome

**CHANGES REQUIRED** for P08-D1 rev 1.

- **High (3):** WF-P08-F001, WF-P08-F002, WF-P08-F003.
- **Medium (4):** WF-P08-F004, WF-P08-F005, WF-P08-F006, WF-P08-F007.
- **Low (4):** WF-P08-F008, WF-P08-F009, WF-P08-F010, WF-P08-F011.
- **Optional:** WF-P08-F012.

Details and dispositions are in [`agent-review-findings.md`](agent-review-findings.md). No
dependent edits should start until a revised decision passes. A recheck should review the revised
D1.3–D1.5 text together, because F001–F004 interact: the fold, the intent existence, the match
predicate and the error classes.

### Process and instruction feedback

- **Batch size.** The gate was reviewable in one round. B1 is large but cohesive. B2 as planned
  combines a schema contract with the whole route surface and setup flows, and may be too large to
  review thoroughly in one batch review (F012.5).
- **Gate scope.** The gate was necessary and correctly scoped. Every High finding is a decision
  defect, not an implementation error, and would have been costly to unwind after the ingest,
  intent and wait code existed.
- **Author records.** The inventory was useful and accurate. TASK criterion 13 condensed the
  RESUME obligation and lost two items (F007).
- **Candidate instructions** (evidence: F001–F003, F005, F006):
  1. Scope: gate template in the workflow skill, the TASK "decision" section. "When a gate moves a
     decision to a new owner, or gives an existing port a new production caller, list each consumer
     of the old value and each acceptance predicate of the reused port, and state its new
     meaning."
  2. Scope: same template. "When replacing a legacy path, enumerate the predicates it enforced
     (authorization, correlation, attribution) and dispose of each explicitly."

  These are recorded for later analysis. No skill or `AGENTS.md` was edited.

### Follow-up 2026-10-22 (from WF-P08-R002)

Two of R002's Low items also applied to rev 1 and were missed in this round:

- **WF-P08-F016 item 2:** `resume_on` expires an event wait before it looks up events, so a reply
  recorded just before the deadline is swallowed.
- **WF-P08-F016 item 3:** the allowlist channel ACL still refuses user-send replies after the
  thread-injection fix proposed in F003.

R001 checked the match predicate (F002) and the thread-level authorization (F003), but did not
follow the event through wait expiry or check the channel-level ACL. The original R001 outcome
stands.

**Correction 2026-10-22 (WF-P08-R003):** WF-P08-F016 item 3 was a false premise. The ACL refusal
falls back to thread-party access (`routing.rs:325-343`), so it was not an R001 miss after all.
Only item 2 remains a genuine R001 miss.

## WF-P08-R002

Architecture gate correction recheck, P08-D1 rev 2. **Outcome: CHANGES REQUIRED.**

### Identity

- **Round:** WF-P08-R002, 2026-10-22, workflow `implement-opus-reviewed-2_2`. Records:
  [`TASK.md`](TASK.md), [`REVIEW.md`](REVIEW.md),
  [`agent-review-findings.md`](agent-review-findings.md).
- **Batch and stage:** B1 + D1.9 + D1.0. Stage: correction recheck of the gate.
- **Reviewer:** B, Opus 5.5, effort xhigh. This is the same reviewer context as R001, continued.
  Phase `p08`, worktree `/Users/mac02/dev/openbot`, branch `workflows`.
- **Commits:** HEAD = base = `da868710d3b235d49856e3e481f8f3735f6b3e56`. No source edits exist.
- **Digest:** `e07e4b1d4a5f9491a6fd1115d251a416d56955383a564f249531e9ba565b47e9`, same exclusions
  as R001.
- **Gated decision:** P08-D1 **rev 2** = `logs/decision-D1-r2.md`, sha256
  `5271630feb82ecdb20f006ffb2ce9611642dbc6b5890e1dd06348699fda0c63a`. The rev 1 file is unchanged
  (`453bf4a0…b7bb`), and the diff is `logs/decision-D1-r1-to-r2.diff`.
- **Coordination:** r5, sha256 `38fd9afe…5c74`, unchanged. Notices WF-C001 r2 and WF-C003 r1.
- **Freshness check:** digest and both hashes verified at start and again before returning.

### Coverage and evidence

All coverage is by inspection; no checks were run.

**Rev 2 read in full.** As requested, D1.3–D1.5 were rechecked together:

- fold and audience;
- intent shape, fence and FKs;
- claim CTE and fairness;
- the `Stale` and per-binding classification;
- reply candidate, awaited-correlation rule, `NotYet`, responder identity, user-send participation;
- `deliver_reply` order and outcome mapping.

D1.1, D1.2 (bound, `activated_at`), D1.7, D1.8, D1.9, D1.0, the invariants, failure modes, verification plan and the TASK dispositions and criterion 13 were also checked.

**Additional code read this round:**

- `workflow_waits` schema: `event_name`, `correlation`, `state`, `execution_id` exist, so §D1.5.3
  is implementable.
- `workflow_message_actions` (`execution_id`, `origin`).
- `wait_commit.rs` `resume_on`: expiry precedes event lookup (lines 48-51 vs 96).
- `participant.rs` `create_person_principal_on` (account email identity bound to the person
  principal) together with `member_on`: responder identity matches for user sends.
- `company_invite.rs` `remove_member` (deletes the `company_members` row).
- Schema FKs to `workflow_binding_revisions` (head and events, NO ACTION).
- `Channel::participant_access` (allowlist ACL).
- `IngressOrigin`, and the simulation route's sender, which is the authenticated user.
- `lifecycle/bindings.rs` configure, which keeps `active`.
- `src/infra/db.rs` pool default 10, `.env.example`, `fly.toml`.

**Confirmed fixed: F001–F011,** each with a dated resolution in the findings. F012 dispositions
are accepted.

**New issues introduced by rev 2:**

- F013: member FK.
- F014: `NotYet` attempt budget.
- F015: pool check against the defaults.
- F016: residual swallow and refuse windows.
- F017: bound bypass through configure.
- F018: optional suggestions.

**Light coverage:** gate P08-D2 does not exist yet, so the B3 provisioning tools were checked
only for scheduling.

### Outcome

**CHANGES REQUIRED** for P08-D1 rev 2.

- **Medium (actionable):** WF-P08-F013, WF-P08-F014.
- **Low (actionable):** WF-P08-F015, WF-P08-F016, WF-P08-F017.
- **Optional:** WF-P08-F018.

All of the R001 Highs are resolved. The remaining issues are narrow text corrections. A rev 3
recheck can be limited to the F013–F017 deltas, plus any section those edits touch, while
unchanged rev 2 sections keep this round's evidence.

### Process and instruction feedback

- **Corrections.** The correction round was effective: each R001 finding was dispositioned
  explicitly in TASK, and the decision marks its changes.
- **New issues.** Two of the new issues came from applying a general rule without checking local
  lifecycle facts. F013 applied tenant-FK hygiene to revocable membership. F015 added a new
  constraint without checking the default of the key it constrains. F014 added a new deferral
  state to an attempt-budgeted queue. Candidate instructions are recorded under F013–F015
  (scopes: persistence AGENTS §Preserve invariants; `src/application/AGENTS.md` §Keep failure
  states distinct). None were edited.
- **Batch size.** The four-batch split is now reviewable.

## WF-P08-R003

Architecture gate correction recheck, P08-D1 rev 3. **Outcome: PASS.**

### Identity

- **Round:** WF-P08-R003, 2026-10-22, workflow `implement-opus-reviewed-2_2`. Records:
  [`TASK.md`](TASK.md), [`REVIEW.md`](REVIEW.md),
  [`agent-review-findings.md`](agent-review-findings.md).
- **Batch and stage:** B1 + D1.9 + D1.0. Stage: correction recheck of the gate.
- **Reviewer:** B, Opus 5.5, effort xhigh, same context as before. Phase `p08`, worktree
  `/Users/mac02/dev/openbot`, branch `workflows`.
- **Commits:** HEAD = base = `da868710d3b235d49856e3e481f8f3735f6b3e56`. No source edits exist.
- **Digest:** `e07e4b1d4a5f9491a6fd1115d251a416d56955383a564f249531e9ba565b47e9`, same exclusions.
- **Gated decision:** P08-D1 **rev 3** = `logs/decision-D1-r3.md`, sha256
  `7c4b9457e146890eaaaa2b8936324726dcdd2da45f86333e7125eb8e2121aaf7`. The rev 2 file is unchanged
  (`5271630f…0c63a`), and the diff is `logs/decision-D1-r2-to-r3.diff`.
- **Coordination:** **r6**, sha256 `23fdc1905d723fd0330e27ab117a83dbde274becb70b23bd915cd60313b8c4db`,
  identical to `logs/coordination-r6.md`. Its diff against r5 changes only the p08 response lines
  for WF-C001 and WF-C003 (batch placement); no notice requirement changed. Applicable notices are
  still WF-C001 r2 and WF-C003 r1.
- **Freshness check:** digest and hashes verified at start and before returning.

### Coverage and evidence

All coverage is by inspection; no checks were run.

**Scope, as agreed in R002:** the rev 2→3 diff in full, plus the sections it touches:

- D1.1 (FK)
- D1.2 (cap)
- D1.4.1 (`NotYet` neutrality)
- D1.5.1 (correlation rule)
- D1.5.3 and D1.5.5(b) (lock)
- D1.5.6 (event before deadline)
- D1.5.7 (allowlist)
- D1.8 (pool default)
- the failure table and verification plan
- the coordination r6 diff and TASK §Coordination

**Unchanged rev 2 sections** reuse R002's evidence; the code and fixtures they rest on are
unchanged (same digest).

**Code read this round:**

- `routing.rs:255-385` (`authorize_channel`, `thread_access_for`) and `:485-507`
  (`find_ancestor_thread`). This verifies the F016.3 reading and dismisses my R002 premise.
- `wait_commit.rs:1-110` (`resume_on` lock order and event lookup).
- `waits.rs:98-150` (`lock`, `row` `FOR UPDATE`).
- The `maintenance::expire_on` lock.
- The `workflow_wait_events` FK and `created_at` default.

**Results:**

- **F013–F017:** fixed, except F016 item 3, which is dismissed as a false premise. Each has a
  dated resolution in the findings.
- **F018:** adopted.
- **New:** WF-P08-F019 (Low, implementation-level lock order) and WF-P08-F020 (optional
  precision notes).

**Light coverage:** gate P08-D2 is not written yet; B3 is out of scope.

### Outcome

**PASS for P08-D1 rev 3** (sha256 `7c4b9457…21aaf7`).

No acceptance-blocking defect remains in the gated decision.

- **WF-P08-F019 (Low):** the `deliver_reply` lock order, run row before wait row. It is not
  gate-blocking because it changes no contract, schema or protocol boundary. It is a **mandatory
  check in the B1 batch review**, including a race test free of deadlock and lock-timeout errors.
- **WF-P08-F020:** optional.

### Process and instruction feedback

- **Corrections.** The correction loop converged in three rounds; each revision was a marked diff
  with per-finding dispositions, which kept rechecks small.
- **Reviewer error.** My R002 F016.3 rested on a partial reading of the authorization path: I
  read the ACL but not its fallback. Candidate (scope: reviewer practice in the workflow skill):
  "Before reporting an authorization refusal, trace the whole decision function, including its
  fallbacks."
- **Batch planning.** No change to batch sizing needed.

## WF-P08-R004

B1 batch review. **Outcome: CHANGES REQUIRED.** This entry was saved early, then completed below.

- **Identity at start:** HEAD = base `da868710d3b235d49856e3e481f8f3735f6b3e56`. The whole batch is
  in the working tree. Digest `90d4e0abbb34a6426258697b3459e8ba3dbdc6889dfeb8fba163f764370e7390`,
  verified at start.
- **Coordination:** r6 `23fdc190…c4db`.
- **Gated decision:** P08-D1 rev 3 `7c4b9457…21aaf7`.
- **Progress checkpoint (inspection so far):**
  - Schema diff reviewed.
  - Domain trigger and triggers module reviewed.
  - Ingest commit and routing reviewed.
  - `commit_on` intents reviewed.
  - Binding lifecycle and persistence: run-as rule, cap, `activated_at`.
  - Admission: `Stale`, expected revision, delivery policy, replay, child inheritance.
  - Intent worker application code and persistence: claim CTE, fence, `NotYet`, transient.
  - `deliver_reply`: lock order run→wait (F019 fixed in code), single clock (F020.2).
  - `resume_on` (D1.5.6) and the D-3 trigger.
  - Message runtime: in-app plan, user-send `thread_principals`, relay trace from intents.
  - Registry D-2, refused kinds, `infra/workflow.rs`, `setup`/`main`/`db.rs`, fixtures.
  - Tests read: competing claimants, crash-replay, poison batch, tenant burst, lifecycle,
    reply race, e2e, allowlist reply.
- **Candidate issues noted so far:**
  - `.env.example` is still at 10 and documents no slots key.
  - Terminal intent failure emits no metric or log.
  - The Resend inbound path has no intent wakeup.
- **Checks:** adjacent-module tests (modules A did not run) started in the background; log in
  the scratchpad, to be copied into `logs/` when done.

### Identity

- **Round:** WF-P08-R004, 2026-10-22, workflow `implement-opus-reviewed-2_2`. Records:
  [`TASK.md`](TASK.md) "B1 implementation log", [`REVIEW.md`](REVIEW.md),
  [`agent-review-findings.md`](agent-review-findings.md).
- **Batch and stage:** B1, batch review. Reviewer B, Opus 5.5, effort xhigh, same context as
  R001–R003. Phase `p08`, worktree `/Users/mac02/dev/openbot`, branch `workflows`.
- **Commits:** base = HEAD = `da868710d3b235d49856e3e481f8f3735f6b3e56`. The whole batch is in the
  working tree: 87 files modified or deleted and 10 new source files.
- **Digest:** `90d4e0abbb34a6426258697b3459e8ba3dbdc6889dfeb8fba163f764370e7390`, exclusions
  `plan/workflow-foundation/phases/p08/docs plan/workflow-foundation/COORDINATION.md marketing
  website`, manifest `logs/b1-snapshot.txt`. Verified at start and before returning.
- **Contract:** P08-D1 rev 3 `7c4b9457…21aaf7`. Carried items WF-P08-F019 and WF-P08-F020.
- **Coordination:** r6 `23fdc190…c4db`, unchanged. WF-C001 r2: the B1 part was verified
  (nesting, workflow tools, child admission refused). WF-C003 r1: B3, not in scope.

### Coverage and evidence

Inspection of the whole production diff:

- **Schema:** new tables, CHECKs, composite FKs, the `in_app` generated column plus its guard
  trigger (D-1), the `check_workflow_wait_commit` change (D-3), `activated_at`, run
  `delivery_policy`.
- **Domain and triggers:** `MessageTrigger`, `MessageTriggerInput`/`ScheduleTriggerInput`.
- **Ingest:** fold removed; audience by directive; one intent per association; outreach and hold
  removed, including the loop-trace and outreach thread-access widenings; `direct_parent`;
  wakeup.
- **Commit:** `commit_on` intents, redelivery returns the existing intents.
- **Bindings:** run-as owner/admin-self rule, `None` keeps the stored run-as, run-as visibility,
  readiness checked as the run-as, the cap checked in `save_binding` under the channel lock,
  `activated_at` kept on configure.
- **Admission:** `Stale` in both the service pre-read and `verify_current`; expected revision;
  policy in replay and in children; saved binding carries the run's actor.
- **Intent worker:**
  - claim CTE: rank, then lock with SKIP LOCKED, plus a failed sweep;
  - FENCE CTE on every write;
  - `NotYet` gives back its attempt; transient handling per binding; classification by condition;
  - shutdown drops the in-flight future and leaves the lease.
- **`deliver_reply`:**
  - ordering: event id, then run lock, then wait lock, then one response per correlation, then
    record;
  - single clock;
  - responder rules;
  - awaited steps derived from the correlation reference.
- **Waits:** `resume_on` consumes an event recorded by the deadline.
- **Message runtime:** in-app plan; user-send `thread_principals`; relay trace read from intents.
- **Registry and publication:** D-2 rule; refused kinds at publication.
- **Composition and serve:** `infra/workflow.rs`, setup, `main` (token bridge, joins), `db.rs`
  default 20, fixtures.

Tests read:

- intent rules, refusal, activation, `Stale`, transient, competing claimants, crash-replay,
  poison batch, tenant burst, lifecycle;
- reply tests: matched, unawaited, wrong responder/expiry, before park, crash-replay, recorded
  before deadline, race;
- end-to-end and allowlist reply;
- tenant FK test;
- wait-guard rewrite;
- infra composition tests.

Checks I ran on the phase cluster (`127.0.0.1:55480`, `SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`):

- `cargo test --lib -- message_tests::intent_tests message_tests::ingress_tests
  message_tests::thread_tests infra::workflow persistence::thread::inbound wait_guard
  registry::message_tests fixtures::tests domain::workflow::trigger application::workflow::triggers
  message_admission` → **81 passed, 0 failed** (`logs/r004-focused.log`).
- Modules affected by the traced impact that A did not run: `cargo test --lib --
  application::services adapters::smtp adapters::resend_api application::transport
  domain::entities adapters::harness application::use_cases::{channel,schedule,agent,
  thread_handoff,response_review}` → **451 passed, 0 failed, 2 ignored** (`logs/r004-adjacent.log`).
- `cargo fmt --check` → clean.
- `cargo clippy --all-targets -- -D warnings` → clean (`logs/r004-clippy.log`).

Relied on A's evidence without rerunning: the 2144-test focused run, the stack budget at 2048 KiB
(35 tests), and the fresh init-schema apply. The full suite has not run (by design until the task
completes).

Confirmed by inspection and tests:

- competing claimants and stale-generation fencing;
- poison batch;
- tenant fairness;
- crash-replay for both admission and reply;
- F019 lock order, with no deadlock;
- the owner/admin-self run-as rule;
- member removal is not blocked;
- the 8-binding cap on configure and on activation;
- WF-C001 nesting and child admission refused;
- memory step kinds refused at publication;
- one run per message per binding;
- no legacy task created at ingress;
- FileOnly and Cc audience.

Deviations D-1..D-4 were assessed:

- **D-1** (generated column plus trigger): acceptable and equivalent.
- **D-2** (no omitted payload schema): acceptable.
- **D-3** (trigger allows an in-time event): consistent with D1.5.6, but its refusal branch lost
  its test (F025.4).
- **D-4:** acceptable until B4.

Deleted legacy tests: each targets legacy dispatch, outreach, hold or pipeline behaviour that
B1 retires. The replacements cover their non-legacy invariants, with these exceptions:

- the `reply`/`channel` responder rules (F021);
- the policy-replay and tenant gaps (F025).

The loop-cycle and hop ingress tests remain (`uncorrelated_inter_channel_cycle_is_rejected`).

**Light coverage:**

- Agent runtime internals and the 60 s supervise deadline: pre-existing and unchanged.
- B2/B3 items: out of scope.
- No `cargo sqlx prepare`. No query macros were changed, so this is not applicable.

### Outcome

**CHANGES REQUIRED** for B1 at digest `90d4e0ab…7390`.

- **Medium:** WF-P08-F021.
- **Low (actionable):** WF-P08-F022, WF-P08-F023, WF-P08-F024, WF-P08-F025, WF-P08-F026.
- **Optional:** WF-P08-F027.

Carried items: WF-P08-F019 fixed; WF-P08-F020 items 1 and 2 adopted, item 3 optional.

All of these are narrow: tests, a small processing guard, observability and docs. No design
change is needed. The recheck can be limited to the correction deltas plus the touched tests.

### Process and instruction feedback

- **Batch size.** B1 was large but reviewable in one pass. The author's implementation log, with
  its deviations and its deleted-test mapping, made the review tractable and was demonstrably
  useful.
- **Missed checks.** Two misses came from checking only one branch of a per-kind rule (F021) and
  from not re-running a decision step's assumptions on retry (F024). Candidate instructions are
  recorded under those findings. No instruction files were edited.

### Follow-up 2026-10-22 (from WF-P08-R005)

R004 read `offer_reply`'s mapping of `Conflict` to `Unmatched` but did not trace that
`deliver_reply`'s wall-clock timeout is itself an `AppError::Conflict` (`lease.rs:267-269`).
WF-P08-F028 records the miss. The original R004 outcome stands.

## WF-P08-R005

B1 correction recheck. **Outcome: CHANGES REQUIRED.** This entry was saved early, then completed below. Digest
`500ae85b9eb6ecb3e11aa1739adf3c30fc56893db95fd232d3f001c6fc691b11` and HEAD/base `da868710` were
verified at start. Coordination r6 is unchanged.

### Identity

- **Round:** WF-P08-R005, 2026-10-22, workflow `implement-opus-reviewed-2_2`. Records:
  [`TASK.md`](TASK.md) "B1 corrections for WF-P08-R004", [`REVIEW.md`](REVIEW.md),
  [`agent-review-findings.md`](agent-review-findings.md).
- **Batch and stage:** B1, correction recheck by finding ID. Reviewer B, Opus 5.5, effort xhigh,
  same context. Phase `p08`, worktree `/Users/mac02/dev/openbot`.
- **Commits:** base = HEAD = `da868710…`, no commits; the batch is the working tree.
- **Digest:** `500ae85b9eb6ecb3e11aa1739adf3c30fc56893db95fd232d3f001c6fc691b11`, same exclusions,
  manifest `logs/b1-snapshot-r2.txt`. Verified at start and before returning.
- **Contract:** D1 rev 3 `7c4b9457…`. Coordination r6 `23fdc190…c4db`, unchanged.

### Coverage and evidence

Correction delta: files changed after the R004 manifest, identified by modification time
because there are no commits.

- `.env.example`, `docs/deploy.md`
- the init schema (`reply_settled` and its CHECK)
- `message_intents.rs` (`fail_expired_intents`, `settle_reply`, claim without the sweep)
- `message_admission.rs` (`claim`, `correlate`, `defer`, `failed`)
- `inbound_event_worker.rs` and `setup.rs` (intent wakeup)
- `routing.rs` doc
- tests: `message_intent_tests.rs`, `message_reply_tests.rs`, `wait_guard_tests.rs`,
  `inbound_multi_task_tests.rs`

All were read. Each of F021–F027 was checked against the code, not against TASK's text; see the
dated resolutions in the findings.

Affected behaviour rechecked:

- the reply-correlation and admission flow after settling;
- `NotYet` stays unsettled;
- fence on `settle_reply`;
- `claim_intents` still excludes attempts ≥ 5 if the sweep misses a row;
- CHECK consistency.

**While tracing the error mapping:** `deliver_reply`'s wall-clock timeout becomes
`AppError::Conflict`, so it is classified as `Unmatched` and now settled → WF-P08-F028.

**Checks on the phase cluster:**

- `cargo test --lib -- message_tests::intent_tests message_tests::ingress_tests
  message_tests::thread_tests infra::workflow persistence::thread::inbound wait_guard wait_tests
  registry::message_tests fixtures::tests message_admission inbound_event_worker`:
  - first run: 109 passed, **1 failed** — the allowlist e2e test timed out
    (`logs/r005-focused.log`);
  - that test alone, 3 runs: passed;
  - full set, 5 more runs (`logs/r005-focused-rerun1..5.log`): 110 passed each time.
  - The cause of the one failure is not established → WF-P08-F029.
- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -D warnings`: clean (`logs/r005-clippy.log`).
- Relied on A's evidence: the broad 2158-test run and the fresh migration log.

### Outcome

**CHANGES REQUIRED** for B1 at digest `500ae85b…1b11`.

- **F021–F025:** fixed.
- **F026:** resolved by deviation D-5.
- **F027:** adopted.
- **New:** WF-P08-F028 (Medium), WF-P08-F029 (Low, unresolved verification gap).

### Process and instruction feedback

- **Corrections.** The correction round was complete and well documented.
- **Reviewer miss.** WF-P08-F028 is a reviewer miss in R004 and recurs the WF-P08-F005 pattern:
  a variant-based classifier meets an error variant that also carries timeouts. Candidate
  instruction recorded under F028.
- **Flaky test.** It shows the value of rerunning the focused set more than once before
  acceptance.

## WF-P08-R006

B1 correction recheck for F028 and F029. **Status: COMPLETE — PASS**. Opened and closed
2026-10-23. Expected digest `b90a7f9c850b737062fc92bb971899f18756b8f086f2641649a39a657b49a468`.

Interim notes (saved before checks):

- Identity verified at start: HEAD `da86871`, digest `b90a7f9c…a468`, coordination r6
  `23fdc190…`.
- F028: classification moved to the adapter. `Undeliverable(cause)` only for `event_id_reused`,
  `payload_rejected` and `event_inbox_full`. Every `Err` is transient. The budget expiry is
  `DatabaseTimeout`. Only `Recorded` commits. The two tests cover the timeout (direct, 200 ms)
  and the worker deferral, and the permanent cause (settled, then admitted by the fallback).
- F029 mechanism, confirmed in source:
  - sqlx-postgres 0.8.6 `begin` raises `transaction_depth` only after `wait_until_ready`, and
    `start_rollback` does nothing at depth 0.
  - Every user-touched connection reaches the idle queue only through `return_to_pool`, which
    runs `after_release`. The other `release` callers (the reaper, `try_min_connections` and the
    `connect` warm-up) handle connections no user has had.
  - `detach`/`leak` take the connection out of the pool; production has no callers except
    `PgListener` (LISTEN only).
  - `close`/`close_on_drop`, pool closed and max lifetime all close the connection, so the
    server rolls back.
  - Simple protocol confirmed (`&str` has no arguments → `Query`). `wait_until_ready` flushes a
    queued `ROLLBACK`, and a `BEGIN` from an unflushed cancelled begin, before the check runs. So
    there is no false positive after a dropped `Transaction`, and an unflushed `BEGIN` is caught
    too.
  - Every production `begin()` wrapped in a timeout is a `pool.begin()` inside the timed
    future, so the `PoolConnection` drops with it.
- Diag evidence (`r005-diag-12`, `r005-diag2-13`): the run succeeded, the reply event and wait
  were consumed, the intent was still `claimed` (attempts 1, lease valid), and all sessions were
  idle with no xact. This fits a claim made inside a leaked transaction and committed later. The
  intent worker's `select!` drops `claim`/`process` mid-flight on shutdown
  (`message_admission.rs:512-527`), which is the trigger.

### Request and artifact

- Request: recheck the corrections for R005 findings F028 (fix) and F029 (investigation and
  fix).
- Correction record: TASK.md "B1 corrections for WF-P08-R005".
- Artifact: base/HEAD `da868710` (branch `workflows`) plus working tree. Manifest
  `logs/b1-snapshot-r3.txt`, with the four usual exclusions.
- Digest `b90a7f9c850b737062fc92bb971899f18756b8f086f2641649a39a657b49a468`, re-verified
  before returning. Coordination r6 (`23fdc190…c4db`) unchanged.
- Changed since R005: `infra/db.rs`, `test_support.rs`, `reply_waits.rs`, `waits.rs`,
  `message_admission.rs`, `message_intents.rs`, `message_reply_tests.rs`,
  `message_intent_tests.rs`, `message_ingress_tests.rs`.
- The stress logs `r005-stress-*` were discarded as TASK directs.

### F028: fixed

- **Classification moved to the source.** `deliver_on` returns `Undeliverable(cause)` only for
  `event_id_reused`, `payload_rejected` and `event_inbox_full`.
- **Timeouts are transient.** Expiry of the 2 s budget is `DatabaseTimeout`, and `offer_reply`
  treats every `Err` as `Transient`. `Unmatched` stays the ordinary no-open-wait result. This
  matches §D1.5.5 rev 3.
- **Cancelled delivery is safe.** A timeout mid-`deliver_on` drops the `Transaction`, which
  queues a ROLLBACK. A timeout mid-`begin` is now covered by the F029 guard.
- **Tests.** Both cover the requested behaviours: the direct 200 ms budget, the worker
  deferral (`reply_settled` false, matched on retry), and the permanent cause (settled, admitted
  by the fallback).
- **Residual risk, acceptable.** A deterministic database error outside the three causes now
  retries 5 times and then fails the intent observably (log + `cause=transient` metric). That
  is the contract's choice.

### F029: fixed (root cause established)

My answers to the three challenges:

1. **The root cause holds.**
   - The source confirms it: `begin` raises the depth after ReadyForQuery, and
     `start_rollback` does nothing at depth 0.
   - The deterministic test reproduces it.
   - The diagnostic state is fully consistent with it: claim uncommitted → `finish_intent` fence
     miss → committed later by a `BEGIN`/`COMMIT` on that backend → every session idle.
   - The trigger is the intent worker's `select!` on shutdown (`message_admission.rs:512-527`).
   - **Independent evidence:** the guard's warning fired once in 35 uncaptured runs
     (`logs/r006-nocapture-18.log`) and that run passed. So the suite really does leak
     transactions, and the guard discards them.
2. **`after_release` covers every way back into the pool.** I traced this in sqlx-core 0.8.6:
   - A connection a user has had reaches the idle queue only through `return_to_pool`, where
     `after_release` runs before the ping.
   - The reaper, `try_min_connections` and the `PoolOptions::connect` warm-up release only
     connections no user has had.
   - `detach()`/`leak()` take the connection out of the pool for good; the only production
     caller is `PgListener::connect_with` (LISTEN, no transactions).
   - `close()`, `close_on_drop()`, a closed pool, max lifetime and runtime teardown close the
     socket, so the server rolls back.
   - A `PoolConnection` kept and reused after a cancelled `begin` would not be covered. No
     production code does that: every timed `begin` is inside the future that owns the
     connection, including the `WriteBound::begin(&mut conn)` sites.
   - No false positive: simple protocol confirmed; `wait_until_ready` flushes queued ROLLBACKs
     first and also flushes an unsent cancelled `BEGIN`, which the check then catches.
   - The production pool is guarded (`init_db`) and so are both shared test pools. Ten ad hoc
     test pools are not → WF-P08-F030 (optional).
3. **The single slot and the diagnostics hide nothing.**
   - The single slot is a harness correction: two slots would need 6 of the 5 test-pool
     connections.
   - The intent worker still runs concurrently with the slot, and competing claims are covered
     by the intent-claim tests.
   - It does confound A's 60/60 evidence. The deterministic test and the observed guard firing
     close that gap.
   - The diagnostics are read before shutdown on a separate connection and only add to the
     panic. Worker panics still surface through `handle.await.unwrap()`.

### Checks on the phase cluster (55480, `workflow08_test`)

- `cargo test --lib -- infra::db message_tests:: wait_tests infra::workflow
  persistence::thread::inbound message_admission inbound_event_worker`: 171 passed, 0 failed
  (`logs/r006-focused.log`).
- The test binary run 20 times on `message_tests::intent_tests message_tests::ingress_tests
  message_tests::thread_tests wait_tests infra::workflow infra::db`: 66 passed every time
  (`logs/r006-repeat-{1..20}.log`).
- `message_tests::ingress_tests message_tests::intent_tests --nocapture`, 40 runs: all 32
  passed every time. One guard warning, in run 18 (`logs/r006-nocapture-{1..40}.log`).
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`: clean
  (`logs/r006-clippy.log`).
- Not re-run (relied on A's logs): the broad 2161-test run (`r005-fix-focused.log`) and the
  stack budget (`r005-stack-budget.log`).

### Outcome

**PASS** for the B1 corrections at digest
`b90a7f9c850b737062fc92bb971899f18756b8f086f2641649a39a657b49a468`.

- **F028:** fixed.
- **F029:** fixed.
- **New:** WF-P08-F030 (Info, optional, non-blocking): ad hoc test pools bypass the guard.
- **Open blocking findings for B1:** none.

### Process and instruction feedback

- **The F029 investigation was a model one.** A diagnosed before shutdown, followed a step-log
  hypothesis, read the library source and wrote a deterministic reproduction. The regression
  test also fails visibly once sqlx fixes the bug, which keeps the guard honest, in line with
  the AGENTS "leave something that fails" principle.
- **Candidate instruction (F030):** build every `PgPool` through the guard.


## WF-P08-R007

B2 batch review: schedules on bindings, manual start/rerun, removal of scheduled dispatch, and
F030. **Status: COMPLETE — CHANGES REQUIRED (Low only)**. Opened and closed 2026-10-23.

- Identity verified at start:
  - HEAD `da868710`, branch `workflows`.
  - Digest `7b932911f8a76a9fb4100af447f33df9388e052f7601576c22f6aa90b8125c51`.
  - Coordination r8 `0950b907…5e22`, which matches `logs/coordination-r8.md`.

Interim notes (saved during review):

- **Schema read.**
  - `schedule_runs`: composite run FK includes `binding_id` (B2-h); slot key partial; command key
    unique; state CHECK; refusal codes = the three classify codes.
  - The trigger captures scope under schedule `FOR SHARE`. Identity and run are immutable. The
    run scope check requires `trigger_id = occurrence`, the same channel and no thread.
- **Lock order traced.**
  - CRUD/run-now: company → channel (association) → schedule `FOR UPDATE` → occurrence insert,
    whose trigger takes `FOR SHARE` on a row already held.
  - Revalidate: company → schedule read.
  - `claim_due`: schedules `FOR UPDATE SKIP LOCKED` → insert → terminal UPDATE (not skip, waits
    on occurrence rows) → candidates `SKIP LOCKED`.
  - `record`: schedule `NO KEY UPDATE` → occurrence → schedule.
  - Admission: company → channel → schedule `FOR SHARE` → occurrence `FOR SHARE`.
  - No cycle found. Every holder of an occurrence lock already holds, or never needs, its
    schedule. `claim_due` skips any schedule another transaction holds. Company-locked writers
    serialize with admissions.
- **Manual start.** `u:` namespace verified against every production `IdempotencyKey::parse`.
  The command key is unique per company, so a key reused on another binding → `Conflict`. No
  production caller yet (routes are B3).
- **HTTP.** `binding_choices(None)` is filtered by `visible_channels`, and the run list goes
  through `managed_schedule`. But the JSON nested routes ignore the path `channel_id` (get,
  delete, run-now, runs, toggle) and the UI routes act on `id` regardless of the path channel.
  No route test tries another tenant's ids on the new `/runs` and `/schedule-bindings` routes
  (rule in `src/adapters/http/AGENTS.md`).
- **F030 done.** Nine pools now use `pool_options()`. The `lease_resolution` exemption is
  justified: the guard's release statement replaces `pg_stat_activity.query`, and sqlx's ping
  sends only Sync.
- **Record fence omits "unexpired lease"**, which plan step 3 named. The generation fence makes
  this benign.

### Request and artifact

- **Request:** WF-P08-R007, the B2 batch review.
- **Artifact:**
  - Base/HEAD `da868710`, branch `workflows`, plus the working tree.
  - Digest `7b932911f8a76a9fb4100af447f33df9388e052f7601576c22f6aa90b8125c51` (manifest
    `logs/b2-snapshot.txt`, the four usual exclusions). Re-verified before returning.
  - Coordination r8 `0950b907…5e22`.
- **Requirements checked:**
  - plan 08;
  - D1-r3 §D1.0 (B2 row), §D1.1 and §D1.9;
  - TASK acceptance criteria 1, 3, 9, 11 (schedule and manual parts) and 13 (scheduled legacy
    runs);
  - `AGENTS.md` plus the `src/`, application, persistence and http guides;
  - WF-C001 r2 (manual start and the rerun link) and WF-C004 r2.
- **Scope:** the B2 delta as enumerated by A. No frozen B1 tree exists, so for files changed in
  both batches I read the B2-relevant parts. The legacy schedule adapter and the
  `job_tenant_*` tests were compared against HEAD.

### Coverage

- **Schema:** `channel_schedules`, `schedule_runs`, the trigger, the FKs (run FK with binding
  B2-h), the partial slot index, the command-key unique constraint, the state and refusal
  CHECKs, and the `background_tasks` source removal.
- **Persistence:** all of `workflow/schedules.rs` (CRUD, run-now capture, compatibility,
  revalidation, cadence claim, `claim_occurrence`, `schedule_binding`, fenced `record`);
  `admission_source::schedule`; the `bindings.rs` revalidation call; the admission
  replay/command-key semantics for manual keys.
- **Application:** all of `schedule_admission.rs`, `manual.rs` and `use_cases/schedule.rs`;
  `classify` reuse.
- **Engine:** materializer spawn, `authorization()`, `schedule_run_now()`, connection budget.
- **HTTP:** JSON and UI schedule routes, the workspace loader (`binding_choices(None)`
  filtering), channel and agent settings loaders, the runs stream, page escaping.
- **Removal:** an exhaustive `rg` of the removed symbols (`SCHEDULED_AGENT_RUN`,
  `ScheduledRunPayload`, `TaskSource::ScheduleRun`, `ensure_schedule_run_thread`,
  `with_schedules`, `source_schedule_run_id`, the removed schedule fields). The only remaining
  hits are B4-owned legacy views, an index expression, historical evidence docs and
  removed-field refusal tests.
- **Tests:** all of `schedule_tests.rs` and `schedule_runtime_tests.rs`, plus the
  admission-source schedule tests, mapped against D1.9 F011.4 and the deleted legacy tests.

### Answers to the challenges

1. **Lock order and deadlock freedom: no cycle found.**
   - Company-locked writers (schedule CRUD, run-now capture, binding save with revalidation,
     admission) serialize on the company row first.
   - `claim_due` takes schedules with `SKIP LOCKED`. It can wait only on occurrence rows (the
     terminal UPDATE), and their holders hold or never need its schedules: admissions hold
     their own schedule `FOR SHARE`, which `claim_due` skips; `record` takes the schedule
     before the occurrence.
   - `record` and admission meet only on the schedule row, in the same order.
   - Delete cascades occurrence rows only after the company lock and the schedule
     `FOR UPDATE`. A late `record` then finds no schedule → `LeaseLost`.
2. **B2-g (captured binding): sound.**
   - Replay is deterministic, and the FK (B2-h) keeps a recorded run on the captured binding.
   - A retarget leaves captured occurrences on the old binding. Admission still rechecks that
     binding's association, run-as and activity, so nothing escapes authority.
   - A delete removes occurrences; already-admitted runs continue, which is consistent with
     criterion 8 (cancelling is separate).
   - Not tested → F032.
3. **B2-a (run-now) and B2-c (manual key namespace): sound.**
   - Run-now's inline claim races the cadence materializer safely: `claim_occurrence`
     re-evaluates its WHERE on the locked row, and `claim_due` skips locked rows.
   - The `u:` namespace is disjoint from every production key (`m:`, `s:`). The command key is
     unique per company, so a reuse on another binding → `Conflict`.
   - The deterministic SHA-256 trigger is required, because replay compares trigger ids.
   - `manual::start` has no production caller yet (routes are B3). The WF-C001 rerun link is
     verified by `head().rerun_of`.
4. **Refusal classification: correct.**
   - An inactive binding is `None` from `schedule_binding`, or `Stale` then a re-read, either
     way → `unavailable`.
   - A revoked run-as gets `NotFound` from admission → `unavailable`.
   - Codes match the refusal CHECK.
5. **HTTP visibility: no leak.**
   - `binding_choices(None)` is filtered through `visible_channels`; `Some(channel)` authorizes
     that channel first.
   - Lists filter to visible channels, and runs go through `managed_schedule`.
   - The path `channel_id` is ignored by nested routes, and other-tenant tests are missing →
     F031 (Low).
6. **Test adequacy (F011.4): every listed case is present.**
   - The e2e combining crash-replay with an LLM count of 1 is sound. The assertions that bite
     are one run per slot, recorded on attempt 2 by the production materializer, no
     thread or legacy task, and a real Rig client against the scripted server.
   - The duplicate-slot test races two real transactions.
   - Gaps → F032.
7. **B2-a..i: none needed a gate.**
   - B2-g is the only semantic refinement of D1.9, and it is cheap to unwind.
   - B2-i (inactive bindings allowed, slots refused visibly) satisfies D1.1 "never a silent
     drop".
8. **Deferrals:**
   - The live column → optional F034.1.
   - The run detail page (p09), the legacy views string (B4) and graft are accepted.
9. **WF-C004 r2 exemption: justified and narrow.**
   - The guard's release statement would replace `pg_stat_activity.query`, and sqlx's
     release ping is a bare Sync.
   - The construction documents the exemption, and a leak there would also fail the test.
   - All other ad hoc pools now use `pool_options()` (F030 closed).

### Findings

- **WF-P08-F031 (Low):** nested routes ignore the path channel; no other-tenant tests for the
  new routes.
- **WF-P08-F032 (Low):** deleted legacy tests not ported (poison batch on `claim_due`,
  cadence-unchanged edit, one-off disable); no B2-g test; lease-fence change unrecorded.
- **WF-P08-F033 (Low):** schedule terminal failures and lease loss are not counted or logged as
  such (recurs F023).
- **WF-P08-F034 (Info, optional):** live column, stale docs, a test leftover.

### Checks on the phase cluster (55480, `workflow08_test`)

- **Focused set:** 409 passed, 0 failed, 1 ignored (`logs/r007-focused.log`).
  - Command: `cargo test --lib -- schedule manual admission_source routes::schedule
    routes::ui_schedules routes::ui_channels routes::ui_agents pages:: infra::workflow infra::db
    lease_resolution claim_tests activation_tests completion_tests batch_recovery
    budget_activation child_runtime action_reconciliation_other_audit thread_lock
    message_tests::ingress_tests job_routine bindings binding_tests`.
  - It includes the WF-C004 r2 exempt test and every F030 pool test.
- **Repeats:** the test binary ×10 on `schedule_tests schedule_runtime_tests admission_source
  manual message_tests::ingress_tests`, 25 passed each time (`logs/r007-repeat-{1..10}.log`).
- **`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`:** clean
  (`logs/r007-clippy.log`).
- **Not re-run (relied on A's logs):** the broad 2529-test run, the stack budget and the scratch
  schema apply.

### Outcome

**CHANGES REQUIRED** for B2 at digest
`7b932911f8a76a9fb4100af447f33df9388e052f7601576c22f6aa90b8125c51`.

- Actionable: three Low findings (F031, F032, F033).
- Optional: F034.
- No Medium or High findings. F030 is closed.
- A recheck can be limited to the correction deltas.

### Process and instruction feedback

- **F033 recurs F023.** The second queue of the phase repeated the first queue's corrected
  gap. Candidate instruction recorded under F033.
- **F032: tests lost in a module replacement.** A deleted test file hid live behaviours that
  had lost their guard. Candidate: map every deleted test in the implementation log.


## WF-P08-R008

B2 correction recheck for F031–F033 and the optional F034. **Status: COMPLETE — PASS**. Opened and
closed 2026-10-23. Expected digest
`e43d8c37b661f17963acebfcf685f9859aeba5d46b9702a3620c0b9aa974b94f`.

### Request and artifact

- **Request:** WF-P08-R008, the recheck of the R007 corrections (TASK "B2 corrections for
  WF-P08-R007").
- **Artifact:** base/HEAD `da868710`, branch `workflows`, plus the working tree.
  - Digest `e43d8c37b661f17963acebfcf685f9859aeba5d46b9702a3620c0b9aa974b94f` (manifest
    `logs/b2-snapshot-r2.txt`, the four usual exclusions). Re-verified before returning.
  - Coordination r8 `0950b907…5e22`, unchanged.
  - `TASK.md` changed during the round. It sits under the excluded docs directory and is the
    author's record, so the artifact is unaffected.

### By finding

- **F031: fixed.**
  - `ScheduleScope` scopes get, delete, toggle, runs and run-now by the path channel, in
    `managed_schedule`. The channel is immutable, so the company-locked persistence recheck
    stays sound.
  - Every nested JSON route and every channel UI route passes the path channel, and the
    workspace routes carry none.
  - The new route test covers another channel, another company, another company with the right
    channel id, the correct path, and a foreign channel on `schedule-bindings`.
- **F032: fixed.**
  - All five tests are present and assert the right things.
  - The poison batch is the rule's two-iteration form, with a full page through the real
    `claim`.
  - The B2-g retarget test proves both halves: the old occurrence runs under binding A, the
    next slot under B.
  - The fence decision is recorded and documented; I agree with it.
- **F033: fixed.**
  - The sweep is two autocommit statements, and neither holds an occurrence lock while taking
    a schedule lock. I checked it against `record` (schedule then occurrence), admission
    (company, schedule share, occurrence share), capture and delete: no inversion is possible.
  - Terminal failures log at error level and are counted by cause. Lease loss is logged and
    counted.
  - The lease-lapse path sets the schedule's `last_error`. The test covers all three paths.
  - Residual: a failed follow-up schedule UPDATE drops the log and metric → optional F035.
- **F034: adopted.**
  - The live region polls every 15 s, and the fragment route answers with the column alone.
    The run-now and toggle forms and the pager still swap the whole region.
  - The SSE route and `channel_wake_ups` are deleted with no remaining users.
  - Docs and the anchor comment are updated, and the leftover is removed.

### Checks on the phase cluster (55480, `workflow08_test`)

- **Focused set:** 342 passed, 0 failed, 1 ignored (pre-existing) (`logs/r008-focused.log`).
  - Command: `cargo test --lib -- schedule manual admission_source routes::live_updates
    routes::ui routes::ui_channels routes::ui_agents adapters::http::pages infra::workflow
    infra::db message_tests::ingress_tests`.
  - It includes the 18 `schedule_tests`, the new route test and the e2e.
- **Repeats:** the test binary ×8 on `schedule_tests schedule_runtime_tests routes::schedule
  admission_source message_tests::ingress_tests`, 34 passed each time
  (`logs/r008-repeat-{1..8}.log`).
- **`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`:** clean
  (`logs/r008-clippy.log`).
- **Not re-run:** I relied on A's stack-budget log (`r007-fix-stack-budget.log`).

### Outcome

**PASS** for B2 at digest `e43d8c37b661f17963acebfcf685f9859aeba5d46b9702a3620c0b9aa974b94f`.

- F031, F032 and F033 are fixed, and F034 is adopted.
- New: WF-P08-F035 (Info, optional, non-blocking).
- No blocking finding is open for B2.

### Process and instruction feedback

- The correction round was complete and well mapped to the findings. It also adopted the
  optional items and recorded the fence decision where both code and record readers see it.


## WF-P08-R009

Architecture gate P08-D2 rev 1 (`logs/decision-D2-r1.md`). **Status: COMPLETE — CHANGES
REQUIRED**. Opened and closed 2026-10-23. Facts are cited against the accepted B2 tree (digest
`e43d8c37…b94f`).

### Request and artifact

- **Gate:** P08-D2 rev 1, sha256
  `dfa1bf4d2b2455d7eef714441707173a3103c2809d673481a3dc52af77840c07` (re-verified before
  returning).
- **Facts cited against:** the accepted B2 tree (base `da868710` plus the working tree, digest
  `e43d8c37…b94f`, re-verified). Coordination r8 `0950b907…5e22`.
- **Requirements checked:**
  - plan 08 (criteria 9–13);
  - REPLACEMENT-MAP §6, row 91;
  - RESUME 06C (criterion 13);
  - NOT_PLANNED row 18 and plan 05 §Assignment (reviewers on authenticated member routes);
  - P08-Q2, Q4 and Q5;
  - D1-r3 §D1.1;
  - WF-C001 r2, WF-C003 r1, WF-C004 r2;
  - F007, F012.3, D-5.
- **No code was run.** This is a decision review.

### Answers to the review questions

1. **B3a/B3b split: sound.** B3a holds the new action path and the transaction composition; B3b
   is breadth over existing commands, including B2's `manual::start`.
2. **Marker-first: the classifier is unchanged and effects are at most once.**
   - `local()` takes the company (`prelock_on`), the fence and attempt, the run `FOR UPDATE` and
     the intent `FOR UPDATE`.
   - It then checks for an existing marker and writes marker, effect and receipt in one
     transaction. Ambiguity resolves by the receipt.
   - A competing dispatcher of the same invocation serializes on the intent row and sees the
     marker.
   - `agent.run` is already marker-owned.
   - Gaps (authority composition, refusal mapping, read-tool path, reconciliation) → F040.
3. **Authority model:**
   - The requester rule is a good addition, and the derivation per root kind is reasonable
     (child → root via `parent_run_id`; manual and scheduled → actor). Two problems:
     - the provisioned binding runs as the Builder's run-as, which bypasses D1.1 → F036 (Medium);
     - an email-origin message can establish the requester, which the plan does not accept for
       privileged human authority → F037 (Medium).
   - `list_company_agents` discloses more than the UI does → F039.
4. **One transaction:**
   - **Lock order: no hazard.** Inside the action transaction the company lock is taken first.
     Provisioning only inserts new rows and takes `FOR SHARE` on member, agent, model connection
     and definition rows. The binding cap's advisory lock is on the new channel. The UI path
     takes the company lock first, and agent edits never take the company lock after their
     agent row. Nothing touches existing channels, schedules or occurrences, so the
     company → channel → schedule → occurrence order is untouched.
   - **Validation: the "no unvalidated path" claim covers the bundle only.** The agent,
     channel, binding and full publish checks are not named → F038 (Medium).
5. **Identity and UI replay:**
   - The deterministic ids are fine.
   - Replay and conflict rest on mutable rows with no stated ordering → F041.
6. **`/run/deadline`: sound.** Run deadlines are immutable and inherited by children, and
   waits are already capped. Canonical rendering → F045 (Info).
7. **Reserved names:** incomplete, and the premise about HTTP is inaccurate → F042.
8. **Verification:** no competing-claimant tests for the new protocol → F043. The Builder
   grants never reach a company Builder → F044.

### Findings

- **Medium:** WF-P08-F036 (run-as), WF-P08-F037 (email-origin requester), WF-P08-F038
  (bypassed validations not named).
- **Low:** WF-P08-F039, F040, F041, F042, F043, F044.
- **Info:** WF-P08-F045.

### Outcome

**CHANGES REQUIRED** for P08-D2 rev 1 (`dfa1bf4d…0c07`).

- A rev 2 recheck can be limited to the F036–F044 deltas and the sections they touch.
- D2.0 and D2.5 can stand as written, apart from the F045 notes.

### Process and instruction feedback

- **The gate was well grounded.** Its cited facts checked out: the symbols exist, `freeze`
  matches its signature, the local path holds, and `/run` already has roots.
- **The gaps cluster.** They concentrate where the design bypasses a service: the binding's
  run-as rule (F036) and the services' validations (F038). Candidate checklist items are
  recorded under those findings.


### Identity recheck before returning

- The gate document sha256 is unchanged (`dfa1bf4d…0c07`).
- **The working tree no longer digests to the accepted B2 value:** it is now
  `d98a8fd71670a779ab0b09ddda8062382f3f32561005fbcacb038b1445198f28`. The manifest file list is
  unchanged.
- **What changed:**
  - `src/adapters/persistence/workflow/schedules.rs` (08:23:52) and `schedule_tests.rs`
    (08:24:05) were modified after the decision document (08:19:15). The change is the optional
    F035 correction: a best-effort schedule update with a warning, and a new test at
    `schedule_tests.rs:1437`.
  - Several deletions are now staged in the git index; staging does not change the digested
    content.
- **Effect on this review:** none of the R009 citations is in those files, so the gate outcome
  stands. I have not reviewed the F035 change; it is for a later round. This is reported to the
  coordinator.

## WF-P08-R010

Gate recheck of P08-D2 rev 2 (`logs/decision-D2-r2.md`). **Status: COMPLETE — PASS** (three Low
findings carried to B3a). Opened and closed 2026-10-23. Expected sha256 `b88fe8ad…8afd`, tree `d98a8fd7…8f28`.

### Request and artifact

- **Gate:** rev 2 sha256 `b88fe8add6ae5738708b4e07d1ed93154dc030f452291ee983946cd6a1b08afd`
  (re-verified before returning). Rev 1 `dfa1bf4d…0c07` is unchanged.
- **Tree:** `d98a8fd71670a779ab0b09ddda8062382f3f32561005fbcacb038b1445198f28`, which matches
  the coordinator's statement that only F035 moved it. Coordination r8 `0950b907…5e22`.
- **Scope:** the deltas for F036–F045, plus the sections they touch. No code was run.

### Rechecked deltas: all R009 findings resolved

- **F036:** the requester is the configuring actor, `created_by`, the event actor and the run-as.
- **F037:** only `application`-origin intents yield a requester.
- **F038:** every bypassed check is named and extracted. `publish_on` adds
  `supported_publication(Publish)`, `verify_on` and `verify_children`. `provisioned_binding`
  runs the `configure` checks, and `save_binding_on` writes configure then activate.
- **F039:** both tools take the requester check.
- **F040:** `ProvisioningActionAuthority` mirrors `MessageActionAuthority`. Composition fails
  closed. Refusals commit under a savepoint, the read tool uses the local path, and
  reconciliation is closed. `ActionTarget::Local` exists (`actions/contracts.rs:23`).
- **F041:** a command record with a request digest is checked first under the company lock.
- **F042:** one reserved-name rule; trusted-check sites named.
- **F043:** competing-claimant tests added.
- **F044:** library copy fixed.
- **F045:** adopted.

### Challenges to the decisions beyond what I prescribed

1. **Refusals as committed receipts: agreed.** A savepoint is the only way to map a unique
   violation without aborting the action transaction, and replay becomes deterministic. Because a
   refusal is now permanent per invocation, it must never be derived from an error variant
   → F046 (Low, carried).
2. **Narrower `Provisioning` classification: acceptable for registration and dispatch.** It
   opens a grant-resolution path where an MCP snapshot of that name captured earlier wins over
   the built-in (`granted_tool` known-first order) → F047 (Low, carried).
3. **Library copy with the default connection's first model: acceptable.** Define the order →
   F049.2.
4. **Requester by intent existence: works today but fails open.** Match the stored source
   exhaustively → F048 (Low, carried).
5. **UI codes:** 404 matches the non-disclosing convention and 409 is right. 422 departs from
   the AppError mapping → F049.1.

### Lock order

- Unchanged from R009. The command-record read and `list_model_connections_on` run under the
  company lock.
- The savepoint adds no lock ordering: it nests inside the action transaction after
  `prelock_on`.

### Outcome

**PASS** for P08-D2 rev 2, sha256
`b88fe8add6ae5738708b4e07d1ed93154dc030f452291ee983946cd6a1b08afd`.

- The architecture is sound, and every R009 finding is resolved or adopted.
- B3a must satisfy WF-P08-F046, F047 and F048 (Low). They are implementation-level refinements
  within the gated design, and the B3a batch review checks them (as F019 was carried into B1).
- WF-P08-F049 is optional.

### Process and instruction feedback

- **Rev 2 was a model correction round.** It mapped every finding to a section, enumerated the
  extracted functions, and explicitly listed its own decisions for challenge.
- **F046 recurs F005/F028**, the error-variant classifier pattern. A candidate instruction is
  recorded under F046.


## WF-P08-R011

B3a batch review (provisioning tools and assistant setup; gate P08-D2 rev 2). **Status: COMPLETE —
CHANGES REQUIRED**. Opened and closed 2026-10-23. Expected digest `9bdad3da…1f3c`.

Interim notes (saved before checks):

- Identity at start: HEAD `da868710`, digest `9bdad3da…1f3c`, coordination r8.
- **Read so far:** persistence `provisioning.rs` (all), application `provisioning.rs`,
  `lifecycle/provisioned.rs`, `publish_on`/`insert_draft_on`, `save_binding_on`,
  `granted_tool` (F047), the `ToolKind` sites, reserved names (HTTP, workflow tools,
  `tool.call`), `/run/deadline` (context, activation), the library copy (F044), the routes, the
  engine composition, the schema, and the tests (dispatch, runtime, route).
- **Carried findings:** F046 (refusals only at named sites; DB/publish errors propagate; the
  savepoint retry is decided by `taken_slug`), F047 and F048 verified.
- **Candidate finding:** the `list_company_agents` receipt has no byte bound. Agent
  descriptions and names are unbounded (`AgentWrite::normalize`), and `validate_action_result`
  caps a result at 64 KiB, so a large listing fails the whole action deterministically. No
  truncation is reported.

### Request and artifact

- **Artifact:** base/HEAD `da868710` (branch `workflows`) plus the working tree.
  - Digest `9bdad3dae00d010689c5660c3c9a0c8895145079ea6c1989cc3dea27287d1f3c` (manifest
    `logs/b3a-snapshot.txt`, the four usual exclusions), re-verified before returning.
  - Coordination r8 `0950b907…5e22`.
- **Contract:** P08-D2 rev 2 (`b88fe8ad…8afd`), with F046–F048 carried from R010.
- **Scope:** the 68 files in `logs/b3a-files.txt` that changed after B2. B1/B2 changes in the
  same files were already accepted.

### Coverage

- **Persistence `provisioning.rs`, all:** authority, preparation, effects, `requester_on`,
  `requester_source`, `list_on`, `provision_guarded`/`provision_on`, `agent_write`,
  `channel_write`, `compiled`, `malformed`, `AssistantChannelService`.
- **Application:** `provisioning.rs` (schemas, trusted snapshot, refusal type,
  `ProvisioningActions::call_tool`) and `lifecycle/provisioned.rs`.
- **Extractions:** `publish_on`/`insert_draft_on`, `save_binding_on`,
  `capture_agents_on`/`agent_selection`, `insert_channel_row_on` (through the creation paths),
  `message_builtins`.
- **Tool kinds and reserved names:** `granted_tool` (F047); every `ToolKind::Provisioning` site
  (authority, resources, preparation, `prepared_tool`, the `workflow_agent` dispatch); reserved
  names (`HttpOperation::validate`, workflow-tool registration, the registry `tool.call`).
- **Templates:** `/run/deadline` (`ReferenceRoot`, activation contexts, `render_deadline`);
  `retarget_agent` (charset-restricted, re-decoded and checked); the shipped catalogue; the
  reviewed fixture.
- **Agents:** library copy (F044) and the Builder definition.
- **Engine and HTTP:** engine composition; routes (JSON and UI; same-origin protection is
  global); the schema table.
- **Tests:** dispatch (10), runtime (3), route, and the F035 test.
- **Not verifiable:** `message_publish.rs` and `message_runtime.rs` contain no diagnostic
  remnants (no `eprintln!`, `dbg!` or diagnostic markers). Byte identity with B2 cannot be
  checked independently, because no frozen B2 copy exists.

### Deviations: all accepted

- **DV-1:** correct. The production `MessagePublicationDirectory` authorizes the publisher and
  returns `message_builtins`. Provisioning holds the company lock with the requester
  authorized, so it is equivalent.
- **DV-2:** harmless. Idempotency rests on the receipt and the command record.
- **DV-3:** acceptable. It matches `/run/id`, and the step input schema validates the value at
  activation.
- **DV-4:** an improvement.
- **DV-5:** equivalent or stronger. It decides only on committed slugs and covers aliases,
  without depending on error wrapping.
- **DV-6:** scope only.
- **`apply(subject)`:** fine.

### Carried findings, verified fixed

- **F046:** refusals only at named sites; persistence errors propagate; the fault-injection test
  leaves nothing behind.
- **F047:** built-in first; an ambiguous capture is refused; test.
- **F048:** exhaustive `source_key` match, failing closed; unit and DB tests.
- **F035:** fixed. **F049** items 1–3 adopted.

### Requester and authority, rechecked in code

- The run actor is checked in `prelock_on` and `lock_current`, the requester in
  `authorize_requester`. The F036 run-as and attribution are asserted in both the dispatch and
  the runtime tests.
- The `application` origin carries the authenticated account's address on every
  `TrustedApplication` path (`ui.rs:1395-1483`, `:1599-1634`, `channel.rs:1158-1176`).
- The emailed admin and the in-app member are refused end to end.

### New findings

- **WF-P08-F051 (Medium):** the address is `{slug}@{company}`, missing `.{app_domain}`. It
  reaches the receipt, the HTTP body, the command record and the card, and no test asserts it.
- **WF-P08-F050 (Low):** the `list_company_agents` receipt has no byte bound against the
  64 KiB result cap, and truncation is not reported.

### Checks on the phase cluster (55480, `workflow08_test`)

- **Broad `cargo test --lib`:** 2775 passed, 0 failed, 22 ignored (`logs/r011-broad.log`).
- **Repeats:** the test binary ×5 on `provisioning assistant_channel schedule_tests`, 42 passed
  and 1 ignored each time (the HydraDB live smoke test, which needs its credentials)
  (`logs/r011-repeat-{1..5}.log`).
- **`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`:** clean
  (`logs/r011-clippy.log`).
- **Not re-run:** I relied on A's stack-budget and scratch-schema logs.

### Outcome

**CHANGES REQUIRED** for B3a at digest
`9bdad3dae00d010689c5660c3c9a0c8895145079ea6c1989cc3dea27287d1f3c`.

- WF-P08-F051 (Medium) and WF-P08-F050 (Low) are open.
- The rest of B3a is accepted, including DV-1..DV-6 and F035/F046–F049.
- A recheck can be limited to the deltas for F050 and F051.


## WF-P08-R012

B3a correction recheck for F051 and F050. **Status: COMPLETE — PASS**. Opened and closed
2026-10-23. Expected digest `22334ec4…77e1`.

### Request and artifact

- **Artifact:** base/HEAD `da868710` plus the working tree.
  - Digest `22334ec4414b79d0a68f8d052a3c787fca241611b8afa44e5bcd24ca7a4e77e1` (manifest
    `logs/b3a-snapshot-r2.txt`, the four usual exclusions), re-verified before returning.
  - Coordination r8 is unchanged.
- **Correction record:** TASK "B3a corrections for WF-P08-R011".

### F051: fixed

- `ProvisioningDeployment` carries `app_domain`, which `WorkflowEngine::new` passes
  (`infra/workflow.rs:209`).
- `provision_on` builds the address with
  `Channel::address_for(ChannelSlug, CompanySlug, app_domain)`
  (`persistence/workflow/provisioning.rs:694-699`).
- The exact address is asserted in four tests: dispatch, production-engine
  (`d.address("helper")`), route, and library copy.

### F050: fixed

- **Field bounds.** Names are cut to 120 chars and descriptions to 300, matching `maxLength` in
  the output schema. Rust `chars()` counts the same code points as JSON Schema `maxLength`.
- **Byte budget.** Every entry is admitted against a 48 KiB budget, measured on compact JSON
  as `canonical` measures it. Models go first, then agents by slug, stopping at the first entry
  that does not fit.
  - Entries plus envelope therefore stay under the 64 KiB result cap by construction.
  - The listing is about 2,000 nodes, well under `canonical`'s 4,096-node limit.
- **Truncation reported.** `LIMIT 201` detects more than 200 agents, and the output has a
  required `truncated` field.
- **Test:** the oversized-directory test covers commit, the truncation flag, the size bound and
  the field cuts.

### Checks on the phase cluster (55480, `workflow08_test`)

- **Focused set:** 36 passed, 1 ignored (the HydraDB live smoke test) (`logs/r012-focused.log`).
  - Command: `cargo test --lib -- provisioning assistant_channel agent_api_tests
    infra::workflow routes::ui_agents`.
- **Repeats:** the test binary ×3 on `provisioning assistant_channel library_builder_copy`, 25
  passed and 1 ignored each time (`logs/r012-repeat-{1..3}.log`).
- **`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`:** clean
  (`logs/r012-clippy.log`).
- R011's broad run (2775 passed) stands: the delta is the rename, the address construction and
  the listing bound.

### Outcome

**PASS** for B3a at digest `22334ec4414b79d0a68f8d052a3c787fca241611b8afa44e5bcd24ca7a4e77e1`.
No blocking finding is open for B3a, and there are no new findings.


## WF-P08-R013

B3b-1 batch review (workflow JSON routes, decision page, WF-C003 tool routes and agent grants).
**Status: COMPLETE — CHANGES REQUIRED**. Opened and closed 2026-10-23 (resumed once after a usage
interruption; the coordinator confirmed the digest unchanged in between).

### Request and artifact

- **Artifact:** base/HEAD `da868710` plus the working tree.
  - Digest `22923f7f5c2c0e626ae535f8452c946f1e1c1028a3d859fd41b7f11484792c20` (manifest
    `logs/b3b1-snapshot.txt`, the four usual exclusions; delta files `logs/b3b1-files.txt`),
    re-verified before returning.
  - Coordination r8 (`0950b907…5e22`) is unchanged.
- **Requirements:** plan 08; TASK "B3b plan (2026-10-23)" and "B3b-1 implementation log"
  (DV-7..DV-10); gate D2 rev 2 (§D2.0, §D2.1, invariants); coordination WF-C002, WF-C003 r1,
  WF-C004 r2; AGENTS.md, `src/AGENTS.md`, `src/adapters/http/AGENTS.md`,
  `pages/AGENTS.md`, persistence `AGENTS.md`.
- **Out of scope:** action reconciliation (B3b-2, gate P08-D3), and the root draft
  `logs/decision-D3-r1-draft.md`.

### Coverage

- **Routes read in full:** `routes/workflows/{mod,definitions,bindings,decisions,tools}.rs`, the
  four test modules and the fixture (`tests.rs`), `pages/workflow_decision.rs`.
- **Supporting code:** `application/workflow/{reads,authorization,decisions,service}.rs`,
  persistence `workflow/{reads,workflow_tools,decisions,controls,control_retry,
  admission_binding,agent_preparation,agent_publication,checkpoints,checkpoint_*,token_budget,
  http_publication,tool_publication}.rs`, lifecycle `definitions.rs` and `contracts.rs`,
  `registry/actions.rs`, `infra/workflow.rs` accessors, `app_state.rs`, `routes/mod.rs`,
  `infra/app.rs`, `security.rs`.

### Points the coordinator asked to challenge

1. **Authorization and tenancy.**
   - Every read calls `authorize(.., Company)` before reading, then checks the record's
     association. Writes go to the owning commands, which authorize.
   - Missing, foreign and hidden records all answer a non-disclosing 404. The `Visibility`
     memo propagates any error other than `NotFound`.
   - DV-7 is accepted. The page resolves the company from the decision row, and
     `read_decision` then authorizes.
   - A stranger on the page gets 404, and so does a stranger's UI submit or comment: the
     re-read in `section_after` fails with `?`. Decision ids are random v4, so the id-only
     lookup is unambiguous in practice. Its cost is in F053.
   - Gap: no cross-tenant test for the comment and UI mutation routes (F058).
2. **Keyed identities.**
   - `keyed_id` is domain-separated (domain ‖ `\0` ‖ 16 company bytes ‖ key). The publish
     domain carries the workflow id, so the same key on another workflow derives another
     version; the company-scoped key lookup then refuses it 409 through `equivalent`.
   - Template copy replays only when the keyed draft's `template_origin` names the same
     template, and the copy command authorizes before the conflict.
   - Run start uses B2's `u:` namespace.
   - Gap: a publish replay ignores the HTTP/MCP selections (F055).
3. **Grant protocol (DV-8).**
   - Compare-and-set against `expected` takes company `FOR NO KEY UPDATE`, then agent
     `FOR UPDATE`. No path inverts that order:
     - agent preparation takes agent `FOR SHARE`, preceded only by `prelock_reviewers`'
       company `FOR SHARE`, so the order is company → agent;
     - the general agent update locks only the agent row and leaves `workflow_tools`
       (`COALESCE`);
     - publication capture runs after `authorize_company`.
   - Bounds: 32 distinct names.
   - The competing test is real: two spawned tasks, one 200 and one 409.
   - No name check at grant time. Late binding to registrations is consistent with P07-Q2, but
     reserved provisioning names are grantable (F057). Protocol notes are in F061.
4. **DV-9 is accepted.** The route calls the same `PostgresPersistence::revoke_workflow_tool`
   that p07's `workflow_tool_revocation_tests` exercise, and those ran green here. The route
   test covers the mapping (stale 409, 204, then 404) and the emptied listing. That meets
   WF-C003's "p08 route tests (authority, revision conflict, revoke)".
5. **Decision forms.**
   - `section_after` turns 400/409/404 into a banner. Other errors propagate to the shared
     toast; htmx does not swap them.
   - CSRF: global same-origin validation applies to cookie-auth POST/PUT/DELETE
     (`security.rs:39-48`).
   - Busy feedback: `hx-disabled-elt` and a spinner.
   - Escaping: text and attributes use the right encoders.
   - Minor markup and feedback notes are in F060.
6. **Status mapping.**
   - The mapping is sound: `Diagnostic` → 422 JSON, `LifecycleError`/`TemplateError` mapped,
     cancel `Applied|AlreadyTerminalOrApplied` → 200.
   - The retry `Unsafe` message is wrong for most of the cases it covers (F056).
   - No handler instruments; `TraceLayer` records method, uri and version only, and no body.
7. **WF-C002.**
   - Children are labelled (`parent_run_id`, `rerun_of`).
   - The planned "child is controlled through its root" rule is neither implemented nor
     recorded (F056).
   - No test has a runtime child.
8. **WF-C004 r2:** `grep -rn "PgPoolOptions::new()" src` shows only guarded pools, the
   regression test, the documented `lease_resolution` pool and a lazy pool
   (`thread/read_tests.rs`, unchanged since base).

### Additional defects found

- **F054 (Medium):** `POST …/validate` passes no HTTP/MCP selections and its body cannot carry
  them, so every draft with a `tool.call`/`mcp.call` on a company operation gets a false 422
  `registry.tool`.
- **F052, F053 (Low):** listings silently cap at 200 and filter visibility after the cap.
  Per-thread visibility is a query loop, and the id-only decision lookup has no supporting
  index or recorded evidence.

### Checks on the phase cluster (55480, `workflow08_test`, `SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --lib routes::workflows` | 8 passed | `logs/r013-routes.log` |
| same ×3 | 8 passed each | `logs/r013-routes-repeat.log` |
| `cargo test --lib -- workflow_tool decision_tests` (includes p07 `workflow_tool_revocation_tests`) | 59 passed | `logs/r013-focused.log` |
| `scripts/stack-budget.sh -- routes::workflows` | 8 passed at 2048 KiB | `logs/r013-stack-budget.log` |
| `cargo clippy --lib --tests -- -D warnings` | clean | `logs/r013-clippy.log` |
| `cargo fmt --check` | clean | `logs/r013-fmt.log` |
| psql on `workflow08_test`: `pg_indexes` for runs/definitions/bindings; `EXPLAIN ANALYZE` of an id-only lookup on a rolled-back 200k-row temp table shaped like `workflow_decisions` (PK `(company_id,id)`) | Seq Scan, 199,999 rows removed | inline (F053) |

The full suite was not run, as instructed.

### Findings

- **Medium:** WF-P08-F054.
- **Low:** WF-P08-F052, F053, F055, F056, F057, F058.
- **Info:** WF-P08-F059, F060, F061.
- **Deviations:** DV-7, DV-8 (with the F061 note), DV-9 and DV-10 are accepted.

### Outcome

**CHANGES REQUIRED** for B3b-1 at digest
`22923f7f5c2c0e626ae535f8452c946f1e1c1028a3d859fd41b7f11484792c20`.
- F054 (Medium) and F052, F053 and F055–F058 (Low) block.
- F059–F061 are optional.
- A recheck can cover the correction deltas, with the route tests, clippy and fmt.

## WF-P08-R014

B3b-1 correction recheck for F052–F061. **Status: COMPLETE — CHANGES REQUIRED**. Opened and
closed 2026-10-23.

### Request and artifact

- **Artifact:** base/HEAD `da868710` plus the working tree.
  - Digest `b075aa52c21ccd966c2372c3a6685b4ee7eeb91032942eda364683fb3ff711ef` (manifest
    `logs/b3b1-snapshot-r2.txt`, the four usual exclusions), verified at the start and again
    before returning.
  - Coordination r8 (`0950b907…`) is unchanged.
- **Correction record:** TASK "B3b-1 corrections for WF-P08-R013 (2026-10-23)". Files changed
  in this round: `logs/r013-fix-files.txt`, 13 files including the init schema (three indexes).
- **Read:**
  - every changed file: routes `mod`, `definitions`, `bindings`, the fixture and the four test
    modules, `pages/workflow_decision.rs`, persistence `reads.rs` and `workflow_tools.rs`,
    `application/workflow/reads.rs`, and the schema diff;
  - `logs/r013-fix-explain.log`.

### Per finding

- **F052 fixed.** Keyset pages:
  - definitions `(title, id)`; bindings `id`; runs `(created_at, id)` descending, with a unique
    tie-breaker;
  - `{items, next}`, with `next` from the scanned rows; a stale cursor or a limit outside
    1..=200 is 400;
  - detail lists keep the newest 200, with `*_truncated`.
  - Notes in F064.
- **F053 fixed.**
  - The thread → channel reduction matches the authorizer:
    - the `(company_id, channel_id, thread_id)` FKs on runs and bindings reference
      `threads(company_id, channel_id, id)` with NO ACTION, checked in `pg_constraint`;
    - associations are immutable (`workflow_runs_preserve_association`);
    - the authorizer's thread step adds only existence and channel match.
  - There is a working-set threshold log (>32 checks).
  - The indexes exist in the recreated `workflow08_test` (`pg_indexes`). The before/after plans
    show the intended index use; the probe-table caveat is in F064.
- **F054 fixed.** `ValidateBody` carries the selections. The test shows 422 without the
  selection and 200 with it.
- **F055 fixed.**
  - The selections are serialized into the keyed version id, so the same key with other
    selections derives another version, which `equivalent` refuses (409, tested).
  - A reordered list also counts as a different request. That is acceptable, because a retry
    resends its body.
- **F056 fixed.**
  - `controllable_run` authorizes the company and the association before naming the parent.
    `parent_run_id` is immutable (`workflow_parent_immutable`), so there is no TOCTOU between
    the read and the command.
  - The test covers the list label, the refusals naming the root, retry of a succeeded root
    (neutral message) and a stranger → 404.
  - Coordinator challenge, placement: accepted for the current ingress, since the route is the
    only production caller. The stated reason does not hold, though: the cascade is
    persistence `cancel_tree_on`, and p07's operator child cancel calls the persistence
    transition. The rule could move into the service (F063, Info).
- **F057 fixed.**
  - The grant refuses to add a provisioning name. An agent already holding them may keep or
    drop them.
  - Coordinator challenge: invariant 6 holds without amendment.
    - Created agents never hold provisioning names (D2.1 `tools` refusal).
    - The library Builder copy is the only writer of them.
    - The route now cannot add them anywhere, which is stricter than the invariant.
    - A Builder that drops them gets them back only by recopying. Acceptable.
- **F058 fixed.** Strangers on both paths and a member are refused 404 on the JSON comment, UI
  submit and UI comment, and no comment row is written.
- **F059 fixed.** Explicit 2 MiB `DefaultBodyLimit`, sized from the 256 KiB source cap.
- **F060 partly fixed.**
  - `fieldset`/`fieldset-legend` exist in the bundle, and the UI submit success path is tested.
  - The new busy status uses `[.htmx-request_&]:inline-flex`, which is not in the committed
    `assets/app.css`, so it never shows, and CI's `npm run check:css` would fail (F062, Low).
- **F061 fixed.** If the current list already equals the requested one, the grant answers 200;
  this is checked after authorization and before the expected compare.

### Also checked

- **`own_database`.** The fixture takes a migrated database of its own, with the guarded pool
  (`pool_options`, WF-C004). This removes the cross-test claim deadlock the author reports
  (`logs/r013-fix-routes-2.log`).
- **New `UNIQUE (id)` on `workflow_decisions`.** The two tests that insert decisions directly
  (`action_approval_sql_tests`, `decision_provenance_tests`) use fresh ids, and both ran green.
- **CSS bundle.** Only one Tailwind utility in the changed page and route files is missing from
  `assets/app.css`, the one F062 names; the others are custom classes from the inline styles.

### Checks on the phase cluster (55480, `workflow08_test` recreated, `SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --lib routes::workflows` | 11 passed | `logs/r014-routes.log` |
| same ×3 | 11 passed each | `logs/r014-routes-repeat.log` |
| `cargo test --lib -- adapters::persistence::tests workflow_tool decision_tests child_runtime_tests action_approval_sql_tests decision_provenance_tests` | 76 passed | `logs/r014-focused.log` |
| `scripts/stack-budget.sh -- routes::workflows` | 11 passed at 2048 KiB | `logs/r014-stack-budget.log` |
| `cargo clippy --lib --tests -- -D warnings`, `cargo fmt --check` | clean | `logs/r014-clippy.log`, `logs/r014-fmt.log` |
| psql: `pg_indexes` (three new indexes present), `pg_constraint` (thread FKs NO ACTION) | as stated | inline |

- **Not run:** `npm run check:css`, because Tailwind is not installed here (no
  `node_modules`). F062 rests on the bundle contents and the build definition.
- The full suite was not run, as instructed.

### Outcome

**CHANGES REQUIRED** for B3b-1 at digest
`b075aa52c21ccd966c2372c3a6685b4ee7eeb91032942eda364683fb3ff711ef`.
- F052–F059 and F061 are fixed, and F060 is partly fixed.
- New: WF-P08-F062 (Low) blocks; WF-P08-F063 and F064 are Info.
- A recheck can cover the F062 delta: the page markup, or a rebuilt `assets/app.css`.

## WF-P08-R015

B3b-1 correction recheck for F062, with the F063 and F064 dispositions. **Status: COMPLETE —
PASS**. Opened and closed 2026-10-23.

### Request and artifact

- **Artifact:** base/HEAD `da868710` plus the working tree.
  - Digest `57566f926c9396e4d9b3eae72e5d6b46d74e5c8b3455d6fca912aa50d18ddd33` (manifest
    `logs/b3b1-snapshot-r3.txt`, the four usual exclusions), verified at the start and again
    before returning.
  - Coordination r8 (`0950b907…`) is unchanged.
- **Correction record:** TASK "B3b-1 corrections for WF-P08-R014".
- **Delta:**
  - the r2 and r3 manifests list the same files;
  - by mtime, only two manifest files changed since R014: `pages/workflow_decision.rs` and
    `routes/workflows/bindings.rs`.

### F062: fixed

- The status span is now `hidden text-sm [.htmx-request_&]:inline` with `role=status`
  (`workflow_decision.rs:67-69`).
- That variant is compiled in `assets/app.css`. Eleven other pages already use it.
- Every `[.htmx-request_&]` variant in `src` is now one the bundle compiles: `hidden`,
  `inline`, `inline-block`, `opacity-80` and `pointer-events-none`.
- My re-scan of every class attribute in the changed HTTP files against the bundle finds no
  missing Tailwind utility; the only gaps are known custom classes and the inert
  `form-control`.
- The span is a descendant of the form htmx marks `htmx-request`, so it shows while the
  request runs, as the comment form's "Posting…" does.
- `npm run check:css` and a visual check were not run (no `node_modules`). TASK records this
  and I do not claim them.

### F063: resolved by disposition

- The doc comment now names `cancel_tree_on` as a persistence transition, and says the route is
  the only production caller of the service's cancel/retry and that a later ingress must apply
  the rule.
- TASK records the move into the service for whichever phase adds another ingress; p09's UI uses
  these routes. Accepted.

### F064: accepted as recorded behaviour

- The three notes are recorded in TASK. Accepted.

### Checks on the phase cluster (55480, `workflow08_test`, `SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --lib -- routes::workflows pages::tests` | 169 passed, 1 ignored | `logs/r015-tests.log` |
| `cargo clippy --lib --tests -- -D warnings`; `cargo fmt --check` | clean | `logs/r015-clippy-fmt.log` |
| class scan of the changed HTTP files against `assets/app.css` | none missing | inline |

- **Not run:** `npm run check:css` (Tailwind not installed) and the full suite, as instructed.
  R014's route, focused and stack-budget runs stand, because the delta is markup and a doc
  comment.

### Outcome

**PASS** for B3b-1 at digest `57566f926c9396e4d9b3eae72e5d6b46d74e5c8b3455d6fca912aa50d18ddd33`.
- WF-P08-F052 to F062 are fixed.
- F063 is resolved by disposition, and F064 is accepted.
- No blocking finding is open for B3b-1.
- Carry to the final gate: run `npm run check:css`, because CI does.

## WF-P08-R016

Architecture gate P08-D3 rev 1 (B3b-2: production action-reconciliation composition and route).
**Status: COMPLETE — CHANGES REQUIRED** (gate not passed). Opened and closed 2026-10-23.

### Request and artifact

- **Decision:** `logs/decision-D3-r1.md`, sha256
  `121779fc3c94826a41c2b1a2d2413fa4b70d15643b468ee513cc7e60508625d4` (verified), with TASK
  "Architecture gate P08-D3".
- **Facts read at:** worktree digest `57566f92…dd33` (unchanged since R015, re-verified).
- **Coordination:** r9 (`e636593b…`). It records WF-C003's p08 verification; no instruction
  changed.

### Sources read

- **Plan and gates:**
  - plan 08 "Public interfaces";
  - D2 rev 2 §D2.0 and §D2.2a items 1–4, and invariants.
- **Application:** `actions/reconciliation/{service,contracts}.rs`.
- **Persistence reconciliation:** `action_reconciliation/{mod,settlement,facts,delivery}.rs`.
- **Persistence resources and dispatch:**
  - `message.rs` (both `MessageActionAuthority` impls);
  - `resources.rs` (`inspect_on`, the `ResourceDirectory` impl);
  - `http.rs` (`connection_on`, `edit_on`, `status_on`, `HttpActionAuthority`, `http_target`);
  - `http_runtime.rs` (target construction), `http_publication.rs`;
  - `action_dispatch.rs` (`authorize_on`), `action_authority.rs` (`prelock_on`, `load_on`);
  - `actions/authorization.rs` (`validate_resource`, `validate_run_authority`).
- **Test-only resource doubles:** `action_reconciliation_authority_support.rs`,
  `action_reconciliation_snapshot_tests.rs`, `mcp_reconciliation_tests.rs`.
- **Error mapping:** `app_error_impl.rs`.
- **Schema (`workflow08_test`):**
  - evidence and command indexes;
  - the `workflow_action_evidence_revision` trigger, which calls `advance_workflow_owned_revision`.

### Challenge points

1. **D3.1.**
   - The command actor, not the frozen actor, is right. It matches the trait
     ("recording past truth requires access") and the service's `inspect` under the command
     actor.
   - Provisioning failing closed is right. Its markers always commit with a receipt
     (`effect_kind='local'`), and the route's targets name only `remote` and `message`.
   - Lock order is confirmed and simpler than stated. Dispatch (`prelock_on`), reconciliation
     (each transaction) and connection edits (`edit_on`) all take the company row
     `FOR NO KEY UPDATE` first, so they serialize there (F070.4).
   - **But** dispatching on the target variant refuses every unslotted HTTP action, which
     freezes as `Local { http }` (F065, Medium).
   - Current readiness is also required, which the failure modes omit (F068).
2. **D3.2 is accepted** for p08. The plan requires the route over the command, not resumption,
   and the prior rule forbids replay on operator claims.
   - I verified annotation only: an `UnknownNote` on a remote or message target records
     `UnknownRecorded` and changes no delivery, job or run state, apart from the revision.
   - Notes in F070.1.
3. **D3.4 is not sufficient** (F067): no delivery status, no receipt presence, and `local`
   markers are listed. Its bounds are fine: 200 markers × ≤ 20 destinations
   (`MAX_MESSAGE_DESTINATIONS`).
4. **D3.5.**
   - The child rule is accepted (F070.3).
   - Error mapping: the decision conflicts with the existing command, where conflicts are `Ok`
     outcomes, some of them durably recorded (F066, Medium).
   - `Timeout` → 500 (F070.2).
5. **D3.6–D3.8.**
   - Invariants 1, 2, 4 and 5 hold against the code.
   - Invariant 3 holds, but the refusal is an `IdempotencyConflict` outcome (F066).
   - D3.7's concurrency statement is imprecise: a competing claimant gets
     `Blocked { StaleSnapshot }` or `RevisionConflict`, depending on the interleaving, because
     each evidence insert advances the run revision (F066).
   - The verification plan does not exercise the new composition on every target or the exact
     competing outcomes (F069).

### Checks

- This is a design review; no code exists for B3b-2.
- psql on `workflow08_test` (read-only): `pg_indexes` and `pg_trigger` for
  `workflow_action_evidence` and `workflow_action_evidence_commands`, and the
  `advance_workflow_owned_revision` source.
- No tests run.

### Findings

- **Medium:** WF-P08-F065, F066.
- **Low:** WF-P08-F067, F068, F069.
- **Info:** WF-P08-F070.

### Outcome

**CHANGES REQUIRED.** P08-D3 rev 1 (`121779fc…25d4`) is not passed.
- F065 and F066 (Medium) and F067–F069 (Low) block.
- F070 is optional.
- A rev 2 recheck can cover the deltas.

## WF-P08-R017

Gate recheck P08-D3 rev 2, which answers R016 (F065–F070). **Status: COMPLETE — PASS**. Opened
and closed 2026-10-23.

### Request and artifact

- **Decision:** `logs/decision-D3-r2.md`, sha256
  `0cacf996a4a2d6a995938a8c9c7cc6deab6e1459ccd9d6f9716f26a7748f558c` (verified). I read it in
  full. The TASK "Architecture gate P08-D3" dispositions match it.
- **Facts at:** worktree digest `57566f92…dd33` (re-verified), coordination r9 (`e636593b…`).

### Per finding

- **F065 resolved.**
  - D3.1 now decides by `resource_kind` in either form. That matches `http_target`
    (`http.rs:313-326`) and the unslotted `Local { http }` freeze (`http_runtime.rs:41-51`).
  - MCP is always `Connection` (`mcp_runtime.rs:35`, `mcp.rs:143`).
  - Provisioning and any other kind get NotFound.
- **F066 resolved.**
  - The table matches the command: outcomes are results; refusals are recorded under the key
    (`record_refusal_on`); `idempotency_conflict` is not recorded.
  - Checked against the code:
    - a stale `VerifiedReference` → `revision_conflict` (snapshot records it before the service's
      missing-verifier check);
    - a fresh one → 400 and no row;
    - the fresh-key rule holds, because the digest includes the revision and the evidence.
  - D3.7's races are exact:
    - same key: the company lock serializes; the second replays in `snapshot` or `settle`
      `replay_on`;
    - different keys: `revision_conflict` or `blocked { stale_snapshot }`, because each evidence
      insert advances the revision.
  - The remaining precision issues are F071 (carried) and F072.
- **F067 resolved.**
  - `has_receipt`, `reconcilable`, and destination `status`/`parts` are added.
  - `local` markers are dropped, and `evidence_conflict` is computed once per execution.
  - The listing no longer depends on whether the execution is complete.
- **F068 resolved by decision.**
  - Readiness stays (the 04.x contract): 404, and cancel is the path; revisit when a verifier is
    added.
  - The trait doc is corrected in B3b-2, and readiness tests are planned.
- **F069 resolved.**
  - Every test goes through the production composition.
  - It covers the five target forms; exact outcomes, rows and revisions; both races; readiness;
    MCP; and a stated fallback that reports the gap.
- **F070 adopted.**
  - Timeout → 503 with `Retry-After: 1` on the workflow routes. In the workflow routes,
    `AppError::Timeout` comes only from reconciliation, which is idempotent by key.
  - p09 notice at acceptance; cancel and rerun are stated.

### New

- **F071 (Low, carried to B3b-2).**
  - Readiness and access are checked before replay, so a replay needs current access.
  - A timeout during `COMMIT` is not a rollback. After a 503, retry with the same key, which is
    the opposite of the 409 rule.
  - Both must be stated in the route docs and the p09 notice, with a replay-after-disable test.
- **F072 (Info).**
  - `evidence_conflict` is returned after the note is recorded.
  - The competing test's revision arithmetic should count evidence rows.
  - The destination flag is narrower than the command's subject check.

### Checks

- Design recheck; no code.
- The cited code paths were re-read: `action_reconciliation/mod.rs` (snapshot order,
  `replay_on`), `settlement.rs`, `facts.rs`, `reconciliation/service.rs` (budget and ordering),
  `app_error.rs` (`Timeout` vs `DatabaseTimeout`), and the `AppError::Timeout` producers in the
  workflow paths.

### Outcome

**PASS** for P08-D3 rev 2
(`0cacf996a4a2d6a995938a8c9c7cc6deab6e1459ccd9d6f9716f26a7748f558c`).
- F065–F069 are resolved and F070 is adopted.
- Carried into the B3b-2 batch review: WF-P08-F071 (Low).
- WF-P08-F072 is optional.

## WF-P08-R018

B3b-2 batch review (production action-reconciliation composition and route). **Status: COMPLETE
— PASS**. Opened and closed 2026-10-23.

### Request and artifact

- **Artifact:** base/HEAD `da868710` plus the working tree.
  - Digest `4156165b358161367b1822179edbde0df23aa64d1ac6148e04797dbed68e20b3` (manifest
    `logs/b3b2-snapshot.txt`, the four usual exclusions), verified at the start and again before
    returning.
  - Coordination r9 (`e636593b…`). No schema change.
- **Contract:** D3 rev 2 (`0cacf996…558c`, R017 PASS), plus the carried F071 (Low) and F072
  (Info). Implementation record: TASK "B3b-2 implementation log (2026-10-23)", DV-11..DV-13.
- **Read:**
  - `action_reconciliation/resources.rs` and the trait-doc diff in `action_reconciliation/mod.rs`;
  - the `MESSAGING` visibility change in `message.rs`;
  - `WorkflowEngine::reconciliation()`;
  - application `reads.rs` (`ActionSummary`, `DestinationSummary`, the hints) and persistence
    `reads.rs` (`actions`);
  - `routes/workflows/reconciliations.rs`, the `mod.rs` 503 branch, and the `bindings.rs` run
    JSON;
  - tests: `reconciliation_fixture.rs` (child of `http_tests`, re-exported under `cfg(test)`),
    `reconciliation_tests.rs`, the two production-composition tests, and the
    `resource_route` unit test.

### Against D3 rev 2

- **§D3.1.**
  - `resource_route` decides by kind in either form: http in both forms, mcp `Connection`,
    messaging `Local`. Everything else, including `Local mcp`, `Connection messaging` and
    provisioning in both forms, is NotFound.
  - The refusal is the first statement of `lock_resource`, before any DB access.
  - Connections take `authorize_company`, then `inspect_on`; messaging delegates.
- **§D3.2a.** The trait doc now states that readiness is required.
- **§D3.3.** `reconciliation()` composes `PostgresActionReconciliation<CompanyReconciliationResources>`,
  `ProductionAuthorizer`, `PostgresPersistence` and verifier `None`. The route uses a fresh
  token, a 5 s budget, and a boxed call. Timeout → 503 with `Retry-After: 1`.
- **§D3.4.** The actions read covers `remote` and `message` dispatches, newest first, with
  201 fetched to report `actions_truncated`.
  - `has_receipt` comes from a per-invocation `EXISTS` on the receipts PK `(company_id,
    invocation_id)`; one dispatch per invocation, so this equals per marker.
  - `evidence_conflict` is computed once per distinct execution through `unnest`.
  - Destinations are delivery-backed through a JOIN, so in-app destinations are excluded, with
    status and a parts `count(*)`. The query uses the `(company_id, invocation_id, …)` and
    `message_delivery_parts(delivery_id, part_index)` indexes, checked in `pg_indexes`.
  - Query cost is bounded: 201 PK probes, ≤ 201 function calls, and ≤ 201 × 20 destination rows
    with an index-only part count. The dispatch scan is the run's own dispatches through
    `(company_id, run_id, …)`.
  - The hints match the rev 2 definitions.
- **§D3.5.**
  - The body maps `delivery_id` and passes `EvidenceInput` through.
  - `ArgumentDigest` validates on deserialize.
  - The status table matches rev 2.
  - The module doc carries both F071 client rules and the readiness/replay condition (F071
    fixed).
  - No `controllable_run`, as decided.
- **Tests.**
  - Both HTTP forms record, replay and leave the run waiting.
  - Every row of the status table is covered, with rows asserted.
  - Readiness: a replay and a new note after disabling → 404, nothing recorded; after
    re-enabling → 200.
  - Tenancy: a stranger on both company paths and a member → 404 (the stranger's own company
    path with this run's id is the HTTP rule's "another tenant's id").
  - Races: different keys and the same key.
  - MCP and messaging through `engine.reconciliation()`.
  - All five forms on the pure router.
- **DVs.**
  - **DV-11 accepted.** The refusal precedes all DB access, and this was rev 2's stated fallback.
  - **DV-12 accepted.** Same service and composition; the route mapping is exercised by HTTP.
  - **DV-13 accepted.** The association checks are the unchanged service and persistence
    code, and their refusals are covered by the existing reconciliation authority suite.

### Coordinator questions

- **Race test.** Tight enough: the either/or is the contract's (D3.7), and evidence = 1 and
  revision + 1 are pinned. Optional tightening is in F073.1.
- **Discovery bounds and cost.** Bounded, and indexed as above.

### Checks on the phase cluster (55480, `workflow08_test`, `SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --lib -- routes::workflows production_composition reconciliation_resources` | 20 passed | `logs/r018-new.log` |
| `cargo test --lib -- action_reconciliation mcp_reconciliation message_reconcile provisioning_dispatch` | 267 passed | `logs/r018-focused.log` |
| `cargo test --lib -- reconciliation_tests` ×8 | 9 passed each | `logs/r018-repeat.log` |
| `scripts/stack-budget.sh -- routes::workflows production_composition reconciliation_resources` | 20 passed at 2048 KiB | `logs/r018-stack-budget.log` |
| `cargo clippy --lib --tests -- -D warnings`; `cargo fmt --check` | clean | `logs/r018-clippy-fmt.log` |
| psql `pg_indexes` on dispatches, receipts, destinations and delivery parts | as stated | inline |

The full suite was not run, as instructed.

### Outcome

**PASS** for B3b-2 at digest `4156165b358161367b1822179edbde0df23aa64d1ac6148e04797dbed68e20b3`.
- F071 is fixed and F072 is adopted.
- New WF-P08-F073 (Info) is optional.
- No blocking finding is open for B3b-2.

## WF-P08-R019

Architecture gate P08-D4 rev 1 (B4: retargeted surfaces, live updates, post-removal schema).
**Status: COMPLETE — CHANGES REQUIRED** (gate not passed). Opened and closed 2026-10-23.

### Request and artifact

- **Decision:** `logs/decision-D4-r1.md`, sha256
  `2f46c343954fd586bd83bc6e3298069690feef51d93086d5cfa33ece6387647d` (verified).
- **TASK sections:** "B3b-2 acceptance and B4 inputs" (P08-Q6..Q8), "B4 plan" (B4a–B4d),
  "Architecture gate P08-D4".
- **Inputs used as leads:** `logs/b4-inventory.md` and `logs/b4-projections.md`.
- **Facts at:** worktree digest `4156165b…20b3` (re-verified), coordination r10 (`1df326af…`).

### Sources verified in code and schema

- **Live updates:** `infra/events.rs` (channels, the `recv()` loop, reconnect) and sqlx 0.8.6
  `PgListener` (`recv` vs `try_recv`).
- **Attention:** `persistence/attention.rs` (`ATTENTION_SQL`: branch shape, `ranked` filter,
  delivery branch).
- **Notifications:**
  - `persistence/notification.rs`: `resolve_projection_on`, `source_snapshot` (task and
    delivery), `recipient_for_source`, `apply_projection_on`;
  - schema `enqueue_actionable_notification_event`, `notification_from_delivery`,
    `notification_from_task_status`, `notify_attention_changed`, `notify_task_chain_changed`,
    `notify_thread_activity`;
  - the `notifications` and `notification_events` indexes and checks.
- **Decisions:** `persistence/workflow/decisions.rs` (`insert_reviewers`, `compose_notices`,
  `eligible_reviewer`, `lock_decision`), `decision_approvals.rs` (`close_attempt`),
  `decision_tokens.rs` (park); `workflow_decision_reviewers` with its guard and constraints.
- **Creation:** `routes/onboarding.rs` (both steps); `provisioning.rs` `AssistantAgent::Existing`
  (the explicit-model refusal); `routes/agent.rs` instruction agents.
- **Schema around channels and runs:**
  - `enforce_enabled_channel_has_active_agent` and its two triggers;
  - `channel_gate` and `channel_assignment.rs` (the gate is exclusive in channel edits);
  - `pg_indexes` for `workflow_runs`, `schedule_runs`, `workflow_message_intent*` and the
    delivery tables.

### Challenge points

1. **D4.1.**
   - One run-level trigger suffices for decisions. All three park paths set `waiting/decision`
     in the transaction that inserts the reviewers, and every submission moves the run.
   - Two gaps:
     - refusals have no run, so they have no live source (F078.2);
     - a LISTEN reconnect silently drops wakes (F077).
   - Mechanics: a WHEN clause cannot read `OLD` on INSERT, and `notify_thread_activity` takes no
     arguments (F083.1).
2. **D4.2/D4.3.**
   - Notifications are one recipient per source, so per-reviewer decision notifications need a
     per-reviewer source identity, and submission and expiry need withdrawal events (F074,
     Medium).
   - The delivery recipient rule is multi-recipient and its scope undefined. The
     `delivery_failure` branch is not unchanged after B4c. PL/pgSQL references to
     `message_deliveries.task_id` would fail only at runtime (F075, Medium).
   - Attention needs a decision row model and two more indexes (F078).
   - The NULL-channel visibility must match the page's privileged rule (F074.3).
3. **D4.3 guard.** A conditional check drops the provable-notice invariant; NULL should be tied
   to a recorded opt-out (F076).
4. **D4.4–D4.6.**
   - Counts are accepted, including dropping the per-agent badges.
   - The dashboard is accepted.
   - Activity is accepted.
   - The message→run lookup and the simulation assume one run per message, but a message admits
     one per binding (≤ 8 per channel) and per channel (F079).
5. **D4.7.**
   - Agent-only creation and the Builder are accepted.
   - Onboarding's channel step would be refused for the agents onboarding creates, which have no
     explicit model (F080, Medium).
   - Plain enabled channels conflict with the active-agent triggers kept until B4c (F081).
6. **D4.8.** `channel_gate` is kept (F083.2). The shared-table end state is accepted, apart from
   the dependent-function enumeration (F075.3).
7. **D4.9–D4.11.** Invariant 4 needs F075. The verification plan misses the per-consumer live
   tests and the cases above (F082).

### Checks

- Design review; no B4 code exists.
- psql (read-only) on `workflow08_test`: `\d workflow_decision_reviewers`, the notification
  indexes and constraints, `\d workflow_message_intent_bindings`, and `pg_indexes` for
  `schedule_runs` and the intent tables.
- No tests run.

### Findings

- **Medium:** WF-P08-F074, F075, F080.
- **Low:** WF-P08-F076, F077, F078, F079, F081, F082.
- **Info:** WF-P08-F083.

### Outcome

**CHANGES REQUIRED.** P08-D4 rev 1 (`2f46c343…647d`) is not passed.
- F074, F075 and F080 (Medium) and F076–F079, F081 and F082 (Low) block.
- F083 is optional.
- A rev 2 recheck can cover the deltas.

## WF-P08-R020

Gate recheck P08-D4 rev 2, which answers R019 (F074–F083). **Status: COMPLETE — PASS**. Opened
and closed 2026-10-23.

### Request and artifact

- **Decision:** `logs/decision-D4-r2.md`, sha256
  `d88aeb869d546d6308e106dddcb66e7274838351c4b2b001629da706027d3bce` (verified). I read it in
  full; the TASK dispositions match.
- **Facts at:** digest `4156165b…20b3` (re-verified), coordination r10 (`1df326af…`).

### Per finding

- **F074 resolved by removing the feature.**
  - There are no in-app decision notifications. Decisions surface through attention My work,
    the decision page, and DecisionNotices e-mail with the preference.
  - That removes both the one-recipient-per-source conflict and the need for withdrawal events.
  - P08-Q8 asked only for the e-mail preference, so this is within the user decisions.
- **F075 resolved.**
  - The single recipient is the run actor through `workflow_message_destinations`.
  - Mailbox sends get attention only, and `company_id IS NULL` notices never notify (their guard
    requires it).
  - The `delivery_failure` branch drops its `background_tasks` join.
  - Every dependent function, trigger and Rust query is listed, with a catalog check and
    post-drop tests. The catalog-check precision note is F085.1.
- **F076 resolved.** `notice_opted_out` with the NULL-equivalence CHECK, and the guard proves the
  opt-out. A race note is in F085.2.
- **F077 resolved for the in-loop reconnect.** I checked sqlx 0.8.6:
  - `try_recv` returns `Ok(None)` only after `connect_if_needed` re-LISTENs
    (`eager_reconnect`), so `Resync` follows the re-subscription;
  - a `pg_terminate_backend` FATAL `ErrorResponse` is ignored by `try_recv`, and the EOF that
    follows takes the `Ok(None)` path, so the planned test exercises it.
  - The outer path (reconnect failure, then backoff, then a new listener) still loses
    notifications without a `Resync`. That is carried as F084.
- **F078 resolved.**
  - One row per decision with defined My work and Team work predicates; a company decision
    follows the page's owner/admin rule.
  - Refusals refresh on the attention SSE's existing 30 s `interval` tick, which yields a
    reconcile (`routes/attention.rs:370-398`), with UI copy saying so.
  - The rerun and `schedule_runs` indexes are added.
- **F079 resolved.** `run_ids`, ≤ 8 per channel intent; the simulation lists every binding.
- **F080 resolved.**
  - Agents are pinned to the default connection, as D2.6 does.
  - There is one assistant channel per agent with a model, under the stable key
    `onboarding:{agent_id}`.
  - Without a model, onboarding stays agent-only, with completion copy and a per-agent refusal
    display.
- **F081 resolved.**
  - B4a drops both triggers and their function.
  - I checked that inbound commit is intent-based and does not rely on `channel_agents`
    (`thread/inbound.rs`).
- **F082 resolved.** Per-consumer live and race tests, trigger and parse tests, and the case
  tests.
- **F083 adopted.** Split triggers; `notify_thread_activity()` reused, since runs carry its three
  columns; the UI copy; a `channel_gate` competing test.

### New

- **F084 (Low, carried to B4a).** Publish `Resync` after the outer listener reconnect, and test a
  failed-then-recovered reconnect.
- **F085 (Info).**
  - Make the catalog check match qualified names.
  - Lock the preference row in `eligible_reviewer`.
  - Base the onboarding replay test on the channel step.
  - Keep the `recipient_for_source` channel join.

### Checks

- Design recheck; no code.
- Re-read: sqlx-postgres 0.8.6 `listener.rs` (`try_recv`, eager reconnect) and
  `connection/stream.rs` (`recv_unchecked`); `routes/attention.rs` (the 30 s tick);
  `thread/inbound.rs` (no agent reliance).

### Outcome

**PASS** for P08-D4 rev 2
(`d88aeb869d546d6308e106dddcb66e7274838351c4b2b001629da706027d3bce`).
- F074–F082 are resolved and F083 is adopted.
- Carried into the B4a batch review: WF-P08-F084 (Low).
- WF-P08-F085 is optional.

## WF-P08-R021

B4a batch review (user-facing surfaces retargeted onto the workflow; legacy entry points cut).
**Status: COMPLETE — CHANGES REQUIRED**. Opened and closed 2026-10-24.

### Request and artifact

- **Artifact:** base/HEAD `da868710` plus the working tree.
  - Digest `5b27364aa4651526e4fec1b3b158e078f80fdfc171cdf0d687bc6d48573c2c16` (the four usual
    exclusions), verified at the start and again before returning.
  - Coordination r10 (`1df326af…`).
- **Delta:** `logs/b4a-files.txt`, 144 files against the B3b-2 snapshot.
- **Contract:** P08-D4 rev 2 (`d88aeb86…3bce`, R020 PASS), carried F084, and F085 notes.
- **Implementation record:** TASK "B4a progress", steps 1–7.

### Coverage (read against D4 rev 2 and the code)

- **Schema:**
  - the four run NOTIFY triggers and `notify_workflow_run_changed()`;
  - indexes `workflow_runs_attention`, `_reruns`, `_open_by_channel`, `_by_thread`,
    `workflow_decisions_open`, `schedule_runs_failed_recent`, `model_usage_calls_run`;
  - `decision_email_enabled`;
  - `notice_opted_out` with its CHECK and the guard arm (NULL ⇔ a recorded
    `decision_email_enabled = false`);
  - the active-agent triggers and function are gone.
- **Live updates:**
  - `infra/events.rs`: `RunScope` with nullable association, a fail-closed parse, `Resync` after
    `listen_all` (first and rebuilt listener, F084) and on `try_recv` `Ok(None)` (F077);
  - every `events.subscribe()` consumer handles `Resync`: `live_updates::wake_ups` maps it to
    `Lagged`, and `attention::wakes_attention` and the notifications SSE reconcile on it.
- **Attention (`ATTENTION_SQL`):**
  - decisions: My work for a current reviewer, with company-associated decisions only for a
    manager (`$14`); Team work one row per decision with "N reviewers";
  - `run_reconciliation`; `run_failure` (7 days, not rerun, actor while a member);
    `binding_refusal` (per binding, both sources, Team work only);
  - `delivery_failure` without the `background_tasks` join, run actor through destinations;
  - Team work is manager-only at the route (`routes/attention.rs:117`);
  - refusal reasons are non-NULL by CHECK, so the concatenated `next_action` cannot be NULL.
- **Notifications:**
  - delivery only, through destination → run → actor principal;
  - `message_deliveries` CHECK guarantees a channel for company deliveries;
  - the legacy sources are withdrawn;
  - links to `/ui/deliveries` and `/ui/work`.
- **Counts:** `RUN_COUNTS_QUERY` over visible channels plus company-wide; the stream is
  manager-only (`load_managed_company`), throttled, and wakes on run changes and `Resync`.
- **Activity, lookup and simulation:**
  - `THREAD_ACTIVITY_SQL` (unfinished first);
  - `THREAD_RUN_LOOKUP_SQL` (writer run, then admitted runs in binding order), company and
    thread scoped;
  - `MessageOutcomeReader`: three bounded, company-scoped reads, with executions capped at 20 and
    shown as "latest 20".
- **Preference:**
  - `eligible_reviewer` reads the preference `FOR SHARE` after member and principal;
    `compose_notices` gives `None` for an opt-out; `insert_reviewers` writes the opt-out;
  - the competing test is real: a toggle holds the row, `pg_stat_activity` shows the park
    waiting, and the park records the committed opt-in.
- **Creation:**
  - onboarding is owner-only; agents are pinned through `pinned_default_model`; one
    `AssistantChannels` call per modelled agent under `onboarding:{agent_id}`; per-agent
    outcomes; the completion GET re-reads stored results;
  - when a later library agent fails, the agents already created in the request are deleted.
- **Deletions and cross-links:**
  - a page-link check against every registered route (literal and `format!` routes): no link
    to a deleted route;
  - no stale `/ui/tasks`, approvals, reviews, handoffs or task-counts references in pages and
    scripts;
  - CI and `package.json` use `test:run-counts`.
- **CSS:** a scan of the class attributes in the changed pages and routes finds only three
  pre-existing exceptions (`text-md`, `library-agent-select`, the JS hook `thread-activity`).

### Coordinator questions

1. **Index changes:** accepted on the evidence (F087.1).
2. **Owner-only simulation:** accepted as the D4.6 rule, but the mailbox still shows "Open in
   Simulator" to every reader and it now dead-ends for non-owners (F086, Low).
3. **Response-contract controls removed:** accepted. The workflow agent step builds its contract
   from the step's own `output_schema` (`application/workflow/agent.rs:100-205`), not from
   `agents.response_contract`.
4. **FOR SHARE protocol:** accepted. The lock order is member, principal, then preference, and
   the toggle path locks only the preference row. Without a preference row no lock is taken,
   but the opt-in path needs none. The race test passed five times out of five.
5. **Transitional items:** acceptable. No user-reachable link targets a deleted route. The
   legacy `/approvals/{token}` formatter has no reachable producer (F087.5). The one dead
   control is F086.

### Checks on the phase cluster (55480; `DATABASE_URL=…/workflow08`, so tests use `workflow08_test`; `SQLX_OFFLINE=true`, `CARGO_INCREMENTAL=0`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --lib -- attention notification run_count dashboard thread_activity message_outcome onboarding infra::events live_updates ui_stream simulation workflow_run_triggers preference decision_tests profile` | 157 passed | `logs/r021-focused.log` |
| `cargo test --lib -- adapters::http` | 334 passed, 1 ignored | `logs/r021-http.log` |
| the preference race and listener tests ×5 | 2 passed each | `logs/r021-race-repeat.log` |
| `scripts/stack-budget.sh -- attention notification run_count thread_activity message_outcome infra::events live_updates ui_stream simulation decision_tests` | 94 passed at 2048 KiB | `logs/r021-stack-budget.log` |
| `cargo clippy --lib --tests -- -D warnings`; `cargo fmt --check` | clean (exit 0) | `logs/r021-clippy-fmt.log` |
| `node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs` | 8 passed | `logs/r021-node.log` |
| static check of `run-counts.mjs` extraction (both markers present, scripts parse) | ok | inline |

- **Not run:** `npm run check:css`, `npm run test:run-counts` (no Playwright), and the full
  suite.

### Findings

- **Low:** WF-P08-F086.
- **Info:** WF-P08-F087.
- **Carried and notes:** F084 fixed; F085 adopted (item 1 due in B4c).

### Outcome

**CHANGES REQUIRED** for B4a at digest
`5b27364aa4651526e4fec1b3b158e078f80fdfc171cdf0d687bc6d48573c2c16`.
- F086 (Low) blocks; F087 is optional.
- A recheck can cover the F086 delta: the button condition or the page message, plus its page
  test.

## WF-P08-R022

B4a correction recheck for F086. **Status: COMPLETE — PASS**. Opened and closed 2026-10-24.

### Request and artifact

- **Artifact:** base/HEAD `da868710` plus the working tree.
  - Digest `a54400486b9484c469752a35a48bfd207bdd020486e540f10c5591d4f4e720c8`
    (`logs/b4a-snapshot-r2.txt`, the four usual exclusions), verified at the start and again
    before returning.
  - Coordination r10.
- **Correction record:** TASK "B4a corrections for WF-P08-R021".
- **Delta:**
  - the r1 and r2 manifests list the same files;
  - by mtime, only three manifest files changed since R021: `pages/mailbox.rs`, `pages/tests.rs`,
    `routes/ui.rs`.

### F086: fixed

- `MessagePane` gains `viewer_membership`. `render_message_pane` (`routes/ui.rs:330-357`) reads
  it through `principal_access_for_user`, whose owner arm is `company.user_id =
  principal.user_id` (`persistence/participant.rs:533-534`), the same rule as the simulation
  routes. No access maps to `None`.
- `simulator_link` (`pages/mailbox.rs:2163-2174`) renders only for `Owner`.
- `render_message_pane` is the only production constructor of `MessagePane`. All four call
  sites use it (`ui.rs:426`, `:571`, `:1225`, `ui_internal_notes.rs:142`).
- The new page test covers Owner (link and label) against Admin, Member and None (neither).

### F087

- Item 2 is withdrawn: `fetch_thread_runs` de-duplicates in Rust with a `HashSet` retain.
- Items 1, 3 and 4 are accepted as recorded.
- Item 5 goes to B4b.

### Checks on the phase cluster (55480; tests use `workflow08_test`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo test --lib -- adapters::http::pages adapters::http::routes::ui adapters::http::routes::channel` | 233 passed, 1 ignored | `logs/r022-tests.log` |
| `cargo clippy --lib --tests -- -D warnings`; `cargo fmt --check` | exit 0 / 0 | `logs/r022-clippy-fmt.log` |

R021's broader checks stand, since the delta is the pane flag and a page test.

### Outcome

**PASS** for B4a at digest `a54400486b9484c469752a35a48bfd207bdd020486e540f10c5591d4f4e720c8`.
- F086 is fixed and F087 resolved.
- No blocking finding is open for B4a.
- Carried to the end of the phase (F087.4): `npm run check:css` (expect an `assets/app.css`
  rebuild) and `npm run test:run-counts` (needs Playwright), with the full suite.

## WF-P08-R023

B4b batch review (delete the legacy runtime). **Status: COMPLETE — CHANGES REQUIRED**. Opened
and closed 2026-10-24.

### Request and artifact

- **Artifact:** base/HEAD `da868710`, plus the working tree and the index. B4b's deletions are
  staged with `git rm`; B4a's are unstaged.
  - Digest `ac0a895808f9c091f86ca10cb74ebb7b9aba5524f3200d1d2aab0632eead63c7`, from the skill
    snapshot with the four usual exclusions. Verified at the start and again before returning.
  - Coordination r10 (`1df326af…`).
- **Manifests:**
  - `logs/b4b-snapshot.txt`;
  - the delta `logs/b4b-files.txt`: 77 deleted and 54 newly modified paths, 33 re-edited since
    the B4a freeze, 110 staged deletions;
  - `logs/b4b-filehashes.txt`.
- **Requirements:**
  - plan 08 and P08-Q1..Q8;
  - TASK "B4 plan", gate P08-D4 rev 2, "B4b work order" and "B4b progress" (DV-B4b-1..8, the
    leftovers, the check table);
  - AGENTS.md and the `src` guides.

### Coverage and what holds

1. **The runtime is gone, and nothing reachable depends on it.**
   - `main.rs` spawns no TaskWorker or memory worker. The workflow engine's `spawn` covers the
     slots, the intent worker and the schedule materializer. That last one replaces HEAD
     TaskWorker's schedule loop, so nothing that loop did is lost: the legacy lease reaping,
     quorum checks and census all went with legacy work.
   - `setup.rs`/`app_state.rs` build no registry, coordinator or approval/review/handoff use
     cases. `ThreadUseCases` is `new + with_monitoring + with_intent_wakeups`, and `ThreadStores`
     lost `tasks`.
   - `infra/events.rs` listens on six channels, none of them task channels. The `Resync`
     protocol is unchanged.
   - The harness module is `classifier/output/redaction/runs`.
   - The rig adapter keeps the shared drive loop with `Binding = Box<WorkflowBinding>`. Every
     added line in `execution.rs` is a doc, an import or the alias.
   - The ledger check is equivalent once `Waiting`/`Superseded` are gone:
     - budget exceeded ⇔ `BudgetExceeded`;
     - `Completed` ⇔ a final output.
   - `state_name` follows. No SQL reads the removed checkpoint states; the `'waiting'` hits are
     `workflow_runs` states.
   - A grep for every deleted type and function finds only comments (F091), `README`/`docs`
     (B4d), the schema (B4c) and the uncalled leftovers (F090).
2. **Deviations.**
   - **DV-B4b-1 is accepted.**
     - The use case keeps owner verification and the owner refusal.
     - `remove_member` is unchanged from HEAD.
     - The page test still covers the confirm and the owner exclusion.
     - F093.2 records a gap that HEAD also had.
   - **DV-B4b-2 is accepted.**
     - The gate, then the channel and assignment locks, then the diff, under the budget.
     - Six database tests, including a lock-timeout test with a competing gate holder.
     - The channel_gate ingress competing test is B4c's (D4 rev 2 §D4.11).
   - **DV-B4b-3 is accepted.** The directory uses `channel.description` only.
   - **DV-B4b-4/5/6 are accepted.**
     - `HarnessKind` stays with `HarnessConfig`.
     - `AgentCapabilityReader` is the skill editor's read.
     - Memory stops now and is deleted in B4c (F093.3).
   - **DV-B4b-8 is accepted.** The `#[cfg(test)]` seam is the production lock-in-id-order-then-
     `cancel_locked_deliveries_on`, keyed by id, with the selection join as the only difference.
     All six tests run.
   - **DV-B4b-7 is not fully accepted: F088.**
3. **Deliveries without `task_id`.** The entity, request, record, entry, row, columns, view select
   and insert are consistent: 14 columns, 14 placeholders, 14 binds. No delivery code reads
   `task_id`. The schema's FK and trigger accept NULL until B4c.
4. **Monitoring.**
   - `record_task_execution` and its metric types are gone from the trait and all three monitors.
   - HEAD's census counted only `queue_kind = 'legacy'` tasks, so no workflow-job coverage was
     lost.
   - The delivery gauges keep the `stuck_work{kind=delivery_*}` keys and the
     `monitoring::stuck_work` target. The publishing interval is 30 s, the same as before.
   - The new SQL has no database test (F092).
5. **Deleted tests.**
   - I compared the test names in the B4a-era logs with the current `--list` (2377 tests), and
     checked file reachability.
   - Every deleted test I checked went with its subject, **except** the four
     `harness_store_tests` (F088). Those are live workflow tests and were never deleted, only
     disconnected.
   - The kept dashboard D4.5 test and the remaining SQL-level isolation tests run.
6. **`.sqlx`:** verified by exact text (F093.1).
7. **Leftovers and stale items:** F089, F090, F091, F093.4–6.

### Checks (cluster 55480; tests use `workflow08_test`)

| Command | Result | Log |
| --- | --- | --- |
| `SQLX_OFFLINE=true CARGO_INCREMENTAL=0 cargo check --all-targets` | exit 0, no warnings | `logs/r023-check.log` |
| `cargo clippy --locked --all-targets -- -D warnings` (offline) | exit 0 | `logs/r023-clippy.log` |
| `cargo fmt --all -- --check`; `git diff --cached --check` | exit 0 / 0 | `logs/r023-fmt.log` |
| `scripts/tests/transport-boundary.sh`; `scripts/transport-boundary-check.sh` | passed / passed | `logs/r023-transport-boundary.log` |
| `node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs` (CI line) | 8 passed | — |
| `cargo test --lib -- adapters::persistence::company_invite application::use_cases::company_invite adapters::persistence::channel_assignment application::use_cases::channel adapters::persistence::delivery application::services::delivery_worker application::services::harness adapters::harness::rig harness_owner_tests adapters::persistence::thread application::use_cases::thread infra::events adapters::http::pages adapters::http::routes::live_updates adapters::http::routes::ui_stream adapters::monitoring domain:: adapters::persistence::dashboard message_ingress application::transport` | **638 passed**, 0 failed, 1 ignored | `logs/r023-focused.log` |
| Scratch copy with `mod store_tests` restored: `cargo test --lib -- store_tests` | 4 passed | `logs/r023-orphan-store-tests.log` |
| `cargo doc --no-deps --lib --document-private-items` | exit 0; unresolved links listed in F091 | `logs/r023-doc.log` |
| Census SQL `PREPARE`/`EXECUTE`/`EXPLAIN` on `workflow08_test` | runs; Seq Scan | — |
| Reachability scan; uncalled-`pub fn` scan against HEAD | 2 orphans; F090 list | `logs/r023-uncalled.txt` |

Not run:
- `cargo sqlx prepare --check` (no sqlx-cli);
- the full suite, `stack-budget.sh`, `npm run check:css` and `test:run-counts`. These are for
  the end of the phase.

### Outcome

**CHANGES REQUIRED** for B4b at digest
`ac0a895808f9c091f86ca10cb74ebb7b9aba5524f3200d1d2aab0632eead63c7`.
- **Blocking:**
  - F088 (Medium): restore `mod store_tests`.
  - F089–F092 (Low): delete the orphan helper, remove the exposed dead code, fix the stale
    comments and links, and add a database census test.
- **Optional:** F093 (Info).
- **Accepted:** DV-B4b-1..6 and DV-B4b-8. DV-B4b-7 is accepted except for F088.
- A recheck can cover the correction deltas plus a rerun of `harness_owner_tests`, the delivery
  suites and `cargo doc`.

## WF-P08-R024

B4b correction recheck for F088–F093. **Status: COMPLETE — PASS**. Opened and closed 2026-10-24.

### Request and artifact

- **Artifact:** base/HEAD `da868710`, plus the working tree and index.
  - Digest `c4b7eaa9c3625d3ac8e0d74a7afa802efcdf8a9dfee1f90a02de81107e63c896`, from the skill
    snapshot with the four usual exclusions. Verified at the start and again before returning.
  - Coordination r10 (`1df326af…`).
- **Manifests:**
  - `logs/b4b-snapshot-r2.txt`;
  - the delta against R023's freeze, `logs/b4b-r2-delta.txt` (2 `.sqlx` files and 1 test helper
    deleted, 44 sources changed or newly in the set);
  - `logs/b4b-filehashes-r2.txt`.
- **Correction record:** TASK "B4b corrections for WF-P08-R023 (2026-10-24)".

### Findings

- **F088 is fixed.** `mod store_tests` is declared again at `harness_owner_tests.rs:255-256`, and
  the four store tests pass on the real tree. My reachability scan finds no undeclared `.rs`.
- **F089 is fixed.** The helper is removed (staged).
- **F090 is fixed, including the cascade.**
  - `AgentExecutionDisposition` and the output's `disposition` field are gone. A grep of `src`
    and the schema finds no reader of `final_output.disposition`: the only checkpoint CHECKs read
    `structured` and `content`, and every remaining "disposition" belongs to the action-evidence
    or token tables.
  - The diagnostics' "pending" arm is gone. `Driven` and `Progress` suspension carry a `WaitId`.
  - The ThreadUseCases, transport, company and participant cascade removals had callers only in
    deleted legacy code (HEAD: `task_worker.rs`, `dispatch.rs`, the old `schedule.rs`,
    `thread/mod.rs`, `thread/support.rs`).
  - `.sqlx` has 25 files. All match the current source exactly, and none of the 20 removed is
    used. The two new removals are verified by exact text in HEAD's `persistence/company.rs`.
- **F091 is fixed.** A re-sweep for the deleted names hits only `src/**/AGENTS.md` (B4d) and the
  task-worker concurrency `expect` text (B4c). `cargo doc` gives 12 unresolved links, all already
  in R023's run; the four B4b ones are gone.
- **F092 is fixed.** The census database test asserts exact counts on `own_database`, with the
  due-now and delivered rows excluded. The comment is corrected.
- **F093 is resolved.** Dispositions are recorded; B4d gains items 2 and 5.

### The intermittent lease test: judged as F094 (Low, not caused by B4b, carried to the phase gate)

- Its code is outside both B4b deltas and unchanged from HEAD, apart from an unrelated pool
  comment in the test file. It failed the same way in B1. It passes alone 5/5.
- Likely mechanism: the resolution's server `lock_timeout` is floored to whole milliseconds, and
  `resolve_retryable` maps that database error to "unknown" at once, so "unknown" can arrive up to
  about 1 ms before `T`. The test's lower bound is exact, and so is the one at `:322`.
- Fix before the full suite: print `elapsed`, then tolerate the millisecond, or wait for the
  deadline before reporting unresolved.

### Checks (cluster 55480; tests use `workflow08_test`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`); `cargo fmt --all -- --check` | exit 0, no warnings / exit 0 | `logs/r024-clippy.log`, `logs/r024-fmt.log` |
| `cargo test --lib -- harness_owner_tests adapters::persistence::delivery application::services::delivery_worker application::services::harness adapters::harness::rig application::use_cases::{company,participant,thread,channel,agent} adapters::persistence::{company,participant,thread} adapters::smtp adapters::protocols::email application::transport infra::config adapters::monitoring domain:: adapters::http::pages adapters::http::routes harness_checkpoint agent_model_step` | **863 passed**, 0 failed, 3 ignored; includes the 4 `store_tests` and the census test | `logs/r024-focused.log` |
| `cargo test --lib -- agent_runtime checkpoint application::workflow::agent` | 36 passed | `logs/r024-checkpoint.log` |
| The intermittent test alone ×5 | 5/5 passed | `logs/r024-lease-repeat.log` |
| `cargo doc --no-deps --lib --document-private-items` | exit 0; 12 unresolved links, none new, four gone | `logs/r024-doc.log` |
| Orphan scan; `.sqlx` exact-text scan; deleted-name re-sweep | clean / 25 of 25 match, 20 removed unused / B4c-B4d only | — |

Not run:
- `cargo sqlx prepare --check` (no sqlx-cli);
- the full suite, `stack-budget.sh`, `npm run check:css` and `test:run-counts`. These are for the
  end of the phase.

### Outcome

**PASS** for B4b at digest `c4b7eaa9c3625d3ac8e0d74a7afa802efcdf8a9dfee1f90a02de81107e63c896`.
- F088–F092 are fixed and F093 is resolved.
- No blocking finding is open for B4b.
- Carried to the phase gate: F094 (Low), plus `prepare --check`, `check:css`, `test:run-counts`,
  `stack-budget.sh` and the full suite.

## WF-P08-R025

B4c-1 batch review (memory deletion, P08-Q1). **Status: COMPLETE — PASS** (with Low F095 carried).
Opened and closed 2026-10-24.

### Request and artifact

- **Artifact:** base/HEAD `da868710`, plus the working tree and index.
  - Digest `94139bfd7ef976af49f6ac6814c86e1b905521d9389a7cc140b72cbe5bec1e11`, from the skill
    snapshot with the four usual exclusions. Verified at the start and again before returning.
  - Coordination r10 (`1df326af…`).
- **Manifests:**
  - `logs/b4c1-snapshot.txt`;
  - the delta against the B4b acceptance (`logs/b4b-filehashes-r2.txt`): `logs/b4c1-delta.txt`,
    with 42 changed, 17 deleted and 14 first touched;
  - `logs/b4c1-filehashes.txt`.
- **Requirements:**
  - TASK "B4c plan", row B4c-1, and "B4c-1 progress";
  - P08-Q1;
  - AGENTS.md and the `src` guides.

### Coverage and what holds

1. **Completeness.**
   - A grep of `src`, the scripts, `.env.example`, `docs/deploy.md`, `fly.toml`, `deploy/` and
     the CI workflow finds no provider-memory term outside the guides and the B4d docs (F096.4).
     The terms searched: `memory_provider`, `retrieve_/persist_*_memory`, `memory_enabled`,
     persistence/recall modes, `memory_max_results`, hydradb, hindsight, `memory_ready`,
     `/memory`, the coordinator and worker names, `MemoryUseCases`, the settings types.
   - No page, route or JS string names memory. The agent-library payload lost its four keys.
   - `main.rs` keeps only the memory plan files under `NO_BACKWARD_COMPATIBILITY_NOTES`; they
     still carry the note.
   - Kept on purpose:
     - `memory_limit_bytes` with its check, and the runtime panel's "Memory limit" row (cgroup);
     - `UNHANDLED_STEP_KINDS = ["memory.load", "memory.save"]`, whose test
       `production_publication_refuses_memory_steps_and_captures_children` passes;
     - the registry's `MemoryRead`/`MemoryWrite` effects;
     - the in-memory test stores.
   - Uncalled-definition scan against HEAD: nothing new.
2. **Positional SQL** (checked statement by statement):
   - companies:
     - INSERT: 9 columns, `$1..$9`, 9 binds, in order;
     - both UPDATEs: `$1..$7` SET, `WHERE id = $8` (`AND user_id = $9`), binds in order;
     - the RETURNING lists match `CompanyDb`.
   - channels:
     - `insert_channel_row_on`: 12 columns, 12 placeholders, 12 binds;
     - UPDATE: `$1..$7`, `$8` id, `$9` company, `$10` response_trigger, bound in exactly that
       order.
   - agents:
     - `AGENT_COLUMNS` matches the `AgentDb` fields;
     - INSERT: 14 columns, `$1..$13` and `COALESCE($14, …)`, 14 binds;
     - UPDATE: `$1..$9`, `$10` id, `$11` contract, `$12` tools.
   - `runtime_metric_samples`: the builder INSERT names 15 columns with 15 `push_bind`s. The
     `ON CONFLICT` set and the bucket SQL no longer name `hydradb_*`.
3. **Migration.**
   - `workflow08_test`'s `_sqlx_migrations` checksum equals the SHA-384 of the current init file,
     so the file applied cleanly.
   - Catalog queries on `workflow08_test` find:
     - no `memory`/`hydra`/`hindsight` table, trigger, index or view;
     - no function whose name or body names one;
     - no constraint naming one, except `runtime_metric_samples_memory_limit_nonnegative`;
     - one column, `runtime_metric_samples.memory_limit_bytes`.
   - No surviving object refers to a dropped one.
4. **Removed tests.**
   - The B4b-accepted test names were compared with the current `--list` (2278 tests).
   - Every one that is gone is memory-only:
     - two dashboard memory-provider panel tests, 13 `entities::memory` tests and three memory
       config tests;
     - `a_blank_agent_form_stays_inside_the_bounds_its_own_fields_declare`: all three asserts were
       memory controls (HEAD `pages/tests.rs:1717-1721`);
     - `obsolete_channel_memory_tuning_is_ignored_by_the_form`: compatibility-only.
   - The surviving page test's replacement assertion is equivalent (F096.2). No non-memory
     coverage is lost.
5. **The two new tests** are adequate (F096.3).
   - The company test drives both renumbered UPDATEs with every field set, checks that an omitted
     policy is preserved, and checks the owner scoping.
   - INSERT transpositions are caught by type or by the CHECKs plus the default-policy assert.
6. **Config and docs.**
   - The memory keys are gone from `.env.example` and `docs/deploy.md`.
   - Every other documented key is read, except `AGENT_RUN_TIMEOUT_SECS` (B4c-2 per plan,
     F096.1).
   - Deferring the memory copy in README and docs to B4d is acceptable: no doc names a memory key.

### Found outside B4c-1's scope

**F095 (Low):** two stale `.sqlx` files left by B4b. My R023/R024 checks and the B4c-1 check were
text-based and missed them; a macro-bound check finds them. They would fail CI's
`prepare --check`. Remove them; B4c-3 must use the macro-bound method.

### Checks (cluster 55480; tests use `workflow08_test`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`); `cargo fmt --all -- --check` | exit 0, no warnings / exit 0 | `logs/r025-clippy.log`, `logs/r025-fmt.log` |
| `cargo test --lib -- adapters::persistence::{company,channel,agent,runtime_metrics,tests} constraint_tests application::use_cases::{company,channel,agent,thread} adapters::http::pages adapters::http::routes infra::config infra::workflow infra::runtime_metrics application::services::runtime_metrics adapters::smtp domain::entities delete_company agent_config agent_api` | **616 passed**, 0 failed, 3 ignored; includes both new tests and the memory-step refusal | `logs/r025-focused.log` |
| `cargo test --lib -- application::transport application::services adapters::monitoring` | 83 passed | `logs/r025-focused-2.log` |
| `node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs`; transport-boundary scripts | 8 passed; both passed | — |
| Catalog queries and migration checksum on `workflow08_test` | as above | — |
| `.sqlx` macro-bound check (validated on HEAD) | 0 missing, 2 stale (F095) | `logs/r025-sqlx-macro-check.py` |

Not run:
- `cargo sqlx prepare --check` (no sqlx-cli);
- `npm run check:css`, the full suite and `stack-budget.sh`. These are for the end of the phase.

### Outcome

**PASS** for B4c-1 at digest `94139bfd7ef976af49f6ac6814c86e1b905521d9389a7cc140b72cbe5bec1e11`.
- The memory deletion is complete and correct, and no non-memory coverage was lost.
- Open:
  - **F095 (Low)**, which B4b introduced and B4c-1 did not cause. Fix it in this tree or by
    B4c-3 at the latest, and use the macro-bound method there.
  - F096 (Info).

## WF-P08-R026

B4c-2 batch review (channel agents and agent runtime config). **Status: COMPLETE — CHANGES
REQUIRED**. Opened and closed 2026-10-24.

### Request and artifact

- **Artifact:** base/HEAD `da868710`, plus the working tree and index.
  - Digest `97531db8994a61e52cb4fe227c5fc2961b7873690e87ce6304a6ae047340802d`, from the skill
    snapshot with the four usual exclusions. Verified at the start and again before returning.
  - Coordination r10.
- **Manifests:**
  - `logs/b4c2-snapshot.txt`;
  - the delta against the B4c-1 acceptance: `logs/b4c2-delta.txt`, with 53 changed, 11 first
    touched and 8 deleted;
  - `logs/b4c2-filehashes.txt`.
- **Requirements:**
  - TASK "B4c plan" row B4c-2, "B4c-2 work order" (DV-B4c2-1..4) and "B4c-2 progress";
  - D4 rev 2 §D4.7/§D4.8;
  - P08-Q4 and P08-Q7;
  - AGENTS.md.

### Coverage and what holds

1. **Dead to the workflow runtime.**
   - A grep of `src`, `scripts`, `docs` (non-md), `.env.example`, `fly.toml` and README finds the
     dropped names only in negative tests (retired-key lists, schema-gone assertions) and in
     README copy (B4d).
   - The model step's contract comes from the step `output_schema`: `application/workflow/agent.rs`
     builds `response_contract()`, and `rig/workflow.rs:278-283` validates `spec.identity`. The run
     deadline comes from the claim window. No workflow SQL reads the dropped columns.
   - Kept, correctly:
     - `add_3rd_party`: ingress at `routing.rs:521`;
     - `default_add_3rd_party`: copied by provisioning `channel_write` (`provisioning.rs:781-799`),
       with `participant_emails: None`, so the channel is team-only (P08-Q4);
     - `HarnessKind`/`RunIdentity.harness`, whose doc is reworded;
     - `ResponseContract`: messages, step identities;
     - the channel-level spam interlock (`use_cases/channel.rs:111-118`).
   - `response_drafts.response_contract` survives in a legacy table that B4c-3 drops.
2. **DV-B4c2-1 is accepted.** `resolve_internal_destination` still enforces:
   - the same company;
   - no self-relay;
   - an enabled target;
   - the company slug match.
   The relay origin is still constructible only by that path; the loop-cycle refusal and the hop
   limit are unchanged; the target's access policy still applies at ingress. No security check is
   lost. The removed check only required an agent list, which D4.7 retired.
   `a_hop_to_a_plain_channel_is_relayed` covers it.
3. **DV-B4c2-2: F097 (Medium).** The edit itself is correct: budget, then the exclusive gate, then
   the tenant `FOR UPDATE`, then the settings write. But the rewritten gate doc claims that the
   inbound commit reads eligibility after the shared gate. It does not: eligibility is decided
   before `commit_on`, and nothing re-reads it under the lock. The new competing test only shows
   that the exclusive side waits for a bare shared lock. The rename and the key prefix are fine;
   only Rust uses the key.
4. **Positional SQL.**
   - agents: INSERT has 11 columns, `$1..$10` and `COALESCE($11)`, 11 binds. UPDATE is `$1..$7`
     SET, `$8` tools, `$9` id. `AGENT_COLUMNS` matches `AgentDb`.
   - channels: INSERT has 8/8/8. UPDATE is `$1..$5` SET, then `$6` id and `$7` company.
   - companies: INSERT has 6/6/6, with RETURNING equal to `CompanyDb`. Both UPDATEs are `$1..$4`
     SET, then `$5` id (and `$6` user).
   - The `channel_edit` lock is `SELECT 1 … company_id = $1 AND id = $2 FOR UPDATE`.
5. **Migration.**
   - `workflow08_test`'s checksum equals the SHA-384 of the current file, so it applied, the comma
     repair included.
   - Catalog: none of the 12 dropped column names remains (except `response_drafts.response_contract`,
     legacy, B4c-3), and none of the three tables or five functions.
   - No function body, view or constraint names a dropped object (`prosrc` regex over every
     dropped table, column and function).
6. **DV-B4c2-3 is accepted** until B4c-3 retires the legacy fixtures (F098.5).
7. **Mailbox attribution.** No non-workflow behaviour is lost (F098.1).
8. **Tests.** I compared the B4c-1 and current `--list` (2278 → 2234 tests): 58 gone, 14 new.
   - Each gone test's subject is removed: assignment, owner, channel tab, response trigger,
     config/contract/timeout/sub-agent, default participants, library modal, contract patch.
   - The renames keep their escaping and attribution checks.
   - The node test lost the schema-JS case with its subject.
   - The removed "Task Executions" link was dead (B4a). The B4d leftovers list is acceptable; I
     would move the address preview earlier (F098.3).
   - `.sqlx`: the macro-bound check gives 23/23/0/0 (F095 fixed).

### Checks (cluster 55480; tests use `workflow08_test`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`); `cargo fmt --all -- --check` | exit 0, no warnings / exit 0 | `logs/r026-clippy.log`, `logs/r026-fmt.log` |
| `cargo test --lib --` with the author's 833-test filter set (`adapters::http`, persistence channel/agent/company/tests, `constraint_tests`, provisioning, use cases agent/channel/company/thread, `message_ingress`, `harness_owner`, `job_`, `authorization_cases`, `response_schema`, `skill`, `persistence::integration`, `domain::entities`, smtp, `infra::`, `agent_model_step`, `agent_recovery`, `workflow::agent`, `channel_gate`, inbound, transport, egress) plus `assistant_channel` | **833 passed**, 0 failed, 3 ignored | `logs/r026-focused.log` |
| `node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs`; the transport-boundary scripts | 7 passed; both passed | — |
| `.sqlx` macro-bound check | 23 sites, 23 files, 0 stale, 0 missing | — |
| Migration checksum and catalog queries on `workflow08_test` | as above | — |
| Token scan for definitions left unused against HEAD | only a pre-existing uncalled trait method (F098.4) and two test helpers | — |

Not run:
- `cargo sqlx prepare --check` (no sqlx-cli);
- `npm run check:css`, the full suite and `stack-budget.sh`. These are for the end of the phase.

### Outcome

**CHANGES REQUIRED** for B4c-2 at digest
`97531db8994a61e52cb4fe227c5fc2961b7873690e87ce6304a6ae047340802d`.
- **Blocking: F097 (Medium).** The minimum is option (b): an honest gate doc and test claim, plus a
  TASK note that B4c-3's F083 test implements and asserts eligibility under the gate. Option (a)
  is preferred.
- **Optional:** F098 (Info).
- Closed here: F095 is fixed and F096.1 is fixed.
- **Accepted:** DV-B4c2-1, DV-B4c2-3 and DV-B4c2-4. DV-B4c2-2 is accepted for the edit itself but
  not for the protocol claim.
- A recheck can cover the correction delta.

## WF-P08-R027

B4c-2 correction recheck for F097/F098. **Status: COMPLETE — CHANGES REQUIRED** (one Low). Opened
and closed 2026-10-24.

### Request and artifact

- **Artifact:** base/HEAD `da868710`, plus the working tree and index.
  - Digest `2d4ea381960e8134768631a06eacc425442310dd0ed06c7a4723ba880e8a7098`. Verified at the
    start and again before returning.
  - Coordination r10.
- **Manifests:** `logs/b4c2-snapshot-r2.txt`; the delta `logs/b4c2-r2-delta.txt` (24 files);
  `logs/b4c2-filehashes-r2.txt`.
- **Correction record:** TASK "B4c-2 corrections for WF-P08-R026" (DV-B4c2-5).

### Assessment

- **F097 is fixed (option (a)).**
  - The revision moves only in `write_channel_settings_on` (`channel.rs:409-456`), which writes the
    row, the slugs, the grants and the email binding.
  - My search finds no other post-create writer of `channels`, `channel_slugs` or the allowlist
    grants in production code.
  - Routing reads the revision in `CHANNEL_SELECT` (`channel.rs:128`). `CommitPlan` copies it per
    intent. Intents and associations both come from `prepared.channels`, one to one.
  - `hold_channel_settings` takes the shared gates and then runs `verify_channel_settings` as its
    own statement (`inbound.rs:132-182`). A missing channel or another revision is refused as
    `Superseded`.
  - The docs (the inbound module, `channel_gate`, `channel_edit`) describe the protocol accurately.
  - Schema on `workflow08_test`: the checksum equals the SHA-384 of the file. `settings_revision`
    is `bigint`, default 0, not null. `valid_inbound_event_error_class` includes `superseded`, and
    its 11 values match the Rust enum.
- **The two competing tests detect the defect, and their passing paths are deterministic.**
  - Edit first: the commit cannot pass the held gate. Without the revision check it would store.
  - Commit first: the edit cannot pass the shared gate before the admission commits.
  - Gap: the edit's 2 s lock budget could time it out under load (**F099**).
- **The error variant and its mappings are justified (F100.3).** The internal relay maps it to
  `Retryable` as well.
- **F098 is resolved.** Item 3 is fixed (`@handle`). The others are deferred as recorded.
- **Scope notes: F100.1 and F100.2.**

### Checks (cluster 55480; tests use `workflow08_test`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`); `cargo fmt --all -- --check` | exit 0, no warnings / exit 0 | `logs/r027-clippy.log`, `logs/r027-fmt.log` |
| `cargo test --lib -- adapters::persistence::thread:: adapters::persistence::channel adapters::persistence::tests constraint_tests application::services::inbound_event_worker adapters::smtp app_error resend_api application::use_cases::thread application::use_cases::channel application::transport domain::entities adapters::http message_ingress provisioning inbound_event` | **690 passed**, 0 failed, 3 ignored; includes both gate tests and the worker retry test | `logs/r027-focused.log` |
| `.sqlx` macro-bound check | 23/23, none stale or missing | — |
| Migration checksum and catalog queries on `workflow08_test` | as above | — |

Not run:
- `cargo sqlx prepare --check` (no sqlx-cli);
- `check:css`, the full suite and `stack-budget.sh`. These are for the end of the phase.

### Outcome

**CHANGES REQUIRED** at digest `2d4ea381960e8134768631a06eacc425442310dd0ed06c7a4723ba880e8a7098`.
- F097 is fixed and the protocol is accepted. F098 is resolved.
- **Open:** F099 (Low), a one-line test-budget change. F100 (Info) is optional.
- A recheck can cover that delta plus a rerun of the two gate tests.

## WF-P08-R028

B4c-2 correction recheck for F099. **Status: COMPLETE — PASS**. Opened and closed 2026-10-24.

- **Artifact:** base/HEAD `da868710`, plus the working tree and index.
  - Digest `95fe39183361bb49316fa8cb1181a5cedf45fa0834bfff5e694e4c094baacbe5`
    (`logs/b4c2-snapshot-r3.txt`). Verified at the start and again before returning.
  - Coordination r10.
- **Delta:** `logs/b4c2-filehashes-r2.txt` and `-r3.txt` differ in exactly one file,
  `src/adapters/persistence/thread/inbound_tests.rs`. Its hash on disk matches r3.
- **F099 is fixed.**
  - `a_settings_edit_queued_behind_a_commit_applies_after_it` spawns the edit with
    `EditBudget { lock_timeout: 30 s, ..DEFAULT }`, with a comment saying why.
  - The statement bound and the deadline stay at 30 s.
  - The 300 ms "still waiting" check is now the only timing assumption, and it cannot fail on a
    correct protocol.
- **F100 is resolved** by its TASK record.

| Command | Result | Log |
| --- | --- | --- |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`); `cargo fmt --all -- --check` | exit 0 / exit 0 | `logs/r028-clippy.log`, `logs/r028-fmt.log` |
| `cargo test --lib -- adapters::persistence::thread::inbound_tests adapters::persistence::channel_edit`, ×3 | 25 passed each run, 0 failed | `logs/r028-tests.log` |

R027's wider checks stand: the delta is one test's budget.

**Outcome: PASS** for B4c-2 at digest
`95fe39183361bb49316fa8cb1181a5cedf45fa0834bfff5e694e4c094baacbe5`.
- F097 and F099 are fixed, and F098 and F100 resolved.
- No blocking finding is open for B4c-2.
- Still open for later: F094 (phase gate), F096 items 2–4 (notes), and the end-of-phase checks
  (`prepare --check`, `check:css`, the full suite, `stack-budget.sh`).


## WF-P08-R029

B4c-3a batch review (legacy runtime schema). **Status: COMPLETE — PASS**. Opened and closed
2026-10-24.

### Request and artifact

- **Artifact:** base/HEAD `da868710`, plus the working tree and index.
  - Digest `fad11e2751bc298f501dbcbd1b6b8049c292ecfcf5d12eec25941eb75f1c1dde`
    (`logs/b4c3a-snapshot.txt`). Verified at the start and again before returning.
  - Coordination r10.
- **Inputs:**
  - TASK "B4c-3 work order" (DV-B4c3-1..5) and "B4c-3a progress" (DV-B4c3-6..7);
  - D4 rev 2 §D4.3/§D4.8;
  - the inventory (not trusted);
  - the delta `logs/b4c3a-delta.txt` (38 files).
- **Pre-edit migration:** `logs/b4c3a-init-schema-before.sql`. Its SHA-256 equals the B4c-2-accepted
  hash in `logs/b4c2-filehashes-r3.txt` (`c07ff2c3…`).

### Migration: independent verification

- **Statement-level diff** with my own splitter, which respects dollar quotes, string literals and
  comments (`logs/r029-stmts.py`, `logs/r029-classify.py`, output `logs/r029-migration-diff.txt`).
  - 1244 statements become 980: 275 removed and 11 added. The 11 added are the rewritten versions
    of 7 tables and 4 functions.
  - Every removed CREATE TABLE is one of the 24 dropped tables or an old version of the 7 rewritten
    ones. Every removed function is one of the 25 dropped functions or an old version of the 4
    rewritten ones.
  - Constraints (126), indexes (20) and triggers (50): every one belongs to a dropped table or
    executes a dropped function. On kept tables the triggers are:
    - `background_tasks`: `bump_attention_version`, `record_initial_ownership`,
      `record_status_event`, `task_status_actionable_notification` (and its delete twin);
    - `principals`: the two release triggers;
    - `channels`: `channels_delete_target_tasks`, which deleted `queue_kind='legacy'` tasks only;
    - `message_deliveries`: `notify_chain` (no listener since B4b) and
      `require_legacy_source_task`.
    All of these are legacy-gated or legacy-only.
  - The remaining removals are exactly the named ones:
    - `background_tasks`: source key and its comment, `awaited_outreach_fk`, `owner_principal_fk`,
      `source_message_fk`, `transition_approval_fk`, `transition_outreach_fk`;
    - indexes `pending_ready_idx` (`owner_principal_kind='agent'`), `thread_correlation_match_idx`
      (legacy task types), `unsettled_channel_idx` and `unsettled_owner_idx` (legacy statuses and
      owner), `waiting_due_idx`;
    - `message_deliveries`: `task_fk` and `task_idx`;
    - `task_harness_runs`: owner FK and `task_harness_runs_scope` (on the dropped
      `ownership_version`; the workflow unique `task_harness_runs_workflow_execution` stays);
    - the three `background_tasks` triggers, whose WHEN clauses all required `queue_kind='legacy'`.
  - No live statement went with a segment. The live `guard_harness_model_owner` and
    `require_harness_owner` triggers are still present.
- **Rewrites, read in full:**
  - `require_harness_owner`: the immutability ROW drops only the dropped columns; owner must be
    `workflow`; the live fence check is unchanged.
  - `notify_attention_changed`: delivery only. Its only caller is
    `message_deliveries_notify_attention`. A company-less notice delivery stays silent.
  - `enqueue_actionable_notification_event`: only the legacy preservation block is gone.
  - `guard_model_usage_task`: requires a run and the exact execution.
  - The tables: the `background_tasks` workflow shape, a `queue_kind` CHECK and default of
    `'workflow'`, the 4-status set; `task_attempts`' 6 stop reasons; `task_harness_runs`' workflow
    owner shape; the attribution and cancellation checks on `message_deliveries`; the notification
    checks for delivery only; the preferences without the 4 flags.
- **Catalog on `workflow08_test`** (checksum equal to the file):
  - My regex over `pg_get_functiondef` (prokind f), constraints, indexes and non-internal triggers
    covers the 24 tables, the 25 functions, every dropped column name, the legacy status, reason
    and cancellation values, the 4 flags and `'legacy'`.
  - It finds nothing, except `thread_messages_entry_kind_check`'s live `'delegation'` (relay
    entries).
  - There are 105 base tables and one app view.
- **The `notification_withdraw_deleted_source` drop is safe.** Its only triggers were on
  `manual_handoffs` and `response_reviews`. Delivery deletes keep `delivery_actionable_notification`.

### Writers against the narrowed checks

- **`background_tasks.status`:** every production writer sets `pending`, `processing`,
  `completed` or `failed`. `recovery.rs:213` binds only `pending` or `failed`. No writer sets
  `stopped` or `dead_letter`.
- **`task_attempts.stop_reason`:** `completed`, `approval_parked`, `effect_parked`, `child_parked`,
  `lease_lost` and `timed_out` (`recovery.rs:179`, `maintenance.rs:65`), or NULL. That is exactly
  the six-value set.
- **`queue_kind`:** every insert sets `'workflow'`. **`owner_kind`** defaults to `'workflow'`.
- **Notifications:** only the delivery source and action are written.
- No code or SQL string names a dropped column. The test hits are negative tests and provider JSON
  keys.

### Dashboard, tests and catalog test

- **DV-B4c3-1 is accepted** (F101.5). The outstanding list and the throughput are consistent with
  the four statuses.
- **Test rewrites keep their purpose:**
  - unknown-job completion refusal;
  - the relay-aware `find_outbound_reply_after`: `entry_kind='conversation'`, with a delegation
    entry asserted not to be the answer;
  - association and shape tests;
  - `owner_kind 'legacy'` is still refused.
  - Deleted suites cover only legacy schema.
  - DV-B4c3-6: `completed` hides the job as `stopped` did. Claim queries select only `pending`, and
    the `HideJob` preparation assertion compares every table, so the change hides the job and
    writes nothing else.
- **D4.8 catalog test:**
  - The scope is right: 26 tables, 28 distinctive column names, the generic `task_id`/`result`/
    `error` checked on their own tables, and 25 functions.
  - The canary self-check is sound.
  - The optional strengthening is F101.2.

### Checks (cluster 55480; tests use `workflow08_test`)

| Command | Result | Log |
| --- | --- | --- |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`); `cargo fmt --all -- --check` | exit 0 / exit 0 | `logs/r029-clippy.log`, `logs/r029-fmt.log` |
| `cargo test --lib -- adapters::persistence:: adapters::http:: infra:: domain::entities application::services application::use_cases application::transport model_usage application::workflow` | **2033 passed**, 0 failed, 1 ignored | `logs/r029-focused.log` |
| Node CI tests; transport-boundary scripts | 7 passed; both passed | — |
| `.sqlx` macro-bound check | 23/23, none stale or missing | — |
| Token scan for definitions left unused against HEAD | only the pre-existing items (F098.4) | — |
| Migration statement diff and catalog scan | as above | `logs/r029-migration-diff.txt` |

Not run:
- `cargo sqlx prepare --check` (no sqlx-cli);
- `check:css`, the full suite and `stack-budget.sh`. These are for the end of the phase.

### Outcome

**PASS** for B4c-3a at digest `fad11e2751bc298f501dbcbd1b6b8049c292ecfcf5d12eec25941eb75f1c1dde`.
- No blocking finding.
- F101 (Info) is optional.
- DV-B4c3-1..7 are accepted.

## WF-P08-R030

B4c-3b batch review (runtime metrics retarget, plus F101.2). **Status: COMPLETE — PASS**. Opened
and closed 2026-10-24.

- **Artifact:** base/HEAD `da868710`, plus the working tree and index.
  - Digest `845d5596b5945223b27143dc847eda42cc5cd620a84edc4d5262b9e39793e1d6`
    (`logs/b4c3b-snapshot.txt`). Verified at the start and again before returning.
  - Coordination r10.
- **Inputs:** D4.8; the TASK B4c-3 work order row B4c-3b and "B4c-3b progress"; F101.2; the
  delta `logs/b4c3b-delta.txt` (22 files).
- **Schema on `workflow08_test`:**
  - The checksum equals the SHA-384 of the current file.
  - `active_workflow_jobs` is integer, default 0, not null. `workflow_worker_slots` is integer,
    default 1, not null.
  - The checks are `_active_jobs_nonnegative`, `_active_jobs_within_slots` and
    `_worker_slots_positive`.
  - No function definition names the old columns.
  - `logs/b4c3b-scratch-apply.log` (980 lines, no errors) and `logs/b4c3b-catalog.log` are now
    complete (F101.1).
- **The gauge** (F102.1):
  - one guard per slot, entered only on `PollWork::Job` and dropped on every exit;
  - one shared gauge, clamped in `observe`;
  - `main.rs` feeds `workflow.active_jobs()` and `workflow.slots()`.
  - Before this batch the sampler's gauge was never incremented, so the panel showed 0. That is
    now fixed.
- **The recommendation** is an observed safe level that cannot exceed the slots (F102.2).
- **Config and docs:**
  - `TASK_WORKER_CONCURRENCY` and the old type names are gone everywhere outside `plan/`,
    `marketing/` and `website/`.
  - The README bullet is accurate (F102.3).
  - `.env.example` and `docs/deploy.md` document `WORKFLOW_WORKER_SLOTS` and the pool rule.
- **F101.2 is fixed.** The scan reads `pg_get_functiondef` for f/p plus 14 quoted retired values.
  The two canaries assert exactly two mentions (F102.4).

| Command | Result | Log |
| --- | --- | --- |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`); `cargo fmt --all -- --check` | exit 0 / exit 0 | `logs/r030-clippy.log`, `logs/r030-fmt.log` |
| `cargo test --lib -- runtime_metrics dashboard_runtime infra:: polling legacy_runtime_schema adapters::persistence::tests adapters::http::pages fairness_worker child_runtime child_decision_round mcp_tests phase03 application::workflow::worker` | **266 passed**, 0 failed, 1 ignored | `logs/r030-focused.log` |
| Node CI tests; both transport-boundary scripts | 7 passed; passed | — |
| `.sqlx` macro-bound check | 23/23 | — |
| Schema and catalog queries on `workflow08_test` | as above | — |

The author's broad run (2031 passed, `logs/b4c3b-focused-2.log`) covers the rest; the delta's
first-touched workflow test files ran in my focused set (42 of their tests).

Not run:
- `cargo sqlx prepare --check` (no sqlx-cli);
- `check:css`, the full suite and `stack-budget.sh`. These are for the end of the phase.

**Outcome: PASS** for B4c-3b at digest
`845d5596b5945223b27143dc847eda42cc5cd620a84edc4d5262b9e39793e1d6`.
- F101.1 is adopted and F101.2 fixed.
- F102 (Info) is optional.
- No blocking finding is open.

## WF-P08-R031

B4d batch review (phase close-out) plus the phase-level acceptance review. **Status: COMPLETE —
CHANGES REQUIRED** (two Low findings). Opened and closed 2026-10-24.

- **Artifact:** base/HEAD `da868710`, plus the working tree and index.
  - Digest `4ec81c1a9bf670f8efb5ef46b9ebac7f8c44261a70fa538b19cecfa598140fc1`
    (`logs/b4d-snapshot.txt`). Verified at the start and again before returning.
  - Coordination r10 (`1df326af740df24e`).
- **Inputs:**
  - plan 08's body and §Acceptance;
  - TASK acceptance criteria 1–13 (`TASK.md:79-123`);
  - the TASK "B4d work order" and "B4d progress";
  - the delta `logs/b4d-delta.txt` (27 files) and `logs/b4d-filehashes.txt`;
  - `logs/b4d-scratch-apply.log` and `logs/b4d-catalog.log`.
- **(a) Docs against code.** Spot-checked (F105.2):
  - README §3.4: the leases, poll and claim sizes, five attempts, the `scheduled_for` input and
    the shutdown joins.
  - README §3.7: the trigger semantics, the Ana example, bounces and SMTP 550, sender-only
    replies.
  - The README highlights and the Rig paragraph.
  - The deploy runbook routes.
  - `rig.md`: the Tools, Skills and Context rows and the rejected fields.
  - The relay headers and the doc anchors.

  All hold except two claims, **F103 (Low)**:
  - `rig.md:137`'s `max_turns` 1–16. No such setting exists; the bound is a fixed 8 per model
    step.
  - `deploy.md:177`'s skill pilot "through `read_resource`". That tool is gone.
- **(b) DV-B4d-1..3 are sound** (F105.4).
  - DV-B4d-2 and DV-B4d-3 are confirmed on `workflow08_test`:
    - the provenance function is user/system only;
    - `model_usage_calls_check3 = 5`;
    - `reserve_on` uses inner joins.
  - DV-B4d-1: the purposes and `depends_on` have no production writer. Keeping them is a
    reasoned call, but the "revisit" belongs in the plan.
  - Left over: the 06C obligation to remove `mcp_connections_by_ids` (TASK criterion 13) was
    never done or decided. It is a trait method plus impl, used only by tests: **F104 (Low)**.
- **(c) Dangling references:** none (F105.3).
- **F094 is fixed** (option (a), 1 ms granularity on both exact bounds, elapsed printed). F101
  and F102 are resolved.
- **(d) Acceptance:** see the matrix in F105.1. Criteria 1, 3, 4, 5 and 6 have direct evidence;
  criterion 6 has it apart from F104.
  - Criterion 2 is direct for runs, replies and waits. For decisions and artifacts it rests on
    structure: a decision's FK to its run's wait, and per-execution outputs. An optional test or a
    recorded argument would close that.
  - The 60-per-hour thread turn guard has never had a test; this predates p08.
- **(e) Before the gate** (F105.5):
  - Recreate the stale dev database `workflow08`: a pre-p08 schema, no `_sqlx_migrations`.
  - Drop `workflow08_scratch`.
  - Then run the carried checks: `cargo sqlx prepare --check`, `check:css`, the full suite,
    `stack-budget.sh` and `test:run-counts`.

  Nothing else blocks.

| Command | Result | Log |
| --- | --- | --- |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`); `cargo fmt --all -- --check` | exit 0 / exit 0 | `logs/r031-clippy.log`, `logs/r031-fmt.log` |
| `cargo test --lib -- adapters::persistence:: adapters::http:: infra:: domain:: application::services application::use_cases application::transport application::workflow model_usage adapters::harness adapters::protocols adapters::smtp adapters::resend_api` | **2182 passed**, 0 failed, 3 ignored (37 filtered out) | `logs/r031-focused.log` |
| `node --test scripts/tests/agent-runtime.mjs scripts/tests/sse-navigation.mjs` | 7 passed | `logs/r031-node.log` |
| `scripts/tests/transport-boundary.sh`; `scripts/transport-boundary-check.sh` | both passed | `logs/r031-boundary.log` |
| `.sqlx` macro-bound check (`logs/r025-sqlx-macro-check.py`) | 23 sites / 23 files; none stale, none missing | — |
| Migration SHA-384 vs `workflow08_test._sqlx_migrations.checksum`; `pg_get_functiondef(valid_creation_provenance)`; `model_usage_calls` CHECKs | equal (`48e3d2eb…1bb3`); user/system only; `num_nonnulls(...) = 5` | — |
| Token, route and dangling-reference greps over `src`, the docs and the guides | as in F103–F105 | — |

Not run:
- `cargo sqlx prepare --check` (no sqlx-cli);
- `npm run check:css` and `npm run test:run-counts` (Playwright);
- the full suite and `scripts/stack-budget.sh`. These belong to the phase gate.

**Outcome: CHANGES REQUIRED** for B4d at digest
`4ec81c1a9bf670f8efb5ef46b9ebac7f8c44261a70fa538b19cecfa598140fc1`.
- Open: **F103** (Low, two doc lines) and **F104** (Low, remove `mcp_connections_by_ids` or
  record a keep decision).
- F105 (Info) is optional.
- F094 is fixed, and F101 and F102 are resolved.
- A recheck can cover just the F103/F104 delta. Phase acceptance is otherwise supported, subject
  to the gate's full checks.

## WF-P08-R032

B4d F103/F104 correction recheck. **Status: COMPLETE — PASS**. Opened and closed 2026-10-24.

- **Artifact:** base/HEAD `da868710`, plus the working tree and index.
  - Digest `00ec454fd48fd7bc7600589474672feb5f76553e437e4f0d3333f2ec595b6829`
    (`logs/b4d-snapshot-r2.txt`). Verified at the start and again before returning.
  - Coordination r10 (`1df326af740df24e`).
- **Delta:**
  - The six changed files match `logs/b4d-filehashes-r2.txt`.
  - Diffing that set against `logs/b4d-filehashes.txt` gives exactly these six. Three of them are
    new to the set (`persistence/mcp.rs`, `mcp_read_tests.rs`, `use_cases/mcp.rs`), and the
    other three changed hash.
- **F103 is fixed.**
  - The Model attempts row matches `PreparedAgent::policy` (`agent.rs:184-191`): the default
    `model_calls: 8`, with `MAX_REPAIR_CALLS` = 2 inside the same budget, and no setting.
  - The Skill pilot row describes the prompt-only skill model.
  - Re-grep: no `max_turns`; `read_resource` only in the dated `rig-verification.md`.
  - My R031 evidence misnamed the impl as `WorkflowModelPreparation`. TASK's
    `WorkflowAgentStep` is also a slip; the type is `PreparedAgent`. The slip is in the records
    only, so it is not a finding.
- **F104 is fixed.**
  - The trait method and impl are gone, with 0 hits outside `plan/`.
  - `catalog_reads_are_tenant_scoped_and_exclude_deleted_rows` keeps the tenant and deleted-row
    coverage on `list_mcp_connections`.
  - `connection_on` replaces both test reads.
  - No `.sqlx` file is orphaned (macro check 23/23).
- **F105 is adopted** as recorded in TASK "B4d corrections for WF-P08-R031".

| Command | Result | Log |
| --- | --- | --- |
| `cargo clippy --locked --all-targets -- -D warnings` (`SQLX_OFFLINE`); `cargo fmt --all -- --check` | exit 0 / exit 0 | `logs/r032-clippy.log`, `logs/r032-fmt.log` |
| `cargo test --lib -- mcp legacy_runtime_schema` | **55 passed**, 0 failed | `logs/r032-focused.log` |
| `.sqlx` macro-bound check | 23/23, none stale or missing | — |
| Hash and grep checks | as above | — |

Not run, all for the phase gate:
- `cargo sqlx prepare --check` (no sqlx-cli);
- `check:css` and `test:run-counts`;
- the full suite and `stack-budget.sh`.

**Outcome: PASS** for B4d at digest
`00ec454fd48fd7bc7600589474672feb5f76553e437e4f0d3333f2ec595b6829`.
- F103 and F104 are fixed, and F105 is adopted.
- No finding blocks B4d.
- Still owed at the phase gate: recreate `workflow08`, then the full checks.
