# P08 agent review findings

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Shared context for the findings of round **WF-P08-R001** (unless a finding says otherwise):
date 2026-10-22; workflow `implement-opus-reviewed-2_2`; task record
[`TASK.md`](TASK.md), review record [`REVIEW.md`](REVIEW.md), notes
[`agent-review-notes.md`](agent-review-notes.md#wf-p08-r001); batch B1 (+ the D1.9 schedule
contract implemented in B2, and the D1.0 batch split); stage **architecture gate P08-D1 rev 1**
(`logs/decision-D1.md`, sha256 `453bf4a05cab5d9b5d358f1c6ea0b58b2b454d2ff65c7562e7375e45e946b7bb`);
reviewer B, Opus 5.5 / xhigh; phase `p08`; worktree `/Users/mac02/dev/openbot` (branch
`workflows`), HEAD = review base = phase start `da868710d3b235d49856e3e481f8f3735f6b3e56`, digest
`e07e4b1d4a5f9491a6fd1115d251a416d56955383a564f249531e9ba565b47e9`. All findings below originate
in the gated decision text (no source edits exist yet). Line numbers are at `da86871`.

## WF-P08-F001 — D1.3: which associations "answer" (and so get an intent, and keep an external audience) is undefined and still wired to the legacy channel trigger

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.3 (and §D1.2).
- **Classification:** actionable defect; **High**; requirements / architecture.
- **Evidence:**
  - D1.3 creates intents "one row per accepted message × channel association, only when the
    folded disposition answers", and `CommitPlan` "gains one `AdmissionIntent` per answering
    association". Today "answering" is per channel and comes from the legacy channel trigger:
    `candidate_answers` evaluates `candidate.channel.response_trigger`
    (`src/application/use_cases/thread/ingest/routing.rs:742-766`), and the message-wide fold is
    `fold_disposition(stated, any_channel_answers, all_outreach)`
    (`src/application/use_cases/thread/ingest/policy.rs:201-211`).
  - The folded disposition is not only an enqueue switch: `message_write`
    (`src/adapters/persistence/thread/inbound.rs:900`, `is_note`) stores a `FileOnly` message as
    `InternalOnly` audience / `Note` entry kind, and the message runtime then answers such a source
    only internally (`reply_target_on`, `src/adapters/persistence/workflow/message.rs:396-401`).
  - D1.2 replaces `ChannelResponseTrigger` with the binding's `MessageTrigger`, deleting the channel
    column only in B3. D1 never says what the commit-time "answers" input becomes in B1. If it stays
    the channel trigger, the channel setting pre-filters binding eligibility (e.g. a Cc'd message
    to a channel whose legacy trigger is `always` gets no intent, so an `any` binding can never see
    it) and turns that customer email into an internal note that `message.reply` can't email. That
    is the "hidden dispatch branch" the plan forbids (§Channels and ingress: "Trigger eligibility
    such as direct-address versus CC selection belongs in explicit binding admission configuration,
    not a hidden dispatch branch") and conflicts with acceptance 5 ("No runtime ... path depends on
    removed channel execution settings"). If it becomes "always answers", messages that legacy
    filed as internal notes now get an external audience. Either way it is a behavior change that
    the gate has to decide.
  - Related: `FileOnly` (stated quiet, or `all_outreach`) creates no intent, so a quiet reply to a
    workflow send can never resume its wait (D1.5). Legacy recorded outreach replies even when the
    fold was `FileOnly` (`apply_outreach_transitions`).
- **Required:** state the B1 fold exactly. Per-association intent creation and the stored
  audience/entry kind must depend only on the sender's explicit directives (address suffix, body
  marker, header) and ingress protections. They must not depend on channel `response_trigger`,
  agent lists or hold policy. Say what audience a message gets when no binding admits it, and
  whether a quiet message can still satisfy a correlated wait. Add a unit/DB test: Cc'd message +
  an `any` binding → intent → run, on a channel whose legacy trigger would not answer.
- **Instruction lesson:** observation: the decision changed who owns eligibility but did not trace
  every consumer of the old eligibility value (the fold also sets message audience). Candidate
  (scope: `implement-opus-reviewed-2_2` gate template): "When a gate moves a decision to a new
  owner, list every current consumer of the old decision's output (grep the value, not the
  function) and state each one's new input."
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed**. §D1.3 'Fold' makes the stored disposition exactly the stated directive, with no channel trigger, agent, hold or outreach input. It states each consumer's new input (audience by directive; every association gets an intent with `admissible`) and documents the Cc behavior change. Invariant 7 and the B1 tests (Cc + `any` → run; FileOnly → intent with `admissible=false`) cover it. D1.4 runs reply correlation before the `admissible` check, so a quiet reply can still resume a wait.

## WF-P08-F002 — D1.5: a reply to any outbound message of an active run is swallowed whenever the run declares any `message.reply` wait

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.5, §D1.4 step 1, invariant 4.
- **Classification:** actionable defect; **High**; correctness / requirements.
- **Evidence:** D1.5 delivers every candidate reply through `record_event_on`. For an active,
  unexpired run, that function returns `Recorded` when **any** `wait.event` step in the frozen graph
  declares the literal name (`src/adapters/persistence/workflow/waits.rs:271-297`). It does not check
  correlation, or whether that step will ever run. D1.5 then counts `Recorded` as matched and
  admits no run. So all of these are recorded, never consumed, and admit nothing:
  - a reply to a different send step than the one awaited (e.g. the customer answers the run's
    `message.reply` while the run waits on a supplier send);
  - a reply after the awaited wait expired, while the run continues on another branch;
  - a reply to a send whose message id no wait ever correlates on.

  The customer's follow-up gets no run and no answer. This contradicts plan §Other entry points
  ("Matched replies resume their intended wait; unrelated messages create independent runs") and
  README ("A waiting run never blocks another message in the same conversation"). Swallowed events
  also fill the `MAX_UNCONSUMED_EVENTS = 128` inbox, after which `record_event` returns `Conflict`
  (`waits.rs:303-307`, `src/application/workflow/waits.rs:42`).
- **Required:** define "matched" so that only a reply whose correlation is, or will be, awaited
  counts. For example: match only when the run has an open `wait.event` for (`message.reply`,
  correlation). If event-before-park must be kept, add a precisely bounded rule, such as marking
  the destination "awaited" at publication when the frozen graph has a `message.reply` wait whose
  correlation reference targets the publishing step, and expiring that mark with the wait. State
  what happens after the wait expires. Add tests: a reply to an unawaited send of a waiting run →
  new run; a reply after wait expiry → new run; event-before-park still resumes.
- **Instruction lesson:** observation: the decision reused an existing port (`record_event`) without
  re-reading its acceptance predicate against the new producer's semantics. Candidate (scope:
  gate template): "When a gate gives an existing port a new production caller, quote the port's
  acceptance predicate and show it matches the new caller's meaning of success."
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed**. §D1.5.2–.3 match only when the frozen graph has a `message.reply` wait whose correlation references the publishing step and an open wait exists for the correlation. A settled or expired wait → `Unmatched`. A `model_tool` send is never awaited. Residual edge windows and the cost of `NotYet` deferrals are tracked separately as WF-P08-F014 and WF-P08-F016.

## WF-P08-F003 — D1.5: wait matching drops the responder-identity criterion, and ingress refuses the replies the design needs

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.5 ("Sender authorization already happened at
  ingress"), plan §Other entry points (outreach), REPLACEMENT-MAP rows 80–81.
- **Classification:** actionable defect; **High**; security / correctness.
- **Evidence:**
  - Legacy outreach correlation required the responder to be the target:
    `find_correlated_outreach_reply` matches `target.email = sender` and the provider key in
    references (`src/adapters/persistence/task/operations.rs:695-732`, called from
    `routing.rs:403-420`). That correlation also authorized a sender who was not otherwise a party
    (`authorize_thread_injection`, `routing.rs:696-716`). D1.5 matches on `reply_to_message_id`
    alone and gives no reason for dropping the identity criterion. Any party to the thread (a
    Cc'd third party, or another participant) who replies to the outbound message therefore
    satisfies a wait meant for one recipient. Because only one response per wait counts, the
    intended recipient's real answer then becomes an unrelated new run. REPLACEMENT-MAP row 80:
    "Existing target/channel access checks remain outside prompts". Telling the workflow to check
    `sender` after consuming the wait does not help: the wait is already spent.
  - The opposite failure, for `user` destinations: a `message.send` to a member "always opens its
    own thread in the run channel" (`src/application/workflow/message_runtime.rs:313-345`).
    Publication writes no `thread_principals` (`write_destinations_on`,
    `src/adapters/persistence/workflow/message_publish.rs:150-205`; the only writers are
    `thread/inbound.rs` and `thread/mod.rs`). When the member replies by email, the reply resolves
    to that thread through the outbound mapping. `authorize_thread_injection` then finds the member
    is not a party, there are no trusted directives and no legacy outreach match, so it bounces
    the reply as a thread injection. The reply never reaches an intent, and the plan's outreach
    pattern ("explicit sends and correlated event waits") cannot complete over email.
- **Required:** specify matching criteria that include the responder identity relative to the
  recorded destination:
  - `reply` kind: the source sender;
  - `user` kind: the member principal/address in `workflow_message_destinations.principal_id` /
    `recipient_address`;
  - `channel` kind: an authorized in-app reply.

  Specify how a correlated destination's responder passes ingress thread-injection without
  becoming a general thread party, or record a justified, user-approved relaxation. Tests: a
  wrong-sender reply does not match (and either becomes ordinary ingress or is refused, as
  specified); a member's email reply to a `user` send matches and resumes the wait.
- **Instruction lesson:** observation: an existing authorization invariant (the outreach target
  match) was dropped because the decision assumed the upstream check was equivalent, without
  tracing it. Candidate (scope: `src/application/AGENTS.md` or gate template): "When replacing a
  legacy correlation/authorization path, list each predicate the old path enforced and state where
  the new path enforces it, or why the user approved dropping it."
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed**. §D1.5.4 adds the expected responder per destination kind: `user` → `principal_id`, `reply` → the source author principal, `channel` → an in-app application reply. User sends record the member as a participant of their dedicated thread. Verified: a member's account email resolves to the person principal through the `account` identity (`participant.rs:260-370`) that `member_on` returns, so a member's email reply resolves to the destination's principal. Residual: an allowlist run channel can still refuse that reply at the channel ACL (WF-P08-F016 item 3).

## WF-P08-F004 — D1.5: dedup order contradicts the stated crash guarantee; `deliver_reply` errors and `message.reply` payload schemas are unconstrained

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.5, §D1.4 failure table.
- **Classification:** actionable defect; **Medium**; correctness / failure behavior.
- **Evidence:**
  1. D1.5 says `deliver_reply` checks "if any `message.reply` event already exists for
     `(run, correlation)` → `Unmatched`; else `record_event_on`", and also that
     `event_id = UUIDv5(intent.id)` "so a reprocessed intent is `Duplicate`". In that order, a crash
     after the event is recorded but before the intent outcome is written makes the retry find its
     own event and return `Unmatched`. The retry then admits a new run for a reply that already
     resumed the wait, so the message is processed twice. The id-duplicate check in
     `record_event_on` (`waits.rs:258-270`) must run before the one-response-per-wait check.
  2. `deliver_reply` can fail with `BadRequest` (payload rejected by the wait's schema,
     `waits.rs:300-302`), `Conflict` (event id reuse or a full inbox, `waits.rs:303-312`), or time
     out (2 s, `waits.rs:37-48`). D1.4 classifies errors only for `admit`, so whether these defer,
     fail the intent, or fall through to ordinary admission is undefined.
  3. `message.reply` is a reserved name, but its `payload_schema` is declared by the author and
     only checked for consistency within one workflow (`registry/mod.rs:198-217`). A published
     workflow can declare a schema that rejects `MessageTriggerInput`, and then every reply errors.
     D1.2 already validates a representative instance for the *input* schema at configure time; the
     reserved event needs the same rule at publication, or a registry-fixed schema.
- **Required:** fix the order: dedup by event id first, then one response per (run, correlation),
  under the producer lock. Classify `deliver_reply` outcomes and errors. Constrain
  `message.reply` wait schemas at publication. Add a crash-replay test: event recorded, outcome
  missing, retry → matched and no admission.
- **Instruction lesson:** cause: the decision prose ordered steps informally. No instruction
  change supported beyond the existing "failure modes" table, which should have caught it.
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed**. §D1.5.5 orders the checks: event-id dedup, then the open-wait recheck, then one response per correlation, then record. Outcomes are classified, and the crash-replay row is in the failure table. §D1.5.1 fixes the payload schema in the registry and refuses non-canonical schemas at publication.

## WF-P08-F005 — D1.4/D1.9: transient admission conflicts are classified as permanent refusals, and a transient failure starves later sibling bindings

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.4 step 3, §D1.9 ("a deterministic admission refusal
  ends `failed` immediately"), failure-mode table.
- **Classification:** actionable defect; **Medium**; correctness / failure behavior.
- **Evidence:**
  - D1.4 treats `Conflict` as a deterministic refusal (`refused(code)`, intent `done`). But
    `WorkflowAdmission::admit` returns `AppError::Conflict` from `verify_current` whenever the
    binding's configuration revision, params, resources or bundle changed between the
    `admission_binding` read (its own REPEATABLE READ transaction,
    `src/adapters/persistence/workflow/admission_binding.rs:47-72`) and the admission transaction
    (`admission.rs:57-80`, `conflict()` at `workflow/mod.rs:71-73`). That is a race with a
    concurrent reconfiguration of a still-active binding. A retry would admit under the new
    revision; D1.4 instead permanently drops that binding's run for the message. D1.9 applies the
    same "deterministic" rule to schedule slots, which would fail a slot because of a concurrent
    edit.
  - "Transient error → stop, defer the intent": stopping at the first transient failure means a
    binding that fails transiently every time (for example `invalid()` = `AppError::Database`,
    `workflow/mod.rs:77-79`, or a timeout) uses up all 5 attempts. Bindings later in the iteration
    order then never get an outcome. This contradicts "one refusal never blocks a sibling binding",
    and the schema already has a per-binding `failed` outcome that could be used.
  - Eligibility is evaluated against one configuration and `admit` re-reads the current one, so a
    trigger change between the two can admit a message the new trigger rejects, or the reverse.
- **Required:** distinguish binding-changed conflicts (re-read eligibility and retry within the
  attempt) from binding-inactive or deleted (refused). Continue to sibling bindings after a
  transient failure and record per-binding `failed` when attempts run out. Pin the evaluated
  configuration revision into the admission, or re-evaluate eligibility on conflict. Apply the
  same classification to D1.9. Tests: reconfigure during processing → admitted under the new
  revision; one binding failing transiently does not prevent a sibling's admission.
- **Instruction lesson:** observation: errors were classified by `AppError` variant without
  checking which call sites produce each variant. Candidate (scope: `src/application/AGENTS.md`
  §"Keep failure states distinct"): "Classify retryable vs terminal by the producing condition, not
  the `AppError` variant; list the producers of each variant you treat as terminal."
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed**. §D1.4.5 adds `AdmissionResult::Stale` with an expected binding revision (re-read up to 3 times), classifies outcomes by producing condition, and makes transient errors per-binding with `failed(transient_exhausted)` on the final attempt. §D1.9 reuses the same classification. Note: `AppError::Conflict` raised by `admission_binding::saved_run` falls into the bounded transient path, which is acceptable.

## WF-P08-F006 — D1.1: run-as selection weakens the existing "admin may only attribute automation to themselves" rule, and silently re-attributes on reconfigure

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.1; user decision P08-Q2.
- **Classification:** actionable defect; **Medium**; security / authorization.
- **Evidence:** existing schedule run-as policy
  (`src/application/use_cases/schedule.rs:229-281`): "The company's owner may attribute a run to
  anybody on the team; an admin may attribute one only to themselves, so an admin cannot make the
  company's automation act as a colleague". Also: "Leaving a stored attribution alone is not a
  change", so an admin may keep an owner-chosen value. D1.1 lets any configuring owner/admin name
  any current owner/admin as `run_as`, which drops the first rule. It also makes `None` mean "the
  configuring actor", so every reconfigure (for example a params edit) by a different admin
  silently moves automatic authority and attribution to that admin, which drops the second rule.
  Separately, P08-Q2 requires that loss of admin "refuses admissions visibly". D1.4 records a
  `refused` row but names no surface in p08 (route, attention item, binding status, or metric/log
  field), whereas the legacy schedule failed "loudly onto `last_error`".
- **Required:** keep the legacy attribution rule, or obtain and record a user decision to relax
  it. On reconfigure, `None` keeps the stored run-as (still revalidated). Name where refused
  admissions are visible in p08. Tests: an admin's attempt to set another admin/owner as run-as is
  refused; an admin's params edit keeps the owner-chosen run-as.
- **Instruction lesson:** observation: the user decision (P08-Q2) stated the new rule, and the
  pre-existing finer-grained protection on the same concept was not searched. Candidate (scope:
  gate template): "For each new authority field, grep the codebase's existing rules on the same
  concept (here `run_as`) and state their disposition."
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed** for the selection rule (owner may name any owner/admin; an admin may name only themselves; `None` keeps the stored run-as on reconfigure) and the refusal surface (§D1.4.6 log, metric, B3 route, B4 attention). Rev 2 also introduced a new defect in the same section's schema (a composite FK to company members), tracked as WF-P08-F013.

## WF-P08-F007 — D1.0: the batch plan omits the 06C obligation to ship `create_agent_channel` and `list_company_agents` as workflow tools

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.0; TASK.md acceptance criterion 13.
- **Classification:** actionable defect; **Medium**; requirements coverage.
- **Evidence:** RESUME "Phase-08 obligations (from 06C)" (`plan/workflow-foundation/RESUME.md:55-56`):
  "create_agent_channel + list_company_agents as workflow tools and Agent Builder creation (revise
  its prompt: workflow tools, explicit provider/model)". RESUME:17-19 adds "ToolId comment wording
  (value_objects.rs) vs workflow tool diagnostics". REPLACEMENT-MAP row 91 requires replacing the
  native `create_agent_channel` with authorized, idempotent provisioning invoked by an agent. D1.0
  schedules "Agent Builder" retargeting (B3) and the user-facing "Create assistant channel" setup
  (B2), but neither workflow tool appears in any batch. TASK criterion 13 omits them as well. If
  these tools need a built-in provisioning action (no HTTP/MCP registration exists for internal
  provisioning), that is a cross-subsystem contract and may itself need a gate.
- **Required:** schedule both tools, and the ToolId wording item, in a named batch with
  verification, or record an explicit disposition. Say whether their action path needs a gate.
- **Instruction lesson:** observation: TASK derived its criteria from RESUME and lost a clause
  while condensing. Candidate (scope: workflow skill, TASK template): "Copy each original
  obligation verbatim into the criteria list before condensing."
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed**. §D1.0 schedules both tools in B3 behind gate P08-D2 and the ToolId wording item in B4. TASK criterion 13 now quotes RESUME verbatim.

## WF-P08-F008 — D1.3/D1.4: intent-queue protocol is under-specified (fence token, outcome fencing, work bound vs lease, fairness, tenant FKs)

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.3 queue columns, §D1.4 claim.
- **Classification:** actionable defect; **Low**; concurrency / bounds.
- **Evidence and required:**
  1. Fence: D1.3 lists `worker_id` and `lease_expires_at` but no per-claim execution token, while
     the queue it copies keeps one (`schedule_runs.materialization_generation`, schema 5664-5684).
     `src/adapters/persistence/AGENTS.md` §Fence: "worker id alone is also insufficient if worker
     ids may be reused". Add a per-claim generation and require it in finish, defer and **outcome
     writes**. Otherwise a stale worker can write `refused` for a binding after the replacement
     wrote `admitted`, or the reverse.
  2. Bound vs lease: there is a 60 s lease, no renewal, and an unbounded number of active
     message-triggered bindings per channel, each costing one admission transaction. Bound the
     bindings per channel at configure/activate, or renew the lease per binding.
  3. Fairness: "at most 4 per company per page (window function)" cannot be one `FOR UPDATE SKIP
     LOCKED` select, because PostgreSQL rejects `FOR UPDATE` with window functions. If ranking
     happens after locking the 32 oldest rows, one tenant's backlog fills every page and other
     tenants wait. `src/application/AGENTS.md`: "one tenant's burst must not starve every other
     tenant". Rank per company before the page limit, or reuse the existing fair-polling pattern.
     Add a burst test.
  4. Tenant integrity: specify composite `(company_id, …)` FKs for message, channel, thread,
     binding and run on both new tables, with a rejection test (persistence AGENTS §Preserve
     invariants).
- **Instruction lesson:** cause: "as for `schedule_runs`" was used as shorthand without copying the
  fence column. No new instruction is needed; the existing persistence rules cover it.
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed**. §D1.3 adds a `claim_generation` and state CHECKs. §D1.4.1 ranks per company in a CTE, then locks the selected ids with SKIP LOCKED, and generation-fences every later write including outcome rows. Tests cover poison batch and tenant burst. §D1.2 bounds message bindings to 8 per channel. §D1.3 adds composite FKs with rejection tests. Residual: the bound can be bypassed through configure, tracked as WF-P08-F017.

## WF-P08-F009 — D1.7: `in_app_only` shape for `user` destinations conflicts with the destination CHECK

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.7.
- **Classification:** actionable defect; **Low**; schema contract.
- **Evidence:** D1.7: "reply/user email targets become in-app publication". The schema requires
  `delivery_id`, `principal_id` and `recipient_address` NOT NULL for `kind='user'`
  (`workflow_message_destinations_check`, schema line 7222). D1 doesn't say whether an in-app
  user destination changes the CHECK, becomes another kind, or posts to the run channel. That
  choice also affects `workflow_message_aggregate` and the message result the run records. The
  affected-contracts list doesn't name the destination table or its SQL guards.
- **Required:** state the `in_app_only` row shape per destination kind and list the
  schema/guard changes. Test: an in-app-only run with a `user` destination writes no delivery and
  still yields a valid message result.
- **Instruction lesson:** cause unknown. No change supported.
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed**. §D1.7 adds the `in_app` column, a CHECK per kind, the guard update, and local-publication receipts. This is consistent with `workflow_message_aggregate` (a NULL delivery counts as local) and with the dispatch commit guard (no wait when there is no delivery).

## WF-P08-F010 — D1.8: pool budget, shutdown bridge and lifecycle tests are missing from the composition contract

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.8 and verification plan.
- **Classification:** actionable defect; **Low**; operations / verification.
- **Evidence:**
  - `WORKFLOW_WORKER_SLOTS` can be up to 32 while the default pool has 10 connections
    (`src/infra/db.rs:9-12`). The listener, task worker, delivery, notification and inbound
    workers already share that pool. A slot holds a lease renewal plus handler transactions. Under
    connection starvation, acquire timeouts let leases lapse and consume attempts.
    `src/infra/AGENTS.md`: "Account for dedicated connections ... when sizing the database pool"
    and "Validate ... limits, and mutually dependent settings at startup".
  - D1.8 says loops are "supervised with the existing shutdown token/JoinSet", but `serve` uses a
    `broadcast` channel and `join_background` (`src/main.rs:204-330`), and `WorkflowWorker::run`
    takes a `CancellationToken` (`src/application/workflow/worker.rs:56`). A bridge is needed.
  - The verification plan has no lifecycle tests for the new loops. `src/infra/AGENTS.md` requires
    them: shutdown between iterations and during active work, lease disposition, no orphaned
    tasks.
- **Required:** validate slots against the pool at startup (or derive the pool size from them).
  Describe the token bridge. Add lifecycle tests for the intent worker and the composed workflow
  worker.
- **Instruction lesson:** cause unknown. No change supported.
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed** for the token bridge, lifecycle tests and the existence of a pool check. The check as specified fails under the repository defaults, a new issue tracked as WF-P08-F015.

## WF-P08-F011 — D1.9: authority, visibility, revalidation and verification gaps in the schedule contract

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.9 (implemented in B2).
- **Classification:** actionable defect; **Low**; requirements / authorization / verification.
- **Evidence and required:**
  1. D1.9 doesn't state who may create, edit or run-now a schedule. Legacy uses `managed_schedule`
     (`schedule.rs:460-477`), and a run-now acts as the binding's run-as. Require the same
     owner/admin authority as `ManageBinding`, so a weaker caller can't trigger work that runs
     with admin authority.
  2. A binding with association `Company` is valid for a schedule on any channel. Configure only
     checks run-as visibility for `Company`, so an allowlisted schedule channel the run-as cannot
     view refuses every slot. Check run-as visibility to the schedule's channel when the schedule
     is saved.
  3. "Schedule save validates the input against the bound version". A later reconfigure of that
     binding to another version is not revalidated against `ScheduleTriggerInput`. Revalidate
     every schedule referencing the binding on configure, as D1.2 does for message triggers.
  4. There is no D1.9 verification list. REPLACEMENT-MAP row 56 gates removal on "duplicate-slot
     and retry-without-second-agent-call tests". RESUME notes that no test drives a scheduled run
     through the agent runtime. Add these, plus run-now `command_key` replay and the refusal/failed
     shape.
- **Instruction lesson:** cause: B2 scope was deferred to its batch review, but the decision fixes
  the schema now. No new instruction is needed.
- **Resolution history:** 2026-10-22 open (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): **fixed**. §D1.9 adds authority (`ManageBinding` plus channel visibility), compatibility and run-as visibility checks at save, revalidation of every referencing schedule on binding configure, and the B2 test list.

## WF-P08-F012 — Optional suggestions (non-blocking)

- **Context:** WF-P08-R001, P08-D1 rev 1 §D1.4, §D1.6, §D1.0.
- **Classification:** optional suggestion; **Info**; design.
- **Evidence and suggestion:**
  1. Eligibility is evaluated at processing time, so an intent committed *before* a binding was
     activated is admitted if it is still pending or deferred when activation lands (for example
     after a database outage backoff). Consider bounding eligibility by the binding's activation
     time, or documenting this as intended, as was done for deactivation.
  2. D1.6 makes the intent row the only durable owner of the relay trace. Any future retention
     sweep of done intents would reset loop protection for later workflow replies. Say so in the
     schema comment.
  3. B2's "Create assistant channel" has to put a selected agent into a template whose agent is
     a literal slug (`fixtures/autonomous-response.yaml`). Copying and publishing per agent, versus
     a resource or parameter, is a cross-subsystem choice. Decide early whether it needs a B2 gate.
  4. WF-C002 targets p09, but p08 B2 ships JSON run routes (P08-Q3). If they list runs by binding,
     apply WF-C002's rule now (filter `parent_run_id IS NULL`, or label children), so p09 doesn't
     inherit a contract that mixes children into binding lists.
  5. Process: B2 combines schedules, manual start, the whole company route surface, the decision
     link page and assistant setup. Consider splitting it at the schedule/manual-start contract so
     its single batch review stays thorough. B1 also composes `DecisionNotices`, whose links target
     the B2 page, so between B1 and B2 the links are dead. That is acceptable only because
     acceptance is phase-level.
- **Instruction lesson:** n/a.
- **Resolution history:** 2026-10-22 open as optional (WF-P08-R001).
  2026-10-22 recheck WF-P08-R002 against rev 2 (`logs/decision-D1-r2.md`, sha256 `5271630f…0c63a`): items 1 (`activated_at` eligibility), 2 (relay retention comment), 4 (WF-C002 list rule in B3) and 5 (four batches) were **adopted**. Item 3 was **deferred** to gate P08-D2 before B3. Accepted.

---

Shared context for the findings of round **WF-P08-R002**: date 2026-10-22; workflow
`implement-opus-reviewed-2_2`; records as above; batch B1 + D1.9 + D1.0; stage **architecture gate
correction recheck, P08-D1 rev 2** (`logs/decision-D1-r2.md`, sha256
`5271630feb82ecdb20f006ffb2ce9611642dbc6b5890e1dd06348699fda0c63a`; diff
`logs/decision-D1-r1-to-r2.diff`); notes [WF-P08-R002](agent-review-notes.md#wf-p08-r002);
reviewer B, Opus 5.5 / xhigh; phase `p08`; worktree `/Users/mac02/dev/openbot`; HEAD = base
`da868710d3b235d49856e3e481f8f3735f6b3e56`, digest
`e07e4b1d4a5f9491a6fd1115d251a416d56955383a564f249531e9ba565b47e9`. All findings below originate in
rev 2's text.

## WF-P08-F013 — D1.1 rev 2: the composite FK from `run_as_user_id` to company members blocks removing any member who was ever a run-as

- **Context:** WF-P08-R002, P08-D1 rev 2 §D1.1 ("`run_as_user_id uuid NOT NULL` (composite FK to
  company members)"). Introduced by rev 2.
- **Classification:** actionable defect; **Medium**; schema / authorization lifecycle.
- **Evidence:**
  - `remove_member` deletes the `company_members` row (`src/adapters/persistence/company_invite.rs:355-380`).
  - Binding revisions are retained history: `workflow_bindings_selected_revision_fk` (schema 13907)
    and the binding-events FK (13851) reference them with NO ACTION.
  - So with the default NO ACTION/RESTRICT on the new FK, removing an admin who appears as run-as
    in any revision, current or historical, fails with an FK violation.
  - With CASCADE, the cascade has to delete revision rows that the head and events still
    reference, so the removal fails anyway (or, if it succeeded, would erase binding history).
    SET NULL conflicts with NOT NULL.
  - P08-Q2 requires the opposite: "loss of admin refuses admissions visibly". The existing
    `workflow_binding_revisions.actor_id` and `workflow_runs.actor_id` carry no member FK, and
    authority is rechecked at use.
- **Required:** do not FK `run_as_user_id` to `company_members`. Mirror `actor_id` (no FK, or at
  most an FK to `users`) and rely on the configure/activate/admission rechecks. Add a test:
  removing an admin who is a binding's run-as succeeds, and the next admission is `refused`, with
  the D1.4.6 warning, metric and route surface.
- **Instruction lesson:** observation: a tenant-integrity rule ("tenant id participates in FKs")
  was applied to a reference whose target row is expected to disappear. Candidate (scope:
  `src/adapters/persistence/AGENTS.md` §Preserve invariants): "Before adding an FK, state what
  happens when the referenced row is deleted by its normal lifecycle (member removal, deactivation)
  and test that path; references to revocable membership are rechecked at use, not FK'd."
- **Resolution history:** 2026-10-22 open (WF-P08-R002).
  2026-10-22 recheck WF-P08-R003 against rev 3 (`logs/decision-D1-r3.md`, sha256 `7c4b9457…21aaf7`): **fixed**. §D1.1 states that `run_as_user_id` has no foreign key, like `actor_id`. A failure-table row and a test cover member removal followed by a refused next admission.

## WF-P08-F014 — D1.4/D1.5 rev 2: `NotYet` deferrals consume the 5-attempt poison budget and can fail the intent before the 5-minute grace ends

- **Context:** WF-P08-R002, P08-D1 rev 2 §D1.4.1 (each claim sets `attempts+1`; "deferred at 5 →
  `failed`"), §D1.4.2 (`NotYet` → defer), §D1.5.3 (`REPLY_PARK_GRACE = 5 min`). Introduced by
  rev 2.
- **Classification:** actionable defect; **Medium**; correctness / queue bounds.
- **Evidence:**
  - `NotYet` is a designed, non-error state: the reply arrives before its wait parks, which
    happens whenever the run is backlogged or still finishing the send step.
  - Rev 2 defers by re-queuing, and every re-claim spends an attempt. Rechecking "until W parks"
    with any backoff whose first four delays total less than 5 minutes exhausts the budget, and
    the intent goes to `failed`.
  - A `failed` intent ends with no match and no admission for any binding. The message is silently
    unprocessed (only logged), which breaks invariant 4 ("anything else is ordinary ingress") and
    plan §Other entry points.
  - The failure table row "Reply before the wait parks → deferred until park, run end, or 5 min"
    is not achievable under the stated attempt rule.
- **Required:** make `NotYet` deferrals attempt-neutral, for example a separate `not_yet_until`
  or a refund on a `NotYet` outcome, with their own bound: the grace deadline. Specify the recheck
  interval. When the grace ends the result is `Unmatched` and processing continues to admission.
  Test: W parks after more than 5 rechecks and the reply still matches; when the grace expires
  with no park, the reply is admitted as a new run and the intent is not `failed`.
- **Instruction lesson:** observation: a new non-error deferral was added to a queue whose attempt
  counter is a poison budget. Candidate (scope: `src/application/AGENTS.md` §"Keep failure states
  distinct"): "Deferrals for a known, non-error waiting condition must not spend the poison/attempt
  budget; give them their own deadline."
- **Resolution history:** 2026-10-22 open (WF-P08-R002).
  2026-10-22 recheck WF-P08-R003 against rev 3 (`logs/decision-D1-r3.md`, sha256 `7c4b9457…21aaf7`): **fixed**. §D1.4.1: a `NotYet` defer is fenced, gives the attempt back, and schedules `available_at = least(now()+2 s×2^k, created_at+grace)`. §D1.5.3: at or after the grace deadline the reply is `Unmatched` and processing continues to admission in the same pass. Tests cover more than 5 rechecks and fall-through at the grace deadline. Each `NotYet` row moves `available_at` into the future, so the poison-batch rule still holds. Note: `k` is undefined; see WF-P08-F020.

## WF-P08-F015 — D1.8 rev 2: the pool check refuses the repository's default configuration

- **Context:** WF-P08-R002, P08-D1 rev 2 §D1.8 "Pool budget". Introduced by rev 2 (the F010
  correction).
- **Classification:** actionable defect; **Low**; operations / configuration defaults.
- **Evidence:**
  - The check refuses startup when `DATABASE_MAX_CONNECTIONS < 8 + 2 × WORKFLOW_WORKER_SLOTS`.
  - With the defaults (`WORKFLOW_WORKER_SLOTS = 2`) it requires at least 12 connections.
  - The pool default is 10 (`src/infra/db.rs:9-12`), `.env.example:3` shows `10`, and `fly.toml`
    sets no override.
  - So `serve` would refuse to start in every default environment, including production.
  - `src/AGENTS.md` / `src/infra/AGENTS.md`: defaults must be production-safe.
- **Required:** raise the pool default (and `.env.example` and the docs) to at least
  `8 + 2 × default slots`, or derive the pool size from the slots. Include the defaults in the
  startup-check test.
- **Instruction lesson:** cause: the new constraint was checked against the new key only, not the
  existing default of the key it constrains. No new instruction is needed.
- **Resolution history:** 2026-10-22 open (WF-P08-R002).
  2026-10-22 recheck WF-P08-R003 against rev 3 (`logs/decision-D1-r3.md`, sha256 `7c4b9457…21aaf7`): **fixed**. §D1.8 sets the pool default to 20 in `db.rs`, `.env.example` and the docs, and a test pins the defaults against the check.

## WF-P08-F016 — D1.5 rev 2: remaining windows where an awaited reply is swallowed or refused

- **Context:** WF-P08-R002, P08-D1 rev 2 §D1.5.3–.5, invariant 4.
- **Classification:** actionable defect; **Low**; correctness edge cases.
- **Evidence and required:**
  1. **Open-wait check vs expiry race.** The open-wait recheck in step (b) runs under the run's
     producer advisory lock. `record_event_on` does not lock the wait or run row, and expiry
     (`wait_commit.rs`, `expire`, which updates `workflow_waits ... state='waiting'`) does not take
     that advisory lock. A wait can therefore expire between (b) and commit, and the event is then
     recorded against a settled wait. Required: lock the wait row `FOR SHARE` in (b), and require
     `deadline > now()` there.
  2. **Expiry beats a recorded event.** `resume_on` expires an event wait whose deadline passed
     (`wait_commit.rs:51`) before it looks for a matching event (`:96`). A reply recorded shortly
     before the deadline but resumed after it is never consumed, and it was already counted as
     matched, so no run is admitted. Required: either consume an event recorded before the wait's
     deadline, or release unconsumed `message.reply` events of an expired wait to ordinary
     admission.
  3. **Allowlist ACL.** D1.5.4 lets a user-send recipient pass the *thread* check. On an
     allowlist run channel, a member who is not listed and not the owner is still refused by the
     channel ACL (`Channel::participant_access`, `src/domain/entities/channel.rs:221-224`), so the
     reply bounces. Required: either require the member to be authorized on the sender channel when
     a `user` destination is planned, or state that such sends are not awaitable and refuse waits
     on them.
- **Instruction lesson:** cause unknown. No change supported.
- **Resolution history:** 2026-10-22 open (WF-P08-R002).
  2026-10-22 recheck WF-P08-R003 against rev 3 (`logs/decision-D1-r3.md`, sha256 `7c4b9457…21aaf7`):
  - **Item 1, fixed in intent:** the wait row is locked `FOR SHARE` and `deadline > clock_timestamp()` is required. The stated serialization does not hold under the existing lock order, so the lock order is tracked as WF-P08-F019.
  - **Item 2, fixed:** new §D1.5.6 consumes an event recorded at or before the deadline ahead of expiry, for every `wait.event`. `record_event` previously had no production caller, so this changes no shipped behavior.
  - **Item 3, dismissed: my original premise was wrong, and the rev 3 reading is verified in code.** In `authorize_channel` (`routing.rs:325-343`), an external sender refused by the ACL gets `thread_access_for`. That function resolves `find_ancestor_thread` through the channel's single email binding (`routing.rs:485-507`), which is the same binding under which `store_email_correlated` maps the outbound user-send key. It returns `Participant` when `is_party_to` holds, and D1.5.4 makes the member's person principal a party of that thread. The account identity resolves the member's email to that same principal (`participant.rs:260-370`). The spam check sees `trusted`, since team membership makes the member trusted. §D1.5.7 adds the allowlist test.

## WF-P08-F017 — D1.2 rev 2: the 8-binding bound is checked only at activation, so configure bypasses it

- **Context:** WF-P08-R002, P08-D1 rev 2 §D1.2 "Bound".
- **Classification:** actionable defect; **Low**; bounds.
- **Evidence:** `BindingService::configure` keeps an active binding active
  (`src/application/workflow/lifecycle/bindings.rs:12-19`: `(next(..), state.active)`). Configuring
  an already active binding to change `message_trigger` from NULL to a rule therefore never passes
  through the activation-time check, and the channel can exceed 8 message-triggered bindings. That
  breaks the D1.4 work bound the lease relies on.
- **Required:** apply the same check, under the same per-channel lock, when configure turns on a
  `message_trigger` for an active binding. Test it.
- **Instruction lesson:** n/a.
- **Resolution history:** 2026-10-22 open (WF-P08-R002).
  2026-10-22 recheck WF-P08-R003 against rev 3 (`logs/decision-D1-r3.md`, sha256 `7c4b9457…21aaf7`): **fixed**. §D1.2 checks the cap in `save_binding` for every change that results in an active message-triggered binding, under a `(company, channel)` advisory lock taken after the company lock, excluding the binding itself. There is a test for configure-while-active.

## WF-P08-F018 — Optional suggestions from the recheck (non-blocking)

- **Context:** WF-P08-R002, P08-D1 rev 2 §D1.5.1/.3, TASK.md, COORDINATION r5.
- **Classification:** optional suggestion; **Info**.
- **Suggestions:**
  1. A `message.reply` wait whose correlation is not a reference into a same-graph message step's
     `messages/<n>/message_id` can never match under D1.5.3 (for example a child workflow waiting
     on `/input/…`). Consider refusing or warning at publication alongside the registry-fixed
     payload rule, so authors learn this before a wait silently runs to its deadline.
  2. Records: TASK.md §Coordination still says WF-C003 routes are "B2". COORDINATION r5's p08
     response says "scheduled in B2 routes". Rev 2 moved routes to B3. Update both at the next
     coordination revision.
- **Resolution history:** 2026-10-22 open as optional (WF-P08-R002).
  2026-10-22 recheck WF-P08-R003 against rev 3 (`logs/decision-D1-r3.md`, sha256 `7c4b9457…21aaf7`): item 1 was **adopted** in §D1.5.1. Item 2 was **done**: COORDINATION r6 (sha256 `23fdc190…c4db`, retained copy `logs/coordination-r6.md`) changes only the p08 responses, putting WF-C001 composition in B1, manual start/rerun in B2 and WF-C003 routes in B3. No notice requirement changed. TASK §Coordination was updated to match.

---

Shared context for the findings of round **WF-P08-R003**: date 2026-10-22; workflow
`implement-opus-reviewed-2_2`; records as above; batch B1 + D1.9 + D1.0; stage **architecture gate
correction recheck, P08-D1 rev 3** (`logs/decision-D1-r3.md`, sha256
`7c4b9457e146890eaaaa2b8936324726dcdd2da45f86333e7125eb8e2121aaf7`; diff
`logs/decision-D1-r2-to-r3.diff`); notes [WF-P08-R003](agent-review-notes.md#wf-p08-r003);
reviewer B, Opus 5.5 / xhigh; phase `p08`; worktree `/Users/mac02/dev/openbot`; HEAD = base
`da868710d3b235d49856e3e481f8f3735f6b3e56`, digest
`e07e4b1d4a5f9491a6fd1115d251a416d56955383a564f249531e9ba565b47e9`; coordination r6 `23fdc190…c4db`.

## WF-P08-F019 — D1.5.5(b) rev 3: locking the wait row before the run row inverts `resume_on`'s lock order

- **Context:** WF-P08-R003, P08-D1 rev 3 §D1.5.5(b) ("locking that `workflow_waits` row `FOR
  SHARE` … (an expiring transition's row update then serializes after this commit)"). Introduced
  by rev 3.
- **Classification:** actionable defect; **Low**; concurrency. Implementation-level and **not
  gate-blocking**: it changes no contract, schema or protocol boundary of P08-D1, only the
  acquisition order inside `deliver_reply`. It is a **mandatory check in the B1 batch review**.
- **Evidence:**
  - `resume_on` locks in this order:
    1. `workflow_runs FOR UPDATE` (`waits::lock`, `waits.rs:103`; and `maintenance::expire_on`);
    2. execution and job `FOR UPDATE`;
    3. the wait row `FOR UPDATE` (`waits::row`, `waits.rs:148`).
  - In the rev 3 order, `deliver_reply` takes the producer advisory lock, then the wait row
    `FOR SHARE`, then inserts into `workflow_wait_events`. That insert's FK
    `(company_id, run_id) → workflow_runs` (schema 14391) needs `FOR KEY SHARE` on the run row,
    which conflicts with the run row's `FOR UPDATE`.
  - So when a reply arrives while expiry or resume is in progress, each transaction holds the lock
    the other needs, and the two deadlock. Detection (or the 1 s `lock_timeout` both paths set)
    aborts one of them.
  - The outcome is still correct after retry:
    - If `deliver_reply` aborts, the retry sees the expired wait and returns `Unmatched`, so the
      message goes to admission.
    - If resume aborts, the event is committed and D1.5.6 then consumes it.
  - But the decision's claim that expiry "serializes after this commit" is false. The race test
    in the verification plan would show deadlock errors, and each one spends a transient intent
    attempt.
- **Required (implementation):** in `deliver_reply`, take the run row first, in `resume_on`'s
  order, for example `SELECT … FROM workflow_runs … FOR KEY SHARE` (which blocks behind a
  `FOR UPDATE` holder), then the wait row `FOR SHARE`. The "wait-row lock vs concurrent expiry"
  test must assert there is no deadlock or lock-timeout error, as well as the outcome.
- **Instruction lesson:** observation: a new lock was added to a path without listing the
  existing lock order of the transactions it races. Candidate (scope:
  `src/adapters/persistence/AGENTS.md` §Preserve the existing correctness guards): "When a
  decision adds a row lock to race an existing transaction, state both transactions' full lock
  order, including FK-induced `KEY SHARE` locks, and show they are consistent."
- **Resolution history:** 2026-10-22 open (WF-P08-R003); deferred to B1 implementation and batch
  review as non-gate-blocking.
  2026-10-22 batch review WF-P08-R004 (digest `90d4e0ab…7390`): **fixed**. `deliver_on` takes the producer advisory lock, then the run row `FOR KEY SHARE`, then the wait rows `FOR SHARE` (`reply_waits.rs:89-130`), which is consistent with `resume_on`'s run→wait order. The producer advisory lock is taken only by producers (`waits.rs:259`, `reply_waits.rs:67`), so no inversion remains. `message_reply_racing_wait_expiry_agrees_without_deadlock` runs leads of 0, 40 and 80 ms and asserts that neither side errors and that both agree. It passed in my rerun (`logs/r004-focused.log`).

## WF-P08-F020 — Optional precision notes on rev 3 (non-blocking)

- **Context:** WF-P08-R003, P08-D1 rev 3 §D1.4.1, §D1.5.5–.6.
- **Classification:** optional suggestion; **Info**.
- **Suggestions:**
  1. `available_at = least(now() + 2 s × 2^k, …)` does not define `k`. Attempts are refunded, so
     `k` can't be derived from them. Use elapsed time since `created_at`, or a small counter.
     Either way the grace deadline bounds it.
  2. `workflow_wait_events.created_at` defaults to `clock_timestamp()` at insert (schema 7472),
     which runs after the (b) check. A delivery that passes (b) a few microseconds before the
     deadline can therefore record `created_at > deadline` and still be expired under D1.5.6.
     Compare against the same instant, for example by inserting the timestamp (b) used.
  3. A wait whose deadline equals the run deadline is still ruled by run expiry (`expire_on`
     first). A reply recorded in the last seconds before the run deadline is then left unconsumed
     and not admitted. If that is acceptable because the run failed, say so in the failure table.
- **Resolution history:** 2026-10-22 open as optional (WF-P08-R003).
  2026-10-22 batch review WF-P08-R004:
  - **Item 1 adopted:** the `NotYet` delay grows with the reply's age, `greatest(2 s, now - created_at)`, capped at the grace deadline (`message_intents.rs`).
  - **Item 2 adopted:** one `clock_timestamp()` decides openness and stamps `created_at` (`reply_waits.rs:116-176`).
  - **Item 3 not addressed:** carried as WF-P08-F027.4 (optional).

---

Shared context for the findings of round **WF-P08-R004**: date 2026-10-22; workflow
`implement-opus-reviewed-2_2`; records as above; batch **B1** (engine composition and message
ingress); stage **batch review** against P08-D1 rev 3 (`7c4b9457…21aaf7`); notes
[WF-P08-R004](agent-review-notes.md#wf-p08-r004); reviewer B, Opus 5.5 / xhigh; phase `p08`;
worktree `/Users/mac02/dev/openbot`. Base and HEAD are `da868710d3b235d49856e3e481f8f3735f6b3e56`
and the whole batch is the working tree. Digest
`90d4e0abbb34a6426258697b3459e8ba3dbdc6889dfeb8fba163f764370e7390`, exclusions
`plan/workflow-foundation/phases/p08/docs plan/workflow-foundation/COORDINATION.md marketing
website`. Coordination r6 `23fdc190…c4db`. These findings concern the implementation, not the
gated decision.

## WF-P08-F021 — Responder identity for `reply` and `channel` destinations is untested

- **Context:** WF-P08-R004, B1. Contract: P08-D1 rev 3 §D1.5.4 and invariant 4. Verification
  plan: "wrong responder → new run".
- **Classification:** actionable defect; **Medium**; verification / security.
- **Evidence:**
  - `expected_responder` (`src/adapters/persistence/workflow/reply_waits.rs:190-216`) is the
    authorization predicate that replaced the legacy outreach target match (WF-P08-F003). It has
    three branches:
    - `user`: the destination principal;
    - `reply`: the author of the answered source message;
    - `channel`: an in-app (`application` origin) reply.
  - Every reply test drives only the `user` branch. `question_source` sends to
    `{"kind":"user"}` (`message_reply_tests.rs:22-45`), and the e2e answer test also asks a member
    (`message_ingress_tests.rs:403-426`).
  - No test reaches `deliver_reply` with a `reply` or `channel` destination (no other test file
    calls `deliver_reply`). So nothing proves these behaviours:
    - a customer's answer to a workflow's awaited `message.reply` resumes the wait;
    - a different party in the thread, such as a Cc'd third party, is ordinary ingress;
    - an email reply to a `channel` post is refused while a member's in-app reply matches.
  - A wrong `destination_id` or kind mapping in those branches would pass the suite.
- **Required:** add DB-backed tests through `deliver_reply`, or the worker, for both kinds:
  - `reply`: the source's author matches; another thread party is `Unmatched`, so the message is
    admitted as ordinary ingress.
  - `channel`: a member's in-app reply (origin `application`) matches; an `external` or
    `internal` origin reply to the same post is `Unmatched`.
- **Instruction lesson:** observation: a test matrix was written for one branch of a per-kind
  rule. Candidate (scope: gate verification plans): "When a rule is per kind or per variant, the
  verification plan names each variant."
- **Resolution history:** 2026-10-22 open (WF-P08-R004).
  2026-10-22 recheck WF-P08-R005 (digest `500ae85b…1b11`): **fixed**. The reply fixture now asks through `Ask::{Member, Customer, Channel}` (`message_reply_tests.rs:24-215`). The expected responder is read back from the stored destination: `principal_id`, or the author of the answered message. New tests:
  - `message_reply_to_a_workflow_reply_matches_only_the_customer_it_answered`: another member's reply is admitted by the fallback binding and the wait is untouched; the source author matches.
  - `message_reply_to_a_channel_post_matches_only_an_in_app_answer`: replies with origin `external` or `internal` are unmatched; `application` matches.

  Both passed in my reruns.

## WF-P08-F022 — `.env.example` still documents a pool default the new startup check refuses

- **Context:** WF-P08-R004, B1. Contract: §D1.8 r3 (F015): "The default `DATABASE_MAX_CONNECTIONS`
  becomes 20 (`src/infra/db.rs`, `.env.example`, configuration docs)"; "docs/examples state the
  relation".
- **Classification:** actionable defect; **Low**; configuration / documentation.
- **Evidence:**
  - `.env.example:2-3` still reads "Maximum number of connections in the database pool
    (default: 10)" with `#DATABASE_MAX_CONNECTIONS=10`.
  - With the default 2 slots, `worker_slots` needs 12 connections (`src/infra/workflow.rs:83-103`).
    An operator who uncomments the example therefore gets a refused startup.
  - `WORKFLOW_WORKER_SLOTS`, a key the application now reads (`src/infra/setup.rs`), appears in no
    doc or example.
  - Root `AGENTS.md`: production documentation and examples must name the configuration keys the
    application actually reads.
- **Required:** update `.env.example`: default 20, an example value that passes, and
  `WORKFLOW_WORKER_SLOTS` with its range and the relation `8 + 2 × slots`. Update the
  configuration docs as well, or record an explicit deferral to B4 with this finding linked.
- **Instruction lesson:** cause: the code side of a contract item was done and its documentation
  half was left implicit. No new instruction is needed (root `AGENTS.md` covers it).
- **Resolution history:** 2026-10-22 open (WF-P08-R004).
  2026-10-22 recheck WF-P08-R005 (digest `500ae85b…1b11`): **fixed**. `.env.example` now gives default 20 and the `8 + 2 × slots` relation, with `#WORKFLOW_WORKER_SLOTS=2` and its range. `docs/deploy.md` adds both keys to the settings table.

## WF-P08-F023 — A terminal intent failure is neither logged nor counted

- **Context:** WF-P08-R004, B1. Contract: §D1.4.1 (`failed` "observable, never reclaimed"),
  `src/AGENTS.md` §traceable operations: "Metrics needed for alerts must distinguish retries,
  terminal failures, lease loss, and stuck work".
- **Classification:** actionable defect; **Low**; observability.
- **Evidence:** an intent reaches `failed` in two silent ways:
  1. The sweep at the top of `claim_intents` turns lease-expired attempt-5 claims into `failed`
     (`message_intents.rs:117-128`). It ignores the affected row count and neither logs nor counts.
  2. `defer_intent(Transient)` at attempt 5 writes `state='failed'` (`message_intents.rs:292-299`),
     for example after repeated `deliver_reply` timeouts. Its caller `offer_reply` logs "retrying
     later" even on that last attempt (`message_admission.rs:300-310`).

  In both cases the message is neither matched nor admitted by any binding. Only the per-binding
  refusal counter exists (`workflow_message_admission_refusals_total`), and it does not fire for
  these.
- **Required:** on every terminal transition of an intent (both paths), emit a structured
  error/warn with company and intent, and a counter distinct from refusals, for example
  `workflow_message_intents_failed_total{cause=lease_expired|transient}`. Assert the counter in the
  existing transient and lifecycle tests.
- **Instruction lesson:** n/a (existing rule).
- **Resolution history:** 2026-10-22 open (WF-P08-R004).
  2026-10-22 recheck WF-P08-R005 (digest `500ae85b…1b11`): **fixed**.
  - `MessageAdmissions::claim` calls the new port method `fail_expired_intents`, which returns the failed rows, and logs and counts each one with cause `lease_expired`.
  - `defer` logs and counts cause `transient` when a transient deferral on the last attempt is applied.
  - `claim_intents` no longer sweeps, and still never reclaims attempts ≥ 5.
  - The worker loop claims through `MessageAdmissions::claim`.
  - Two tests assert the counter for each cause.

## WF-P08-F024 — A retried intent can resume a wait after it has already admitted runs

- **Context:** WF-P08-R004, B1. Contract: §D1.4.2 ("`Matched` → done"; a matched reply admits
  nothing) and invariant 4.
- **Classification:** actionable defect; **Low**; correctness (edge).
- **Evidence:**
  - `MessageAdmissions::process` calls `offer_reply` again on every claim of the intent
    (`message_admission.rs:205-218`).
  - In `deliver_on`, an *open* wait is delivered regardless of the park grace. The grace only
    turns "no wait yet" into `Unmatched` (`reply_waits.rs:136-142`).
  - So this sequence is possible:
    1. A reply arrives before its wait parks.
    2. At the grace deadline it falls through as `Unmatched`, and binding A admits a run.
    3. Sibling binding B fails transiently, so the intent is deferred.
    4. The wait W parks.
    5. On the retry, `offer_reply` returns `Recorded`, and `finish_intent(Some(run))` writes
       outcome `matched` over an intent that already admitted A's run.
  - The same message then both started an independent run and answered the wait, and B is never
    admitted. Each step above is reachable under the bounded retry rules.
- **Required:** once an intent has fallen through to admission, never offer it as a reply again.
  For example: skip `offer_reply` when the intent already has binding outcome rows, or persist a
  `reply_checked` flag with the first `Unmatched`. Add a test following the sequence above.
- **Instruction lesson:** observation: a decision step was assumed to run once per message but is
  re-executed on each retry. Candidate (scope: `src/application/AGENTS.md` §Durable effects):
  "A multi-step retryable processor must persist each step's decision, or prove re-running it
  cannot change an earlier step's committed meaning."
- **Resolution history:** 2026-10-22 open (WF-P08-R004).
  2026-10-22 recheck WF-P08-R005 (digest `500ae85b…1b11`): **fixed**.
  - New column `reply_settled`. Its CHECK requires a `reply_to` and forbids it alongside outcome `matched`.
  - `settle_reply` is fenced by the claim and runs only on `Unmatched`.
  - `process` offers a reply only while it is unsettled; `NotYet` does not settle.
  - `message_reply_that_fell_through_is_never_matched_on_a_retry` reproduces the reviewer's sequence. Both bindings are admitted, the wait stays waiting, and no event is recorded.

  Note: settling made the misclassification in WF-P08-F028 durable.

## WF-P08-F025 — Verification gaps against the contract's test list

- **Context:** WF-P08-R004, B1. Contract: §D1.3 r2/r3 (tenant FKs, "a rejection test inserts a
  foreign-company reference for each"), §D1.7 (policy in replay equivalence, policy from the
  intent), D-3 (the trigger change).
- **Classification:** actionable defect; **Low**; verification.
- **Evidence:**
  1. `intent_rows_cannot_reach_across_tenants` (`inbound_multi_task_tests.rs:243-288`) covers the
     association genuinely. Its binding case uses `Uuid::new_v4()`, a non-existent binding, so it
     proves existence, not tenancy. There are no cross-company cases for `run_id`,
     `matched_run_id`, `reply_to_message_id` or `responder_principal_id`.
  2. Replay with the same key and a different `delivery_policy` should give `Conflict`
     (`admission_replay.rs`). It is untested; `InAppOnly` appears in no admission test.
  3. The hop from an intent's `in_app_only` to the admitted run's `delivery_policy` (the worker's
     `admission_request`) is untested. The tests cover only the intent's recorded policy and the
     runtime under a given run policy.
  4. D-3 keeps a refusal branch in `check_workflow_wait_commit` for an event-wait completion whose
     consumed event was recorded after the deadline. The rewritten
     `workflow_wait_event_deadline_guard_is_independent_of_run_deadline` now exercises only the
     application-level expiry, so that DB backstop has no test.
- **Required:** add the four tests:
  1. foreign-company binding, run and principal references are rejected;
  2. policy replay `Conflict`;
  3. intent `in_app_only` → run `in_app_only` (no delivery row);
  4. a direct SQL completion with a late event raises 23514.
- **Instruction lesson:** n/a.
- **Resolution history:** 2026-10-22 open (WF-P08-R004).
  2026-10-22 recheck WF-P08-R005 (digest `500ae85b…1b11`): **fixed**.
  - `message_intent_rows_cannot_name_another_companys_records`: a foreign binding, run, matched run, answered message and responder each give 23503; it uses its own database with two companies.
  - `message_intent_admission_replay_with_another_delivery_policy_conflicts`.
  - `message_intent_in_app_policy_reaches_the_admitted_run`.
  - `workflow_wait_commit_guard_refuses_an_event_recorded_after_the_deadline`: a direct SQL completion fails at commit with the guard's message.
  - The weak binding case was removed from the thread tenancy test.

## WF-P08-F026 — `TemplateCatalogue` from the gated composition is neither composed nor recorded as a deviation

- **Context:** WF-P08-R004, B1. Contract: §D1.8: "`infra::workflow` builds … the shipped
  `TemplateCatalogue` (fixtures as seed templates)".
- **Classification:** actionable defect; **Low**; contract conformance / records.
- **Evidence:** `src/infra/workflow.rs` composes no `TemplateCatalogue`, and neither does setup or
  `AppState`; a grep finds no production reference. TASK's deviations D-1..D-4 do not mention it.
  Nothing in B1 needs it (template copy and assistant setup are B3), but the gated contract lists
  it.
- **Required:** compose it, or record a deviation that moves it to B3 alongside gate P08-D2.
- **Instruction lesson:** n/a.
- **Resolution history:** 2026-10-22 open (WF-P08-R004).
  2026-10-22 recheck WF-P08-R005 (digest `500ae85b…1b11`): **resolved by deviation**. TASK records D-5, moving `TemplateCatalogue` composition to B3 with gate P08-D2. Accepted.

## WF-P08-F027 — Optional suggestions from the B1 review (non-blocking)

- **Context:** WF-P08-R004, B1.
- **Classification:** optional suggestion; **Info**.
- **Suggestions:**
  1. The doc comment on `authorize_thread_injection` (`routing.rs:549-553`) still describes
     outreach replies and a returned outreach match.
  2. The Resend inbox path commits through `InboundEventWorker` (`inbound_event_worker.rs:290`)
     without notifying `intent_wakeups`, so those intents wait for the 1 s poll. Consider waking
     there too.
  3. The production `HttpRuntime` uses `HttpDestinationPolicy::default()` (public origins only)
     with no operator key, whereas the MCP client reads `MCP_INTERNAL_ENDPOINTS`. This is safe,
     but document that workflow `http.request` cannot reach private origins, or add a key.
  4. The F020.3 window (a reply recorded just before the *run* deadline stays unconsumed) is
     still undocumented in the failure table.
- **Resolution history:** 2026-10-22 open as optional (WF-P08-R004).
  2026-10-22 recheck WF-P08-R005 (digest `500ae85b…1b11`): all four items were **adopted**:
  1. the routing doc was updated;
  2. `InboundEventWorker` notifies `intent_wakeups` after a non-duplicate commit with intents (wired in setup; no test);
  3. `docs/deploy.md` documents that workflow `http.request` reaches public origins only;
  4. the TASK failure-table addendum was added.

---

Shared context for the findings of round **WF-P08-R005**: date 2026-10-22; workflow
`implement-opus-reviewed-2_2`; batch **B1**; stage **correction recheck**; notes
[WF-P08-R005](agent-review-notes.md#wf-p08-r005); reviewer B, Opus 5.5 / xhigh; phase `p08`;
worktree `/Users/mac02/dev/openbot`; base and HEAD `da868710d3b235d49856e3e481f8f3735f6b3e56`;
digest `500ae85b9eb6ecb3e11aa1739adf3c30fc56893db95fd232d3f001c6fc691b11`; contract D1 rev 3;
coordination r6.

## WF-P08-F028 — A wall-clock timeout in `deliver_reply` is treated as "cannot be delivered", so an awaited reply is permanently routed away from its wait

- **Context:** WF-P08-R005, B1. Contract: §D1.5.5 r3: "`Conflict` (event id reused for different
  content, or inbox full) → `Unmatched` + error log; `Database`/timeout → transient (defer the
  intent)". This was present at R004's digest too (an R004 miss); WF-P08-F024's correction made it
  durable.
- **Classification:** actionable defect; **Medium**; correctness / failure classification.
- **Evidence:**
  - `deliver` wraps the reply transaction in a 2 s `tokio::time::timeout` and maps expiry to
    `lease::timed_out()` (`reply_waits.rs:37-48`). That function returns
    `AppError::Conflict("Workflow lease transaction timed out")` (`lease.rs:267-269`).
  - `MessageAdmissions::offer_reply` sends every `AppError::Conflict` (and `BadRequest`) to the
    "the reply cannot be delivered to that run" arm (`message_admission.rs:358`). It returns
    `None`, and `correlate` then calls `settle_reply` (`message_admission.rs:300`).
  - So a reply whose delivery merely ran out of wall-clock time is treated as permanently
    unrelated. It is settled, and offered to the channel's bindings as a new run. Its wait never
    receives it, and the run times out at the wait deadline.
  - This can happen when cumulative lock waits approach 2 s without a single statement reaching
    the 1 s `lock_timeout`, for example two waits of about 0.9 s on the run and wait rows against
    a concurrent `resume_on`, or under database CPU pressure.
  - Postgres lock timeouts themselves are handled correctly: they become
    `AppError::DatabaseTimeout`, which is transient.
  - The existing transient test (`message_reply_delivery_failing_on_the_last_attempt_fails_the_intent_observably`)
    passes only because a single lock wait hits `lock_timeout` first.
- **Required:**
  - Classify the wall-clock timeout of `deliver_reply` as transient. For example, map it to
    `AppError::DatabaseTimeout` there, or give `offer_reply` a timeout-specific arm.
  - Keep `Unmatched` only for the reused-id/full-inbox `Conflict` and the payload `BadRequest`.
  - Add a test where `deliver_reply` exceeds its wall-clock budget without a single statement
    timing out, for example by slowing it with a test-only `pg_sleep` trigger as
    `wait_guard_tests` does. The test should assert `defer(Transient)` and that the reply is not
    settled.
- **Instruction lesson:** observation: the same error variant (`AppError::Conflict`) carries both
  a deterministic refusal and a timeout across the workflow persistence layer. Candidate (scope:
  `src/application/AGENTS.md` §Keep failure states distinct): "Never encode a timeout or lock
  failure as `Conflict`/`BadRequest`; classifiers by variant then turn retryable failures into
  decisions." This recurs after WF-P08-F005.
- **Resolution history:**
  - 2026-10-22 open (WF-P08-R005). Linked: R004 note follow-up.
  - 2026-10-23 **fixed** (WF-P08-R006, digest `b90a7f9c…a468`). The adapter now classifies at
    the source:
    - The three permanent causes return `ReplyIngress::Undeliverable(cause)`: `event_id_reused`
      (`reply_waits.rs:101`), `payload_rejected` (`:176`) and `event_inbox_full` (`:181`).
    - Budget expiry is `AppError::DatabaseTimeout` (`reply_waits.rs:35-62`, `deliver_within`).
    - `offer_reply` maps `Undeliverable` to an error log and `None`, and every `Err` to
      `Transient` (`message_admission.rs:352-376`). The variant is documented in
      `waits.rs:67-78`.
    - Only `Recorded` commits.
    - Tests: `message_reply_delivery_out_of_time_is_retried_not_settled`
      (`message_reply_tests.rs:756`) covers a 200 ms budget exceeded while the producer lock is
      held, with no statement timing out → `DatabaseTimeout`. Through the worker, a lock timeout
      defers and leaves `reply_settled` false, and the reply matches on retry.
      `message_reply_the_run_can_never_take_is_ordinary_ingress` (`:809`) covers a reused event
      id → settled and admitted.
    - Both pass in `logs/r006-focused.log` and in 20 repeats (`logs/r006-repeat-{1..20}.log`).

## WF-P08-F029 — Nondeterministic failure of the allowlist e2e test under a parallel focused run

- **Context:** WF-P08-R005, B1, verification.
- **Classification:** unresolved verification gap; **Low**.
- **Evidence:**
  - In my first focused run (`logs/r005-focused.log`, 110 tests, 39.8 s against the usual
    ~20 s), `message_ingress_member_email_reply_resumes_the_question_on_an_allowlist_channel`
    failed at `message_ingress_tests.rs:221` with "the engine never reached: the answer matched".
    For 30 s the answer's intent never got any outcome; neither matched nor admitted.
  - It passed in 3 isolated reruns and in 5 further full focused reruns
    (`logs/r005-focused-rerun1..5.log`).
  - The cause is not established:
    - The intent can stay claimed for its 60 s lease if `process` returns `Err`, for example on a
      pool acquire under the test database's 5-connection cap while the engine's three loops
      share it.
    - Repeated transient deferrals with exponential backoff also add up to about 30 s.
    - It is not the WF-P08-F028 path: that one would have produced `admitted` quickly.
- **Required:** establish the cause. For example, capture `process` errors and the intent's
  state and attempts when `run_until` times out, then fix the test budget or the defect.
  Re-run the focused set several times before the next recheck.
- **Instruction lesson:** cause unknown.
- **Resolution history:**
  - 2026-10-22 open (WF-P08-R005).
  - 2026-10-23 **fixed** (WF-P08-R006, digest `b90a7f9c…a468`). The cause is established: a
    sqlx-postgres 0.8.6 cancellation bug.
    - `PgTransactionManager::begin` raises `transaction_depth` only after `BEGIN`'s
      ReadyForQuery. Its `Rollback` guard's `start_rollback` does nothing at depth 0. So a
      `begin()` dropped in flight returns the connection to the pool inside an open
      transaction. I checked this in `sqlx-postgres-0.8.6/src/transaction.rs`.
    - The intent worker's `select!` drops `claim`/`process` mid-flight on shutdown
      (`message_admission.rs:512-527`). That happens at every `run_until` boundary.
    - A's diagnostics (`r005-diag-12`, `r005-diag2-13`) fit a later claim running uncommitted
      inside that transaction: run succeeded, reply event and wait consumed, intent still
      `claimed` with attempts 1, every session idle with no open transaction. So the fence in
      `finish_intent` missed the worker's own claim, and a later `BEGIN`/`COMMIT` on that
      backend committed it.
    - Fix: `infra::db::discard_leaked_transactions` (`db.rs:34-55`). It adds an `after_release`
      check, `transaction_timestamp() <> statement_timestamp()` over the simple protocol
      (confirmed: `&str` has no arguments → `Query`). It is on the production pool
      (`db.rs:65`) and on both shared test pools (`test_support.rs:218`, `:308`).
    - Coverage: every connection a user has had reaches the idle queue only through
      `PoolConnection::return_to_pool`, which runs `after_release` before the release ping.
      The other `release` callers (the reaper, `try_min_connections` and the `connect`
      warm-up) handle connections no user has had.
    - `detach`/`leak` remove the connection from the pool. Production's only caller is
      `PgListener` (LISTEN only).
    - `close`, `close_on_drop`, a closed pool, max lifetime and runtime teardown all close the
      socket, so the server rolls back.
    - No false positive: `wait_until_ready` flushes a queued `ROLLBACK` before the check runs.
      It also flushes an unsent `BEGIN` from a cancelled begin, which the check then catches.
      An aborted transaction makes the check fail, which also closes the connection
      (`close_hard`).
    - Every production `begin()` under a timeout is inside the timed future, so its
      `PoolConnection` drops with it. That includes the `WriteBound::begin(&mut conn)` sites,
      where `conn` is owned by the attempt future.
    - The regression test `a_cancelled_begin_never_leaks_its_transaction` reproduces the leak
      deterministically and pins both the guard and the absence of a false positive.
    - The guard fired once in 35 uncaptured runs of the ingress and intent modules
      (`logs/r006-nocapture-18.log`), and that run passed. So the suite's cancellations really
      do leak, and the guard discards them.
    - The focused set passed 20/20 (`logs/r006-repeat-{1..20}.log`) plus 40/40 uncaptured runs
      (`logs/r006-nocapture-*.log`).
    - The single worker slot in the e2e is a separate harness correction: two slots at two
      connections each, plus the intent worker and polling, would need 6 of the test pool's 5
      connections. It does not hide a B1 defect, because the intent worker still runs
      concurrently with the slot.
    - The diagnostics are taken before shutdown on their own connection and only add to the
      panic.
    - Residual: ad hoc test pools bypass the guard (WF-P08-F030, optional).

## WF-P08-F030 — Ad hoc test pools bypass the leaked-transaction guard (optional)

- **Context:** WF-P08-R006, B1 correction for WF-P08-F029; test infrastructure.
- **Classification:** optional suggestion; **Info**.
- **Evidence:**
  - `discard_leaked_transactions` covers the production pool and the two shared test pools.
  - Ten test-local pools are still built with a bare `PgPoolOptions::new()`:
    - `thread_lock_test_support.rs:15`
    - `task/claim_tests.rs:365`, `:475`
    - `workflow/lease_resolution_tests.rs:466`
    - `workflow/activation_tests.rs:118`
    - `workflow/completion_tests.rs:107`
    - `workflow/batch_recovery_tests.rs:27`
    - `workflow/budget_activation_tests.rs:90`
    - `workflow/child_runtime_tests.rs:481`
    - `workflow/action_reconciliation_other_audit_tests.rs:294`
  - `thread/read_tests.rs:103` is a lazy pool that never connects, so it does not count.
  - These are restart-simulation and single-connection pools. I found none that cancels a
    `begin` today, so this is not a defect. But a test that later adds a timeout around work on
    one of them would bring back the F029 class of flake there.
- **Suggested:** build test-local pools through one helper that applies
  `discard_leaked_transactions`. Alternatively, note in the test-support docs that a bare
  `PgPoolOptions` skips the guard.
- **Instruction lesson:** candidate (scope: `src/adapters/persistence/AGENTS.md`): "Build every
  `PgPool`, test pools included, through `infra::db::discard_leaked_transactions`. sqlx 0.8.6
  leaks a cancelled `begin()`'s transaction back to the pool."
- **Resolution history:**
  - 2026-10-23 open, optional (WF-P08-R006).
  - 2026-10-23 **fixed** (WF-P08-R007, digest `7b932911…5c51`).
    - `test_support::pool_options()` (guarded) backs `test_pool`, `own_database` and the nine
      test-local pools.
    - `lease_resolution_tests.rs:466-472` keeps one documented bare pool under WF-C004 r2. I
      judged that exemption justified: the guard's release statement would replace the
      `ROLLBACK` the test reads from `pg_stat_activity`, and sqlx's release ping is a bare Sync.
    - `rg "PgPoolOptions::new()" src` shows only guarded constructions, the regression test's
      bare pool, that test and the lazy `read_tests` pool.

## WF-P08-F031 — Nested schedule routes ignore the path channel, and the new routes have no other-tenant tests

- **Context:** WF-P08-R007, B2 HTTP retarget (B2-f). Rule: `src/adapters/http/AGENTS.md` "Scope
  the object being used, not a sibling object". Acceptance criterion 10 ("nested resource
  authorization incl. channel/thread").
- **Classification:** actionable; **Low**; HTTP authorization hygiene and test coverage. No data
  leaks: every handler authorizes against the schedule's own channel through `managed_schedule`.
- **Evidence:**
  - The JSON routes `/api/companies/{company_id}/channels/{channel_id}/schedules/{id}` (GET and
    DELETE), `…/run-now`, `…/runs` (new in B2) and `…/toggle` all bind `_channel_id` and never
    use it (`routes/schedule.rs:230`, `:257`, `:276`, `:301`, `:351`).
    - So `/channels/X/schedules/S` acts on a schedule S of channel Y and returns it, as long as
      the caller manages Y.
    - Only `update_schedule_json` compares the path channel.
  - The UI routes `/ui/channels/{channel_id}/schedules/{id}/{delete,run-now,toggle}` act on `id`
    and then render the card of the path channel, which may be a different one.
  - No HTTP or use-case test attempts another tenant's (or another channel's) schedule id on the
    new `GET …/schedules/{id}/runs` and `GET …/channels/{channel_id}/schedule-bindings` routes, or
    on the retargeted run-now. `schedule_channel_tests.rs` has one test, about channel
    reassignment. The rule requires such a test "for every new detail, nested, download, and
    mutation route".
- **Required:**
  - Load the schedule under the path's channel and return 404 on a mismatch, in the use case
    or in persistence (for example `managed_schedule(user, company, channel, id)`), for every
    nested route.
  - Add tests that try a schedule id of another company, and of another channel of the same
    company, on the runs, run-now, toggle, get and delete routes, and a foreign channel on
    `schedule-bindings`.
- **Instruction lesson:** observation: these handlers predate B2, but B2 rewrote them and added a
  route with the same `_channel_id` pattern. Candidate (scope: `src/adapters/http/AGENTS.md`): "A
  path segment the handler does not use for scoping is a defect: bind it and require it in the
  ownership predicate, or drop it from the route."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R007).
  - 2026-10-23 **fixed** (WF-P08-R008, digest `e43d8c37…b94f`).
    - `ScheduleScope { company_id, channel_id }`: `managed_schedule` returns not-found for a
      schedule of another channel. A schedule's channel is immutable (trigger), so the
      preflight holds for the company-locked persistence write.
    - Every nested JSON route and every `/ui/channels/{channel_id}/schedules/{id}/…` route passes
      `ScheduleScope::channel(company, path channel)`, and no `_channel_id` binding is left. The
      `/ui/schedules/{id}` workspace routes carry no channel, so they use the company scope.
    - `nested_schedule_routes_refuse_another_channels_or_companys_schedule` tries another
      channel, another company, and another company with the right channel id. It covers JSON
      GET, runs, run-now, toggle and DELETE, and UI run-now, toggle and delete, all 404, and
      checks the schedule is untouched afterwards. It also covers the correct path and a
      foreign channel on `schedule-bindings`.

## WF-P08-F032 — Deleted legacy schedule tests leave live behaviours unguarded; B2-g and the lease-fence change untested or unrecorded

- **Context:** WF-P08-R007, B2 persistence (`workflow/schedules.rs`) replacing
  `adapters/persistence/schedule.rs`, whose tests were deleted with it. Rules:
  - `src/application/AGENTS.md` poison-batch rule ("For every retryable worker queue, add a
    poison-batch regression test … two consecutive iterations … does not reclaim the same
    rows").
  - plan step 3 (TASK.md:513-515): outcome writes "fenced by worker, generation and unexpired
    lease (inventory gap 6)".
- **Classification:** actionable; **Low**; verification gap plus an unrecorded deviation.
- **Evidence:**
  - These behaviours were covered by the deleted `postgres_schedule_persistence_crud_and_claim_works`
    (HEAD `schedule.rs:891-1330`) and are not ported:
    - A failed materialization stays out of the next `claim_due` page until its backoff expires
      (the poison batch). The new code still relies on it: `record_error` backoff
      (`schedules.rs:762-775`) and the full-page `continue` in `ScheduleMaterializerWorker::run`.
      Today only `claim_occurrence` is shown returning `None` under backoff
      (`schedule_tests.rs:546-550`).
    - An edit that leaves the cadence alone keeps `next_run_at` and does not resume a paused
      schedule. The rule still exists in the new `update` (`cadence_changed`,
      `schedules.rs:279-290`, `:335-375`).
    - A captured one-off is disabled with `next_run_at = NULL` (`schedules.rs:631`).
  - **B2-g has no test.** No test retargets a schedule after an occurrence was captured and
    shows that the old occurrence still admits under the captured binding while the next slot
    uses the new one. `admission_source_tests` shows only that another binding's occurrence is
    refused.
  - **The record fence changed without a record.** The legacy `record_run_task` required
    `materialization_lock_expires_at > CURRENT_TIMESTAMP`, and
    `workflow_job_schedule_record_preserves_scope_and_lease_fence` (deleted) asserted that a
    lapsed lease refused the record.
    - The new `record` fences only on status, worker and generation (`schedules.rs:812-816`),
      unlike plan step 3. The implementation log does not mention the change.
    - My view: dropping the predicate is safe, because a reclaim changes the generation. It is
      arguably better, since a slow 5th attempt can still record its run before the terminal
      sweep. Either way it should be stated and pinned.
- **Required:**
  - Port the three regressions above as DB-backed tests. The poison batch must be the
    two-iteration form on `claim_due` with a page of failures.
  - Add a B2-g retarget-after-capture test.
  - Record the lease-fence decision (or restore the predicate) and pin it with a test.
- **Instruction lesson:** candidate (scope: `src/adapters/persistence/AGENTS.md`): "When a
  module is replaced, list the deleted tests and map each one to a ported test or to the removed
  behaviour it covered, in the implementation log."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R007).
  - 2026-10-23 **fixed** (WF-P08-R008, digest `e43d8c37…b94f`). Five DB tests:
    - The poison batch: a full page of 10 transient failures, and the immediate second
      iteration claims nothing.
    - An edit keeps the next run and the pause, and a cadence change re-anchors without
      resuming.
    - A captured one-off is disabled with `next_run_at` NULL.
    - B2-g: a retarget after capture admits the old occurrence under binding A and the next
      slot under B.
    - A lapsed but unreclaimed claim records its outcome.
    - The fence decision is recorded in TASK ("B2 corrections for WF-P08-R007") and on
      `PostgresPersistence::record` (`schedules.rs:826-834`). I agree with it.

## WF-P08-F033 — Schedule occurrence terminal failure and lease loss are neither counted nor logged as such

- **Context:** WF-P08-R007, B2 materializer. Rules:
  - `src/AGENTS.md:235-237`: "Metrics needed for alerts must distinguish retries, terminal
    failures, lease loss, and stuck work".
  - `src/application/AGENTS.md:36-38`: "move a poison task to an observable terminal state".
  - Precedent: WF-P08-F023, the same gap for message intents, fixed in B1 with
    `workflow_message_intents_failed_total{cause}`.
- **Classification:** actionable; **Low**; observability.
- **Evidence:**
  - Only refusals are counted (`workflow_schedule_admission_refusals_total`,
    `schedule_admission.rs:218-224`).
  - **5th transient failure.** `record_error` turns the occurrence `failed`
    (`schedules.rs:762-775`). The only trace is the per-attempt warning "A schedule occurrence
    could not be admitted yet" (`schedule_admission.rs:232-243`), which is wrong for a terminal
    outcome. There is no metric.
  - **Lease lapse at the retry limit.** The `terminal` CTE in `claim_due` (`schedules.rs:640-650`)
    fails the occurrence silently: no log, no metric. It also does not set the schedule's
    `last_error`, so the schedule card does not show it.
  - **Lease loss.** `Materialized::LeaseLost` is neither logged nor counted by
    `ScheduleMaterializerWorker::run`.
- **Required:**
  - Count and log (error level, structured fields) every terminal occurrence failure with a
    `cause` (`transient`, `lease_expired`), and lease loss.
  - Make the lease-lapse path name the failure on the schedule as `record_error` does.
  - Cover both with a test, as F023's correction did.
- **Instruction lesson:** observation: a recurrence of F023 in the second queue of the same
  phase. Candidate (scope: `src/application/AGENTS.md`): "Every queue's terminal transitions and
  lease loss emit one counter labelled by cause; review the second queue of a phase against the
  first one's corrections."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R007).
  - 2026-10-23 **fixed** (WF-P08-R008, digest `e43d8c37…b94f`).
    - **The sweep.** `fail_expired_occurrences` (`schedules.rs:579-614`) runs first in
      `ScheduleMaterializer::claim`. It is an autocommit occurrence UPDATE over a
      `FOR UPDATE SKIP LOCKED` subquery, then an autocommit `last_error` UPDATE per schedule.
      Neither holds an occurrence lock while it takes a schedule lock, so the
      schedule-then-occurrence order holds. I checked this against `record`, admission,
      capture and delete.
    - **`record_error`** returns `ErrorRecorded` from `RETURNING materialization_status`.
    - **Logging and metrics.** `failed()` logs at error level and counts
      `workflow_schedule_occurrences_failed_total{cause}`. `lost()` warns and counts
      `workflow_schedule_occurrence_leases_lost_total`, including a refusal or run whose record
      lost the claim.
    - **Test.** `schedule_terminal_failures_and_lost_claims_are_observable` covers all three
      paths, the schedule's `last_error` included.
    - **Residual:** if the follow-up schedule UPDATE fails, the occurrence is already failed and
      is never returned again, so its log and metric are lost → optional WF-P08-F035.

## WF-P08-F034 — Optional suggestions from the B2 review (non-blocking)

- **Context:** WF-P08-R007, B2.
- **Classification:** optional suggestion; **Info**.
- **Items:**
  1. **The live runs column is now effectively static.**
     - `schedule_runs_stream` wakes only on the channel's message activity
       (`ui_schedules.rs:368-381`), and scheduled runs no longer create messages. The column
       therefore refreshes only on reconnect.
     - This is a recorded known limit (p09). Until then, consider waking on occurrence
       outcome writes (for example a `pg_notify` in `record`), or rendering the column as
       non-live, so the UI does not imply live status.
  2. **Docs still describe scheduled work as task-ledger dispatch with schedule-run anchors**
     (`docs/rig.md:140`, `docs/transport_architecture.md:532-538`). Update them with B4's docs
     pass (criterion 11), or now.
  3. **Leftover** `let _ = pid;` and its unused `pg_backend_pid()` query in
     `schedule_capture_serializes_with_a_rejected_channel_move`
     (`schedule_tests.rs:1008-1026`).
- **Resolution history:**
  - 2026-10-23 open, optional (WF-P08-R007).
  - 2026-10-23 **adopted** (WF-P08-R008, digest `e43d8c37…b94f`).
    1. The runs column polls `/ui/schedules/runs` every 15 s; the route now returns the column
       alone, which the poll swaps. The SSE route, `schedule_runs_stream` and `channel_wake_ups`
       are deleted. The run-now and toggle forms and the pager still replace the whole live
       region.
    2. `docs/rig.md` and `docs/transport_architecture.md` are updated, as is the
       `ConversationAnchor` comment.
    3. The `pid` leftover is removed.

## WF-P08-F035 — The expired-occurrence sweep can lose a terminal failure's log and metric (optional)

- **Context:** WF-P08-R008, the F033 correction.
- **Classification:** optional suggestion; **Info**.
- **Evidence:**
  - `fail_expired_occurrences` commits the occurrences as `failed` in its first statement
    (`schedules.rs:582-596`).
  - It then updates each schedule's `last_error` with `?` (`:597-606`).
  - `ScheduleMaterializer::claim` logs and counts only after the whole call returns
    (`schedule_admission.rs:172-175`).
  - So if a schedule UPDATE fails (a connection loss or a statement timeout), or the process
    stops between the statements, the already-failed occurrences are never returned again.
    Their error log and `workflow_schedule_occurrences_failed_total{cause=lease_expired}` are
    lost, and some schedules keep a stale `last_error`.
- **Suggested:** return the failed occurrences even when a `last_error` update fails (log that
  failure separately), or log and count before updating the schedules. The occurrence row itself
  stays correct either way.
- **Resolution history:**
  - 2026-10-23 open, optional (WF-P08-R008).
  - 2026-10-23 **fixed** (WF-P08-R011, digest `9bdad3da…1f3c`): `fail_expired_occurrences` makes the schedule `last_error` update best effort (warning) and still returns the failed occurrences, so they are logged and counted; test `an_expired_occurrence_is_reported_when_its_schedule_cannot_name_it` (own database, row-scoped trigger).


## WF-P08-F036 — The provisioned binding runs as the Builder's run-as, not as the human who asked

- **Context:** WF-P08-R009, gate P08-D2 rev 1, §D2.3 step 5 and §D2.2 authority. Inputs: P08-Q2
  ("recorded on the binding revision (defaults to the configuring caller)") and D1.1 r3 ("an admin
  may name only themselves").
- **Classification:** actionable; **Medium**; authority model.
- **Evidence:**
  - D2.3 step 5 sets `run_as = actor`. For the tool, `actor` is the Builder run's actor (the
    Builder binding's run-as), while `requester` is the human who asked (D2.2).
  - `check_run_as` lets an actor name somebody else only when the actor is the company owner
    (`bindings.rs:171-205`). `BindingService::configure` defaults the run-as to the configuring
    caller (`lifecycle/bindings.rs:27-31`).
  - With the Builder run as owner O, admin A, as requester, gets a new, active, message-triggered
    binding running as O. Its agent's prompt, tools and skills are designed in A's conversation.
    The created assistant then acts with O's visibility, including allowlisted channels A cannot
    see. Through binding configuration A cannot name O.
  - D2.3 does not say which actor configures the binding, draft and version (the events and
    the `PreparedBinding` actor).
- **Required:**
  - Make the requester (already required to be a current owner/admin) the configuring actor and
    the run-as of the provisioned binding. The reviewed `reviewer` is the requester already.
  - Alternatively, state why the Builder's run-as is right and keep the D1.1 rule: refuse unless
    requester = run actor or the requester is the owner.
  - Name the actor recorded on the draft, version and binding events.
  - Add a test with a Builder run as the owner and an admin requester.
- **Instruction lesson:** candidate (scope: `plan/workflow-foundation` gate checklist): "When an
  agent tool creates a binding, check the run-as against D1.1 as if the requester had configured
  it."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R009).
  - 2026-10-23 **resolved** (WF-P08-R010, rev 2 `b88fe8ad…8afd`): fixed in rev 2 §D2.2/D2.3 (the requester is the configuring actor, `run_as`, `created_by` and event actor; `check_run_as` passes on its ordinary rule; F036 test planned).

## WF-P08-F037 — A message-root requester can be established by email, which the plan does not accept for privileged human authority

- **Context:** WF-P08-R009, §D2.2 requester derivation.
- **Classification:** actionable; **Medium**; authentication of a privileged action.
- **Evidence:**
  - A message-triggered root's requester is "the triggering message's author principal",
    whatever its origin.
  - External mail is accepted on a DMARC pass (`ingest/policy.rs:158-164`). DMARC proves domain
    alignment, not the individual sender. A same-domain sender, or a relay that does not enforce
    `From`, can be attributed to an admin's principal.
  - The plan keeps privileged human authority on authenticated member routes:
    - "Reviewers act only through authenticated member routes; email-only reviewers and link
      credentials are not supported" (`05-human-decisions-and-waits.md:69-71`);
    - NOT_PLANNED row 18.
  - Creating agents and live channels with admin authority is at least as privileged as an
    approval.
  - The intent already records the origin: `workflow_message_intent_origin` ∈
    `external | internal | application` (init schema `:7321`). `application` is the signed-in
    path, whose sender comes from the authenticated account (`routes/channel.rs:1305-1324`,
    `commit.rs:176`).
- **Required:**
  - For a message-triggered root, count the requester only when the triggering intent's origin
    is `application`. Otherwise refuse with `provisioning_forbidden`.
  - Name the relational derivation: `run.trigger_id → workflow_message_intents → messages.author_principal_id
    → person principal → company member`, and for a child the root found through
    `parent_run_id`.
  - Test an email-origin admin message refused, and a signed-in admin message accepted.
- **Instruction lesson:** candidate (scope: `src/application/AGENTS.md`): "Authority derived
  from a message author requires an authenticated origin; a DMARC pass authenticates a domain,
  not a member."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R009).
  - 2026-10-23 **resolved** (WF-P08-R010, rev 2 `b88fe8ad…8afd`): fixed in rev 2 §D2.2 (only `application`-origin intents yield a requester; the lookup is spelled out; email, member and signed-in tests planned). Detection refinement → WF-P08-F048.

## WF-P08-F038 — "No unvalidated path" is shown for the bundle only; the agent, channel and binding validations that one transaction bypasses are not named

- **Context:** WF-P08-R009, §D2.3 ("Validation of the bundle runs through the same compiler and
  publication checks … so no unvalidated path is opened").
- **Classification:** actionable; **Medium**; contract completeness of a costly-to-unwind
  composition.
- **Evidence:**
  - D2.3 names persistence helpers (`insert_agent_on`, `create_agent_principal_on`, the
    `save_binding` pieces) but none of the validations their owning services run before those
    helpers. The one transaction bypasses those services. Not named:
    - **Agent:** `AgentWrite::normalize`, `validate_response_contract` and
      `validate_model_selection` (`use_cases/agent.rs:674-677`).
    - **Channel:** `ChannelWrite::normalize_with`, slug/alias rules, and the email binding and
      grants writers (`channel.rs:388-460`). There is no tx-scoped channel-create helper today;
      the inserts live inside `create_with_agent`.
    - **Binding:** `ConfiguredBinding::new`, which validates params and slots
      (`binding/mod.rs:42-60`); `check_message_trigger`; the application `check_readiness`; and,
      because the binding is active at once, the activation checks `check_bundle` and the
      content hash against the selectable version (`lifecycle/bindings.rs:6-110`).
    - **Publish:** `supported_publication(…, ToolCheck::Publish)` and `verify_children`
      (`definitions.rs:192-200`). D2.3 names `verify_on` only. `save_binding`'s
      `verify_selection` runs `supported_publication` with `ToolCheck::Run`, which is weaker.
      The created agent's `tools` may capture workflow-tool snapshots.
- **Required:**
  - Enumerate the validations `provision_assistant_on` reuses, for agent, channel, binding and
    publication, and extract them into shared tx-scoped functions rather than copying them.
  - Specify the new tx-scoped channel-create helper.
  - State that `PreparedBinding` is built through a lifecycle constructor that runs the
    configure and activate checks. Its fields are `pub(super)`.
- **Instruction lesson:** candidate (scope: gate checklist): "A composition that bypasses a
  service must list every check that service performs, not only the persistence helpers it
  reuses."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R009).
  - 2026-10-23 **resolved** (WF-P08-R010, rev 2 `b88fe8ad…8afd`): fixed in rev 2 §D2.3 (agent, channel, compile, publish and binding checks named and extracted: `insert_channel_on`, `model_allowed`/`list_model_connections_on`, `compile_bundle`, `capture_agents_on`, `publish_on` with `supported_publication(Publish)`/`verify_on`/`verify_children`, `provisioned_binding`, `save_binding_on` ×2).

## WF-P08-F039 — `list_company_agents` gives any member of the Builder channel what the app shows only to owners and admins

- **Context:** WF-P08-R009, §D2.2 ("`list_company_agents` needs only the run actor").
- **Classification:** actionable; **Low**; information disclosure.
- **Evidence:**
  - The Builder's channel is `team` (D2.6 via D2.4), so every member can write to it.
  - The tool returns every agent's slug, name, description, provider and model, plus the
    company's model connections.
  - The application restricts agent listing to company managers:
    `AgentUseCases::list_company_agents` → `verify_company_manager` (`use_cases/agent.rs:703-709`).
- **Required:** apply the D2.2 requester check, as amended by F037, to `list_company_agents`
  too, or record why members may see the directory.
- **Instruction lesson:** n/a.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R009).
  - 2026-10-23 **resolved** (WF-P08-R010, rev 2 `b88fe8ad…8afd`): fixed in rev 2 §D2.2 (both tools take the requester check; the forbidden receipt holds no listing).

## WF-P08-F040 — The D2.2 local-action path leaves authority composition, refusal mapping and the read tool's path unspecified

- **Context:** WF-P08-R009, §D2.2.
- **Classification:** actionable; **Low**; specification gaps. The at-most-once claim itself
  holds.
- **Evidence:** I checked that `local()` holds. It takes the company first (`prelock_on`), then
  the fence and attempt, the run `FOR UPDATE` and the intent `FOR UPDATE`. It checks for an
  existing marker, then writes marker, effect and receipt in one transaction, and resolves
  ambiguity by the receipt (`action_dispatch.rs:210-276`). So at-most-once holds, and the
  `agent.run` classifier invariant (`actions/failure.rs:10-31`) is preserved. Unspecified:
  1. **The production `SqlActionAuthority` for provisioning** and its composition. Every
     production dispatch uses `NoLocal` today (`workflow_message/mod.rs:53`,
     `workflow_mcp/mod.rs:49`, `workflow_http/mod.rs:73`). What does `lock_current` lock and
     return (resource status, `approval_required: false` per P08-Q5), as
     `MessageActionAuthority` does (`message.rs:30-49`)?
  2. **Refusal mapping.** An `Err` from `apply` rolls back the marker and effect
     (`action_dispatch.rs:236`). Which errors become the tool errors
     `provisioning_forbidden`, `provisioning_conflict` and `invalid_input`, and which become
     step failures or transient errors? Agent and channel creation elsewhere are not
     company-locked (`channel.rs:388-480`), so a slug race surfaces as a unique violation that
     must map to `provisioning_conflict`, not a transient database error.
  3. **How `list_company_agents` (effect `Read`) is dispatched:** a local action with a receipt,
     or a read outside ActionService.
  4. **Whether a provisioning invocation can enter action reconciliation**, and with which
     `SqlReconciliationResources`.
- **Required:** specify 1–4 in the gate.
- **Instruction lesson:** n/a.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R009).
  - 2026-10-23 **resolved** (WF-P08-R010, rev 2 `b88fe8ad…8afd`): fixed in rev 2 §D2.2a (authority and composition, committed refusal receipts under a savepoint, the read tool on the local path, reconciliation closed). Refusal-classification refinement → WF-P08-F046.

## WF-P08-F041 — UI command replay and conflict are inferred from mutable rows, with no stated ordering

- **Context:** WF-P08-R009, §D2.3 step 6 and D2.4.
- **Classification:** actionable; **Low**; idempotency semantics.
- **Evidence:**
  - Replay is "an existing binding with the derived id returns the stored result; an equal key
    with a different request is `Conflict`". No stored request or result exists. The comparison
    would be against the channel slug and name, the agent and the binding as they are now, and
    those can change after creation. A rename then turns an honest retry into a 409, and the
    "stored result" is rebuilt from current rows.
  - Identity is step 6, but replay detection must run first, under the company lock and before
    the slug-collision refusal of step 3. Otherwise a retry after success is refused as a
    collision.
  - B1/B2 store command identity with a request digest instead (`workflow_admission_commands`,
    `admission_replay.rs:22-60`).
- **Required:**
  - Write a provisioning command record (company, command key, request digest, created ids) in
    the provisioning transaction.
  - Check it first, after the company lock.
  - Define the replay result from it.
- **Instruction lesson:** n/a.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R009).
  - 2026-10-23 **resolved** (WF-P08-R010, rev 2 `b88fe8ad…8afd`): fixed in rev 2 §D2.4 (`workflow_provisioning_commands` with a request digest, read first under the company lock, stored result).

## WF-P08-F042 — The reserved-name rule rests on an inaccurate premise and covers too few registration paths

- **Context:** WF-P08-R009, §D2.1 names, Invariant 4.
- **Classification:** actionable; **Low**.
- **Evidence:**
  - "Reserved like `message.send`/`message.reply`: a company HTTP operation or workflow-tool
    registration with either name is refused". But today only workflow-tool registration
    refuses `message.*` (`workflow_tools.rs:180`). HTTP operation registration does not
    (`workflow/http.rs:200-216`, `application/workflow/http.rs:76-101`), and MCP operations
    reserve nothing.
  - `ToolSnapshot::kind()` classifies by name (`publication/types.rs:131-145`). Any HTTP, MCP
    or slot snapshot named `list_company_agents` or `create_agent_channel` therefore becomes
    `Provisioning`, and must be refused by the trusted-snapshot check at every site that matches
    the kind: `supported_publication`, agent preparation and dispatch authority, as for messaging.
  - The dot-free names can collide with third-party MCP tool names.
  - `create_agent_channel` input `tools` may name the provisioning tools themselves, so created
    agents could provision in turn. The gate does not decide this.
- **Required:**
  - Refuse the reserved names at HTTP, MCP and workflow-tool registration, and in the registry
    `tool.call` literal (as `registry/actions.rs:88` does for messaging).
  - Name the trusted-check sites.
  - Decide and state whether `tools` may grant provisioning names. I recommend refusing.
  - Note the MCP collision behaviour.
- **Instruction lesson:** n/a.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R009).
  - 2026-10-23 **resolved** (WF-P08-R010, rev 2 `b88fe8ad…8afd`): fixed in rev 2 §D2.1 (one reserved-name rule at HTTP, workflow-tool and `tool.call`; trusted-check sites named; MCP behaviour stated; `tools` cannot grant provisioning). Grant-resolution refinement → WF-P08-F047.

## WF-P08-F043 — The verification plan has no competing-claimant tests for the new protocol

- **Context:** WF-P08-R009, "Verification plan (B3a)". Rule: `AGENTS.md` "When adding or changing a
  concurrency protocol, include a test with competing claimants rather than relying only on
  sequential mocks."
- **Classification:** actionable; **Low**; verification plan.
- **Evidence:**
  - B3a adds the first production `SqlLocalAction` (effect inside the action transaction),
    deterministic-id UI idempotency, and slug claims shared with creators outside the company
    lock.
  - The plan tests only sequential replay ("a replay of the same call creates nothing new"; a
    crash between commit and observation).
- **Required:** add these DB tests:
  - the same provisioning invocation dispatched concurrently by a stale and a current worker →
    one effect, one receipt;
  - two concurrent UI commands with one key → one creation and one replay;
  - the tool and the UI (or a plain channel creation) racing for one channel slug → one success
    and a deterministic `provisioning_conflict`.
  - Also add tests for the F036, F037 and F039 rules, the requester per root kind (child, manual,
    scheduled), and the MCP reserved names.
- **Instruction lesson:** n/a.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R009).
  - 2026-10-23 **resolved** (WF-P08-R010, rev 2 `b88fe8ad…8afd`): fixed in rev 2 Verification (stale vs current worker, same-key UI, tool/UI/plain-create slug races, authority per root kind).

## WF-P08-F044 — The Builder's new grants and explicit model do not reach a company Builder

- **Context:** WF-P08-R009, §D2.6 and the 06C obligation in criterion 13.
- **Classification:** actionable; **Low**; scope gap.
- **Evidence:**
  - D2.6 puts the grants on the library definition. But a library agent becomes a company agent
    through `library_agent_write`, which sets `workflow_tools: None`
    (`use_cases/agent.rs:347-349`). That copy also falls back to the company default when the
    library names no model (`:654-668`).
  - `derive_on` refuses an agent without explicit provider/model in workflows
    (`agent_publication.rs:69-73`).
  - The library install skips an existing definition (`persistence/agent.rs:350-360`).
  - D2.4's `Existing(agent)` needs a company agent, so after B3a no company Builder carries the
    grants or a workflow-usable model unless an admin edits it by hand.
- **Required:**
  - Specify how the company Builder gets the grants and an explicit provider/model in B3a (the
    copy carries `workflow_tools`; the copy needs an explicit model), or state that B4 owns it
    and what B3a demonstrates.
  - Add a test of the library → company → assistant-channel path.
- **Instruction lesson:** n/a.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R009).
  - 2026-10-23 **resolved** (WF-P08-R010, rev 2 `b88fe8ad…8afd`): fixed in rev 2 §D2.6 (library copy carries `workflow_tools` and an explicit default-connection model for workflow-capable definitions; library → company → assistant-channel test).

## WF-P08-F045 — Optional notes on D2.5 and the batch split (non-blocking)

- **Context:** WF-P08-R009.
- **Classification:** optional suggestion; **Info**.
- **Items:**
  1. **`/run/deadline` is sound.**
     - `ContextReference` already has `run.id`/`run.parent_id` (`domain/workflow/context/mod.rs:99-160`).
     - The run row has an immutable `deadline` (admission + 24 h, inherited by children;
       `admission_write.rs:60-120`).
     - Waits are already capped at the run deadline (`waits.rs:219`). The new root mainly
       removes a fixed instant from the parameters.
     - Specify a canonical RFC3339 rendering (UTC, fixed precision): step inputs are digested
       and replayed.
     - Document the root in `docs/workflow-language-v1.md`.
  2. **The B3a/B3b split is sound.** B3b is breadth over existing commands, including B2's
     `manual::start`.
  3. **Template compilation runs inside the action transaction** while the company lock is held
     (`prelock_on`). Keep it bounded within the `WriteBound` attempt; the templates are small,
     so this is acceptable.
- **Resolution history:**
  - 2026-10-23 open, optional (WF-P08-R009).
  - 2026-10-23 **adopted** (WF-P08-R010, rev 2 `b88fe8ad…8afd`): adopted in rev 2 §D2.5 (canonical microsecond UTC rendering, documentation, split unchanged).

## WF-P08-F046 — Refusals must be classified by deterministic site, never by `AppError` variant from reused functions

- **Context:** WF-P08-R010, rev 2 §D2.2a.2. A refusal now commits as a permanent typed-error
  receipt for that invocation.
- **Classification:** actionable; **Low**; carried as a B3a implementation check.
- **Evidence:**
  - `Invalid` includes "a template compile or publication diagnostic". `Forbidden` and
    `Conflict` come from checks that are reused functions.
  - Those functions return generic `AppError`s:
    - `publish_on` (`Conflict` for the draft or key replay);
    - `supported_publication` (`NotFound`, `authority.rs:96-139`);
    - `verify_on` (`Conflict`, `agent_publication.rs:151-190`);
    - `authorize_company` (`NotFound`).
  - In this codebase the same variants also carry transient failures. `lease::timed_out()` is
    `AppError::Conflict` (WF-P08-F028), and F005/F028 were exactly this pattern.
  - A transient error classified by variant would be committed as a permanent
    `provisioning_conflict`, `forbidden` or `invalid` receipt.
- **Required (B3a):**
  - Build each `ProvisioningRefusal` only at an enumerated deterministic site:
    - the pre-checks;
    - the pure validators and compile diagnostics;
    - a `NotFound` from the requester's `authorize_company` call specifically;
    - unique violations matched by constraint name.
  - Propagate every other error from the reused persistence functions as `Err`.
  - Add a test where an injected lock or statement timeout inside `provision_on` leaves no
    marker and no receipt.
- **Instruction lesson:** recurs F005/F028. Candidate (scope: `src/application/AGENTS.md`):
  "Never derive a durable refusal from an error variant; construct it where the deterministic
  check fails."
- **Resolution history:**
  - 2026-10-23 open, carried to the B3a batch review (WF-P08-R010).
  - 2026-10-23 **fixed** (WF-P08-R011, digest `9bdad3da…1f3c`): refusals come only from named sites: requester `authorize_company` NotFound, `taken_slug`, `malformed`, `normalize`, model/tool/skill resolution, capture and compiler `BadRequest`/diagnostics. Publish and binding persistence errors propagate as `Err`. After a savepoint rollback a fresh `taken_slug` decides conflict versus `Err` (DV-5). Test `provisioning_transient_fault_leaves_no_marker_receipt_or_effect`.

## WF-P08-F047 — With the narrower classification, a provisioning grant can resolve to a third-party MCP tool of the same name

- **Context:** WF-P08-R010, rev 2 §D2.1 classification (MCP tools with the reserved names stay
  `Slot`) and the trusted-check site `agent_publication.rs:295`.
- **Classification:** actionable; **Low**; carried as a B3a implementation check.
- **Evidence:**
  - `granted_tool` first reuses a snapshot already captured under the grant's name. For a
    `Slot` snapshot from an MCP slot it returns `Ok(None)` (`agent_publication.rs:278-303`),
    before any built-in name is considered (`:308`).
  - Under rev 2 an MCP tool named `create_agent_channel` is `ToolKind::Slot`. In a workflow that
    captures it, an agent granted `create_agent_channel` would be prepared against the MCP tool
    and send the agent spec (prompt, model) to that server, believing it is provisioning.
  - The declared-name check rev 2 cites (`agent.rs:119-121`) cannot fire: only one snapshot of
    that name exists.
  - Messaging is safe only because its classification is purely name-based, so such an MCP
    snapshot is `Message` and fails the trusted check.
- **Required (B3a):**
  - In `granted_tool`, resolve a reserved provisioning name to the built-in before reusing
    captured snapshots, and refuse as ambiguous when another snapshot of that name was captured.
  - Add a test with a workflow selecting an MCP tool named `list_company_agents` and an agent
    granted the built-in → refused at publication.
- **Instruction lesson:** n/a.
- **Resolution history:**
  - 2026-10-23 open, carried to the B3a batch review (WF-P08-R010).
  - 2026-10-23 **fixed** (WF-P08-R011, digest `9bdad3da…1f3c`): `granted_tool` resolves a provisioning name to the built-in first and refuses as ambiguous any captured non-trusted snapshot of that name (`agent_publication.rs:306-315`). Test `provisioning_grants_capture_the_built_in_or_refuse_an_ambiguous_capture`.

## WF-P08-F048 — Requester derivation should match the stored trigger source exhaustively and fail closed

- **Context:** WF-P08-R010, rev 2 §D2.2 requester derivation. A message root is detected by the
  existence of an intent with `id = root.trigger_id`; any other root takes `root.actor_id`.
- **Classification:** actionable; **Low**; carried as a B3a implementation check.
- **Evidence:**
  - The trigger source is stored exactly: `workflow_admissions.source_key` is
    `["manual", …] | ["message", message_id] | ["schedule", …]` (`admission_binding.rs:7-33`),
    and run inspection rebuilds `causality.trigger().source()`.
  - Detecting by intent existence makes "not found" mean "manual or scheduled → requester =
    actor (an owner/admin)". That fails open in two cases:
    - a message root without an intent row (another admission path, now or later);
    - any future root source (e.g. a webhook in p09).
- **Required (B3a):**
  - Read the root's source from its admission and match `TriggerSource` exhaustively:
    - Message → the intent with `id = trigger_id` and `message_id` equal to the source's message,
      origin `application`, otherwise forbidden;
    - Manual and Schedule → `actor_id`;
    - anything else → forbidden.
  - Test a message root without an intent → forbidden.
- **Instruction lesson:** n/a.
- **Resolution history:**
  - 2026-10-23 open, carried to the B3a batch review (WF-P08-R010).
  - 2026-10-23 **fixed** (WF-P08-R011, digest `9bdad3da…1f3c`): `requester_on` reads the root's single stored `source_key` and matches it exhaustively; a message root needs an intent with `id = trigger`, the source's `message_id`, origin `application` and a person author. Anything else, or a chain deeper than 64, is none. Unit and DB tests.

## WF-P08-F049 — Optional notes on rev 2 (non-blocking)

- **Context:** WF-P08-R010.
- **Classification:** optional suggestion; **Info**.
- **Items:**
  1. **UI `Invalid` → 422 departs from the project's mapping.** `AppError::BadRequest` → 400,
     and 422 is used only for `Execution` (`adapters/http/app_error_impl.rs:12-34`). Prefer 400
     unless B3b adopts 422 for every validation refusal.
  2. **Library-copy model.** "The default connection's first enabled model" depends on the
     stored order of `models`; define the order (as stored, or sorted) so copies are
     reproducible. The warning already names the choice.
  3. **Same key, different admin.** A replay of the same UI key by a different admin returns
     the stored result. Fine, but say so, or include the caller in the digest.
  4. **Savepoint refusals: agreed.** They are the only way to turn a unique violation into a
     committed refusal without aborting the action transaction.
- **Resolution history:**
  - 2026-10-23 open, optional (WF-P08-R010).
  - 2026-10-23 **adopted** (WF-P08-R011, digest `9bdad3da…1f3c`): item 1 (Invalid → 400), item 2 (connections ordered `is_default DESC, provider`; the library copy uses `models[1]` of the default), item 3 (caller in the digest).


## WF-P08-F050 — `list_company_agents` bounds the agent count but not the receipt size, so a large company fails the call deterministically

- **Context:** WF-P08-R011, B3a, `list_on` (`persistence/workflow/provisioning.rs:313-358`). Rules:
  - `src/AGENTS.md` "Bound work at every external boundary … make overload produce a controlled
    rejection";
  - persistence `AGENTS.md` "Operational views must cap their working set and visibly report
    truncation".
- **Classification:** actionable; **Low**; bounded-output defect.
- **Evidence:**
  - The listing caps agents at 200 and models at 256, but not bytes.
  - Agent `name` and `description` have no length bound (`AgentWrite::normalize`,
    `use_cases/agent.rs:125-170`, only trims). The output schema leaves them unbounded
    (`application/workflow/provisioning.rs`).
  - `validate_action_result` canonicalizes the result with a 64 KiB cap
    (`actions/dispatch.rs:208-210`, `canonical.rs:15-24`). Its error fails `local()` with no
    receipt (`action_dispatch.rs:236`).
  - So a company whose agents' listing exceeds 64 KiB (for example about 100 agents with
    600-byte descriptions) gets an `Err` on every `list_company_agents` call. The Builder's step
    retries into terminal failure, and the model is never told why.
  - The response also does not say when more than 200 agents exist.
- **Required:**
  - Bound each listed field (for example the description cut to a few hundred characters at a
    char boundary) and the whole listing below the receipt cap.
  - Report truncation (`truncated: true`, or a count) in the output schema.
  - Add a DB test with an oversized directory that returns a valid, truncated receipt.
- **Instruction lesson:** candidate (scope: `src/application/AGENTS.md`): "A tool result built
  from unbounded rows must be sized below the receipt cap by construction, with truncation
  reported."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R011).
  - 2026-10-23 **fixed** (WF-P08-R012, digest `22334ec4…77e1`): `list_on` cuts names to 120 and descriptions to 300 chars (also `maxLength` in the output schema). It admits models, then agents by slug, while each compact entry fits a 48 KiB budget, which keeps the result under the 64 KiB cap by construction. It reads `LIMIT 201` and reports a required `truncated`. Test `provisioning_lists_an_oversized_directory_within_its_bound` (205 agents with 210-char names and 600-char descriptions → committed, `truncated`, bounded, fields cut).

## WF-P08-F051 — The created channel's address omits the app domain on every path

- **Context:** WF-P08-R011, B3a, `provision_on` result (`persistence/workflow/provisioning.rs:661-671`).
  Contract: D2.1 output `channel: {id, slug, address}`; D2.6 has the Builder "report the
  address".
- **Classification:** actionable; **Medium**; wrong value in the primary output contract.
- **Evidence:**
  - The result builds `address: format!("{}@{company_slug}", channel.slug)`, for example
    `helper@acme`.
  - A channel answers at `{channel}@{company}.{app_domain}`. The domain's own constructor is
    `Channel::address_for(channel_slug, company_slug, app_domain_name)`
    (`domain/entities/channel.rs:327-333`), which the legacy `create_agent_channel` used
    (`provision_response`, `agent_channel_tool.rs` before `4dd01b3`).
  - The wrong address reaches:
    - the tool receipt the Builder reads and reports to the user;
    - the HTTP 201 body;
    - the stored command record, so replays repeat it;
    - the agent page's card (`ui_create_assistant_channel`).
  - No test asserts the address. `ProvisioningEffects`/`ProvisioningTemplates` carry no app
    domain, although the engine has one (`WorkflowEngine::new(.., app_domain, ..)`).
- **Required:**
  - Build the address with `Channel::address_for` and the deployment's `APP_DOMAIN_NAME`,
    passed into the provisioning composition.
  - Assert the exact address in the dispatch, runtime and route tests.
- **Instruction lesson:** candidate (scope: gate/test checklist): "Assert every field of a new
  output contract, not only ids and codes."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R011).
  - 2026-10-23 **fixed** (WF-P08-R012, digest `22334ec4…77e1`): `ProvisioningDeployment` carries `APP_DOMAIN_NAME` from `WorkflowEngine::new`, and `provision_on` uses `Channel::address_for`. Exact addresses are asserted in the dispatch, production-engine (`d.address("helper")`), route and library-copy tests.

## WF-P08-F052 — Workflow listings silently cut at 200 rows, and visibility is filtered after the cut

- **Context:** WF-P08-R013, B3b-1, `WorkflowReads` (`persistence/workflow/reads.rs`) and the
  listing routes (`routes/workflows/bindings.rs`). Rule: persistence `AGENTS.md:185-186`
  ("Operational views must cap their working set and visibly report truncation rather than
  silently returning partial data").
- **Classification:** actionable; **Low**; silent partial data in operational views.
- **Evidence:**
  - Every listing binds `LIMIT MAX_LISTED` (200, `application/workflow/reads.rs:15`):
    - definitions (`reads.rs:136-142`);
    - versions (`:168-172`);
    - bindings (`:193-199`);
    - runs (`:223-236`);
    - a run's executions (`ORDER BY activation, id`, `:267-271`) and waits (`:289-293`).
    None reports that more rows exist, and the JSON is a bare array.
  - `bindings::list` (`bindings.rs:55-69`) and `bindings::runs` (`:332-351`) drop rows the
    caller cannot see after the SQL cap. An admin who is not allowed on a busy channel can
    therefore get far fewer than 200 rows, even none, while visible older rows exist. The
    response does not say so.
  - The execution list keeps the oldest 200. A long run (revision rounds, loops up to
    `max_steps`) loses its newest steps, which are the ones an operator inspects.
- **Required:**
  - Report truncation in each listing: fetch `MAX_LISTED + 1`, then return
    `{items, truncated}` or a cursor.
  - Either apply association visibility before the cap, or report that hidden rows reduced
    the page.
  - For executions, keep the newest, or report the cut.
  - Test: more than 200 rows gives `truncated`, and a hidden-channel majority does not hide
    visible rows silently.
- **Instruction lesson:** recurs F050's rule for HTTP reads. Candidate (scope:
  `src/adapters/http/AGENTS.md`): "A filtered listing applies its filter before the cap, or says
  the page is partial."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R013).
  - 2026-10-23 **fixed** (WF-P08-R014, digest `b075aa52…11ef`): keyset pages `?after&limit` (1..=200) answer `{items, next}`, with `next` following the scanned rows, so a page an admin sees partly still continues. A stale cursor or a bad limit is 400. Versions, executions and waits keep the newest 200 with `*_truncated`. Tests `run_listing_pages_newest_first` and the admin page-of-one check. Cursor note in F064.

## WF-P08-F053 — Per-thread visibility is a query loop, and two new lookups have no supporting index or recorded evidence

- **Context:** WF-P08-R013, B3b-1. Rule: persistence `AGENTS.md:170-190`:
  - "A bounded result set does not imply bounded database work … Replace per-parent query loops
    with bounded batch reads";
  - "When an optimization is deliberately deferred pending production evidence, add a named
    duration or working-set metric, or a threshold log".
- **Classification:** actionable; **Low**; unbounded or looped database work behind bounded
  pages.
- **Evidence:**
  1. **Per-thread loop.** `Visibility` (`bindings.rs:72-112`) memoizes per
     `RelatedAssociation`.
     - Message-admitted runs are thread-associated (`application/workflow/message_admission.rs:470`),
       so every distinct thread is a separate `LifecycleAuthorizer::authorize`
       (`authorization.rs:150-192`): the access context, then the channel, then the thread,
       each sequential and unlocked.
     - A 200-run page of a mail binding is therefore about 600 round trips, and the channel is
       re-read for every thread.
     - The doc comment ("a listing of 200 runs on one channel checks the channel once") holds
       only for channel-associated runs.
  2. **Decision lookup by id alone.** `decision_company`
     (`persistence/workflow/reads.rs:328-338`) runs `WHERE id = $1`.
     - Every index on `workflow_decisions` leads with `company_id` (PK `(company_id,id)`,
       `migrations/…init_schema.sql:9474-9485`).
     - On a rolled-back temp table of 200k rows shaped like it, the planner chose a Seq Scan
       (199,999 rows removed).
     - This runs on every page GET, UI submit and UI comment, which is the entry point of every
       review notice.
     - Ids are unique only by v4 randomness, and `fetch_optional` would return an arbitrary
       row if two companies shared one.
  3. **Run listing.** `list_runs` sorts `ORDER BY created_at DESC, id` and optionally filters
     `binding_id`. No index has `(company_id, created_at)` or `(company_id, binding_id, …)`
     (`pg_indexes` on `workflow08_test`), so each page reads and sorts all of the company's
     runs.
- **Required:**
  - Replace the per-thread loop with a batch: one membership read, plus one query for the page's
    distinct channels and threads.
  - For items 2 and 3, either record representative `EXPLAIN (ANALYZE, BUFFERS)` evidence and a
    named duration or working-set metric (the rule's deferral path), or fix them. For item 2,
    `UNIQUE (id)` on `workflow_decisions` would also enforce the uniqueness DV-7 relies on; the
    schema is edited in place.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R013).
  - 2026-10-23 **fixed** (WF-P08-R014, digest `b075aa52…11ef`): `Visibility` reduces a thread association to its channel. The `(company_id, channel_id, thread_id)` FKs to `threads(company_id, channel_id, id)` are NO ACTION, and `workflow_runs_preserve_association` keeps the association immutable, so this equals the authorizer's thread check (verified on `workflow08_test`). Past 32 checks it logs `workflow listing visibility checks` with the count. New indexes `workflow_decision_id` (UNIQUE), `workflow_runs_newest` and `workflow_runs_binding_newest`, with before/after plans in `logs/r013-fix-explain.log`. `list_runs` builds its WHERE from the filters present.

## WF-P08-F054 — The validate route cannot validate any draft that uses an HTTP or MCP operation

- **Context:** WF-P08-R013, B3b-1, `definitions::validate` (`routes/workflows/definitions.rs:116-137`).
  B3b plan: `POST /{w}/validate`; publish takes "HTTP/MCP selections".
- **Classification:** actionable; **Medium**; a delivered interface returns a wrong answer for a
  primary class of workflows.
- **Evidence:**
  - `validate` builds `engine.definitions(Vec::new(), Vec::new())` (`definitions.rs:129`). Its
    body is `RevisionBody { expected_revision }` with `deny_unknown_fields` (`:116-120`), so it
    cannot carry selections.
  - `DefinitionService::validate` compiles through the publication directory
    (`lifecycle/definitions.rs:47-66`, `compile` at `:188-198`).
  - With no selections, `HttpPublicationDirectory` and `McpPublicationDirectory` capture no tool
    (`persistence/workflow/http_publication.rs:19-57`, `tool_publication.rs:17-49`).
  - The registry then refuses any `tool.call`/`mcp.call` whose tool is not in
    `facts.tools`, with `registry.tool` "Supply the selected tool argument and result contract"
    (`registry/actions.rs:104-118`). The route returns this as a 422 diagnostic.
  - `publish` with selections succeeds on the same draft (`definitions.rs:175-227`).
    `validate` therefore reports a false error for every HTTP or MCP workflow and can never
    say `valid` for one.
  - The route tests validate only a `data.map` draft (`lifecycle_tests.rs:33-64`).
- **Required:**
  - Accept the same `http`/`mcp` selection bodies on validate as on publish, sharing the parser
    with `publish`.
  - Route test with a company HTTP operation: validate without the selection → 422
    `registry.tool`; with it → 200 `valid` and the same `content_hash` publish then returns.
- **Instruction lesson:** candidate (scope: `src/adapters/http/AGENTS.md`): "A preview route
  (validate, dry-run) takes every input its committing twin takes."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R013).
  - 2026-10-23 **fixed** (WF-P08-R014, digest `b075aa52…11ef`): `ValidateBody` takes `http`/`mcp`, parsed by the shared `selections()`. Test `validate_and_publish_carry_http_selections`: 422 without the selection, 200 with it.

## WF-P08-F055 — A publish retry under the same key with different HTTP/MCP selections replays the old version instead of 409

- **Context:** WF-P08-R013, B3b-1, `definitions::publish` (`definitions.rs:175-227`). B3b plan
  verification: "idempotent replay with the same key, and the same key with a different request
  → 409".
- **Classification:** actionable; **Low**; key-reuse semantics.
- **Evidence:**
  - The version id is `keyed_id("workflow.publish:{w}", company, key)` (`:209-214`). The
    selections go into `engine.definitions(http, mcp)`, not into `PublishDraft`.
  - `PublishDraft::equivalent` (`lifecycle/contracts.rs:72-78`) compares company, workflow,
    expected revision, version and key only.
  - A retry with the same key and revision but other `http`/`mcp` selections is therefore
    `equivalent`. The lifecycle (`lifecycle/definitions.rs:75-90`) and `publish_on`
    (`persistence/workflow/definitions.rs:189-193`) return the earlier bundle, and the route
    answers 200 with the old version. The caller believes the new selections were published.
  - The route test covers same-key replay and a stale revision under another key, not same key
    with a different request (`lifecycle_tests.rs:62-79`).
- **Required:**
  - Make the selections part of the keyed request. For example, hash a canonical encoding of
    the parsed selections into the `keyed_id` input, so a different request derives another
    version and `equivalent` refuses it 409.
  - Test same key, other selections → 409.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R013).
  - 2026-10-23 **fixed** (WF-P08-R014, digest `b075aa52…11ef`): the version id is keyed on key ‖ `\0` ‖ the serialized selections. The same key with other selections → 409 (same test).

## WF-P08-F056 — The planned WF-C002 child-control rule is neither implemented nor recorded, and the retry refusal message is wrong for most of the cases it covers

- **Context:** WF-P08-R013, B3b-1, `bindings::cancel`/`retry` (`bindings.rs:418-473`). TASK B3b
  plan (`TASK.md:1084-1085`): "`POST /{r}/cancel` and `/retry` (expected revision; the WF-C002
  child rule: a child is controlled through its root)". WF-C002: children carry the parent's
  binding for provenance, and lists label them.
- **Classification:** actionable; **Low**; plan-versus-implementation gap and misleading
  refusal.
- **Evidence:**
  - Neither the route nor `WorkflowService::cancel`/`retry` (`application/workflow/service.rs:87-163`)
    looks at `parent_run_id` or the trigger source.
  - `controls::control` (`persistence/workflow/controls.rs:64-129`) cancels a runtime child
    directly (`cancel_on` → `cancel_tree_on`, `:163-198`). The parent then takes
    `workflow.child_cancelled`. So a child is controllable on its own, contrary to the plan.
    The implementation log records no deviation.
  - Retry of a child is refused, `Unsafe` (`control_retry.rs:8-17`). So is retry of any run that
    is not `failed`: `retry_on` returns `false` when `state != Failed`.
  - The route maps every `Unsafe` to 409 "The failed step's effect is uncertain; reconcile it
    before retrying" (`bindings.rs:465-469`). That is false for a child, which is never
    retried (recovery is a revision round or a rerun, 07-D1 §D1.7), and false for a queued,
    running or succeeded run.
  - No test has a runtime child. `lifecycle_tests.rs:127` asserts only `parent_run_id: null`
    on a top-level run.
- **Required:**
  - Decide the child rule. Either refuse cancel/retry of a run with `parent_run_id` (409 naming
    the parent or root), or record a deviation that direct child cancel is allowed because
    07-D1 defines its outcome.
  - Make the retry 409 message state the real condition, for example "only a failed top-level
    run whose failed step is known safe can be retried; reconcile an uncertain effect first".
  - Add a route test with a runtime child: list label non-null, and cancel/retry behaving as
    decided.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R013).
  - 2026-10-23 **fixed** (WF-P08-R014, digest `b075aa52…11ef`): `controllable_run` authorizes the company and the association, then refuses a run with a parent (409, naming it). The `Unsafe` message is neutral. Test `child_runs_are_controlled_through_their_parent`, including a stranger → 404. Placement note in F063.

## WF-P08-F057 — The grant route lets any agent, including a provisioned one, be granted the provisioning tools

- **Context:** WF-P08-R013, B3b-1, `grant_agent_tools` (`persistence/workflow/workflow_tools.rs:300-341`).
  Gate P08-D2 rev 2: §D2.1 (`tools`: "reserved provisioning names are refused, so a created
  agent cannot provision in turn", `decision-D2-r2.md:102`) and invariant 6 ("Created agents
  cannot be granted provisioning", `:360-362`).
- **Classification:** actionable; **Low**; an accepted gate invariant is not preserved by a new
  write path.
- **Evidence:**
  - The grant checks only count (≤ 32) and uniqueness. `create_agent_channel` and
    `list_company_agents` are valid `TypeName`s and are stored.
  - At the next publication that uses the agent, `granted_tool` resolves a provisioning name to
    the built-in provisioning snapshot (`agent_publication.rs:306-314`).
  - After B3b-1, one owner/admin `PUT …/agents/{a}/workflow-tools` therefore makes any agent a
    provisioner, including one created by `create_agent_channel`. Before B3b-1, only the
    library Builder copy wrote these names.
  - Invariant 4 (an owner/admin requester at call time) still applies, so this is a design gap,
    not a direct escalation. No test covers it.
- **Required:**
  - Either refuse reserved provisioning names in the grant route (the Builder keeps getting them
    through the library copy), or record a deviation that an explicit owner/admin grant is
    outside invariant 6 and amend the invariant.
  - Test whichever is decided.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R013).
  - 2026-10-23 **fixed** (WF-P08-R014, digest `b075aa52…11ef`): a grant that adds a provisioning name is 400; an agent already holding them may keep or drop them. Invariant 6 holds with no amendment. Tested.

## WF-P08-F058 — The decision comment route and the two UI decision mutations have no other-tenant test

- **Context:** WF-P08-R013, B3b-1, `decision_tests.rs`. Rule: `src/adapters/http/AGENTS.md`
  "Tests must attempt a valid id belonging to another tenant for every new detail, nested,
  download, and mutation route."
- **Classification:** actionable; **Low**; verification gap. The code paths do answer 404 by
  analysis.
- **Evidence:**
  - Strangers (own and foreign company paths) and a member are tried on
    `POST …/workflow-decisions/{d}/submit` (`decision_tests.rs:128-140`), on the JSON read
    (`:141-146`) and on the page GET (`:147-152`).
  - No stranger or foreign-path attempt is made on:
    - `POST /api/companies/{c}/workflow-decisions/{d}/comments`;
    - `POST /ui/workflow-decisions/{d}/submit`;
    - `POST /ui/workflow-decisions/{d}/comments`.
  - The UI pair resolves the company from the decision row, and a refusal first becomes a banner
    (`decisions.rs:204-227`). Their 404 for a stranger therefore depends on the subsequent
    re-read failing. That is exactly the kind of path the rule wants pinned.
  - The tenancy probe's "every route (20)" omits these, template copy and the grant. Those last
    two have their own tests.
- **Required:**
  - Add the three attempts (stranger on the owner's path, and stranger on their own company
    path for the JSON comment): each 404, with no comment row written.
  - Correct the "every route" wording.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R013).
  - 2026-10-23 **fixed** (WF-P08-R014, digest `b075aa52…11ef`): JSON comment, UI submit and UI comment by a stranger (both paths) and by a member → 404, with the comment count unchanged. The tenancy probe covers 22 routes.

## WF-P08-F059 — The workflow routes set no explicit body limit (optional)

- **Context:** WF-P08-R013, `workflows::router()` (`routes/workflows/mod.rs:34-125`). Rule:
  `src/adapters/http/AGENTS.md` "Set an explicit body limit appropriate to each route".
- **Classification:** **Info**.
- **Evidence:**
  - No `DefaultBodyLimit` is set, so axum's implicit 2 MiB applies to every route, from the
    grant and control bodies to draft saves. Sources are capped at 256 KiB
    (`compiler/rebase.rs:4`), decision data at 1 MiB (`workflow_decisions_data_check`) and
    comments at 16 KiB.
  - Several routers set explicit limits (`mcp.rs:36`, `model_usage.rs:24`,
    `ui_internal_notes.rs:63`). Many older JSON routes do not.
- **Suggestion:** set explicit limits per route group, sized to these payloads.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R013).
  - 2026-10-23 **fixed** (WF-P08-R014, digest `b075aa52…11ef`): explicit `DefaultBodyLimit` of 2 MiB on the workflow router.

## WF-P08-F060 — Decision page: inert daisyUI v4 classes, partial busy state, untested UI success path (optional)

- **Context:** WF-P08-R013, `pages/workflow_decision.rs`; `pages/AGENTS.md` (progress feedback;
  "don't propagate" inert v4 leftovers).
- **Classification:** **Info**.
- **Evidence:**
  - New markup uses `<label class="form-control …">` and `<div class="label">` (`:57`, `:61`).
    `form-control` does not exist in daisyUI 5 (0 matches in `assets/app.css`), although the
    log says "daisyUI 5 classes; no banned v4 classes".
  - The submit form disables its buttons and shows a spinner, but neither relabels a button
    nor sets `aria-busy` on `#workflow-decision`. The comment button does relabel
    ("Posting…").
  - The UI submit success path (the "Decision submitted" banner) is untested. Only the
    invalid-JSON banner and the JSON submit are covered.
- **Suggestion:**
  - Use daisyUI 5 `fieldset`/`legend` or plain labels.
  - Add `aria-busy` while the request runs.
  - Add one UI-submit success assertion.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R013).
  - 2026-10-23 **partly fixed** (WF-P08-R014, digest `b075aa52…11ef`): daisyUI 5 `fieldset`/`fieldset-legend`, and the UI submit success path is tested. The new "Submitting…" status uses a class that the committed CSS lacks, so it never shows, and CI's CSS check breaks. See F062.
  - 2026-10-23 **fixed** (WF-P08-R015, digest `57566f92…dd33`): the busy status uses the compiled `[.htmx-request_&]:inline` (F062).

## WF-P08-F061 — Grant protocol notes (optional)

- **Context:** WF-P08-R013, DV-8.
- **Classification:** **Info**. DV-8 is accepted.
- **Notes:**
  1. Without a key, a client that lost the response of a successful grant gets 409 on retry,
     because the current list now equals `tools`. Consider answering 200 when the current list
     already equals the requested one, which keeps the replacement idempotent.
  2. Unregistered names are grantable. That is consistent with name-based late binding
     (P07-Q2), and p09 should warn about such names.
  3. Count and name validation precede authorization, so a stranger can get a 400. This
     discloses nothing, because it does not depend on any record's existence.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R013).
  - 2026-10-23 **fixed** (WF-P08-R014, digest `b075aa52…11ef`): a grant whose current list already equals the requested one answers 200. Tested.

## WF-P08-F062 — The decision page's new "Submitting…" status uses a utility the committed CSS lacks: it never shows, and CI's CSS check fails

- **Context:** WF-P08-R014, F060 correction, `pages/workflow_decision.rs:67-69`. Rules:
  - `pages/AGENTS.md`: every action needs visible progress feedback, verified in the real app;
  - AGENTS.md: CI must exercise the build (`.github/workflows/ci.yml:40` runs
    `npm run check:css`).
- **Classification:** actionable; **Low**. It is a regression from R013 (the spinner showed
  there) and a CI break.
- **Evidence:**
  - The busy status is
    `<span class="hidden … [.htmx-request_&]:inline-flex" role="status">…Submitting…</span>`.
  - `assets/app.css` is committed and unchanged in this round. Its only `[.htmx-request_&]`
    variants are `hidden`, `inline`, `inline-block`, `opacity-80` and `pointer-events-none`
    (`rg -o "htmx-request_[^ {]*" assets/app.css`). `inline-flex` is the single variant in
    `src` the bundle lacks.
  - So the span stays `display: none` while the request runs. Neither the spinner nor
    "Submitting…" appears, and `role=status` announces nothing. The R013 markup used
    `[.htmx-request_&]:inline-block`, which is compiled, so its spinner did show.
  - Only the disabled buttons (`hx-disabled-elt`) remain as feedback.
  - `check:css` (`package.json:10`) rebuilds from `@source "../src/**/*.rs"` and runs
    `git diff --exit-code -- assets/app.css`. The new candidate now changes the output, so CI
    fails. `scripts/check.sh:86-97` does the same comparison.
  - I could not run Tailwind here (no `node_modules`). The conclusion rests on the bundle
    contents and the documented build.
- **Required:**
  - Use a compiled variant (for example `[.htmx-request_&]:inline`), or rebuild
    `assets/app.css` with `npm run build:css` and include it.
  - Then check the busy state in the running app, in both themes, as `pages/AGENTS.md`
    requires.
- **Instruction lesson:** candidate (scope: `src/adapters/http/pages/AGENTS.md`): "A new
  Tailwind utility or variant in `src` needs `npm run build:css`; `cargo test` cannot see a
  class the bundle lacks."
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R014).
  - 2026-10-23 **fixed** (WF-P08-R015, digest `57566f92…dd33`): `hidden text-sm [.htmx-request_&]:inline`. That variant is compiled in `assets/app.css` and already used on eleven other pages. A scan of every class attribute in the changed HTTP files against the bundle finds no missing utility. `npm run check:css` and a visual check were not run (no `node_modules`), as recorded in TASK.

## WF-P08-F063 — The child-control rule lives only in the HTTP route, and its stated reason does not hold (optional)

- **Context:** WF-P08-R014, F056 correction, `controllable_run` (`routes/workflows/bindings.rs:473-496`).
- **Classification:** **Info**. The decision is accepted for the current ingress.
- **Evidence:**
  - Today the route is the only production caller of `WorkflowService::cancel`/`retry`
    (`rg "CancelWorkflowRequest {"` outside tests). So the rule holds, and the run is authorized
    before its parent is named.
  - The doc comment and the request say the internal child controls use the command. They do
    not:
    - the parent→child cascade is `cancel_tree_on`, inside the persistence transition
      (`controls.rs:177-198`);
    - p07's "operator cancels the child" test calls the persistence `WorkflowTransitions::cancel`
      (`child_runtime_tests.rs:174-196`), not the service.
  - `WorkflowService::cancel`/`retry` already load the `RunHead`, whose
    `causality.trigger().source()` is `TriggerSource::Child` for a child (as `retry_on` uses).
    The rule could therefore sit there and still refuse after both authorizations, without
    breaking those callers.
- **Suggestion:**
  - Move the refusal into the service, so a later ingress (p09 UI, MCP or agent tools) cannot
    skip it.
  - Otherwise, correct the comment and note the constraint for p09.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R014).
  - 2026-10-23 **resolved** (WF-P08-R015, digest `57566f92…dd33`): the doc comment is corrected, naming `cancel_tree_on`, and says a later ingress that calls the service must apply the rule. Moving the refusal into the service is recorded in TASK for the phase that adds such an ingress; p09's UI uses these routes.

## WF-P08-F064 — Paging notes: `next` can name a hidden record, the definitions cursor follows a mutable title, and the probe plans are index-only (optional)

- **Context:** WF-P08-R014, F052/F053 corrections.
- **Classification:** **Info**.
- **Notes:**
  1. `next` is the id of the last scanned row (`bindings.rs:65`, `:387`). When that row is a
     binding or run the caller cannot see, its id is returned, although reading it answers a
     non-disclosing 404.
     - Short pages already reveal that hidden rows exist; that is the accepted F052 trade-off.
       An opaque cursor, such as an encoded `(created_at, id)`, would avoid handing out the id.
  2. The definitions cursor is resolved by the row's current `title`
     (`persistence/workflow/reads.rs:155-186`). Renaming that definition between pages can
     skip or repeat rows. This is acceptable for an admin listing, and an encoded cursor would
     settle it too.
  3. `logs/r013-fix-explain.log` probes a narrow table, so the "after" plans are index-only.
     The real `list_runs` reads 14 columns, so it will be an index scan with up to 201 heap
     fetches. That still uses the intended indexes and is still bounded.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R014).
  - 2026-10-23 **accepted** (WF-P08-R015, digest `57566f92…dd33`): recorded in TASK as accepted behaviour.

## WF-P08-F065 — D3.1 dispatches on the target variant, so every unslotted HTTP action, a normal production case, is refused

- **Context:** WF-P08-R016, gate P08-D3 rev 1 §D3.1 (`decision-D3-r1.md:73-87`).
- **Classification:** actionable; **Medium**. The new composition refuses a primary target it
  exists to serve.
- **Evidence:**
  - D3.1 sends `Local { messaging }` to `MessageActionAuthority`, refuses every other `Local`
    (NotFound), and handles HTTP/MCP only under `Connection { .. }`.
  - An HTTP operation published without a slot freezes as `ActionTarget::Local { resource:
    <connection id>, resource_kind: "http" }`:
    - `http_runtime.rs:41-51` builds `Connection` only when `tool.contract.connection` is set;
    - unslotted selections leave it `None` (`http_publication.rs:47`; `slot` is optional in the
      publish body, `routes/workflows/definitions.rs:206`).
  - Dispatch accepts both forms: `http_target` matches `Connection | Local` with kind `http`
    (`http.rs:313-326`), and `validate_run_authority` accepts `(Local, None)`
    (`actions/authorization.rs:167-175`).
  - B3b-1's own HTTP test publishes this way (`validate_and_publish_carry_http_selections`).
    So the B3b-2 fixture's HTTP action would most likely be `Local { http }`, which D3.1
    refuses.
- **Required:**
  - Dispatch on `resource_kind`, whichever variant carries it, as `http_target` does:
    - `messaging` (`Local`) → `MessageActionAuthority`;
    - `http` (`Local` or `Connection`) and `mcp` (`Connection`) → `authorize_company` plus
      `inspect_on`;
    - `provisioning` and anything else → NotFound.
  - Cover each form in the unit and route tests: `Local{http}`, `Connection{http}`,
    `Connection{mcp}`, `Local{messaging}` and `Local{provisioning}`.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R016).
  - 2026-10-23 **resolved** (WF-P08-R017, rev 2 `0cacf996…558c`): D3.1 decides by `resource_kind` in either form; `Local{http}` and `Connection{http}` → company + `inspect_on`; provisioning/other → NotFound; the five forms are in the verification plan.

## WF-P08-F066 — The route's outcome-to-status contract contradicts the existing command, and the verification plan asserts the contradiction

- **Context:** WF-P08-R016, §D3.5, §D3.6.3, §D3.7 and the verification plan. Existing contract:
  `ReconciliationOutcome` (`actions/reconciliation/contracts.rs`); persistence `snapshot`,
  `replay_on`, `record_refusal_on` and settlement `refusal_on`.
- **Classification:** actionable; **Medium**. The route contract p09 will build on is undefined
  where it matters.
- **Evidence:**
  - Conflicts are **`Ok` outcomes**, not errors:
    - a stale `expected_revision` makes `snapshot` record `RevisionConflict` under the command
      key and return it (`action_reconciliation/mod.rs:100-113`);
    - the same key with another request returns `IdempotencyConflict` (`:258-270`), not
      recorded;
    - a competing claimant whose snapshot preceded the winner's settlement gets
      `Blocked { StaleSnapshot }`, recorded (`settlement.rs:40-55`, `:155-161`), because every
      evidence insert advances the run revision (`workflow_action_evidence_revision` →
      `advance_workflow_owned_revision`);
    - one whose snapshot follows it gets `RevisionConflict`.
  - D3.5 says "the answer is 200 `{outcome, …}`" and also "409 for conflicts". The verification
    plan expects a stale revision → 409 and the same key with another note → 409, while D3.7
    and the competing test expect "409 or a recorded outcome". Without a stated table, the
    tests cannot be written deterministically, and p09 cannot tell a refusal from a success.
  - Refusals are durable under the key, and the request digest includes the revision. So a
    retry with the **same key** and a corrected revision is `IdempotencyConflict`, and the
    client must use a new key after any refusal. The decision does not say so.
  - Ordering detail: the missing-verifier check runs after `snapshot`. A `VerifiedReference`
    with a stale revision therefore records a `RevisionConflict` row and returns it, while a
    fresh one gets 400 with no row. The plan's "VerifiedReference → 400, with no evidence row"
    holds only for a fresh revision.
  - Naming: the body uses `delivery_id`; `ReconciliationTarget` serializes `delivery`.
- **Required:**
  - State the table. One consistent choice:
    - 200 with `outcome` for `UnknownRecorded` and the other recorded outcomes, `Blocked`
      included, with `replayed`;
    - 409 for `RevisionConflict` and `IdempotencyConflict`, carrying the outcome and the current
      revision, whether fresh or replayed.
  - State the fresh-key rule after a refusal, and the VerifiedReference ordering.
  - Align the body field names with the serde types, or name the mapping.
  - Make D3.6.3, D3.7 and the verification plan assert the exact outcome and status for each
    case: stale; same key with another note; competing claimants in both interleavings
    (StaleSnapshot and RevisionConflict); same-key concurrent (one apply, one replay).
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R016).
  - 2026-10-23 **resolved** (WF-P08-R017, rev 2 `0cacf996…558c`): outcome→status table (recorded outcomes 200; revision/idempotency/evidence conflicts and `blocked` 409 with the outcome body); fresh-key rule; ordering incl. the stale `VerifiedReference`; `delivery_id` mapping; exact D3.7 interleavings. Precision notes carried as F071/F072.

## WF-P08-F067 — The discovery read is not enough to form a valid message-delivery command, or to tell which markers are uncertain

- **Context:** WF-P08-R016, §D3.4.
- **Classification:** actionable; **Low**. A client cannot use the read for its stated purpose.
- **Evidence:**
  - A message target is accepted only while its delivery `status = 'outcome_unknown'`, and only
    for a single-part email with recipient, source binding and `request_started_at`. Any other
    destination is a non-disclosing 404 (`mod.rs:116-125`; `delivery.rs:36-117`).
  - D3.4 lists destinations as `(ordinal, delivery_id)` only, so the client cannot tell which
    destination is reconcilable and must probe with 404s.
  - Remote markers are listed for every incomplete execution with no receipt indicator. A marker
    whose receipt exists (`workflow_action_receipts` by invocation) is indistinguishable from an
    uncertain one.
  - `local` markers (provisioning, always receipted, D2.2a item 4) are listed although no target
    kind can name them.
- **Required:**
  - Add each destination's delivery `status` (or a `reconcilable` flag computed by the snapshot's
    own predicate).
  - Add per-marker receipt presence.
  - Leave out `local` markers, or flag them not reconcilable.
  - Compute `evidence_conflict` once per execution, not per row.
  - Optional for p09: the action's prior evidence (`workflow_action_evidence` notes and
    outcomes), so annotations are visible.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R016).
  - 2026-10-23 **resolved** (WF-P08-R017, rev 2 `0cacf996…558c`): `has_receipt`, `reconcilable`, destination `status`/`parts`; `local` markers dropped; `evidence_conflict` once per execution; no longer limited to incomplete executions.

## WF-P08-F068 — Reconciliation requires the connection to be ready now; the failure modes and tests omit it

- **Context:** WF-P08-R016, §D3.1, §D3.7; `validate_resource` (`actions/authorization.rs:97-115`),
  called in both transactions (`mod.rs:93`, `settlement.rs:34`) and in the service.
- **Classification:** actionable; **Low**. The behaviour is undocumented in an operator-facing
  path.
- **Evidence:**
  - `validate_resource` requires `authorized`, `readiness == Ready` and the frozen contract still
    registered. HTTP `status_on` reports a disabled connection as `Revoked` and a bearer
    connection without its secret as `Unavailable`, and lists only currently registered
    operations (`http.rs:239-265`).
  - So an operator who disables a misbehaving connection, removes its secret or unregisters the
    operation can no longer annotate the uncertain action. They get 404 "Workflow action
    authority" (`denied()`), the same answer as no access.
  - The trait documents the opposite intent: "Recording past truth requires access; it does not
    require current I/O approval" (`mod.rs:18-20`).
  - D3.7 lists only "connection deleted".
- **Required:**
  - Decide and state it: either accept it (cancel is then the only path, and the docs and p09
    say so), or relax readiness for reconciliation while keeping ownership and access.
  - Add the disabled, secret-removed and operation-unregistered cases to the failure modes and
    the tests.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R016).
  - 2026-10-23 **resolved** (WF-P08-R017, rev 2 `0cacf996…558c`): D3.2a keeps readiness (404; cancel is the path; revisit with a verifier); the trait doc line is corrected in B3b-2; readiness tests planned.

## WF-P08-F069 — The verification plan does not prove the new composition on every target, or the competing cases precisely

- **Context:** WF-P08-R016, verification plan (`decision-D3-r1.md:202-233`); AGENTS.md
  (competing claimants for a concurrency protocol).
- **Classification:** actionable; **Low**. Verification gap.
- **Evidence:**
  - The existing persistence suites use their own `SqlReconciliationResources` doubles
    (`action_reconciliation_snapshot_tests.rs:6`, `mcp_reconciliation_tests.rs:11`,
    `action_reconciliation_authority_support.rs:22`). None of them exercises
    `CompanyReconciliationResources`.
    - The plan lets the message-delivery target fall back to those suites "if the delivery
      fixtures are not cheap", so the new messaging delegation and the route's
      `message_delivery` mapping could ship untested.
  - The competing test accepts "409 or a recorded outcome". It neither fixes the interleavings
    (F066) nor asserts the durable effects: exactly one evidence row, one revision advance, and
    one command row per key.
  - There is no same-key concurrent case.
  - There is no MCP route case. The F065 target forms and the F068 readiness cases are absent.
- **Required:** the verification plan adds:
  - direct `lock_resource` tests through `CompanyReconciliationResources` for every target form
    in F065;
  - at least one messaging reconciliation through the production composition, direct or by
    route;
  - competing different-key claimants in both interleavings, asserting outcome, status, rows and
    revision;
  - a same-key concurrent pair;
  - the F068 cases.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R016).
  - 2026-10-23 **resolved** (WF-P08-R017, rev 2 `0cacf996…558c`): every test through the production composition; five target forms; exact outcomes/rows/revisions; both races; readiness; MCP; stated fallback with the gap reported.

## WF-P08-F070 — Notes on D3.2, D3.3 and D3.5 (optional)

- **Context:** WF-P08-R016.
- **Classification:** **Info**.
- **Notes:**
  1. **D3.2 is accepted as the p08 scope.** The plan requires the route over the owning command
     with unchanged semantics, not resumption. The prior phases' rule (operator claims never
     permit replay; `mcp_reconciliation_tests.rs:1-2`) forbids a stub verifier.
     - Recovery is cancel and then a rerun (B2 `rerun_of`). The decision could say so, and p09
       should label the control as an annotation.
  2. **Timeouts.** `AppError::Timeout` maps to 500 "Execution timed out"
     (`app_error_impl.rs:30-32`). The per-transaction `lock_timeout`/`statement_timeout` of 5 s
     equal the whole 5 s budget, so contention will surface as a 500, not a retryable 503. The
     command is idempotent by key, so this is safe, but 503 or `DatabaseTimeout` would say what
     happened.
  3. **D3.5 child rule is accepted.** Reconciliation records evidence about the child's own
     frozen action and, without a verifier, reopens nothing. If a verifier is added later,
     whether a child's job may reopen on its own should be revisited with WF-C002.
  4. **D3.1 lock order is confirmed, and is simpler than stated.**
     - Dispatch (`action_authority::prelock_on`), reconciliation (each transaction) and
       connection edits (`http.rs:85-106`) all take `authorize_company`'s company row
       `FOR NO KEY UPDATE` first, so they serialize on it.
     - The later orders cannot deadlock with each other: dispatch takes the fence, then the run,
       then the connection; reconciliation takes the run, then the execution and job, then the
       intent, then the connection.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R016).
  - 2026-10-23 **adopted** (WF-P08-R017, rev 2 `0cacf996…558c`): Timeout → 503 + `Retry-After: 1` on the workflow routes; p09 notice (annotation, not resume) at B3b-2 acceptance; D3.2 states cancel and rerun.

## WF-P08-F071 — Replay and timeout are stated more strongly than the command guarantees, which reverses the retry rule after a 503 (carried to B3b-2)

- **Context:** WF-P08-R017, gate P08-D3 rev 2 §D3.5 ("Fresh-key rule", "Ordering"), §D3.7
  ("Budget exhausted").
- **Classification:** actionable in B3b-2 (route docs and tests); **Low**. Client-contract
  precision, not a design defect. It does not block the gate.
- **Evidence:**
  1. **Readiness comes before replay.**
     - In `snapshot`, `lock_resource` and `validate_resource` run before `replay_on` and the
       revision check (`action_reconciliation/mod.rs:91-100`). The service's own `inspect`
       repeats this before it returns a recorded result.
     - So a same-key replay, or a stale-revision command, on a resource that is no longer ready
       (or after access was lost) answers 404 and records nothing.
     - The ordering list puts the recording (4) before readiness (5). The fresh-key rule says a
       replay "returns the recorded outcome" without that condition. The service documents it:
       "A snapshot or exact replay, both requiring current resource access".
  2. **A timeout is not a guaranteed rollback.**
     - The 5 s budget is a `tokio::time::timeout` around the whole operation
       (`reconciliation/service.rs:89`). When it fires during the settlement's `COMMIT`, the
       evidence and command rows may already be committed.
     - D3.7's "each transaction rolls back" does not hold. This is exactly the class `AppError`'s
       doc separates from `DatabaseTimeout` ("definitely not applied").
     - So after a 503 the client must retry with the **same key and request**, which replays a
       committed record. A new key would add a second note, or get `revision_conflict`. That is
       the opposite of the 409 rule (new key), and the decision does not say it.
- **Required in B3b-2:**
  - State both conditions in the route docs and the p09 notice: a replay requires current access
    and readiness; after a 503, retry with the same key, and after a 409, use a new key.
  - Correct D3.7's rollback wording in the implementation record.
  - Add a route test: a replay after disabling the connection → 404. A forced timeout during
    settlement is optional.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R017).
  - 2026-10-23 **fixed** (WF-P08-R018, digest `4156165b…20b3`): the `routes/workflows/reconciliations.rs` module doc states both rules (new key after 409; the same request and key after 503, since a timeout may follow a committed settlement; every call, a replay included, needs current access and a ready resource, or it is 404 with nothing recorded). The D3.7 rollback wording is corrected in TASK. Test `reconciliation_needs_a_ready_resource_even_to_replay`.

## WF-P08-F072 — Rev 2 precision notes (optional)

- **Context:** WF-P08-R017.
- **Classification:** **Info**.
- **Notes:**
  1. `evidence_conflict` is returned **after** the note is recorded: `truth_on` inserts the
     evidence first, then checks `workflow_action_has_evidence_conflict`. So the evidence row,
     the command row and the revision bump all exist. The table's "the action already has
     conflicting evidence" with 409 reads as a refusal. Without a verifier it is unreachable, so
     only the wording matters.
  2. The competing test's "read revision plus one per recorded row" should say per **evidence**
     row. A recorded refusal (command row only) does not advance the revision; only
     `workflow_action_evidence` inserts do.
  3. The destination `reconcilable` flag (`outcome_unknown` and one part) is narrower than the
     command's subject check, which also needs a recipient, a source binding and
     `request_started_at`. It is a hint; the command stays authoritative, as D3.4 says.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R017).
  - 2026-10-23 **adopted** (WF-P08-R018, digest `4156165b…20b3`): `evidence_conflict` is documented as recorded but conflicting; the race test asserts revision + 1 per evidence row; the destination flag is documented as a hint.

## WF-P08-F073 — B3b-2 test and observability notes (optional)

- **Context:** WF-P08-R018, B3b-2.
- **Classification:** **Info**.
- **Notes:**
  1. **Race test.** The either/or (`revision_conflict` or `blocked{stale_snapshot}`) is the
     contract's own (D3.7), and the test pins evidence = 1 and revision + 1. It is acceptable
     as is. It could also:
     - assert 409 for the `revision_conflict` branch too (only the `blocked` branch checks the
       status);
     - assert that the loser's refusal is recorded under its key: two command rows, or a replay
       of the loser's request returning its outcome with `replayed: true`.
  2. **Foreign marker.** `reconciliation_refuses_strangers_members_and_foreign_markers` accepts
     404 **or** 400 for a random marker. By the code it is 404 (`marker_on` → `missing()` inside
     the snapshot), so the test can pin 404.
  3. **Unlogged timeouts.** The new 503 branch in `WorkflowError::into_response` bypasses
     `AppError::into_response`, which is where the app logs every failed request
     (`tracing::error!`). Reconciliation timeouts are therefore silent. A `warn!` with the route,
     or a counter, would keep them visible.
  4. **Body errors.** A malformed body field (for example a bad `argument_digest`) is axum's
     `Json` rejection, 422, not the table's 400. That is consistent with the other workflow
     routes; worth one line in the route doc for p09.
  5. **Message test.** `workflow_message_unknown_note_through_the_production_composition` checks
     the verified reference with `is_err()` and not the 400 kind or the absence of a row. The
     route test covers the kind for HTTP.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R018).

## WF-P08-F074 — D4.3's per-reviewer decision notifications do not fit the notification model, and nothing withdraws them

- **Context:** WF-P08-R019, gate P08-D4 rev 1 §D4.3; `adapters/persistence/notification.rs`;
  schema `enqueue_actionable_notification_event` (:837), `notifications` indexes.
- **Classification:** actionable; **Medium**. The contract cannot deliver what it promises.
- **Evidence:**
  1. **One active recipient per source.**
     - A notification source is `(company, source_kind, source_id, action_kind)`.
     - `resolve_projection_on` withdraws any event once a newer generation of the same source
       exists (`notification.rs:592-635`).
     - `apply_projection_on` withdraws every active notification of the source whose recipient
       differs (`:914-946`, `recipient_user_id <> $6`).
     - Indexes `notifications_one_active_action_idx` and `notification_events_identity_key`
       enforce the same identity.
     - So with `source_id = decision`, which D4.3's
       `enqueue_actionable_notification_event('decision', 'decision_review', …)` implies, each
       reviewer's INSERT event supersedes the previous one: only the last reviewer keeps a
       notification.
     - Reviewer rows have no uuid identity: PK `(company_id, decision_id, principal_id)`, and
       `notice_delivery_id` becomes nullable.
  2. **No withdrawal events.**
     - The projection is re-evaluated only when an event is enqueued.
     - D4.3 adds only the reviewer-INSERT trigger, so a submission or a wait expiry enqueues
       nothing, and every reviewer's notification stays `active` after the decision closes.
     - Legacy sources enqueue on every lifecycle change (for example
       `notification_from_task_status`).
  3. **Company-associated decisions.**
     - `recipient_for_source` inner-joins `channels` on the source channel and calls
       `notification_principal_can_view($1, $3, $2)` (`:854-880`). A NULL `channel_id` drops the
       recipient, so the query and the function both need a no-channel rule.
     - D4.3's rule ("a current reviewer") is weaker than the page. For a channel-less run,
       `lock_decision` requires the reviewer to be currently privileged (owner/admin), so a
       demoted reviewer's notification would link to a 404.
- **Required:**
  - Give each reviewer a source identity, for example a uuid `id` on
    `workflow_decision_reviewers`, or a keyed uuid of `(decision, principal)`. Resolve the
    snapshot through it: reviewer row, decision `submitted_at`, wait state, and current
    visibility.
  - Enqueue one event per reviewer source on submission (`workflow_decisions` UPDATE OF
    `submitted_at`) and on wait settlement (`workflow_waits` UPDATE OF `state` for
    `reason='decision'`).
  - Define the NULL-channel visibility the same way as the page: current owner/admin for
    company-associated decisions.
  - Test with two reviewers: both get a notification; both resolve on submission and on expiry.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R019).
  - 2026-10-23 **resolved** (WF-P08-R020, rev 2 `d88aeb86…3bce`): no in-app decision notifications (delivery is the only source); decisions surface in attention My work, on the page, and by e-mail through DecisionNotices with the preference. This avoids the one-recipient-per-source conflict and the withdrawal events. `notifications.channel_id` stays NOT NULL.

## WF-P08-F075 — Delivery-failure recipients and branches are under-specified, and the B4c column drop leaves runtime-only failures in plpgsql

- **Context:** WF-P08-R019, §D4.2 (`delivery_failure` "unchanged"), §D4.3 (recipient fix), §D4.8
  (drop `message_deliveries.task_id`).
- **Classification:** actionable; **Medium**. An undefined recipient model, and a hazard that
  breaks every delivery status update.
- **Evidence:**
  1. **Recipients.**
     - "Or for a schedule- or system-created delivery the channel's owner/admins" names several
       recipients, while a source has one responsible principal (F074.1).
     - It does not say which deliveries those are:
       - schedule-started runs have an actor, so their deliveries resolve through
         `workflow_message_destinations` like any run;
       - decision-notice and notification e-mails are written with `company_id IS NULL`
         (reviewer guard `:4088-4093`), so `notification_from_delivery` returns early and never
         notifies. That must stay, or a failing notification e-mail would notify about itself.
  2. **The attention branch.**
     - `delivery_failure` is not "unchanged". It LEFT JOINs `background_tasks` on
       `delivery.task_id` for its owner and responsibility columns, and filters
       `delivery.task_id IS NULL OR task.queue_kind='legacy'` (`attention.rs:318-352`).
     - B4c drops `message_deliveries.task_id`.
     - Its responsibility (channel team) also differs from D4.3's notification recipient (the run
       actor).
  3. **Runtime-only breakage.** PL/pgSQL bodies are not checked against columns at CREATE time.
     These keep compiling after the drop and fail when they run:
     - `enqueue_actionable_notification_event`'s workflow-suppression clause reads
       `delivery.task_id` and `background_tasks` (`:845-862`), on every delivery failure via
       `notification_from_delivery`;
     - `notify_attention_changed` reads `row_data->>'task_id'`;
     - `notify_task_chain_changed` reads `NEW.task_id` for `message_deliveries`;
     - `delivery/enqueue.rs:30` calls `require_legacy_task_on`.
- **Required:**
  - Make the recipient the run actor through `workflow_message_destinations(company_id,
    delivery_id)`. A company delivery with no destination row gets one named principal (for
    example the company owner) or none, stated. Keep `company_id IS NULL` deliveries out.
  - Rewrite the `delivery_failure` branch with the same responsibility rule.
  - List every function, trigger and query that references the dropped columns, with its
    rewrite or deletion, in the B4c plan.
  - Add a B4c test that drives a workflow delivery to `dead_letter` and to `outcome_unknown`
    after the drop and asserts the notification and the attention row.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R019).
  - 2026-10-23 **resolved** (WF-P08-R020, rev 2 `d88aeb86…3bce`): one recipient: the run actor of a `workflow_message_destinations` delivery while a current member who can read the channel. A person's mailbox send gets no notification (attention only), and `company_id IS NULL` notices never notify. The `delivery_failure` branch no longer joins `background_tasks`. The dependent PL/pgSQL and Rust SQL are listed with their rewrite or deletion. B4c adds a function-body catalog check and post-drop `dead_letter`/`outcome_unknown` tests. Notes in F085.

## WF-P08-F076 — A conditional notice guard loses the "provable notice" invariant unless NULL is tied to a recorded opt-out

- **Context:** WF-P08-R019, §D4.3; `workflow_decision_reviewer_guard` (schema :4078-4097).
- **Classification:** actionable; **Low**.
- **Evidence:**
  - Today the guard proves that every reviewer row was notified: a notification-purpose,
    company-less delivery whose idempotency key names this decision and principal.
  - D4.3's "check the delivery only when it is set" lets any code path insert a reviewer row
    without a notice, an opted-in reviewer included, and nothing detects it.
- **Required:**
  - Allow NULL only when the reviewer has opted out at insert time. Either read
    `user_notification_preferences.decision_email_enabled = false` in the guard, or snapshot the
    flag in a reviewer column (for example `notice_opted_out boolean NOT NULL`) checked as
    `(notice_delivery_id IS NULL) = notice_opted_out`.
  - Test that a NULL notice for an opted-in reviewer is refused.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R019).
  - 2026-10-23 **resolved** (WF-P08-R020, rev 2 `d88aeb86…3bce`): `notice_opted_out` with `CHECK ((notice_delivery_id IS NULL) = notice_opted_out)`; the guard requires a stored `decision_email_enabled = false` for an opt-out; a refused-NULL test. Race note in F085.

## WF-P08-F077 — A LISTEN reconnect loses wakes silently; D4.10's "the reconnect logic emits lag" does not hold

- **Context:** WF-P08-R019, §D4.10 ("A lost wake"), §D4.1; `infra/events.rs:298-366`;
  sqlx-postgres 0.8.6 `PgListener`; `src/adapters/http/AGENTS.md` (reconcile on every
  reconnect).
- **Classification:** actionable; **Low**. Pre-existing, but the new protocol relies on it and
  the decision states the opposite.
- **Evidence:**
  - The listener loops on `listener.recv()`. sqlx documents that `recv()` reconnects
    transparently and that "any [notifications] received while the connection was lost, will
    not be returned". Only `try_recv()` signals the reconnect, by returning `Ok(None)`.
  - Nothing publishes a lag or reconcile event on reconnect, and `broadcast` lag happens only
    when a subscriber falls behind.
  - Only some consumers have a periodic recheck (the counts `RefreshScheduler`, 60 s).
    Attention, activity and simulation would stay stale until the next unrelated event.
- **Required:**
  - Use `try_recv()` and publish a process-wide "reconcile" event on `Ok(None)` (and after the
    outer reconnect), which every consumer treats like lag; or give each consumer a bounded
    periodic reconcile.
  - Correct D4.10, and test a forced listener reconnect.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R019).
  - 2026-10-23 **resolved** (WF-P08-R020, rev 2 `d88aeb86…3bce`): the listener uses `try_recv()` and publishes `MailboxEvent::Resync` on `Ok(None)`, which every consumer treats like lag; a forced-reconnect test. sqlx re-LISTENs before returning `Ok(None)` (`eager_reconnect`), so the resync follows the re-subscription. The outer reconnect is carried as F084.

## WF-P08-F078 — The attention branches need a multi-reviewer row model, a live source for refusals, and the indexes the new predicates need

- **Context:** WF-P08-R019, §D4.2; `ATTENTION_SQL` (`attention.rs:87-387`).
- **Classification:** actionable; **Low**.
- **Evidence:**
  1. **One responsible principal per row.**
     - Every branch yields one row with one `responsible_principal_id`. `ranked` filters
       `my_work` by `raw.responsible_principal_id = $3` (`:353-365`).
     - A decision has up to 16 reviewers (`ordinal` 0..15). D4.2 does not say whether it is one
       row per reviewer (Team work shows duplicates) or one row per decision (My work needs a
       reviewer-membership predicate instead of `responsible_principal_id`).
  2. **No live source for refusals.**
     - `binding_refusal` rows come from `workflow_message_intent_bindings` and `schedule_runs`.
     - Neither emits NOTIFY, and a refusal creates no run, so `workflow_run_changed` never fires
       for it. The list goes stale until an unrelated event.
  3. **Indexes.**
     - "Not superseded by a rerun" probes `workflow_runs.rerun_of_run_id`, which has no index:
       only the FK, and `pg_indexes` lists none.
     - Failed `schedule_runs` in the company's 7-day window have no supporting index: there are
       only `(schedule_id, created_at)` and the materialization partials.
     - "The refusal index exists already" covers the intent bindings only.
- **Required:**
  - State the decision row model and its My work and Team work predicates.
  - Give refusals a live source: a NOTIFY on the refusal rows, or a periodic reconcile.
  - Add indexes for the rerun probe and the failed `schedule_runs` window, with B4a EXPLAIN
    evidence.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R019).
  - 2026-10-23 **resolved** (WF-P08-R020, rev 2 `d88aeb86…3bce`): one row per decision; My work for a current reviewer (owner/admin while company-associated, as on the page); Team work with "N reviewers"; never Unassigned. Refusals refresh on the 30 s attention tick (verified in `routes/attention.rs`), with UI copy saying so. Indexes for the rerun probe and the failed `schedule_runs` window.

## WF-P08-F079 — One inbound message can admit several runs, but D4.6 gives a message one run

- **Context:** WF-P08-R019, §D4.6 (`ThreadMessageView.task_id → run_id`, simulation "its run");
  schema `workflow_message_intent_bindings` (PK `(company_id, intent_id, binding_id)`),
  `workflow_message_intents` (UNIQUE `(company_id, message_id, channel_id)`).
- **Classification:** actionable; **Low**.
- **Evidence:**
  - An inbound message has one intent per channel it reached, and each intent admits one run
    per active binding (up to 8 per channel). The plan's acceptance even requires parallel
    message runs in one conversation.
  - A single `run_id` for an inbound message is therefore ambiguous. So is the simulation's
    "an admitted message shows its run".
  - Agent replies are unambiguous: `workflow_message_destinations` is unique by `message_id`.
- **Required:**
  - Make the inbound lookup a bounded list (`run_ids`, with the binding and outcome per entry),
    or state the selection rule.
  - Show every admitted or refused binding per message in the simulation.
  - Test with two bindings.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R019).
  - 2026-10-23 **resolved** (WF-P08-R020, rev 2 `d88aeb86…3bce`): `run_ids: Vec<Uuid>` (≤ 8 per channel intent); the simulation shows every binding outcome; tests with two bindings.

## WF-P08-F080 — Onboarding's "create its channel" is refused for the agents onboarding creates

- **Context:** WF-P08-R019, §D4.7 (onboarding step 2 through `AssistantChannelService` with
  `AssistantAgent::Existing`).
- **Classification:** actionable; **Medium**. The default onboarding path would fail.
- **Evidence:**
  - `AssistantAgent::Existing` refuses an agent without an explicit provider and model: "Choose
    the agent's provider and model first: a workflow runs an agent only on an explicit model"
    (`persistence/workflow/provisioning.rs:569-590`).
  - Onboarding creates its agents with no model:
    - from instructions with `ModelOverrides::default()` (`routes/onboarding.rs:266-276`);
    - from the library with the definition's provider and model, which are None for ordinary
      library agents (`library_agent_write`; D2.6 writes an explicit model only for definitions
      that grant workflow tools).
  - Step 1 lets the company be created with no model connection at all (`onboarding.rs:112-136`).
  - The current flow also accepts several library agents plus a custom one; D4.7 speaks of "the
    first agent".
  - "A command key derived from the onboarding session" names no stable input.
- **Required:**
  - Define the model rule. For example, onboarding requires a model connection and writes the
    first agent's explicit provider and model from the company default, as D2.6 does. Or, when
    no usable model exists, the channel step shows the refusal and links to the agent page.
  - State how many agents onboarding creates and which one gets the channel.
  - Derive the command key from stable inputs, for example the company id and a fixed label.
  - Test onboarding with an instruction agent, a library agent, and no model connection.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R019).
  - 2026-10-23 **resolved** (WF-P08-R020, rev 2 `d88aeb86…3bce`): onboarding pins created agents to the default connection (as library pinning does), provisions per agent with key `onboarding:{agent_id}`, skips channels without a model, and reports per agent on `/ui/onboarding/companies/{c}/complete`; tests for each path.

## WF-P08-F081 — B4a's plain channels conflict with the active-agent constraint triggers that stay until B4c

- **Context:** WF-P08-R019, §D4.7 ("creation makes a plain channel … the constraint triggers go
  in B4c"); TASK B4 plan (B4c is the single schema edit).
- **Classification:** actionable; **Low**. A sequencing hazard.
- **Evidence:**
  - `enabled_channel_active_agent_check` (AFTER INSERT OR UPDATE OF `enabled` ON channels) and
    `channel_assignment_active_agent_check` raise "enabled channel must have an active agent at
    position 0" unless `channel_agents` has a position-0 row (schema :653-676, :10910, :10938).
  - In B4a, creating an enabled plain channel with no agent fails at commit, as does editing
    one to enabled.
- **Required:** either drop the two triggers (and their function) in B4a with a minimal schema
  edit, or keep B4a's channel creation on the existing agent-assignment path until B4c. Say
  which, and test creating an enabled plain channel in B4a.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R019).
  - 2026-10-23 **resolved** (WF-P08-R020, rev 2 `d88aeb86…3bce`): B4a's schema edit drops both active-agent constraint triggers and their function; inbound commit is intent-based (no `channel_agents` reliance); a test for an enabled plain channel.

## WF-P08-F082 — The verification plan does not prove the new live protocol for every consumer, or the cases above

- **Context:** WF-P08-R019, §D4.11; `src/adapters/http/AGENTS.md` ("Tests must cover an event
  between render and subscribe and two reads completing out of order whenever a new live or
  partial-page update protocol is introduced").
- **Classification:** actionable; **Low**. Verification gap.
- **Evidence:**
  - Only attention gets the event-between-render-and-subscribe and out-of-order cases. Run
    counts, activity, simulation and decision notifications are new consumers of
    `workflow_run_changed` or `thread_activity`, and they get a single wake test at most.
  - Nothing tests these:
    - the trigger's firing rule: INSERT, and a state or `waiting_reason` change, but not
      revision-only updates;
    - the payload's fail-closed parse;
    - the F074 lifecycle (two reviewers, submission, expiry);
    - the F075 post-drop delivery path;
    - the F076 guard;
    - the F077 reconnect;
    - the F079 multi-run message;
    - the F080 onboarding without a model;
    - the F081 sequencing.
- **Required:** add these to D4.11 under the sub-batch that implements each.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R019).
  - 2026-10-23 **resolved** (WF-P08-R020, rev 2 `d88aeb86…3bce`): per-consumer live and race tests, trigger firing and parse tests, and tests for each finding's case.

## WF-P08-F083 — D4 notes (optional)

- **Context:** WF-P08-R019.
- **Classification:** **Info**.
- **Notes:**
  1. **D4.1 trigger mechanics.**
     - PostgreSQL forbids `OLD` in the WHEN of a trigger that fires on INSERT, so the trigger
       needs two triggers (INSERT; UPDATE … WHEN …) or a check in the function body.
     - `notify_thread_activity()` is a trigger function with no arguments, not a callable
       `notify_thread_activity(company, channel, thread)`. Either call `pg_notify('thread_activity',
       …)` inline, or attach it as its own trigger with `WHEN (NEW.thread_id IS NOT NULL)` and
       the same transition rule.
     - The single run-level trigger does cover reviewer assignment and every submission: all
       three park paths (business, action approval, token allowance) set `waiting/decision` in
       the transaction that inserts reviewers (`decisions.rs:183`, `decision_approvals.rs:131`
       via `close_attempt`), and submissions set `running` or a terminal state.
  2. **Accepted:**
     - dropping the per-agent count badges (runs record no agent);
     - the D4.5 dashboard mapping;
     - the Builder as an agent-only library install whose grants come from the copy (consistent
       with F057's route rule);
     - keeping `channel_gate`. When the assignment code is deleted, keep the exclusive gate in
       the channel-settings edit path, and keep one competing disable-vs-ingress test.
  3. **The 7-day windows** for `run_failure` and `binding_refusal` are a product choice; say so
     in the UI copy, for example "in the last 7 days".
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R019).
  - 2026-10-23 **adopted** (WF-P08-R020, rev 2 `d88aeb86…3bce`): split INSERT/UPDATE triggers; the existing `notify_thread_activity()` reused (runs carry its three columns); the 7-day copy; a channel_gate competing test.

## WF-P08-F084 — Publish `Resync` after the outer listener reconnect too (carried to B4a)

- **Context:** WF-P08-R020, gate P08-D4 rev 2 §D4.1 "Reconnects"; `infra/events.rs`
  (`run_mailbox_event_listener`, `listen_until_error`); sqlx-postgres 0.8.6
  `PgListener::try_recv`.
- **Classification:** actionable in B4a; **Low**. It does not block the gate.
- **Evidence:**
  - Rev 2 publishes `Resync` only on `try_recv()` → `Ok(None)`.
  - With `eager_reconnect` (the default), sqlx reconnects and re-LISTENs before returning
    `Ok(None)`. If that reconnect fails, or any non-I/O error occurs, `try_recv` returns `Err`.
  - `listen_until_error` then returns, and `run_mailbox_event_listener` sleeps
    `RECONNECT_BACKOFF` and builds a new `PgListener`. Every notification during that gap is
    lost, and rev 2 publishes no `Resync` for it.
  - A `pg_terminate_backend` in the planned test goes through the `Ok(None)` path: the FATAL
    `ErrorResponse` is ignored by `try_recv`, and the following EOF triggers the reconnect. So
    the test does not cover the outer path either.
- **Required in B4a:** after a new listener completes `listen_all` in the outer loop, publish
  `Resync` before handling notifications. Test with a listener whose reconnect fails once (for
  example a pool that refuses the first connection) and assert `Resync` after recovery.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R020).
  - 2026-10-24 **fixed** (WF-P08-R021, digest `5b27364a…2c16`): `listen_until_error` publishes `Resync` right after `listen_all`, for the first listener and for every outer rebuild, and on `try_recv` `Ok(None)` (`infra/events.rs:326-358`). Test `the_listener_resyncs_on_start_and_reconnect_and_drops_a_bad_run_payload` (start Resync is the outer path; `pg_terminate_backend` → Resync). Repeated ×5 green.

## WF-P08-F085 — Rev 2 implementation notes (optional)

- **Context:** WF-P08-R020.
- **Classification:** **Info**.
- **Notes:**
  1. **Catalog check precision.** "No function body mentions a dropped table or column" must
     match qualified names or dropped tables. A bare `task_id` also appears legitimately, for
     example `task_attempts.task_id`, which is kept, so a naive search would give false
     positives.
  2. **Opt-out race.** `eligible_reviewer` reads the preference without a lock, and the guard
     reads it again at insert. A concurrent toggle between the two fails the park with 23514,
     which the attempt then retries. Reading the preference row `FOR SHARE` in the eligibility
     query, after member and principal, makes compose and guard agree.
  3. **Onboarding replay.** `onboarding:{agent_id}` makes the channel step replay-safe. A replay
     of the whole POST is stopped earlier by agent slug uniqueness. The "replayed onboarding post
     does not duplicate channels" test should replay the channel step for existing agents (for
     example after a partial failure), not re-create the agents.
  4. **Recipient join.** `notifications.channel_id NOT NULL` assumes every company delivery has a
     channel, which holds for message deliveries. Keep the inner join to `channels` in
     `recipient_for_source` as rev 2 states.
- **Resolution history:**
  - 2026-10-23 open (WF-P08-R020).
  - 2026-10-24 **adopted** (WF-P08-R021, digest `5b27364a…2c16`): (2) `eligible_reviewer` reads the preference `FOR SHARE` after member and principal, with the competing test `workflow_decision_park_waits_for_a_preference_toggle_in_flight`; (3) the onboarding replay test replays the channel step ("replay adds none"); (4) `recipient_for_source` keeps the inner join; (1) the catalog check is due in B4c.

## WF-P08-F086 — The mailbox's "Open in Simulator" button now leads non-owners to "Company not found."

- **Context:** WF-P08-R021, B4a. The simulation became owner-only on every route (D4.6),
  `routes/channel.rs:754-768`. The button is rendered for every mailbox viewer
  (`pages/mailbox.rs:2218-2219`).
- **Classification:** actionable; **Low**. B4a introduced a user-reachable dead control.
- **Evidence:**
  - At base `da86871`, `simulate_channel_page` accepted any user who could load the channel
    (`get_company`, then `get_company_channel`). Only the POST and the stream were owner-only.
  - B4a adds `company.user_id == user.id` to the page GET and to the open-thread routes
    (`channel.rs:766`, `:1287`). A non-owner gets the bare `error_alert("Company not found.")`
    page.
  - The thread detail pane still renders
    `<a href="/companies/{c}/channels/{ch}/simulate?thread_id=…">Open in Simulator</a>` for
    every reader of the thread, admins and members included, with no ownership condition.
  - No test covers a non-owner on the mailbox pane or the page.
- **Required:**
  - Render the button only for the company owner. The pane already knows the viewer and the
    company. Alternatively, have the page answer non-owners with an explicit "Simulation is
    available to the company owner" message.
  - Add a page test for the button: absent for an admin, present for the owner.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R021).
  - 2026-10-24 **fixed** (WF-P08-R022, digest `a5440048…20c8`): `MessagePane.viewer_membership` (from `principal_access_for_user`, owner = `companies.user_id` as in the simulation routes; no access = `None`). `simulator_link` renders only for `Owner`. Every pane goes through `render_message_pane`, the internal-note re-render included. Test `only_the_company_owner_is_offered_the_simulator`.

## WF-P08-F087 — B4a notes (optional)

- **Context:** WF-P08-R021.
- **Classification:** **Info**.
- **Notes:**
  1. **Index deviations accepted, with evidence** (`logs/b4a-index-explain.{sql,log}`):
     - `workflow_decisions_open` and `model_usage_calls_run` are added;
     - D4.2's `workflow_decision_reviewers (company_id, user_id)` is not added. My work is driven
       from open decisions and probes the reviewer primary key
       `(company_id, decision_id, principal_id)`, so its cost stays bounded by open decisions,
       not by a reviewer's history.
  2. **Run lookup.** `THREAD_RUN_LOOKUP_SQL` claims "de-duplicated" in TASK but has no
     `DISTINCT`. Duplicates cannot arise today: the destination run and the admitted runs come
     from different thread rows. The wording should match.
  3. **Notification link for a demoted actor.** The delivery-failure notification links to
     `/ui/deliveries`, which requires a managed company. Notification visibility only requires
     channel readability, so a run actor who was demoted to member gets a link that refuses.
     Rare; consider linking to the thread.
  4. **Not run, and needed at the end:**
     - `npm run check:css`: the deleted pages remove class candidates, so expect an
       `assets/app.css` rebuild;
     - `npm run test:run-counts`: no Playwright here. I checked statically that both extraction
       markers exist (`RUN_COUNTS_SCRIPT` in `pages/run_counts.rs`, `EVENT_DELEGATION_SCRIPT` in
       `mailbox.rs`) and that the extracted scripts parse.
  5. **Legacy approval link.** `application/use_cases/approval.rs:163` still formats
     `/approvals/{token}`, whose route is deleted. Nothing reachable creates an approval any more
     (the approval and task entry points are cut, and inbound records intents), and B4b deletes
     the code.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R021).
  - 2026-10-24 **resolved** (WF-P08-R022, digest `a5440048…20c8`): item 2 withdrawn: `fetch_thread_runs` de-duplicates in Rust (`HashSet` retain, `thread/views.rs`); items 1, 3 and 4 accepted as recorded; item 5 goes to B4b.

## WF-P08-F088 — B4b silently dropped four workflow checkpoint-store tests, two of them competing-claimant tests

- **Context:** WF-P08-R023, B4b. The edit to `persistence/workflow/harness_owner_tests.rs`
  removed the legacy-supersede test. The same hunk also removed the two lines after it, the ones
  that declare the store tests (HEAD `harness_owner_tests.rs:320-321`):
  `#[path = "harness_store_tests.rs"] mod store_tests;`.
- **Classification:** actionable; **Medium**. Live tests of live code stopped running, and
  nothing reports it. This contradicts DV-B4b-7 ("none was weakened") and the AGENTS.md rule
  that a concurrency protocol keeps a test with competing claimants.
- **Evidence:**
  - `src/adapters/persistence/workflow/harness_store_tests.rs` is still tracked and unmodified,
    but no module declares it. A reachability scan of `src/` finds exactly two undeclared files:
    this one and the file in F089. `cargo test --lib -- --list` lists no `store_tests` test.
  - The four tests exercise the **workflow** checkpoint store, not legacy code:
    `open_workflow_checkpoint`, `reserve_workflow_execution` and `load_workflow_checkpoint`
    (`persistence/workflow/checkpoints.rs:54`, `:101`, `:171`).
    - `workflow_harness_owner_store_competing_open_and_execution_replay` (`:24`): two concurrent
      opens and two concurrent reservations; exactly one is `Applied` and both return the same
      checkpoint.
    - `…_fence_and_actor_revocation` (`:97`): a stale fence; an actor revocation racing an open.
    - `…_new_claim_retains_checkpoint_and_debits_execution` (`:148`).
    - `…_deferred_failure_rolls_back_open_and_debit` (`:233`).
  - Nothing else covers the concurrent open, the concurrent reservation or the revocation race.
    `workflow_harness_owner_competing_open_has_one_execution_identity` competes raw SQL inserts,
    not the store API. The other `open_workflow_checkpoint` and `reserve_workflow_execution`
    callers in tests run sequentially.
  - In a scratch copy with the two lines restored and nothing else changed, all four compile and
    pass on `workflow08_test` (`logs/r023-orphan-store-tests.log`: 4 passed).
  - DV-B4b-7 lists only "the legacy-supersede test and the legacy-store assertions" for
    `harness_owner_tests`.
- **Required:**
  - Restore the two lines that declare the module. No test change is needed.
  - Rerun `harness_owner_tests`.
  - Optional: add a cheap guard against an undeclared `*.rs` under `src/` (a script or a unit
    test) so the next accidental orphan fails CI.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R023).
  - 2026-10-24 **fixed** (WF-P08-R024, digest `c4b7eaa9…c896`): `harness_owner_tests.rs:255-256` declares `mod store_tests` again. The four store tests run and pass (`logs/r024-focused.log`). My reachability scan finds no undeclared `.rs` file. The guard is not added; it is recorded as an instruction candidate.

## WF-P08-F089 — A legacy test helper is left behind, uncompiled, since its includer was deleted

- **Context:** WF-P08-R023. `src/application/use_cases/thread/model_usage_tests.rs` was
  included only from `thread/inter_channel_tests.rs` (HEAD `:748`,
  `#[path = "model_usage_tests.rs"] mod model_usage_assertions;`). B4a deleted that file (and I
  missed the orphan in WF-P08-R021).
- **Classification:** actionable; **Low** (cleanup; a dead file that names deleted fixtures).
- **Evidence:**
  - The file's own header reads "Assertions shared by production dispatch, repair,
    approval-resume and cancellation scenarios". It reads `model_usage_calls … AND task_id=$2`
    and uses the deleted dispatch `Fixture`.
  - No module declares it. Its helpers `assert_projection` and `partial_response` have no
    callers (`logs/r023-uncalled.txt`).
  - It is not the same file as `persistence/workflow/model_usage_tests.rs`, which
    `admission_tests.rs` still includes.
- **Required:** `git rm` it with the rest of the legacy dispatch tests.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R023).
  - 2026-10-24 **fixed** (WF-P08-R024, digest `c4b7eaa9…c896`): the file is removed (`git rm`, staged).

## WF-P08-F090 — Dead code the deletions exposed is still in place

- **Context:** WF-P08-R023. TASK "B4b progress" says the dead code the deletions exposed was
  removed. These items lost their last caller in this batch and are still compiled. They are
  `pub`, so neither rustc nor clippy reports them.
- **Classification:** actionable; **Low**.
- **Evidence** (name-based scan of current against HEAD, `logs/r023-uncalled.txt`; each checked
  by hand):
  - **Harness output and the rig loop.**
    - `AgentExecutionDisposition::Suspended` (`services/harness/output.rs:40`) is never
      constructed. Its producers were the legacy `execution.rs:571` and `test_support.rs:359`;
      its consumers were `dispatch.rs:515` and `:1010`.
    - `AgentExecutionOutput::stamp_duration` (`output.rs:50`; only caller
      `agent_runner/mod.rs:245`) and its doc paragraph ("the caller that also owns the timeout and
      the lease").
    - `Driven::Suspended(Option<WaitId>)` is now always `Some`: `workflow.rs:119`, `:150` and
      `:213` are the only producers. That leaves the `Suspended(None)` arm at `workflow.rs:386`
      ("Workflow suspension has no wait") unreachable. The `Option` existed for the legacy
      binding's checkpoint-held waits.
  - **ThreadUseCases.**
    - `save_message_with_deliveries` (`thread/mod.rs:1032`; only caller `task_worker.rs:1243`).
    - `compose_delivery` (`:1056`; callers `task_worker.rs:1226`, `human_completion.rs:101`,
      `:226`).
    - `get_agent_history` (`:734`; callers only in the deleted `external_reply_tests`).
  - **Transport and message write.**
    - `InboundEnvelope::addressed_in` (`transport/ingress.rs:421`; only caller `dispatch.rs:176`).
    - `MessageWrite::with_structured` (`thread/message_write.rs:150`; only caller
      `dispatch.rs:532`).
  - **Test support** (`#[cfg(test)]`, lower value):
    - `thread/test_support.rs`: `queued_deliveries` `:182`, `insert_thread` `:206`,
      `email_write` `:1212`;
    - `protocols/email/test_support.rs`: `only_mail` `:26`, `recording_transport` `:60`;
    - `persistence/test_support.rs`: `test_machine` `:73`.
- **Required:**
  - Delete these, including the `Suspended` disposition variant and the `Option` around
    `Driven::Suspended`. No backward compatibility is needed: the checkpoint JSON is reset with
    the database in B4c.
  - Or record, item by item, why one stays (none is on the B4c/B4d list).
  - `agent_run_timeout_from_env` is also uncalled, but B4c owns it with `AGENT_RUN_TIMEOUT_SECS`
    (F093.4).
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R023).
  - 2026-10-24 **fixed** (WF-P08-R024, digest `c4b7eaa9…c896`): every listed item is removed, plus a scan cascade: `create_message_with_deliveries`, `classify_external_destination` and `ExternalDestinationClassification`, `identities_for_principals`, the company team account and email queries, `CompanyTeamAccount`, `from_access_role`, and `agent_run_timeout_from_env` with its constant. `AgentExecutionDisposition` and the output `disposition` field are gone. No SQL, schema check or fixture reads `final_output.disposition`. `Driven` and `Progress` suspension now carry a `WaitId`. 2 more `.sqlx` files were removed; I verified both by exact text in HEAD's `persistence/company.rs` and absent from the current source. The 25 remaining files all match.

## WF-P08-F091 — Comments and intra-doc links still name items this batch deleted

- **Context:** WF-P08-R023, review point 7. `cargo doc --no-deps --lib --document-private-items`
  (`logs/r023-doc.log`) reports unresolved links. These are the ones B4b's own deletions caused,
  plus plain comments naming deleted code. They are code comments, not the documentation B4d
  owns.
- **Classification:** actionable; **Low**.
- **Evidence:**
  - **Deleted `InboundTaskPayloadV1`** (unresolved):
    - `application/transport/ingress.rs:398`;
    - `use_cases/thread/mod.rs:1436-1442`, which also still says "A durable task now carries…".
  - **Deleted `ports::AgentHarness`** (unresolved): `services/harness/classifier.rs:3`, `:12`.
  - **`transport/dependency_tests.rs:115-122`:** links the deleted `AgentHarness`, and says
    "`src/application/services/harness/tests.rs` asserts the same thing one module in". That file
    is deleted.
  - **`use_cases/thread/mod.rs:236`:** "Work driven by a task commits through
    `TaskPersistence::commit_agent_dispatch`", which is deleted.
  - **`persistence/dashboard.rs:5`:** "Kept out of [`super::task`]" (unresolved; the module is
    deleted).
  - **`app_error.rs:13`:** "an execution deadline the task worker treats as final".
  - **`entities/harness.rs:133-134`:** "kept as a value until the legacy runner's harness
    registry is removed in phase 08". The registry is now removed. The reason that remains is
    DV-B4b-4: `HarnessConfig` still uses it until B4c.
  - **`adapters/monitoring/in_memory_monitor.rs:9`:** gives `stuck_work{kind="dead_lettered"}`
    as its example. That kind no longer exists.
- **Required:**
  - Reword or remove each of these in B4b.
  - The repository docs stay in B4d as planned: README, `docs/*`, `src/**/AGENTS.md`,
    `.cargo/config.toml` and `scripts/stack-budget.sh` comments (F093.5).
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R023).
  - 2026-10-24 **fixed** (WF-P08-R024, digest `c4b7eaa9…c896`): each comment is reworded. `cargo doc` with private items reports 12 unresolved links, every one already present in R023's run; the four that named B4b items are gone (`logs/r024-doc.log`). What remains is in `src/AGENTS.md`, `persistence/AGENTS.md` (B4d) and the task-worker concurrency `expect` text (B4c, with `TASK_WORKER_CONCURRENCY`).

## WF-P08-F092 — The relocated stuck-delivery census has no database-backed test

- **Context:** WF-P08-R023, review point 4. The census moved out of the deleted
  `persistence/task/operations.rs` into a new adapter method, `census_stuck_deliveries`
  (`persistence/delivery/queue.rs:431-462`). `DeliveryWorker::reap()` publishes it
  (`delivery_worker.rs:300-346`).
- **Classification:** actionable; **Low**. It is new SQL, and its only test is a mock.
- **Evidence:**
  - The worker test checks the gauge names and the zeroes against a mock queue
    (`delivery_worker_tests.rs:175-196`, mock `:318`). No test runs the SQL.
  - HEAD's only database test of the census, `the_census_sees_each_kind_of_stuck_work`
    (`task/tests.rs:6048`), planted task rows only. It went with `task/tests.rs`, so the delivery
    arms were never covered by a database test, and now nothing is.
  - Failure mode: a broken statement would only log "Could not take the stuck-delivery census"
    and publish no gauge. The alert would go silent, which is what this census exists to prevent.
  - I checked the statement by hand: `PREPARE`/`EXECUTE` with a `300 seconds` interval on
    `workflow08_test` runs, and `EXPLAIN` shows a Seq Scan on `message_deliveries`.
  - The comment "through `message_deliveries_claimable_idx` for the overdue arm" is therefore
    wrong. One `FILTER` aggregate over the whole table cannot use that index for one arm. The
    claim is inherited from HEAD's comment; the 30-second full scan is not a regression.
- **Required:**
  - Add a database-backed test on `own_database`, or as "at least ours". Plant a `dead_letter`, a
    pending row past `available_at + overdue_after`, an `outcome_unknown`, and a pending row due
    now. Assert three buckets ≥ 1 and that the due-now row is not counted.
  - Correct the index comment.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R023).
  - 2026-10-24 **fixed** (WF-P08-R024, digest `c4b7eaa9…c896`): `delivery::tests::the_stuck_delivery_census_counts_each_stall_and_not_a_due_delivery` runs on `own_database` and asserts exact counts {1, 1, 1}. A pending row due inside the threshold and a delivered row are excluded. It passes. The comment now says "one sequential pass".

## WF-P08-F093 — B4b notes (optional)

- **Context:** WF-P08-R023.
- **Classification:** **Info**.
- **Notes:**
  1. **`.sqlx` verified independently.**
     - Each of the 27 remaining files matches a current query text exactly. None of the 18
       removed texts appears in the current source, and every target compiles offline.
     - Precision: two of the 18 texts also appeared in HEAD's `task/controls.rs` and
       `channel_assignment_tests.rs` (both now deleted or rewritten), so "only in
       `company_invite.rs`" is slightly off. No impact.
     - `cargo sqlx prepare --check` is still owed. It is due with B4c's regeneration.
  2. **DV-B4b-1 and open decisions.**
     - A submission requires a listed reviewer (`persistence/workflow/decisions.rs:470-474`).
       Removing a member who is the only listed reviewer of an open decision leaves it unsettleable
       until its wait or run deadline, or until someone cancels the run. Re-inviting them creates a
       new principal, so it does not help.
     - HEAD's work-at-stake did not cover workflow decisions either, so this is not a regression.
     - Worth a line in DV-B4b-1, and a p09 follow-up: say it in the removal confirmation, or
       offer reassignment.
  3. **Memory jobs.**
     - With MemoryWorker stopped (planned), nothing drains provisioning jobs or
       `memory_cleanup_jobs`. The latter are still enqueued on company delete
       (`persistence/company.rs:164`).
     - B4c must remove those enqueue paths in the same step that deletes memory (P08-Q1).
  4. **`AGENT_RUN_TIMEOUT_SECS` is no longer read.** `agent_run_timeout_from_env` has no caller,
     yet `docs/deploy.md:193` and `:343`, `.env.example:13` and `fly.toml:24` still name it. That
     falls to B4c and B4d per the plan. AGENTS.md requires it closed by the end of the phase.
  5. **B4d's doc list should name the subsystem guides.** `src/AGENTS.md:160`,
     `src/adapters/persistence/AGENTS.md` (`:132-133`, `:221`, `:270`, `:292`, `:385`,
     `:414-416`, `:440-441`, `:448`) and `src/application/AGENTS.md` still describe deleted code.
     B4d names only the persistence memory section.
  6. The `task_ready`, `task_chain_changed` and ownership NOTIFY triggers still fire with no
     listener. This is harmless until B4c drops them.
  7. `RemovalBudget` now bounds every channel edit, not only a removal. A rename is optional.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R023).
  - 2026-10-24 **resolved** (WF-P08-R024, digest `c4b7eaa9…c896`): dispositions are recorded in TASK. Item 2 goes to B4d's close-out list and item 5 to B4d's doc list; items 1, 3, 4, 6 and 7 are accepted as recorded.
  - 2026-10-24 **correction** (WF-P08-R025): item 1's verification (mine in R023, repeated in R024) only checked that each query text appears somewhere in `src/`. Two of the files it passed are stale: their text survives only in runtime `query_as`/`query_scalar` calls. See F095.

## WF-P08-F094 — An intermittent lease-resolution timing test, from before B4b and outside it

- **Context:** WF-P08-R024. The coordinator asked me to judge
  `lease_tests::resolution_tests::workflow_lease_resolution_stall_beyond_deadline_unknown_then_one_expiry`.
  It failed at `assert!(elapsed >= gated_policy().persistence_deadline())`
  (`lease_resolution_tests.rs:398`) in the corrections' second run, and in B1's run 1
  (`logs/b1-run1.log`, then line 399).
- **Classification:** actionable; **Low**. It is **not caused by B4b**, but it does not belong at
  the final full-suite gate either.
- **Evidence:**
  - **Not B4b.** None of `persistence_bound.rs`, `workflow/lease.rs`, `lease_claim.rs`,
    `lease_resolution_gate.rs` or `application/workflow/lease.rs` is in `logs/b4b-files.txt` or
    `logs/b4b-r2-delta.txt`. The first four are unchanged from HEAD. The only working-tree edit
    to `lease_resolution_tests.rs` is an older pool comment, unrelated. The same assertion failed
    in B1, long before B4b. Alone, it passes 5/5 here (`logs/r024-lease-repeat.log`, about 3.7 s
    each).
  - **Likely mechanism: the "unknown" can be reported up to about 1 ms before T.**
    - In resolution, `claimed_on` calls `lock_scope`, which calls `set_server_timeouts(db,
      bound.server_timeout())` (`workflow/lease.rs:273-280`). That function formats
      `timeout.as_millis()`, which floors (`persistence_bound.rs:452-462`), and sets it as both
      `lock_timeout` and `statement_timeout`.
    - The next statement is `SELECT … FOR UPDATE` on the run row, which the stalled COMMIT holds.
      The server's lock timer starts at the lock wait and runs the floored milliseconds. When the
      round trips before the wait take less than the dropped fraction, the server cancels slightly
      before the client deadline.
    - `resolve_retryable` maps any `Ok(Err(_))` straight to `unresolved()`
      (`persistence_bound.rs:316-329`), so the claim returns "unknown" a fraction of a millisecond
      before `T`. The test's lower bound is exact, so it fails.
    - This is an inference from the code; the assertion prints no `elapsed`, so the margin has not
      been measured.
    - The same exact lower bound appears at `lease_resolution_tests.rs:322`, in another
      stall-resolution test, so it carries the same risk.
  - **Not a safety issue.** Unknown is still the correct classification, and the stalled COMMIT
    can land at any time either way (the test then checks that one expiry is charged). What fails
    is the test's exact lower bound, not the lease protocol.
- **Required, before the phase gate's full suite** (CI must stay deterministic):
  - Print the margin: `assert!(elapsed >= …, "{elapsed:?}")`.
  - Then choose one:
    - (a) let the test tolerate the server's millisecond granularity (`elapsed + 1 ms >= T`),
      with a comment saying why;
    - (b) have `resolve_retryable` wait for `self.deadline` before reporting `unresolved()` after
      a server timeout, if "unknown never before T" is meant to be a guarantee;
    - (c) round the server timeout up. This conflicts with "a statement starting late can never
      outlive the client bound", so (a) or (b) is preferable.
  - Not a B4b blocker.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R024); carried to the phase gate (B4d).
  - 2026-10-24 **fixed** (WF-P08-R031, digest `4ec81c1a…0fc1`): option (a). `SERVER_TIMEOUT_GRANULARITY` (1 ms, `lease_resolution_tests.rs:11-14`, with the floored-milliseconds comment) is added on both exact lower bounds (`:327`, `:406`), and both print `{elapsed:?}`. No other test asserts an exact lower bound against a server lock timeout (`action_reconciliation_proof_clock_tests.rs:140` is a static policy check). All 13 lease-resolution tests passed in the R031 focused run.

## WF-P08-F095 — Two `.sqlx` files are stale (no macro produces them) and would fail `cargo sqlx prepare --check`

- **Context:** WF-P08-R025, review of the whole artifact. The files came from B4b r1, which
  deleted the work-at-stake tests in `persistence/company_invite.rs`. Those tests held the
  compile-time macros, HEAD `company_invite.rs:963` (`query_scalar!`) and `:1129`.
  - The text checks in R023 and R024 (mine), and the B4c-1 check "all 25 remaining files' queries
    found verbatim in `src`", missed them. Both texts still occur in **runtime** calls, which use
    no cache file.
- **Classification:** actionable; **Low**. The offline build is unaffected. CI's
  `cargo sqlx prepare --check -- --all-targets` (`ci.yml:100`) refuses a query cache that holds
  files no macro generates.
- **Evidence:**
  - `logs/r025-sqlx-macro-check.py` parses every `query!`/`query_as!`/`query_scalar!` literal in
    `src/`. It computes sqlx's file key, the SHA-256 of the query text; all 25 file names were
    re-checked against their own `query` field. It then compares the keys with `.sqlx/`.
    - Current tree: 23 macro sites, 23 distinct keys, 25 files, 0 missing, **2 stale**.
    - Validated on HEAD (`git archive`): 49 sites, 45 keys, 45 files, 0 stale, 0 missing.
  - `query-e83a43e16f43…` is `SELECT kind, user_id FROM principals WHERE id = $1`. It now appears
    only in `sqlx::query_as(...)` at `company_invite.rs:518` and `:621`.
  - `query-fe922e729f61…` is `SELECT id FROM principals WHERE company_id = $1 AND agent_id = $2`.
    It now appears only in `sqlx::query_scalar(...)` at `job_auxiliary_fixtures.rs:42`.
- **Required:**
  - `git rm` both files: now in B4c-1's tree, or at the latest in B4c-3.
  - Replace B4c-3's planned "`.sqlx` by exact-text check" with this macro-bound check (or
    `cargo sqlx prepare --check` where sqlx-cli is available). A text match cannot tell a macro
    from a runtime call.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R025); not a B4c-1 regression. Carried to B4c-3 if not fixed sooner.
  - 2026-10-24 **fixed** (WF-P08-R026, digest `97531db8…802d`): both files are `git rm`'d. My macro-bound check reports 23 sites, 23 files, none stale or missing, and TASK now names it as the `.sqlx` gate.

## WF-P08-F096 — B4c-1 notes (optional)

- **Context:** WF-P08-R025.
- **Classification:** **Info**.
- **Notes:**
  1. **`AGENT_RUN_TIMEOUT_SECS` is documented but unread.** Since B4b r2 removed its parser, no
     code reads it, yet it is still named in `.env.example:13`, `fly.toml:24` and
     `docs/deploy.md:191` and `:341`.
     - Every other key in `.env.example`, `docs/deploy.md` and `fly.toml` is read. The exceptions
       are expected: `RUST_LOG` (EnvFilter), `RUST_MIN_STACK` (std), and the database app's
       `DATABASE_SLOW_QUERY_LOGGING_ENABLED`.
     - B4c-2 owns it per the B4 plan. AGENTS.md says documented keys must be ones the app reads, so
       remove it there.
  2. **`library_agent_fields_reuse_the_advanced_editor_and_avatar_picker`.** The replacement
     assertion "Capabilities &amp; advanced options" (`pages/tests.rs:110`) is the `<summary>`
     that `agent_settings.rs:924` always renders. It shows the shared editor is used, as "Memory
     persistence" did. An assertion on the `open` attribute would also pin `advanced: true`.
     Optional.
  3. **`the_memory_provider_schema_is_gone`.** It is adequate: tables, columns and function names.
     Function bodies are not scanned; my catalog query on `workflow08_test` finds no `prosrc`
     naming memory, hydradb or hindsight. The D4.8 body-catalog test in B4c-3 covers that class
     generally.
  4. **Doc deferral accepted.** No README or `docs/*` file names a memory config key. The
     remaining feature copy (README ×4, `docs/rig.md` ×2, `docs/transport_architecture.md` ×2,
     `persistence/AGENTS.md` ×9) is B4d's by plan. Also for B4d: `docs/deploy.md:485` still cites
     "`ERROR_BACKOFF` in `task_worker.rs`".
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R025).
  - 2026-10-24 item 1 **fixed** (WF-P08-R026): `AGENT_RUN_TIMEOUT_SECS` is gone from `.env.example`, `fly.toml` and `docs/deploy.md`. Items 2–4 are accepted as notes.

## WF-P08-F097 — The rewritten channel gate claims an eligibility guarantee the inbound commit does not implement, and its new competing test cannot see the difference

- **Context:** WF-P08-R026, DV-B4c2-2 (review point 3). B4c-2 redefines the gate's purpose for the
  workflow model.
  - The rewritten module doc (`persistence/channel_gate.rs:1-14`) says it stops a concurrent inbound
    commit "from admitting a message against channel settings an edit is about to replace — the
    enabled switch, the access mode, the allowlist".
  - It also says the inbound commit "takes it **shared** … and reads eligibility in a statement that
    starts after it is held".
- **Classification:** actionable; **Medium**. A concurrency protocol's stated guarantee does not
  hold, and the competing test added for it checks only lock mechanics (AGENTS.md: a changed
  concurrency protocol needs a competing-claimant test). It is not a runtime regression against
  HEAD, where the gate guarded assignment eligibility; that reader went with the legacy runtime.
- **Evidence:**
  - Eligibility is decided **before** the commit, in the application's routing, from pool reads
    outside the commit transaction. See the `participant_access` calls at `thread/ingest/routing.rs:196`
    and `:292`, and `channel.enabled`/`participant_access` at `:687`.
  - In `commit_on` (`persistence/thread/inbound.rs:79`), after `acquire_channel_gates_on(…, Admit)`
    (`:90`), no statement re-reads `channels.enabled`, `access_mode` or `channel_principal_grants`.
    - The only check is that each transport binding is `active` (`first_unbound_association`, `:259`).
    - The intents are inserted with the `admissible` flag the application computed earlier
      (`create_intents`, `:645`).
    - The schema has no trigger that reads `channels.enabled`.
  - So an edit that disables a channel, or narrows its access or allowlist, can commit after routing
    has read the old settings and before `commit_on` takes the gate. The message is then admitted
    under the replaced settings. The exclusive lock orders the two transactions but prevents no
    stale admission.
  - `an_edit_waits_for_an_admission_holding_the_gate_then_applies` (`channel_edit_tests.rs:135`)
    holds the gate with a bare `acquire_channel_gate_on`, not `commit_inbound`. It asserts only
    that the edit waits (after a fixed 300 ms sleep) and that the rename applies afterwards. It
    would pass even if no admission path took the gate.
  - The contract, D4 rev 2 §D4.8 (`logs/decision-D4-r2.md:266-269`): channel-setting edits keep the
    exclusive lock, and B4c's competing test shows that "a disable against a concurrent ingress
    serializes, and the ingress sees either the enabled or the disabled channel, never a mix" (F083).
- **Required** (either):
  - **(a) Implement the guarantee.**
    - Inside `commit_on`, after the gate, re-read each intent channel's `enabled` and the sender's
      access in a statement that starts after the lock. On a change, refuse the commit or record the
      intent as not admissible.
    - Replace or extend the test so it drives `commit_inbound` against a concurrent disable, in both
      orders:
      - the edit holds the gate, so the commit waits, then sees the channel disabled;
      - the commit holds the gate, so the edit waits, and the message is admitted under the old
        settings.
  - **(b) At minimum for B4c-2:**
    - Reword the gate doc to state what it does today: it orders an inbound commit against a
      settings edit; eligibility is read before the commit, and B4c-3 closes that gap.
    - Rename the test's claim accordingly.
    - Record in TASK that B4c-3's F083 test must assert the eligibility property and implement (a).
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R026).
  - 2026-10-24 **fixed** (WF-P08-R027, digest `2d4ea381…7098`), option (a). `channels.settings_revision` moves only in `write_channel_settings_on`, which is the only post-create writer of the channel row, its slugs, its grants and its email binding. Routing reads the revision in `CHANNEL_SELECT` with the settings. `hold_channel_settings` takes the shared gates, then re-reads the revision in its own statement, and a mismatch or a missing channel is refused as `AppError::Superseded`. Two competing tests drive the real commit, in both orders; F099 covers a robustness gap in the second.

## WF-P08-F098 — B4c-2 notes (optional)

- **Context:** WF-P08-R026.
- **Classification:** **Info**.
- **Notes:**
  1. **Mailbox attribution.**
     - `message_bubble_chat` now binds `(writer, avatar_url) = (message.author.display(), None)`
       (`pages/mailbox.rs`). The avatar is always `None`, so this can be simplified.
     - No non-legacy behaviour is lost. The channel's single agent was a legacy proxy, and a
       workflow reply records its own author principal.
     - A p09 follow-up could draw the authoring agent's name and face from that principal.
  2. **"Used by" on the agent pane.** The agent edit pane's list of channels running the agent
     (`agent_edit_pane_lists_the_channels_running_the_agent`) went with `channel_agents`. In the
     workflow model the equivalent is binding usage, for p09.
  3. **Address preview.** The agent create pane still previews `handle@company`, although agents
     have no address. That is UI copy, not documentation. Prefer fixing it with the agent forms
     rather than in B4d; deferral is acceptable.
  4. **A pre-existing uncalled trait method.** `AgentPersistence::get_by_company_slug_and_agent_slug`
     (declared at `use_cases/agent.rs:109`, implemented at `persistence/agent.rs:332` and in the
     mock) has no caller, and had none at HEAD either. It belongs to B4d's exhaustive search.
  5. **DV-B4c2-3 fixtures.** They pick the owner with `LIMIT 1` and no `ORDER BY`. That is
     acceptable until B4c-3 retires them.
  6. **Slug collision on agent update.**
     - It is now `Conflict` (JSON 409), the same as create. The `/ui` update re-renders the pane
       with the message on any error.
     - HEAD's `BadRequest` "Address … is already in use" belonged to the owned-channel address,
       which is gone.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R026).
  - 2026-10-24 **resolved** (WF-P08-R027): item 3 is fixed (the create pane previews `@handle`, and the JS prefixes `@`). Items 1 and 2 go to p09, 4 to B4d, 5 to B4c-3; 6 needs no change.

## WF-P08-F099 — The commit-first gate test gives the queued edit only the default 2 s lock budget

- **Context:** WF-P08-R027, review point (d).
  `a_settings_edit_queued_behind_a_commit_applies_after_it` (`thread/inbound_tests.rs:584`) spawns
  `update_channel_within(&pool, update, EditBudget::DEFAULT)` (`:599`) while the admission holds the
  gate.
- **Classification:** actionable; **Low** (a flake risk in a CI database test).
- **Evidence:**
  - `EditBudget::DEFAULT` sets `lock_timeout` to 2 s (`channel_edit.rs:39`). It applies to the
    advisory-lock wait.
  - The edit waits for the 300 ms sleep, the assertions, the whole remaining `commit_on`
    (association checks, provider-key locks, thread resolution, message, participant, mapping,
    intent and delivery inserts) and the commit.
  - On a loaded full-suite or `stack-budget.sh` run, that can pass 2 s. The edit then returns
    `Timeout`, and `expect("the edit applies once the commit is done")` fails although the protocol
    is correct. F094 shows this suite's timing bounds do fail under load.
  - The edit-first test has no such bound: `commit_inbound` sets no lock timeout.
- **Required:** give the spawned edit a lock budget far above the test's own duration, for example
  `EditBudget { lock_timeout: Duration::from_secs(30), ..EditBudget::DEFAULT }`. The 300 ms
  "still waiting" check is then the only timing assumption, and it cannot fail spuriously.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R027).
  - 2026-10-24 **fixed** (WF-P08-R028, digest `95fe3918…cbe5`): the spawned edit gets `lock_timeout: 30 s`, with a comment. The edit's own lock-timeout refusal stays covered in `channel_edit_tests`. The inbound and `channel_edit` suites passed 3/3 here.

## WF-P08-F100 — F097 correction notes (optional)

- **Context:** WF-P08-R027.
- **Classification:** **Info**.
- **Notes:**
  1. **Scope is accepted.** Three things that also feed routing stay outside the revision: transport
     binding status (`integration/binding.rs:174`, `email_binding.rs:120` UPDATE `channel_bindings`
     outside the settings write), company membership, and principal deletion.
     - Binding status is re-checked in the commit's own `first_unbound_association` statement, but
       not under the gate. A revoke can still land between that check and the commit: the FK
       KEY SHARE on the binding does not conflict with a non-key UPDATE.
     - A member removal changes the access context, not channel settings.
     - Principal deletion is serialized by the participant FKs.
     None of these was claimed by D4.8. Record them if later work needs that guarantee.
  2. **The check's position is right.**
     - The gate stays the first lock, in the same order as the edit (gate, then row).
     - The check precedes binding verification, the provider-key locks and redelivery recognition.
     - A redelivery whose channel changed is refused once, re-routed and then recognised. That is
       the same outcome as an ordinary redelivery after an edit.
  3. **The new variant is justified.**
     - `Conflict` is terminal for the event worker, so reusing it would drop a message that a
       re-route would accept.
     - The SMTP 451 and the internal relay's `Retryable` (`egress.rs`) both lead to a re-route.
     - HTTP 409 reaches only in-app sends and the simulation. That is acceptable, and a friendlier
       UI retry is optional.
     - A database round-trip test of every `InboundEventErrorClass` against
       `valid_inbound_event_error_class` would stop enum/function drift. The values match today,
       11 against 11.
  4. **(e) and (f) are acceptable.**
     - The raw-SQL switch-on keeps the routed revision equal to the stored one, and the comment says
       so.
     - `pub(super)` on `commit_on` and `hold_channel_settings` reaches only
       `adapters::persistence::thread`.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R027).
  - 2026-10-24 **resolved** (WF-P08-R028): recorded in TASK. Binding status under the gate is a candidate for p09 or B4d. The error-class round-trip test was not taken (optional).

## WF-P08-F101 — B4c-3a notes (optional)

- **Context:** WF-P08-R029.
- **Classification:** **Info**.
- **Notes:**
  1. **The scratch-apply and catalog logs are thin.** `logs/b4c3a-scratch-apply.log` is empty, and
     `logs/b4c3a-catalog.log` holds only a table count. My checks stand on independent evidence
     instead:
     - `workflow08_test`'s `_sqlx_migrations` checksum equals the SHA-384 of the current file;
     - my statement-level before/after diff (`logs/r029-migration-diff.txt`, from
       `logs/r029-stmts.py` and `logs/r029-classify.py`);
     - my own catalog scan.
     Next time, keep the scan's output, not just its count.
  2. **The catalog test could see more.**
     - It reads `pg_proc.prosrc`, which is empty for SQL-standard-body functions
       (`LANGUAGE sql … RETURN …`, as in `valid_inbound_event_error_class`). A `'legacy'` literal in
       such a body would go unseen. Table references there are already caught by dependency
       tracking. `pg_get_functiondef` on `prokind = 'f'` covers both.
     - The retired *values* could also be scanned: `pending_approval`,
       `waiting_for_third_party_reply`, `channel_agent_removed`, `task_stopped`, and the retired
       `stop_reason` values. My scan of those finds nothing today.
  3. **Unused icons.** `Icon::People` and `Icon::Goal` now appear only in their own match arms.
     They belong to B4d's exhaustive search, which the TASK already notes.
  4. **The DV-B4c3-5 deferral to B4d is accepted.** None of the items is an execution entry point,
     and D4.8 does not name them. Once B4d does them, `model_usage_calls`'
     `num_nonnulls(…) IN (0,5)` can narrow to `= 5`, because `guard_model_usage_task` now refuses a
     run-less row.
  5. **Not flagging a failed job (DV-B4c3-1) is right.** A failed workflow job is not actionable at
     the job level. It shows in the dashboard's throughput `failed` count, and its run surfaces in
     attention through D4.2's `run_failure` branch.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R029).
  - 2026-10-24 item 1 **adopted** and item 2 **fixed** (WF-P08-R030, digest `845d5596…e1d6`): the apply and catalog logs are now complete. The scan reads `pg_get_functiondef` for `prokind` f/p and the 14 quoted retired values, and its two canaries cover a PL/pgSQL body and a SQL-standard body. Items 3–5 stand as notes (3 and 4 go to B4d).
  - 2026-10-24 items 3 and 4 **done in B4d** (WF-P08-R031, digest `4ec81c1a…0fc1`): `Icon::People`/`Goal` are removed, and each DV-B4c3-5 item is decided (DV-B4d-1..3, accepted; see F105.4). Item 5 stands as a note. **Resolved.**

## WF-P08-F102 — B4c-3b notes (optional)

- **Context:** WF-P08-R030.
- **Classification:** **Info**.
- **Notes:**
  1. **The gauge's scope is sound.**
     - `WorkflowWorker::run` handles one candidate at a time, and each slot holds at most one
       `ActiveWorkflowJobGuard`. All slots share one `ActiveWorkflowJobs` (`infra/workflow.rs:395`,
       `:407`). So active jobs ≤ slots holds even before `observe` clamps the value
       (`infra/runtime_metrics.rs:109-110`), and the schema CHECK
       `active_jobs_within_slots` cannot fire on a correct sample.
     - The count includes a job candidate's step-kind lookup and a claim that comes back
       `Deferred` or `Unsupported`. That is brief. Expiries, receipts and wait resumes are
       excluded; they are short maintenance work.
  2. **The recommendation is an observed safe level, not a capacity forecast.**
     - `RECOMMENDATION_SQL` picks the highest active-jobs level with healthy CPU, RSS and acquire
       p95, and pool headroom (`pool_active_p95 ≤ min_pool − 2`), so it can never exceed the
       configured slots.
     - The headroom criterion is an observed-pressure check, not a sizing formula. It need not
       change with the 2-connections-per-slot rule, which startup already enforces.
     - Optional: say in the panel that raising `WORKFLOW_WORKER_SLOTS` also needs
       `DATABASE_MAX_CONNECTIONS ≥ 8 + 2 × slots`. The deploy docs already say so.
  3. **Docs.** No file outside `plan/`, `marketing/` and `website/` names `TASK_WORKER_CONCURRENCY`
     or the old types. The README "Workflow Worker Slots" bullet matches the code: default 2, range
     1–16, a 30 s `LEASE`, a 1 s `POLL`, and the admission wakeup. README §3.4's Shutdown and
     Leased Execution bullets and its TaskWorker diagram are still stale, as recorded for B4d.
  4. **F101.2's lists and canaries are sufficient.** The values are quoted in the pattern, so the
     canary's own name (`legacy_runtime_…`) does not match. Optional: also add the legacy
     `transition_reason` values, such as `approval_requested` and `outreach_started`.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R030).
  - 2026-10-24 **resolved** (WF-P08-R031, digest `4ec81c1a…0fc1`): item 3's README §3.4 Shutdown, Leased Execution and diagram are rewritten and match the code (`main.rs:297-308`, `infra/workflow.rs:63-67`, `lease.rs:42-43`, `MESSAGE_INTENT_LEASE`, `SCHEDULE_LEASE`, `DELIVERY_LEASE_SECONDS`). Item 4's optional `transition_reason` values are in `RETIRED_VALUES`. Items 1–2 stand as notes.


## WF-P08-F103 — Two rewritten doc claims name a removed setting and a removed tool

- **Context:** WF-P08-R031, point (a). B4d rewrote `docs/rig.md` and the deploy runbook, and both
  still carry one claim the code no longer supports.
- **Classification:** actionable; **Low**. Documentation only, but the root `AGENTS.md` requires
  that production docs "name configuration keys the application actually reads". An operator
  following either line looks for something that does not exist.
- **Evidence:**
  1. **`docs/rig.md:137`** says: "Model attempts | Default 8; `max_turns` 1–16, shared by
     continuations, skills and repairs".
     - No `max_turns` exists anywhere in production code (`rg -i turns src` outside tests finds
       only prose). It was an agent `config_json` field. B4c-2 removed `config_json`, and
       `rig.md:27` itself now says payloads naming `config_json` are rejected.
     - `WorkflowModelPreparation::policy()` (`application/workflow/agent.rs:184-191`) always
       starts from `RigExecutionPolicy::default()`, which is `model_calls: 8`
       (`application/services/harness/runs/state.rs:25-34`). The 16 is only the `validate`
       ceiling (`:36-58`), and nothing can request it.
     - Skills no longer spend model attempts. They are appended to the preamble
       (`agent.rs:283-288`), and `read_resource` is gone from production code.
  2. **`docs/deploy.md:177`**, a pilot row that must pass before opening access, says: "Skill |
     Attached skill loads through `read_resource` and its saved result survives continuation".
     - `read_resource` survives only as a fixture string in
       `application/services/harness/runs/tests.rs:34`. No production tool loads a skill, and
       `rig.md`'s Skills row (`:50`) and the B4d progress note both say so.
     - The pilot evidence this row demands therefore cannot be produced.
- **Required:**
  - `rig.md:137`: state the real bound. For example: "8 per model step, fixed (no setting),
    shared by continuations and repairs (at most 2 repairs); the workflow's root `model_calls`
    budget also applies."
  - `deploy.md:177`: restate the skill pilot for the prompt-only model. For example: "An
    `agent.run` step whose agent has an attached skill follows its instructions, and the
    instructions survive a continuation, because they are in the frozen preamble."
  - Re-grep the docs for `max_turns` and `read_resource` (outside `rig-verification.md`, which is
    a dated record) after the edit.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R031, digest `4ec81c1a…0fc1`).
  - 2026-10-24 **fixed** (WF-P08-R032, digest `00ec454f…6829`). `rig.md:137` now reads "8 per model step, shared by continuations and repairs; not configurable per agent or step". That matches `RigExecutionPolicy::default()` and `MAX_REPAIR_CALLS` = 2 within the same budget. `deploy.md:177` now asks for a controlled `agent.run` step whose agent has an attached skill to answer by the skill's instructions. A re-grep of README, `docs/`, the guides, `.env.example` and `fly.toml` finds no `max_turns`, and finds `read_resource` only in the dated `rig-verification.md:35`. Correction to this finding's evidence: the method at `agent.rs:184` is `PreparedAgent::policy` (impl at `:103`), not `WorkflowModelPreparation::policy`.

## WF-P08-F104 — The 06C obligation to remove mcp_connections_by_ids is neither done nor dispositioned

- **Context:** WF-P08-R031, points (b) and (e). TASK acceptance criterion 13 (`TASK.md:113-119`,
  verbatim from `RESUME.md:55-60`) lists `mcp_connections_by_ids (test-support only)` among the
  items to remove with the legacy binding.
- **Classification:** actionable; **Low**. The method is dead production code, not an execution
  entry point. But it is a named phase obligation, and no record decides it.
- **Evidence:**
  - It is still there:
    - the trait method `McpPersistence::mcp_connections_by_ids`
      (`application/use_cases/mcp.rs:92-98`);
    - its Postgres impl (`adapters/persistence/mcp.rs:129-...`).
  - Production has no caller. `rg mcp_connections_by_ids src` finds only:
    - `adapters/persistence/mcp_read_tests.rs:27`, `:44` and `:50`, which test the method itself;
    - `adapters/persistence/workflow/mcp_test_support.rs:296` and `:349`, which read a fixture
      connection back.
  - No TASK, notes or findings entry names it after line 119. B4d's self-only uncalled scan
    could not catch it, because its test callers count as references.
  - The other 06C items are met or dispositioned:
    - `create_agent_channel` and `list_company_agents` are in `workflow/provisioning.rs`;
    - unhandled step kinds are refused;
    - the schedules are workflows;
    - `AgentRunner`, `HarnessRegistry`, `ApprovalUseCases`, outreach and quorum are gone (0
      production hits);
    - `HarnessKind` stays as the D4.8 literal (DV-B4b-4);
    - the `ToolId` comment is done in B4d.
- **Required:** one of:
  - remove the trait method, the impl and `mcp_read_tests.rs`'s tests of it, and point the two
    `mcp_test_support.rs` reads at a direct query or `list_mcp_connections`; or
  - record an explicit keep decision with its reason in TASK, against criterion 13.
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R031, digest `4ec81c1a…0fc1`).
  - 2026-10-24 **fixed** (WF-P08-R032, digest `00ec454f…6829`). The trait method and its Postgres impl are removed (22 deletions; no mock defined it), with 0 hits outside `plan/`. `mcp_read_tests.rs` now tests `list_mcp_connections`: tenant scoping, deleted rows excluded, and the foreign company seeing only its own. The two `mcp_test_support.rs` reads go through `connection_on` (`:305-316`, the live catalog filtered by id). The macro check is still 23/23, so no `.sqlx` file is orphaned. Clippy and fmt are clean, and `cargo test --lib -- mcp legacy_runtime_schema` gives 55 passed.

## WF-P08-F105 — Phase acceptance evidence and gate notes (optional)

- **Context:** WF-P08-R031, points (b), (d) and (e).
- **Classification:** **Info**.
- **Notes:**
  1. **Acceptance matrix** (plan §Acceptance). TASK has no author-side matrix, so this one could
     be copied into the gate record. Every test named here passed in the R031 focused run
     (`logs/r031-focused.log`).
     - **(1) One run engine.**
       - Email: `message_ingress_end_to_end_each_message_gets_its_own_run_and_reply`
         (`message_ingress_tests.rs:354`).
       - Schedule: `schedule_end_to_end_a_due_slot_runs_its_workflow_once`
         (`schedule_runtime_tests.rs:52`).
       - Manual: `manual_start_returns_its_run_replays_its_key_and_links_a_rerun`
         (`schedule_tests.rs:1088`).
       - Child: `workflow_call_success_completes_parent_once_with_child_output`
         (`child_runtime_tests.rs:50`).
       - All four go into `workflow_runs` and are executed by the same `WorkflowWorker` slots
         (`infra/workflow.rs:388-430`).
     - **(2) Parallel message runs in one conversation.**
       - Separate runs and replies, each naming its own source and run: the email end-to-end
         test.
       - Separate waits: `workflow_wait_same_thread_messages_remain_independent`
         (`wait_deadline_tests.rs:135`).
       - Separate membership: `workflow_admit_sql_message_alias_equivalence_and_independent_runs`.
       - Decisions and artifacts are separate **by structure only**:
         - `workflow_decisions_wait_fkey` → `workflow_waits(company_id, run_id, execution_id,
           id)`;
         - `UNIQUE(company_id, wait_id)`;
         - step outputs are per execution.
       - No test parks a `decision.human` in each of two same-thread message runs. Optional: add
         one, where settling one decision resumes only its run, or record the structural
         argument in the gate record.
     - **(3) No extra work.**
       - `schedule_slot_is_admitted_once_under_competing_materializers` (`schedule_tests.rs:321`);
       - `a_redelivery_returns_the_first_delivery_and_enqueues_nothing_further`
         (`inbound_tests.rs:391`);
       - `two_concurrent_deliveries_of_one_message_produce_one_of_everything` (`:465`);
       - `each_association_gets_one_intent_and_redelivery_adds_none`;
       - `a_redelivery_of_the_same_mail_is_accepted_and_stored_once` (Resend);
       - `message_intent_competing_claimants_admit_one_run_and_fence_the_stale_worker`.
     - **(4) Protections on the new routes.**
       - Ingress, now ending in intents:
         - hop limit, spam and unauthorized sender (`thread/tests.rs:306/514/694/2017`);
         - quiet and Cc intents (`:4037-4112`).
         - The 60-per-hour thread turn guard (`exceeds_turn_limit`, `ingest/routing.rs:504`) has
           no test, at HEAD or now. The gap predates p08 and is not a regression; a focused test
           is optional.
       - Threading:
         - `concurrent_different_messages_sharing_a_new_thread_key_open_one_thread`;
         - `message_ingress_member_email_reply_resumes_the_question_on_an_allowlist_channel`.
       - Workflow routes:
         - `workflow_routes_refuse_other_tenants_and_members` and
           `workflow_listings_hide_channels_an_admin_cannot_see`;
         - CSRF from the global `browser_security` (`infra/app.rs:41`, unit-tested in
           `security.rs:88`).
     - **(5) No dependence on removed settings.**
       - The D4.8 catalog tests (`legacy_runtime_schema_tests.rs`, both pass).
       - The agent API refuses `config_json`, `run_timeout_secs`, `response_contract` and
         `sub_agent_ids` (`agent_api_tests.rs:119-126`).
       - No channel page or entity names agents, memory, triggers or review settings.
       - The harness is the fixed `HarnessKind::Rig` literal.
     - **(6) Exhaustive search.** My token scan over `src`, excluding tests, gives 0 production
       hits for:
       - the legacy task types and workers, `AgentRunner`, `HarnessRegistry`,
         `ApprovalUseCases` and outreach;
       - the channel-agent, trigger and memory settings;
       - the `/tasks` routes.

       What remains is negative tests, the `NO_BACKWARD_COMPATIBILITY_NOTES` plan paths,
       historical guide prose, and F104.
  2. **The doc spot-checks otherwise hold.**
     - README §3.4:
       - leases: 30 s jobs with a 10 s heartbeat (`LEASE/3`), 60 s intents and occurrences,
         120 s deliveries, 20 per claim, a 500 ms poll and a 30 s reap;
       - five occurrence attempts (`schedule_runs` CHECK);
       - the `scheduled_for` input;
       - the shutdown joins, which match `main.rs:297-308` plus the three workflow handles.
     - README §3.7: the trigger semantics match `MessageTrigger::admits`
       (`domain/workflow/trigger.rs:43-50`), including the Ana example. The reply goes only to
       the source's sender (`reply_target_on`, `persistence/workflow/message.rs:371-427`).
     - The deploy runbook routes all exist under `/api/companies/{company_id}`
       (`routes/workflows/mod.rs`).
     - `rig.md`: the Tools, Skills and Context rows, the rejected agent fields and the
       reconciliation wording are accurate. F103 covers the exceptions.
     - The headers and anchors resolve.
  3. **No dangling references.** None found to:
     - `get_by_company_slug_and_agent_slug`, `hydrate_ingest_configuration`, `agent_channel`,
       `mark_sent_by_outreach`/`outreach_sent` and `stored_email_history`;
     - `Icon::People`/`Goal`, `CreatorType::Agent`, `CreationProvenance::agent` and
       `source_task_id`;
     - `/companies/{id}/tasks` and the deleted `docs/performance/` files.

     `source_channel_id` and `default_true` remain, but as unrelated relay and schedule names.
  4. **DV-B4d-1..3 are sound.**
     - **DV-B4d-2.** The provenance function on `workflow08_test` allows only user and system
       (`pg_get_functiondef`), and its seven CHECK users are unchanged. The only agent creator,
       `create_agent_channel`, writes `CreationProvenance::user(requester)`
       (`persistence/workflow/provisioning.rs:544`).
     - **DV-B4d-3.** `model_usage_calls_check3` is `num_nonnulls(...) = 5`, and `reserve_on`
       inner-joins the execution, run and budget.
     - **DV-B4d-1.** The rationale holds: `initiates_conversation` and `permitted_by` use the
       purposes, and the worker enforces predecessors.

       But `reply`, `mirror` and `outreach` have no production constructor, and only
       `test_support` sets `depends_on_delivery_id`. "Revisit with the transport or cutover
       plans" is recorded only in TASK. Optional: carry it into the plan's follow-ups so it
       survives the phase.
  5. **Gate prerequisites.**
     - `workflow08_test` matches the migration: the SHA-384 is `48e3d2eb…1bb3`, equal to
       `_sqlx_migrations.checksum`.
     - The dev database `workflow08` on :55480 still holds a pre-p08 schema: 131 tables, legacy
       tables among them, and no `_sqlx_migrations`. The plan says both databases are dropped
       and recreated, and `cargo sqlx prepare --check` and any migrate run at the gate need a
       freshly migrated `DATABASE_URL`. So recreate `workflow08`, and drop the author's
       `workflow08_scratch`, before the gate.
     - The carried gate checks are still owed: `cargo sqlx prepare --check`, `npm run
       check:css`, the full suite, `scripts/stack-budget.sh` and `npm run test:run-counts`.
  6. **Optional wording.** The unchanged README §3.9 Compose bullet still says "agents apply
     unchanged" and "(real dispatch)". In substance it is right: `Run` maps to
     `ReplyDelivery::Send` and `RunTest` to `InAppOnly` (`routes/channel.rs:1015-1021`).
- **Resolution history:**
  - 2026-10-24 open (WF-P08-R031, digest `4ec81c1a…0fc1`).
  - 2026-10-24 **adopted** (WF-P08-R032, digest `00ec454f…6829`). TASK "B4d corrections for WF-P08-R031" adopts item 1 as the gate's evidence map. It records criterion 2's structural argument in place of a test, the pre-p08 turn-guard gap, item 4's revisit as a coordinator candidate, and item 5's gate notes (recreate `workflow08`, drop `workflow08_scratch`). Item 6 stands as a note.
