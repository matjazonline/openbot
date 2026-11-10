# p05 — OpenBot integration (memory-service phase 05)

## Identity
- Phase ID `p05`; plan `plan/memory-service/` (originals: README.md, 01, 05-openbot-integration.md, 06B.2).
- Workflow: `implement-opus-reviewed` until 2026-11-08 ~20:10Z, then `implement-opus-reviewed-2_2`
  (phase-local continuation; earlier records retained unchanged at `tmp/memory-service-run/`).
- Record root: /Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/phases/p05/docs
- Coordination: /Users/mac03/.codex/worktrees/60a0/openbot/plan/memory-service/COORDINATION.md r1
  (I001–I003 acknowledged; no cross-phase notices). Root is its writer.
- Worktree /Users/mac03/.codex/worktrees/60a0/openbot, detached HEAD a6bed48a89d986204d7df13d4b4c5a58458176cf
  (= phase start = plan start; nothing committed). Preserved pre-existing work: untracked `tmp/`
  (tmp/claude-mem reference checkout; tmp/memory-service-run run records, private PG cluster data, tools).
- Snapshot: `bash /Users/mac03/.claude/skills/implement-opus-reviewed-2_2/scripts/snapshot.sh tmp plan/memory-service/phases/p05/docs plan/memory-service/COORDINATION.md`
  (exact exclusions: `tmp`, `plan/memory-service/phases/p05/docs`, `plan/memory-service/COORDINATION.md`).
  Old-workflow identities were git tree ids from `tmp/memory-service-run/freeze.sh` (excludes tmp/ only).

## Sibling changes in this worktree (not p05)
- ~23:22Z another session added untracked `plan/agent-workflow-call/` (README + 01–06) and later
  `plan/agent-default-channel/` (4 files), with 11 entries in `src/main.rs` NO_BACKWARD_COMPATIBILITY_NOTES. Documentation-only; preserved, not part of any p05 batch,
  not excluded from snapshots (it changes the digest: R1-return digest f808b0a4…82f4).

## Roles and settings
- Root: session 6fedfb2e-40aa-486c-bd0b-051b5a3add7b, claude-opus-5-5, effort xhigh (user-confirmed 2026-11-08).
  Root is author A for batches after p05-B2 (user choice). Context: auto-compact at 75%
  (CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=75 in env, verified); user asked root to run up to 85% —
  the shared setting compacts at 75%, continuity via these records.
- A (p05-B2 only): agent af38660db9d70c7bd ("A gen8"), Opus, effort **high**; returned 21:51Z at 69.5% with B2a–c done, B2d not started (spawned before the switch;
  user chose to keep it for its in-flight batch). Earlier phase-05 authors (gen6 a4c17713f9f4e7b2b,
  gen7 a550ec35ea3a3f984) retired.
- B: agent abdd866fb428eea05, Opus/xhigh, spawned 2026-11-08 ~21:55Z for the p05-B2 review (user choice). Earlier reviewers
  (gen4 a5916c4a63f198c6f, gen5 a217255500990fff2, both high) retired; their reviews are in
  tmp/memory-service-run/reviews/.

## Requirements and acceptance
- Originals as above; binding design `plan/memory-service/implementation-decisions.md` "Phase 05 decisions"
  E1–E12 (architecture gate PASS r4, tmp/memory-service-run/reviews/p7a-arch-r4.md, record tree 6238f2bc)
  plus the company-scope U2 addendum, and all "P7b implementation notes" blocks.
- Acceptance: every 05A/05B item and its focused checks (memory_integration_, memory_scope_,
  memory_access_level_, memory_preparation_, memory_capture_ and named regressions) pass with
  independent review; competing-claimant tests for each new concurrency protocol; nothing in 01's
  access-level contract violated. Full suite and stack budget belong to 06C.

## Batches
| Batch | Scope | Status |
| --- | --- | --- |
| P7a | Architecture gate for all of phase 05 (E1–E12) | PASS r4 (old workflow) |
| P7b = p05-B1 | 05A.1–2 + lifecycle parts of 05B.7: config, MemoryProvisioner, adapter + contract fake, credentials, tenants/namespaces/lifecycles, reconciler, owner locks, opt-in settings + retry | accepted in phase (old workflow): tmp/memory-service-run/reviews/p7b-code-r4.md, tree 113801e442204b9277285d0be088f0492757d7a6 |
| p05-B2 | 05A remainder minus agent tools: exactness (content_exact, hooks, coverage, child/history), MemoryClient::save, memory.load/memory.save steps, gating, memory_deliveries + delivery worker, disable/owner/cancel intent handling, samples, step-level E6 race | **accepted in phase** 2026-11-09 (MEM-P05-B2-R3 PASS, digest 9c7929bf1e3b9fce84664264e0530133669cf078bc77ff45a8ad587dafd51abc recomputed by root, tree dddff33c; rounds R1–R3; manifests MEM-P05-B2-R1..R3 valid; F10 deferred, F16 optional → B3). R1 reviewed digest d20858a7e356ccd1cce9dbad6bff214a2bdf37dd60dd3ad00c7e8fab10c2f314, tree acbda41e, review base 113801e4; manifest MEM-P05-B2-R1 valid (9 PASS/2 FAIL/2 NOT EXERCISED) |
| p05-B3 | B2d: four agent memory tools + catalog/publication/dispatch + tool consumption records + tool-level E6 race | **accepted in phase** 2026-11-09: B PASS MEM-P05-B3-R3 for digest 8857f53867e70b2edb79a9b13a0d21d4b9f05f5729f129daec690f5744bea777 (freeze tree a69f88c5). Root recomputed: current digest de7efd2c2d7d8e04a84df686695e1a1fd8ce79e179dd48ce6dc09c8cfa573f3c = B's return digest; freeze tree 9d036804 differs from a69f88c5 only in sibling plan files and 12 sibling NOTES lines in src/main.rs (no p05 file). Rounds R1 (CHANGES: F1 Major, F2–F4 Minor, F5 opt) → R2 (F6 Minor, F7 opt) → R3 PASS; user rulings U4, U5; manifests MEM-P05-B3-R1..R3 valid |
| p05-B4 | 05B.1–4 (E7): automatic recall at agent preparation, frozen pack + level raise, prompt block + usage sentence, budget, replay, `context.validate` once per resumed attempt, optional unavailability vs fail-closed refusal | **accepted in phase** 2026-11-09: B PASS MEM-P05-B4-R3 for digest 8490e2e6c999eea62a7738e2e762b18c1fbda91cbec21f106612bc761d4f78ee (freeze tree 2a0d2550), root recomputed the same digest (HEAD a6bed48a, coordination r3); R1 CHANGES REQUIRED (F1–F4 Minor, F5 optional) → R2 (F6 Minor, F7 optional) → R3 PASS; manifests R1–R3 valid. Not integrated; full suite/stack budget/live/browser at the 05 gate |
| p05-B5 | 05B.5–7 (E8, E9): capture policy (`memory_agent_capture`, settings), generation intents at exchange acceptance, generation delivery, lifecycle races (disable vs save/capture, delete/removal) | **accepted in phase** 2026-11-10: B PASS MEM-P05-B5-R2 for digest 6041e394d913523eb06c8aaac8348c675134643e8947fe1281a6a20eca9e4f1e (freeze tree 8e18b451); root recomputed the same digest (HEAD a6bed48a, coordination r4 incl. U7); R1 CHANGES REQUIRED (F1, F2 Minor; F3, F4 optional) → U7 → R2 PASS; manifests R1–R2 valid. Optional F5 (doc comment above `configured`) carried into B6. Not integrated; the five prefixes rerun on the final tree in B6 |
| p05-B6 | Phase-05 gate (E11): `memory_integration_live_` `#[ignore]` tests and `scripts/memory-integration.sh` (service build, `provision-db.sh`, `admin bootstrap`, provisioner, loopback `serve` with a fake Messages API, `-- --ignored`) plus its CI step; all five prefixes; `scripts/stack-budget.sh`; boundary scripts; credential rotation; `sqlx prepare --check`; browser check of the settings memory section. Split from B5 (R) so B5's review stays thorough and the gate builds on an accepted contract | **accepted in phase** 2026-11-10: B PASS MEM-P05-B6-R1 for digest 88808f5ad06857e86e417fe9593fd6b72185bdc01b42ad52871054c77e81b0a1 (freeze tree 7388d1c2); root recomputed the same digest (HEAD a6bed48a, coordination r4); B reran the five prefixes on the frozen tree (148 passed) and the live script (2/2, failure path cleans up). B5 impact of the source-ref fix confirmed limited. Optional B6-F1 (CI service toolchain pin), F2 (socket-style admin URLs), F3 (operator token on curl argv) carried into phase 06. CI execution of the new job/step and the all-target full suite remain 06C/CI |

## Carried obligations (from P7b acceptance)
Browser check of the settings memory section (both themes, busy states); live-service tests
(`memory_integration_live_`, scripts/memory-integration.sh); full suite + stack-budget at the 05 gate/06C;
pre-existing transport-boundary failure (channel_reply_continuation_views_support.rs:59, identical at base);
06A: settled-row retention, 1,000-credential ceiling (service change), managed_by reassignment vs 404/409
settles, provisioner rotation.

## Verification mapping
To be recorded by A per batch (criterion → test name → decisive assertion → evidence), per
/Users/mac03/.claude/skills/implement-opus-reviewed-2_2/references/verification-contracts.md.
Earlier batch evidence: plan/memory-service/05-openbot-integration.md "Verification record".

## p05-B2 decisions, verification mapping and evidence (owner: A gen8 af38660db9d70c7bd)

Status (2026-11-08 ~21:50Z, A gen8 returning): B2a exactness, B2b delivery queue, B2c steps and the
step-level E6 race (`memory_access_level_competing_recall_and_save_freeze_preserve_the_watermark`,
`memory_access_level_load_output_never_precedes_its_committed_observation`) implemented and verified
(see the 05 Verification record entry "p05-B2" and `logs/MEM-P05-B2-R1.evidence.json`).
NOT DONE in this batch: B2d — the four agent memory tools (search, timeline, details, save), their
publication/catalog/dispatch, tool consumption records and the tool-level E6 race/mutations; the
design sketch below is the plan. A returned at ~68% context instead of starting B2d across the 75%
compaction: B2d touches ~12 exhaustive-match sites (`AgentBuiltinTool`, `ToolKind`, `AgentToolTarget`)
and is the size of the originally separate P7d. Next bounded action: implement B2d per the sketch,
starting from `domain/entities/agent_capability.rs` (variants) and `publication/types.rs::ToolKind`.

### Decisions (beyond E1–E12; each also in the decision record "P7c implementation notes")
- B2a-1 Exactness in the domain: `HumanInputClass`, `ExactnessTarget {Run, AgentScope, UserScope,
  CompanyScope}`, `human_input_exactness(class, target)` in `entities/access_level.rs`.
- B2a-2 Classifier `application/workflow/exactness.rs`: `step_accounting(kind)` /
  `step_output_exact(kind, output)`, `tool_accounting(target)` / `tool_result_exact(Option<target>)`;
  unknown kinds/targets and a result for no prepared tool lower. `decision.human` is exact only with
  empty feedback and null/empty data; `message.ask` lowers.
- B2a-3 Hook 1 lives in `batch_commit::complete`, which now takes `CompletedStep{activated, graph}`
  so every caller must supply the frozen graph (6 callers + `commit_wake`); the lowering is folded
  into the same `UPDATE workflow_runs SET state…, content_exact = content_exact AND $5`.
  `channel_reply_resume` loads the saved binding for this.
- B2a-4 Hook 2 in `checkpoint_updates::update_on` for `InvocationResult` (target from
  `workflow_prepared.tools` by the saved call's tool id).
- B2a-5 Found path: a reviewer's channel-reply edit (`workflow_decisions.data`) becomes the published
  output, so `channel_reply_resume::complete_saved_on` lowers exactness when the submission carries data
  (`exactness::reviewed_output_exact`). Review feedback reaches no model or output and is not counted.
- B2a-6 Admission: manual inexact, schedule exact, message trigger by stored role/bit
  (`knowledge::stored_message_exactness`: human → derived inexact; agent/system → stored bit; else error).
- B2a-7 Schema: `workflow_runs.content_exact` NOT NULL no default, guard: only lowered, frozen after
  terminal; `messages.content_exact` NULL iff human (CHECK `messages_content_exact_shape`), immutable
  like `access_level`; `agent_history_exchanges.content_exact` NOT NULL, insert must equal the run's,
  updates follow `exchange AND run` everywhere `access_level` follows `GREATEST`.
- B2a-8 Test fixture `knowledge::exactness_fixture::mark_exact` sets a run exact with
  `session_replication_role = replica` for one transaction (precedent `child_output_tests`), because
  almost every fixture admits manual runs, which start inexact. Stand-in: it bypasses admission, so
  admission exactness is proved separately by real schedule/manual/message admissions.
- B2b-1 `memory_deliveries` per E8 but without a run FK (E8 names only company and namespace FKs; the
  freeze is testable without a workflow run). `transient_retries` drives outage backoff; outages never
  spend attempts and are bounded by the intent's `expires_at` (6 days).
- B2b-2 No heartbeat: the delivery deadline (45 s) is below the lease (60 s) and above the HTTP timeout
  (≤ 30 s), so a live attempt cannot outlive its lease (deviation from E8's "20 s heartbeat").
- B2b-3 Separate permits: `MemoryPool::Delivery` (own global semaphore sized like background, the
  background per-company share); lifecycle stays on `Background`.
- B2b-4 Run cancellation cancels the run's open intents by an AFTER UPDATE OF state trigger on
  `workflow_runs` (one place for every cancel path; lock order run → intent).
- B2b-5 Owner removal cancels intents after its namespace update (which waits out a concurrent freeze's
  FOR SHARE), selecting the owner's `removing` namespaces.
- B2c-1 No `memory` arm in `knowledge_release::evaluate_on`: neither the step nor the tools route
  through ActionService's release; a save's label is ≥ the run's content by construction (exact level
  or the ceiling), so the arm would be unreachable. Deviation from E4, for review.
- B2c-2 `memory.save` parks with a new attempt stop reason `memory_parked` (CHECK, job-completion guard
  branch, supervisor proof) rather than overloading the message `effect_parked` protocol; the effect
  wait insert guard accepts a wait owned by the step's memory intent; the poller wakes it on a terminal
  intent; `wait_commit` resumes from the intent before the message receipt path.
- B2c-3 Agent scope resolves the literal key against the bundle's agent snapshots (agents a model step
  of the workflow runs); anything else is `memory.unknown_agent` at publication.
- B2c-4 Destination (E3) = thread → lowest human reader; otherwise the run actor's level
  (`knowledge::destination_level_on`); read ceiling = min(run ceiling, destination, 9); destination
  below content refuses before any request.
- B2c-5 Gating: unconfigured → `UNHANDLED_STEP_KINDS` refused as before; configured → handled, and
  publication requires `memory_tenants.state = enabled` (current read, `MemoryPublicationGate`).
  Runtime: disabled → terminal `memory.not_enabled` (load and save), before any intent or request.
- B2c-6 Not ready at runtime → retryable `memory.not_ready` after `ensure_namespace` in its own
  transaction (never inside the run transaction).

### B2d design sketch (to implement; mirrors the knowledge tool end to end)
- Domain `AgentBuiltinTool::{SearchMemory, MemoryTimeline, MemoryDetails, SaveMemory}` (names
  `search_memory`, `memory_timeline`, `memory_details`, `save_memory`); update `every_builtin` in
  `exactness_tests.rs`, `parse`/`name`, `agent_contract::agent_action_target` ("memory").
- `AgentToolTarget::Memory { tool: MemoryTool }` (+ `builtin()` mapping, `prepared_tool` consistency,
  `agent_context::declaration`), exactness: search/timeline/details = RaisingRead, save = Receipt.
- Catalog `persistence/agent_authoring/catalog.rs::builtins` offers them only while configured and the
  company is enabled; snapshots in `agent_publication/tools.rs::builtin` with a trusted-snapshot check
  like `knowledge::builtin_knowledge_snapshot`; publication gate refuses memory tools when not enabled.
- Dispatch `AgentActionTools::with_memory(Arc<dyn MemoryToolDispatch>)`, `ToolInvocation::Memory`.
- Application `workflow/memory_tools.rs`: `MemoryToolRuntime { store, client }`: `memory_tool_context`
  (replay a completed invocation; else authority: scopes eligible per E2 — company, the run's agent,
  user only for a private session — read ceiling as for the step), service call outside any tx
  (search ≤10 snippets / timeline ±5 / details ≤4 whole bodies, all under `MAX_TOOL_RESULT_BYTES`),
  `observe_memory_tool` = one tx: `checkpoint_updates::update_on(InvocationResult)` + exposed ≤ ceiling
  + `raise_content_level_on` + `workflow_memory_consumptions` row (invocation id) + live, before the
  result reaches the model (pattern `knowledge_checkpoint::observe_on`). Save tool: key
  `ob.t.<invocation simple>`, `external_sources` = this execution's consumed observations (≤64, level
  desc, id), `deliveries::enqueue_save_on` in the checkpoint's ownership tx, result = intent receipt
  `{status:"accepted", intent}` (delivery asynchronous; a tool save does not park the agent).
- E6 race test `memory_access_level_competing_recall_and_save_freeze_preserve_the_watermark`
  (own_database, two connections, both orders, exact and fallback; lock wait via pg_blocking_pids),
  then rerun under mutations A (freeze from claim-time `RunKnowledge`) and B (result returned before
  the observation commits) in a separate `CARGO_TARGET_DIR` under tmp/memory-service-run/; each must fail.

### Acceptance mapping (B2c, steps)
| Criterion | Case | Decisive assertion | Test (status) |
| --- | --- | --- | --- |
| save → intent → delivery → wake; load raises before output | real handler, persistence, worker, `resume_wait` | parked + suspension proof; key `ob.s.<execution>`; Pending until delivered; output label 9 (manual run, company scope → ceiling); load item level 9, `exposed_level` 9, run content 0 → 9, consumption (9,9); one service save | `memory_integration_save_and_load_steps_round_trip_through_the_intent` (PASS) |
| disabled at runtime | disable after enable | terminal `memory.not_enabled`, no intent, no service call | `memory_integration_steps_fail_terminally_once_memory_is_disabled` (PASS) |
| configured gating | configured engine; not enabled; enabled; unknown agent | handled; `memory.not_enabled`; publishes; `memory.unknown_agent` | `memory_integration_configured_publication_gates_memory_steps_per_company` (PASS) |
| unconfigured stays refused | production engine without memory | `workflow.unsupported_step` | `production_publication_refuses_memory_steps_and_captures_children`, `production_handler_covers_every_io_kind_or_refuses_it` (PASS) |
| literal scope | ref-bound scope | `registry.literal` | `memory_scope_step_scope_must_be_a_literal` (PASS) |
| outputs | budget omission; save output; outcome codes | whole-item omission, exposed kept; codes | `memory_integration_load_output_…`, `memory_integration_save_output_and_terminal_outcomes` (PASS) |

### Acceptance mapping (B2b, delivery)
| Criterion | Case | Decisive assertion | Test (status) |
| --- | --- | --- | --- |
| E3/U2 labels at save | ceiling 6 / exact 4: agent → 4, company → 6 (unknown), inexact agent → 6 | frozen `consumed`, service-labelled `access_level` | `memory_access_level_saved_label_follows_scope_and_exactness` (PASS) |
| One request, same key+body on retry; replay of the intent | ambiguous drop-after-write, then resend | one service execution, 2 calls, request unchanged, `replayed` | `memory_integration_delivery_resends_the_same_request_after_an_ambiguous_outcome` (PASS) |
| Stale epoch → failed(disabled); expiry; poison batch | 4 unsendable intents, then 2 more passes | settled at claim, never reclaimed, no request | `memory_integration_delivery_settles_unsendable_intents_once_poison_batch` (PASS) |
| Fencing, attempts | two concurrent claimants; expired lease reclaim; 8th attempt | distinct claims; stale completion Lost; `lease_expired` | `memory_integration_delivery_fences_competing_claimants_and_bounds_attempts` (PASS) |
| Disable fails, owner hook cancels, enable revives nothing | retire agent namespaces; disable/enable | `cancelled/owner_removed`, `failed/disabled`, no save call | `memory_integration_disable_fails_and_owner_removal_cancels_open_intents` (PASS) |
| E9 competing | freeze vs disable, real concurrent tx, both orders (pg_blocking_pids) | intent failed / freeze NotEnabled; nothing delivered after re-enable | `memory_integration_disable_races_save_and_capture_then_reenable` (PASS; capture half is 05B) |
| Answer classification | every `MemoryServiceError` | retry/terminal mapping | `memory_integration_delivery_classifies_every_service_answer` (PASS) |
| Schema allowlist | memory tables/columns/functions exact | includes `memory_deliveries` and the trigger | `the_memory_provider_schema_is_gone` (PASS) |

Unproved by B2b: the run-cancel trigger has no test yet (needs a workflow run fixture; due in B2c with
the save step); the wait wake on a terminal intent lands with the effect wait in B2c.

### Acceptance mapping (B2a, exactness)
| Criterion | Production path / case | Decisive assertion | Test (status) |
| --- | --- | --- | --- |
| E3 kind/target coverage | `registry::TYPES`, every `AgentBuiltinTool`, every `AgentToolTarget` | each listed once with its accounting; unknown kind/target lowers | `memory_access_level_every_step_kind_and_tool_target_is_classified` (PASS) |
| U2 | `human_input_exactness` | conversational never exact; authored exact except company scope | `memory_access_level_human_input_exactness_follows_u2` (PASS) |
| decision.human choice vs free text | pure rule; real `submit_decision` through decisions.rs → complete | empty feedback/null data stays exact, feedback lowers | `memory_access_level_human_decision_choice_is_exact_and_free_text_is_not`, `memory_access_level_decision_choice_stays_exact_and_feedback_lowers` (PASS) |
| Hook 1 callers | completion.rs (`http.request` lowers; `context.history` stays), batch.rs (pure stays), decisions.rs, wait_commit (`wait.event` lowers), recovery route_failure, channel_reply_resume (reviewer edit lowers), child finish | run bit after the real caller | `knowledge_run_level_http_current_connection_level_allows_classified_dispatch`, `workflow_action_receipt_actual_result_precedes_shared_completion_writer`, `memory_access_level_pure_batch_keeps_exactness`, `memory_access_level_decision_…`, `memory_access_level_event_wake_lowers_exactness`, `memory_access_level_routed_failure_passes_the_exactness_hook`, `channel_reply_authoring_native_real_review_closes_reject_and_publishes_only_edited_approval`, `memory_access_level_child_copies_…` (PASS) |
| Hook 2 paths | approved HTTP tool result, unknown-tool refusal, knowledge observation | lowered, lowered, stays exact | `workflow_agent_run_protected_tool_waits_for_approval_and_dispatches_once`, `workflow_agent_run_refuses_model_invented_tool_without_dispatch`, `knowledge_search_driver_persists_observation_and_classification_before_continuing` (PASS) |
| Child copy / inexact child lowers parent | `children::create_on`, `child_waits` absorb | child starts exact from exact parent; inexact child lowers parent | `memory_access_level_child_copies_exactness_and_an_inexact_child_lowers_its_parent` (PASS) |
| History lowers | `context.history`; history freeze `bool_and` | human or inexact agent message lowers; selected inexact exchanges lower | `memory_access_level_history_read_lowers_for_conversational_or_inexact_messages`, `memory_access_level_history_freeze_lowers_for_inexact_exchanges` (PASS) |
| Admission by source | schedule / manual / human / system / agent-inexact trigger | exact, inexact, inexact, exact, inexact | `memory_access_level_admission_exactness_follows_the_trigger_source` (PASS) |
| Schema guards | raise refused; message bit shape; message bit immutable | constraint names | `memory_access_level_schema_only_lowers_exactness_and_shapes_message_bits` (PASS) |
| Regression | all `persistence::workflow::`, `persistence::thread::`, `application::workflow::` | 2006 passed / 0 failed (before the B2a test edits and the reviewer-edit lowering; rerun at the end) | `a8-p7c1-workflow-1.log` |

Unproved by B2a stand-ins: `mark_exact` bypasses admission (covered separately); the recovery caller
is exercised only with an accounted kind (lowering through it relies on the shared writer);
the workflow-tool child absorb (`workflow_tool_calls::call_result`) has no dedicated test yet.

### Contract impact (B2a)
| Changed contract | Producers / consumers | Disposition |
| --- | --- | --- |
| `workflow_runs.content_exact` | admission_write (root, child), knowledge lower/absorb, batch_commit, checkpoint_updates, channel_reply_resume, child_waits, workflow_tool_calls, history_reader, history freeze; message stamp reads it; exchange insert/updates read it; triggers settle_agent_history_run/routed_failure | all updated; raw fixtures updated (3 named-column inserts) |
| `messages.content_exact` | `thread/message.rs::insert_canonical_message` (role-shaped stamp); readers: admission trigger message, `context.history` | fixtures with agent/system rows updated (4) |
| `agent_history_exchanges.content_exact` | storage insert, checkpoint (2), frozen, 4 trigger updates, capacity fixtures (2) | updated |
| `batch_commit::complete` / `commit_wake` signature | 6 + 2 callers | all pass `CompletedStep` |
| `AgentToolTarget::builtin` | `agent_preparation::targets`, coverage test | extracted, one mapping |

### p05-B2 correction round R2 (A gen8; review MEM-P05-B2-R1, findings F1–F13 in agent-review-findings.md)

Status: every actionable finding resolved, each with a test; optional F11 and F12 taken, F10 recorded.
F2 follow-up (root decision): three base recovery tests that used `memory.save` as their example
of an unknown effect are retargeted to an unresolved ActionService marker (see the end).
Evidence manifest `logs/MEM-P05-B2-R2.evidence.json` (digest zeros); logs copied as
`logs/MEM-P05-B2-R2-*.txt`, mutation patches as `logs/MEM-P05-B2-R2-mutation-*.patch.txt`.

| Finding | Resolution | Test (assertion) |
|---|---|---|
| F1 terminal intent → failed step | No code change (the path worked); now tested for disabled, expired and refused intents. | `memory_integration_terminal_intent_wakes_and_fails_the_parked_save` (memory_steps_tests.rs): `poll_work` lists no Wait while the intent is open and lists exactly `[Wait]` once it is terminal; `resume_wait` completes on `FinalError`; run `failed`, wait `completed`, `memory_delivery_failed` audit present; intent `failed/disabled`, `expired/expired`, `failed/authority`. |
| F2 memory.save marker-owned (E4) | `"memory.save"` added to `marker_owned_effect_kind` (failure.rs) with the E4 reason; the classification tests list it as marker-owned. | `memory_integration_interrupted_save_without_an_intent_retries`: a claimed save whose lease expires before any intent retires `safe` through `recovery::retire_on(Expired)` (run running, job pending), and the retry parks exactly one intent. Mutation D (drop it from the set) fails this test (`unknown`/`waiting`/`failed`). |
| F3 history exactness discrimination | Test rewritten: the run is admitted on an exact agent message in its own thread, so the snapshot holds no other human row. | `memory_access_level_history_read_lowers_for_conversational_or_inexact_messages`: cases agent_exact → true, agent_inexact → false, system_inexact → false, human → false; asserts the snapshot is exactly the two rows and that the expectation took both values. Mutation C (ignore the stored agent bit) fails it. |
| F4 race stand-ins and mutations | New production-entry race `memory_access_level_competing_rise_and_parked_save_freeze_the_committed_level` (memory_park_race_tests.rs): the real `memory.save` park (`MemoryStepRuntime::execute` → `park_on`: `runtime::facts` FOR UPDATE OF run, `lease::lock_fence`, `enqueue_save_on`) races an in-flight `raise_content_level_on` 4 → 6 in both orders, on an agent-scoped save so an exact run's level is discriminated (a company save always sends `unknown`, U2). Rise first: the park waits (`pg_blocking_pids`) and freezes 6. Park first: the park is held after taking the run lock (a third transaction holds the tenant row), the rise waits on the park, the intent keeps 4 and the run ends at 6. Inexact runs freeze `unknown` in both orders. Mutation A follows E6's definition: the park freezes from the claim-time `RunKnowledge` read before the run lock and passed to `freeze_save_on` as a `LockedRun`; it fails this test. Patches kept under logs/. | see "Stand-ins" below |
| F5 missing 05A/E3 cases and competing tests | All added. | `memory_access_level_load_reads_at_the_thread_audience_and_refuses_a_lower_destination` (thread run, readers 9/8/6 → admitted ceiling 6, the load returns exactly levels 0–6 of seeded 0–9, exposed 6, run 6, consumption read_ceiling 6; a reader at 3 joining afterwards → reads 0–3; with content 4 the drop to 3 refuses as `KNOWLEDGE_AUDIENCE_BELOW_CONTENT` with no `observations/*` call); `memory_access_level_delivery_refuses_an_intent_without_its_trusted_ceiling` (frozen `producing_ceiling` removed → `failed/invalid_intent`, body cleared, no save sent); `memory_integration_owner_removal_races_save_freeze_in_both_orders` (`freeze_save_on` vs `retire_namespaces_on`, both orders with a real lock wait: freeze first → cancelled/owner_removed; removal first → `NotReady`, no intent); `memory_integration_disable_during_an_in_flight_delivery_loses_its_completion` and `memory_integration_run_cancel_during_an_in_flight_delivery_loses_its_completion` (the worker's save is held in the fake; disable / run cancel settles the claimed intent; the completion is fenced out: state stays failed/disabled or cancelled/run_cancelled with no receipt); `memory_access_level_workflow_tool_child_exactness_reaches_the_calling_run` (`call_result` absorb: exact child keeps an exact caller, inexact child lowers it, and the saved-result replay keeps the bit). |
| F6 unopenable envelope | The claim settles `failed/credential_unavailable` (logged) instead of failing the pass. | `memory_integration_delivery_settles_an_unopenable_credential_poison_batch`: an envelope sealed under another key is settled, the healthy intent behind it is delivered, then the queue is empty. |
| F7 concurrency and fairness | `MemoryDeliveryWorker::with_lanes(background_in_flight)` runs that many concurrent loops; `with_memory` takes the lane count (composed from `MEMORY_SERVICE_BACKGROUND_IN_FLIGHT`). The claim skips a company with a live claimed intent (index `memory_deliveries_in_flight`). Bound at 1 lane documented in the decision record. | `memory_integration_delivery_claims_are_fair_across_companies` (busy company's older backlog skipped while its save is in flight; another company's intent claimed; both at share → empty; after completion the busy company's next intent); `memory_integration_delivery_lanes_send_companies_concurrently` (two companies' saves must be in flight together to pass a 2-party barrier: 2 lanes deliver both, 1 lane delivers neither in the window). Mutation E (drop the in-flight exclusion) fails the fairness test (the second claim takes the busy company). The competing-claimant fencing test now puts its second intent in another company. |
| F8 retention and indexes | `request` nullable, CHECK `(state IN ('pending','claimed')) = (request IS NOT NULL)`; every terminal settlement (claim settle, completion, disable, owner removal, run-cancel trigger) sets it NULL; indexes `memory_deliveries_leased`, `memory_deliveries_in_flight`, `memory_deliveries_open_by_run`. Row retention: 06A obligation (decision record). | saved-label test asserts `request == None` after delivery; settlement tests assert it for failed/cancelled. |
| F9 catalogue | `WorkflowServices::step_catalogue` builds two static catalogues and returns the one matching `refused_kinds()`; the memory notes already name `memory.not_enabled`. Test helper `WorkflowEngine::with_unserved_memory` (cfg(test)). | `the_catalogue_covers_every_runnable_type_with_a_compiling_example` (configured: all TYPES including memory with the note; unconfigured: memory left out); `the_catalogue_lists_memory_steps_only_when_the_memory_service_is_configured` (UI page lists both kinds and the note); the existing unconfigured UI test still asserts their absence. |
| F10 (optional) channel-reply lowering isolation | Not taken. Recorded as defence in depth: in the reviewed template the `decision.human` gate always lowers first, so `complete_saved_on`'s lowering is a second guard; isolating it needs a reviewed approval with an edit but no feedback/data, left to the 06C regression. | — |
| F11 (optional) | Taken: the run is raised to max(`exposed_level`, item levels) and that level is recorded. | round-trip and thread-load tests (exposed = carried). |
| F12 (optional) | Taken: ref key renamed `content_sha256`; B3/E7 takes `sources_sha256` from the `context.pack` PackItem (decision record). | — |
| F13 flaky claimant order | Rows looked up by `a.id`. | fencing test, rerun in the delivery set. |
| B3 note: ordering | `settle_open_on` and the trigger lock rows `ORDER BY id FOR UPDATE` before updating. | `memory_integration_competing_disable_and_run_cancel_settle_every_intent` (4 rounds × 6 intents, disable and cancel released together by a barrier: both succeed, every intent terminal once, body cleared). Forced: `memory_integration_disable_and_run_cancel_overlapping_in_id_order_never_deadlock` (a holder locks the middle intent; the run-cancel trigger takes the rows below it and waits; disable starts while it waits; on release both succeed and all six end `failed/disabled`). Mutation F (DESC order in `settle_open_on`) fails it with `deadlock detected`. |

Stand-ins (F4): (1) the rise is `knowledge::raise_content_level_on` in a bare transaction, the primitive every
production raiser uses under its own run lock (`memory_steps::observe_on`, `knowledge_checkpoint`, history and
child absorbs). `observe_memory_load` itself cannot race `park_memory_save` in one run: both run inside a fenced
step execution and a run has one claimed execution at a time, so the only concurrent writers of a run's level are
other fenced paths that take the run row first; the race on the shared primitive is therefore the production
interleaving. (2) Park-first holds the park mid-transaction with a third transaction on the `memory_tenants` row
(the park's next lock after the run row); that holder is the stand-in, the park itself is production code.
(3) The older shared-function race (`memory_access_level_competing_recall_and_save_freeze_preserve_the_watermark`)
stays: it is the test that catches mutation B (freeze read without FOR UPDATE), which the production race does
not, because `park_on` already holds the run row from `runtime::facts`. (4) Thread-run memory.load uses a real
thread admission; readers are inserted as company members and `thread_principals` rows (fixture SQL).

Mutations, each in an isolated copy (`tmp/memory-service-run/mut-r2/tree`, own `CARGO_TARGET_DIR`,
both deleted after the run), patches as `logs/MEM-P05-B2-R2-mutation-*.patch.txt`:
- A fails `memory_access_level_competing_rise_and_parked_save_freeze_the_committed_level`; B fails
  `memory_access_level_competing_recall_and_save_freeze_preserve_the_watermark`; C fails
  `memory_access_level_history_read_lowers_for_conversational_or_inexact_messages`. These three ran
  before the coordinator's isolation rule: each patch was applied to this worktree (own target) and
  reverted.
- D fails `memory_integration_interrupted_save_without_an_intent_retries`; E fails
  `memory_integration_delivery_claims_are_fair_across_companies` (the lanes test still passes).
- F: the barrier test `memory_integration_competing_disable_and_run_cancel_settle_every_intent`
  passes with the DESC order (`a8-r2-mutation-F-unordered-settlement-1.log`). The cause: a user
  cancel and disable both take the company row (`authorize_company`, FOR NO KEY UPDATE), so they
  never overlap on intents. The new forced test,
  `memory_integration_disable_and_run_cancel_overlapping_in_id_order_never_deadlock`, fails under F
  with `disable: Err(Database("... deadlock detected"))` (`a8-r2-mutation-F-unordered-settlement.log`).
  It passes in id order, 10/10 under a concurrent regression (`a8-r2-order-forced-repeat.log`).
  Stand-ins: a third transaction holding the middle intent is the pause between row locks. The
  cancel is the bare run → `cancelled` update that fires the trigger, rolled back afterwards.
  That update is what a guard failure's child cancellation (`cancel_children_on`) issues without
  the company lock. No production hook was added.

Restore: the first D attempt (in this worktree) filled the disk
(`a8-r2-mutation-D-save-not-marker-owned.disk-full.log`) and left
`src/adapters/persistence/memory/deliveries.rs` and `src/application/workflow/actions/failure.rs`
unwritable mid-revert. Both were restored from the frozen R1 tree `acbda41e` and the R2 edits
re-applied: `failure.rs` is byte-identical to its recorded pre-mutation hash (`8cca27c3`);
`deliveries.rs` carries the same SQL as the mutation patches' context (B, E and F apply to it), with
re-written comments. The R2 gates (05 Verification record) ran on the restored tree.

F2 follow-up (root decision, option 1): F2 makes `memory.save` retry-safe without a marker. It
was the last I/O kind outside both sets, so three base tests that used
`registry::example("memory.save")` as their unknown-effect example failed after F2
(`a8-r2-regression.log`, `a8-r2-regression-isolated.log`):
- `workflow_budget_runtime_unknown_effect_exhaustion_reconciles`
- `workflow_poison_effect_output_and_spent_pending_reconcile`
- `workflow_failure_interrupted_and_expired_effects_cannot_replay`

Each now starts from `action_receipt_tests::unresolved_marker`, the `action_uncertainty_tests`
pattern. The fixture's I/O step claims, then a remote ActionService dispatch commits its
possible-dispatch marker and loses the provider's answer. No receipt exists, so the shared SQL
predicate reports the effect as unknown. For the spent-pending case, the marker is
`ProviderIdempotency`: its lease expires, the job returns to pending, and the run's horizon is
then pushed past the provider's retention. The assertions are unchanged: reconciliation, an
unknown retry-safety, no replay, `Unsafe` retry and the budget usage. Each test carries a one-line
note on why `memory.save` no longer serves (`a8-r2-retarget-1.log`: 3/3).

### p05-B2 correction round R3 (A gen8; recheck MEM-P05-B2-R2: F14 minor, F15 optional, F11 nit)
Evidence manifest `logs/MEM-P05-B2-R3.evidence.json` (digest zeros); logs as `logs/MEM-P05-B2-R3-*.txt`.

| Finding | Resolution | Test (assertion) |
|---|---|---|
| F14 delivery lanes vs the pool reservation | Lanes now share one `tokio::sync::Mutex` (`MemoryDeliveryWorker.database`) around `claim_delivery` and `complete_delivery`. Each store call takes and returns its pool connection inside the lock, so the worker holds at most one connection at any lane count. Only the sends (up to 45 s each, no connection held) overlap. Chosen over per-lane reservation because that keeps `RESERVED_DATABASE_CONNECTIONS` (14) and the `worker_slots` check true unchanged. Reserving one connection per lane would make the default deployment (pool 20, 2 slots, 4 lanes: 13 + 4 + 4 = 21) refuse to start unless a default changed. The cost is small: a claim is one indexed `SKIP LOCKED` row (lock_timeout 2 s) and a completion one fenced update, both short against the 60 s lease. `docs/deploy.md` and the `RESERVED_DATABASE_CONNECTIONS` comment state this; `docs/workflow-operations.md` already counts the worker once. | `memory_integration_delivery_lanes_send_companies_concurrently` now wraps the store in a counter that holds each call 30 ms. With 2 lanes both saves are still in flight together, and the peak of concurrent store calls is exactly 1 (1 lane as well). Mutation G (drop the lock) fails it: `lanes=2: concurrent store calls`, 2 ≠ 1. |
| F15 (optional, taken) marker-owned invariant docs | `failure.rs` (module header and `marker_owned_effect_kind`): the invariant now reads "a committed record of the effect: ActionService's marker-first protocol, or the documented equivalent". It gains a "`memory.save` joins (phase 05 E4)" paragraph: the intent committed with the park is its marker. `WorkflowHandler::execute` (worker.rs) and `ProductionHandler` (infra/workflow.rs) are generalized the same way. | comments only |
| F11 nit | `carried_level(exposed, items)` (application/workflow/memory.rs) is the one computation: the higher of the service's `exposed_level` and every item's level. `observe_on` raises the run and records the consumption with it, and the `memory.load` output's `exposed_level` now reports the same value. An item without a valid level refuses as `memory.refused` before anything is observed. | `memory_access_level_load_carries_the_highest_of_the_label_and_its_items` (unit: L0 with items 2/6 → 6, L8 → 8, empty → exposed, level 10 → None). `memory_integration_save_and_load_steps_round_trip_through_the_intent`: the load's client now under-reports `exposed_level` 0, yet output, run level and consumption all carry 9. |

## p05-B3 — agent memory tools (author: root, Opus/xhigh; reviewer B abdd866fb428eea05)
Scope: E5 four tools (`search_memory`, `memory_timeline`, `memory_details`, `save_memory`),
catalog/publication/dispatch, tool consumption records, tool save intents, E2 user scope, E5
defense in depth, tool-level E6 race + mutations A/B. Review base: p05-B2 accepted digest
9c7929bf…1abc (tree dddff33c). Coordination r1 (I001–I003) rechecked, no new notices.

### Decisions (B3)
- D1 Tool reads commit in one ownership transaction before the result is returned toward the
  model (`memory_tool_checkpoint.rs::observe_on`): saved-invocation check (as knowledge), exposed
  ≤ both the authorized and the current ceiling, `InvocationResult`, `raise_content_level_on`,
  one `workflow_memory_consumptions` row (invocation id), live. Refusals (`refuse_on`) record the
  error result only: no consumption, no raise. Replay requires the recorded exposure ≤ the current
  ceiling (`KnowledgeRelease` otherwise); a save receipt or refusal replays without one. An
  ambiguous commit returns the unresolved error (no continuation).
- D2 Consumption items for tool reads are `MemoryRecallRef {scope, id, revision, access_level,
  content_sha256|null}` for exactly the items shown (search/timeline snippets carry no hash,
  details carry the body hash); trailing items dropped by the result bound are not recorded, but
  the raise still covers everything the service returned (conservative, as `memory.load`).
- D3 Save provenance (E5 text): `external_sources` = distinct (scope, id) of this execution's tool
  consumption rows (all attempts), level = max seen, ordered level desc then id, ≤ 64, reference
  `memory:<scope>:<uuid>`; read under the run lock (`recalled_on` after `load_call`). Key
  `ob.t.<invocation simple>`; intent via `enqueue_save_on` (locked run re-read); receipt
  `{status:"accepted", intent}` as the invocation result; no park.
- D4 User scope (E2): the root run (recursive parent walk ≤ 64) is bound to an unattached session
  whose creator is the root's actor and this run's actor, AND the root's admission source is a
  person's manual start (`SourceKey::is_manual`). Reason: every admission binds an unattached
  session (`agent_history_sessions::capture`), so without the source check a schedule root would
  qualify. E2's "manual run outside a session" = a manual run associated with a channel/thread
  (its session is attached) — not eligible. Ineligible scopes are absent, never provisioned.
- D5 Not-ready eligible scopes the call names are provisioned by `provision_memory_scope` in their
  own transaction (owner re-resolved, then `ensure_namespace`), and the call answers
  `memory.not_ready` when nothing ready remains.
- D6 Enabled gating: an agent version with memory tools publishes only while memory is enabled
  (`agent_publication::tools::memory_publication_on` from `resolve_tools`); a workflow bundle's own
  memory snapshots likewise on `ToolCheck::Publish`. Live re-validation (`check_source_on` →
  `direct_registration_on`), the admission manifest capture and readiness (`Run`) do not check it:
  after a disable a published agent keeps running and each memory call answers
  `memory.not_enabled` (06-D5 revocation at runtime). (A first cut refused in
  `direct_registration_on`, which made every checkpoint write of an in-flight run fail NotFound;
  a second refused in the manifest capture, which runs at admission. Both reverted.)
- D7 Schema (init edited in place, test DBs reset): `agent_binding_target_shape` and
  `agent_binding_reserved_alias` list the four names; `is_reserved_tool_name` includes them.
- D8 E5 defense in depth: `EndpointPolicy::reserving(MEMORY_SERVICE_URL)` refuses every endpoint on
  the service's origin tuple (scheme, host, effective port), before operator exceptions; wired into
  the MCP client policy (`setup/clients.rs`, so MCP connection writes via `validate_endpoint` and
  MCP calls) and the workflow HTTP policy (`WorkflowEngineSetup.http_destinations`). Workflow HTTP
  connection writes are not policy-validated today; they are refused at call. Alias forms (IP for
  a name) are not recognised, as E5 states.
- D9 Not taken: F16 (optional) and F10 stay deferred (06A).
- D10 Known edge (recorded, not fixed): memory enabled, then `MEMORY_SERVICE_URL` removed — enable
  itself is refused while unconfigured, but already published memory tools then hit the dispatch
  arm's `ServiceUnavailable` (fail closed, nothing read or saved; the step fails retryable).

### Verification mapping (B3)
| Criterion | Test | Decisive assertion |
|---|---|---|
| E5 tools offered with built-in contracts; no authority inputs; production dispatch | `memory_integration_agent_memory_tools_search_and_save_through_the_production_dispatch` | request tools: description = built-in, property keys = schema keys, `additionalProperties:false`; model sees L5/L3 items, never the L7 text; run 5 and exact; one consumption (ceiling 5, exposed 5, 3 refs with scopes); one intent: key `ob.t.<save invocation>`, agent scope, consumed exact 5, ceiling 5, `external_sources` [L5 company, L3 agent, L0 company]; user namespace requested not ready; 2 searches, 0 service saves |
| Input bounds / no authority fields | `memory_scope_tool_inputs_carry_no_authority_and_enforce_their_bounds`; `memory_scope_tool_arguments_parse_strictly_with_documented_defaults` | every schema refuses level/extra keys and out-of-bound values; host parse refuses company save, duplicate scopes, blank text; defaults limit 5, timeline ±5, save scope agent |
| Trusted snapshot exactness, reserved names | `memory_scope_builtin_snapshots_are_reserved_and_exactly_trusted` | widened input/output, changed effect/revision or a connection → not trusted; all four reserved |
| Catalog only while enabled | `memory_scope_catalog_offers_memory_tools_only_while_enabled` | false → none; true → all four (+4) |
| Publication only while enabled | `memory_scope_memory_tools_publish_only_while_memory_is_enabled` | agent publish NotFound "Memory is not enabled"; same draft publishes after enable |
| E3 ceiling, record once, replay, lowered-ceiling replay refuses | `memory_access_level_agent_memory_search_is_ceilinged_recorded_once_and_replayed` | items L2,L5 only (L6 absent); invocation result = returned; 1 service call and 1 consumption after replay; grant 3 → `KnowledgeRelease`, no call |
| Details/timeline within ceiling, no oracle; refusal raises nothing | `memory_integration_agent_memory_details_and_timeline_stay_within_the_ceiling` | details order [agent, company], L7 id absent, body hash recorded; timeline [first, second]; hidden anchor → `memory.not_found`, level/consumptions unchanged |
| E2 user scope | `memory_scope_user_memory_requires_the_actors_own_manual_session` | manual root: user search + save (intent user scope, user = actor); schedule root: both `memory.scope_unavailable`, no intent, no user namespace, no service call |
| Not ready | `memory_scope_unprovisioned_scope_is_requested_and_refuses_not_ready` | `memory.not_ready`; agent namespace created not ready; company not provisioned; nothing raised; no service call |
| Revocation at runtime | `memory_scope_agent_memory_tools_refuse_once_memory_is_disabled` | after disable: search `memory.not_enabled`, replayed identically; agent still prepares a new call; save `memory.not_enabled`; nothing raised/recorded/contacted |
| E6 save freeze (production `save_memory`) | `memory_access_level_competing_rise_and_tool_save_freeze_the_committed_level` | rise first: save waits (`pg_blocking_pids`), freezes 6; save first: rise waits on the save, intent keeps 4; inexact → `unknown`; run ends 6. Mutation A must fail it |
| E6 read commits before exposure | `memory_access_level_tool_read_commits_its_rise_before_the_model_sees_it` | with the run row held after the service answered, the observe transaction waits and the call has not returned (level 0); after release: result, level 6, 1 consumption. Mutation B must fail it |
| Save provenance order/bound | `memory_access_level_save_sources_are_distinct_level_ordered_and_bounded`; e2e above | distinct, max level, level desc then id, 64 cap |
| Bounded result ↔ refs | `memory_access_level_bounded_hits_record_exactly_what_is_shown` | refs = shown items; level 10 is a fault |
| E5 defense in depth | `memory_integration_company_connections_cannot_reach_the_memory_service_origin`; `memory_integration_http_connection_to_the_memory_service_origin_is_refused` | same origin (case/default port) refused even when listed as an exception; other port/host allowed; HTTP client refused |
| Exactness classification | `memory_access_level_every_step_kind_and_tool_target_is_classified` | memory search/timeline/details RaisingRead, save Receipt (10 samples) |

Stand-ins and what they leave unproved: the schedule root is the same run with its admission row
replaced (fixture SQL; the admission identity is immutable on update); child inheritance of user
eligibility is not exercised (no child fixture; SQL walk only); the race's competing rise is a raw
`raise_content_level_on` transaction (a run's tool calls are sequential, so the production
competitor is another execution of the run — as B2's accepted step race), and save-first holds the
save after its run lock with a tenant-row holder; the reservation's setup wiring
(`clients::build`, `mail::workflow`) is composition code without a test; contract fake only (no
live service); a tool intent's delivery is the B2 worker path (not re-run here); D10 untested.

### Evidence (B3)
Manifest `logs/MEM-P05-B3-R1.evidence.json` (valid: 7 PASS / 0 FAIL / 1 NOT EXERCISED), digest
0d0b4622ed5d78726877bfccc1604450e4a67eff42ddff189957a58090336c3d (exclusions as recorded), freeze
tree dfeeca6774cffca03ca7302a7e73b0d6a64dd270 (`tmp/memory-service-run/freeze.sh p05-B3-R1`);
B3 delta = `git diff dddff33c dfeeca67 -- src migrations` (52 files; `src/main.rs` there is the
sibling sessions' plan-path entries, not p05).
- Focused selection (`logs/MEM-P05-B3-R1-list.txt`, 426): 425 passed, 0 failed, 1 ignored
  (pre-existing browser fixture exporter), final formatted code (`-focused.txt`). An earlier run
  failed only `memory_access_level_every_step_kind_and_tool_target_is_classified` (sample count
  6 → 10 after the four memory targets); fixed.
- clippy `-D warnings` lib+tests clean; fmt and `git diff --check` clean; fresh migration +
  `sqlx prepare --check` pass (no query macro changed).
- Mutation A (`-mutation-A-tool-save-prelock-level.patch.txt`, `-mutation-A.txt`): the race test
  fails (exact rise-first freezes 4, want 6). Mutation B (`-mutation-B-read-returned-before-commit.patch.txt`,
  `-mutation-B.txt`): the read test fails ("no result before the observation commits") and the e2e
  test fails (level 0, want 5). Isolated copy + own CARGO_TARGET_DIR, deleted after; ≥ 20 GiB free
  (root removed this worktree's `target/debug/incremental`, 14 GiB own cache, before B).
- Not exercised: full suite, stack budget (05 gate/06C), live service, browser check, Docker/CI.


### p05-B3 correction round R2 (root; review MEM-P05-B3-R1, findings F1–F5)
Coordination rechecked: revision 2 (I003 gains U4/U5, written by root for this round). The R1
digest drift B reported (sibling `src/main.rs` NOTES entries removed at 12:27Z) is sibling work; the
R2 artifact is re-frozen.

| Finding | Resolution | Test (decisive assertion) |
|---|---|---|
| F1 Major user scope vs foreign-authored manual runs | User ruling **U4** (2026-11-09, "keep and document"): eligibility rests on run facts only, never authorship. Recorded in E12 (U4) with the E11 case list amended, COORDINATION I003 r2, docs/workflow-operations.md "Agent memory tools". Code: the rule is now a pure decision `memory_tools::user_scope_owner(run_actor, Option<&UserScopeRoot>)` over facts read by `user_scope_owner_on` (root by parent walk, its session creator/attachment, its admission source). | `memory_scope_user_scope_eligibility_matrix` (unit: own manual root → actor; same-actor child → actor; other-actor child, schedule, message/thread root, manual attached session, foreign session creator, no root → none). `memory_scope_a_manual_roots_child_inherits_the_user_scope` (DB: a real child from the production child path and its root both resolve to the actor). `memory_scope_user_memory_requires_the_actors_own_manual_session` now asserts the version's publisher ≠ the run actor (the documented foreign-authored case, eligible). Stand-in: thread/attached cases are unit-level (no channel/thread fixture in this harness); the SQL reads the session columns directly. |
| F2 adversarial tool tests | Added on the production entry points (`call_memory_tool`, `observe_memory_read`, `refuse_memory_tool`, `save_memory`). | `memory_integration_forged_calls_and_fences_refuse_before_the_service` (7 forged fences × 4 entry points, changed arguments, a bound-but-other tool (save via a search invocation), unbound details/timeline → all Err; the database refuses widening a prepared tool ("prepared inputs are immutable"); no service call, nothing committed, invocation still pending, the genuine call then works). `memory_integration_a_replacement_claim_fences_the_stale_saver_and_replays_one_intent` (stale owner's save and call Err; new owner replays the receipt; a second save through the invocation returns the same receipt; one intent with the original body). `memory_integration_service_auth_refusals_are_controlled_and_raise_nothing` (401 and 403 → `memory.refused`, recorded and replayed, level 0, no consumption, one request each). Reviewer mutation C re-run: see evidence. |
| F3 multi-scope search = one operation | New port call `MemoryClient::search_scopes(&[ScopedSearch])`: one interactive permit for the whole tool call, requests concurrent, answers in order, mixed companies a contract fault; the adapter's `client()` split into acquire + `send()`; the search tool uses it (count mismatch → internal error). Test doubles delegate. Contract fake gains persistent `holds`/`arrivals`. | `memory_integration_a_multi_scope_search_is_one_interactive_operation` (three held requests arrive together while the company's interactive slots in use = 1; a concurrent fetch of the same company completes meanwhile; 0 after). Under the old per-request permits only two could arrive (share 2). |
| F4 unconfigured service | User ruling **U5** (2026-11-09, "Document it"): documented operator edge with remedy (E12 U5, docs/deploy.md "Memory service", COORDINATION r2); D10 now names every surface: editor list, publication, calls. | — (documentation) |
| F5 optional | Taken for the catalog: `memory_scope_authoring_catalog_lists_memory_tools_only_while_enabled` (production `tool_options`: four listed while enabled, none after disable). Not taken for the bundle gate: built-in snapshots are created only by agent publication and tool registrations cannot use reserved names, so a workflow bundle's own snapshots never carry a memory tool; the `ToolCheck::Publish` arm stays as the exhaustive-match defence. | as stated |
R2 evidence: manifest `logs/MEM-P05-B3-R2.evidence.json` (digest 50d052b7…3457, freeze tree
d9d63035; delta vs R1 `git diff dfeeca67 d9d63035`, 18 files; `src/main.rs` there is sibling work).
Focused selection 433 listed, 432 passed / 0 failed / 1 pre-existing ignored; clippy -D warnings,
fmt, `git diff --check` clean; reviewer mutation C now fails
`memory_integration_forged_calls_and_fences_refuse_before_the_service` (isolated copy deleted).


### p05-B3 correction round R3 (root; recheck MEM-P05-B3-R2: F6 Minor, F7 optional)
| Finding | Resolution | Test (decisive assertion) |
|---|---|---|
| F6 the single-operation test passed with per-request permits | The contract fake now counts held calls now/peak (`held_now`, `held_peak`; `arrivals` removed). The test holds each search 1.5 s and waits (≤ 5 s) until **three are held at the same time**, which per-request permits (share 2) can never reach; then slots in use = 1, a concurrent fetch completes meanwhile, peak = 3, 0 after. | `memory_integration_a_multi_scope_search_is_one_interactive_operation`; reviewer mutation D re-run (evidence). |
| F7 (optional, taken) attached session on stored rows | Real manual runs of one binding: one in a channel (attached session), one company-wide, same actor. | `memory_scope_a_manual_run_in_a_channel_has_no_user_scope` (message_tests: the channel run's session is attached and resolves to no user scope; the company-wide run resolves to the actor). |

R3 evidence: manifest `logs/MEM-P05-B3-R3.evidence.json`, digest 8857f53867e70b2edb79a9b13a0d21d4b9f05f5729f129daec690f5744bea777,
freeze tree a69f88c5d835a9fb5f7447338e5dcb787fc6c6e4 (delta vs R2 `git diff d9d63035 a69f88c5`). Focused 813 listed, 811
passed / 0 failed / 2 pre-existing ignored; clippy -D warnings, fmt, diff --check clean; reviewer mutation D now fails the F6
test (isolated copy deleted; root cleared its own 11 GiB incremental cache first to keep ≥ 20 GiB free).
\n
## p05-B4 — automatic recall (05B.1–4, E7) (author: root, Opus/xhigh)
Coordination r2 checked (U4 applies to recall's user scope). Review base: B3 accepted freeze tree a69f88c5.

### Design (B4, from E7; deviations marked)
- Ports: `MemoryClient::context_pack(&[ScopedPack]) -> Vec<MemoryPack>` — one interactive operation
  (one permit, concurrent requests, all under the recall deadline `MEMORY_SERVICE_RECALL_DEADLINE_MS`),
  compact detail; `MemoryClient::context_validate(credential, namespace, ceiling, &[PackedRevision])`
  — statuses in request order. Contract fake: `context/pack` (candidates by query or recency,
  whole items within max_tokens/max_bytes, digests) and `context/validate`.
- Plan (`MemoryRecallPersistence::recall_plan`, rolled-back read through the preparation's own
  authorization): `NotApplicable` (not `agent.run`, no history exchange, agent does not bind
  `SearchMemory`, memory not enabled) | `Frozen` (the exchange already has a frozen preparation) |
  `Needed{scopes (E2/U4 eligible, ready, with credentials), not_ready, ceiling, max_tokens, query}`.
  Not-ready eligible scopes are provisioned in their own transaction (as tools). Budget:
  `max_tokens = min(8192, (request_input − reserved_output − mandatory)/4)` (**deviation**: E7
  named 2,048, but the service's accounting, 64 + 2 tokens per item byte with ~400-byte items, left
  the agent and user quarters room for no item; found by the e2e test), split company 1/2,
  agent 1/4, user 1/4 (an ineligible user quarter is unused); `max_bytes` 65,536 (the harness has no
  byte policy, so E7's `max_context_bytes/4` term is not applicable); under 256 tokens →
  `skipped_budget`. Query: the trigger message body, else the current input text cut at a character
  boundary to 1,024, else recency.
- Fetch (application `MemoryRecall`, outside any transaction): transport, 429, 503, permit or deadline
  exhaustion → `unavailable{code}` and the run continues; 401/403/refused → fail closed (error).
- Freeze (`frozen::freeze_on`): an existing freeze discards the fetched pack unseen. Else re-read the
  ceiling under the owner/run lock; changed → `unavailable{ceiling_changed}` (pack discarded unseen);
  else require `exposed ≤ ceiling`, set `PreparedAgent.memory = FrozenMemory{state, items, digests,
  exposed_level, valid_until}` before history selection (the pack is mandatory input for the
  history budget), raise `content_level` to the pack's exposed level, record a
  `workflow_memory_consumptions` row (nil invocation; items `{scope,id,revision,access_level,
  content_sha256,sources_sha256}`), then the existing history absorb and `audience_on`. Restore
  re-applies the saved memory and compares the whole frozen preparation.
- Prompt: the frozen items as their own untrusted JSON block before the current input in the initial
  prompt; the preamble of an agent that binds `SearchMemory` gains one fixed usage sentence. Pure
  construction; nothing reloads per tool round.
- Resume: once per resumed attempt (a claim whose preparation was restored, not newly frozen), before
  its first provider entry, `context.validate` the frozen pack items per scope: any
  `changed`/`expired`/`unavailable`/401/403 → terminal `memory.context_revoked`; service unavailable →
  retryable `memory.context_unavailable` (attempt consumed). Never a new pack or a lower level.
  Deviation: recorded memory *tool* items are not validated (tool results carry no `sources_sha256`,
  which `context.validate` requires); a tool replay is held by its recorded exposure ≤ current ceiling.
- Tool save provenance (E5) includes recall rows: `recalled_on` reads this execution's rows including
  the nil-invocation recall row (a `memory.load` step never shares an execution with `agent.run`).

### Decisions and deviations (B4, beyond the design above)
- D1 Recall cap 8,192 tokens instead of E7's 2,048 (see design; found by the e2e test: at 2,048 the
  agent and user quarters held no item). Still a quarter of the request's remainder at most.
- D2 A resumed attempt is one whose plan read found the exchange's preparation already frozen
  (`RecallPlan::Frozen`); the owner lock makes plan and preparation one owner's view. Revalidation
  runs in `WorkflowAgentRuntime::run` after preparation and before `run_ready`, once per claim.
- D3 Revalidation statuses: all `valid` → continue; any other status, 401/403, a namespace now
  absent or 404 → terminal `memory.context_revoked`; outage (503, capacity, deadline) → retryable
  `memory.context_unavailable`. A ceiling now below the frozen exposure → revoked before any call.
- D4 Recall failures: 503/capacity/deadline/404 → frozen `unavailable{code}` and the run continues;
  401/403/refusal/protocol → `AppError::Forbidden` (fail closed, nothing frozen, no provider entry).
- D5 The recall consumption row uses the nil invocation id and `MemoryRecallRef` items with
  `sources_sha256`; E5 tool-save provenance now includes it (`recalled_on` no longer filters it).
- D6 `prepare_agent` keeps its signature (101 call sites); `prepare_agent_with_recall` carries the
  fetched recall; the plain call passes none. The pack is bound before history (`with_memory`), so
  the history budget counts it; restore re-binds the saved pack before comparing the whole document.
  The `memory` key enters the budget only when items exist, so non-memory agents' budgets are unchanged.
- D7 Contract fake `context/pack` matches any query word of ≥ 3 letters (stand-in for ranked hybrid
  search) and `context/validate` reports changed/expired/unavailable from the seeded rows.

### Verification mapping (B4)
| Criterion | Test | Decisive assertion |
|---|---|---|
| 05B.1–2 bounded pack before provider entry; own untrusted block; usage sentence only in system; no instruction promotion; ceiling | `memory_preparation_recall_freezes_a_ceilinged_pack_into_the_untrusted_context` | system has the usage sentence and none of the memory text; user context has `recalled_memories` with the L5/L3 items before the current input; the L7 item absent; frozen memory Included, ceiling 5, exposed 5, 3 items, 2 digests; run 5 and exact; one nil-invocation consumption with `sources_sha256`; history budget counts the pack; 2 pack calls, 0 validate; user scope requested (U4) |
| 05B.3 exact replay, no new pack, same level, one revalidation per resumed attempt | `memory_preparation_a_resumed_attempt_replays_the_pack_and_validates_once` | attempt 2's memory block = attempt 1's; frozen preparation unchanged; level unchanged; still 2 pack calls; 2 validate calls (one per scope with items); one model request |
| 05B.3 revocation is the controlled refusal; unverifiable retryable; never lower | `memory_preparation_a_revoked_or_unverifiable_pack_stops_the_resumed_attempt` | changed revision → Terminal `memory.context_revoked`; validate 503 → Retryable `memory.context_unavailable`; no provider entry; no new pack; level unchanged. Mutation G must fail it |
| 05B.4 optional unavailability vs fail-closed refusal | `memory_preparation_optional_unavailability_continues_and_refusal_fails_closed` | pack 503 → run completes, no memory block, frozen `unavailable{unavailable}`, nothing raised or recorded; pack 403 → error, no provider entry, nothing frozen, level 0 |
| E7 freeze: changed ceiling discards unseen; an existing freeze wins | `memory_preparation_a_changed_ceiling_discards_the_fetched_pack_unseen` | fetched under L6 while the run has L5 → `unavailable{ceiling_changed}`, no items, nothing raised or recorded; a later fetch replays the same frozen memory; recall budget 8,192 on default limits. Mutation F must fail it |
| U3/E7 applicability | `memory_preparation_recall_applies_only_to_memory_search_agents_while_enabled` | no `search_memory` → no pack call, no usage sentence, no frozen memory; memory disabled → no pack call, sentence kept (binding), no frozen memory |
| Adapter contract | `memory_preparation_adapter_packs_and_validates_through_the_contract` | packs newest first within ceiling 5, request order, whole items within a tight budget; statuses valid → changed/expired → unavailable above the ceiling |
| Unit | `memory_preparation_recall_budget_shares`, `memory_preparation_optional_failures_are_distinct_from_refusals` | 1/2, 1/4, 1/4, never 0; optional vs refusal classification |

Stand-ins: the contract fake for the service (ranking approximated, D7); the paused first attempt
uses `PreparedOnly` tools (the attempt ends at its first tool call); the changed ceiling is injected
by preparing with a fetched recall whose ceiling differs (the plan/freeze gap cannot be timed from
outside). Not exercised: the `skipped_budget` path end to end (needs a request policy near its
limit; the computation is checked on defaults), live service, browser.

### Evidence (B4)
Manifest `logs/MEM-P05-B4-R1.evidence.json`: digest a998bca3…eece1, freeze tree 3749a60c (delta vs
B3 `git diff a69f88c5 3749a60c -- src`, 27 files; `src/main.rs` there is sibling NOTES work). Focused
862 listed, 860 passed / 0 failed / 2 pre-existing ignored; recall/agent paths pass at 2 MiB stack;
clippy -D warnings, fmt, diff --check clean; mutations F and G each fail exactly their target test.


### p05-B4 correction round R2 (root; review MEM-P05-B4-R1, findings F1–F5)

| Finding | Resolution | Tests / evidence |
| --- | --- | --- |
| F1 resumed pack replayed unvalidated without recall | The guard is structural: `WorkflowAgentRuntime.recall` is no longer optional; `new` composes `MemoryRecall::unconfigured(store)` (method bound `S: MemoryRecallPersistence + Clone`, only `PostgresPersistence` implements `AgentPreparation`), `with_memory` replaces it with the configured recall; `compose_handler`'s explicit guard removed. `validate` runs on every claim: a frozen pack counts when the attempt resumed or no client exists (then nothing was frozen this attempt); with earlier memory and no client → retryable `memory.context_unavailable`. Agents without a pack and without memory tools return before any read. Optional suggestion (resumed from the preparation result) not taken: under the claim's lease the plan read and the freeze cannot disagree (an earlier attempt's freeze is seen as `Frozen`; a stale owner is fenced at the freeze). | `memory_preparation_a_resumed_pack_without_a_configured_service_is_unavailable`: pack frozen by the recall runtime, resumed on a plain runtime (no `set_recall`) → (Retryable, `memory.context_unavailable`), 0 provider entries. Mutation K (no client → Valid) fails it: `agent.unresolved`. |
| F2 recorded tool items never revalidated | `revalidation()` also returns this execution's tool consumption items (invocation ≠ nil). `validate` checks the highest recorded exposure (pack and tool items) against the current ceiling (above → revoked), then the pack via `context.validate`, then the tool items via `observations:fetch` per scope in chunks of 64: absent id or changed revision → `memory.context_revoked`; outage → `memory.context_unavailable`. Mechanism and residual (tool-item provenance/expiry undetected) recorded in E7. The B3 tool fixture now prepares through a revalidating runtime (`Tools::pausing_tool_reads`, stand-in `ToolReadsOnly`: production persistence with recall planning off, so B3's consumption/level/call assertions stay about tool reads alone). | `memory_preparation_resumed_attempts_revalidate_recorded_tool_items`: a completed `search_memory` call, then the recall runtime: revision corrected since → (Terminal, `memory.context_revoked`), 0 provider entries; unchanged → completes with the tool result replayed, 1 provider entry; exactly one revalidation fetch each. Mutation J (tool items skipped) fails it: `agent.unresolved` ≠ revoked. B3 `memory_integration_agent_memory_details_and_timeline_stay_within_the_ceiling` and `memory_scope_user_memory_requires_the_actors_own_manual_session` now revalidate their earlier reads on the second preparation. |
| F3 E11 deadline and concurrent-recall cases | Two tests added (below). | `memory_preparation_a_recall_past_its_deadline_continues_without_memory`: pack held 3 s, recall deadline 1 s (request timeout 5 s) → run completes in < 2.5 s, frozen `unavailable{deadline}`, no recalled block, 1 provider entry. Mutation H (deadline = request timeout) fails it: 3.20 s. `memory_preparation_two_concurrent_recalls_in_one_company_both_succeed`: two three-scope recalls, packs held 700 ms: all six requests held at once within 650 ms while exactly 2 company slots are in use, both return 3 packs, peak 6, 0 slots after. Mutation I (permit per request) fails it: never six held (`Elapsed`). |
| F4 recall cap only in TASK | U6 (user, 2026-11-09): 8,192-token cap and fixed 64 KiB byte bound accepted; recorded in E12 and the E7 budget text (COORDINATION r3). | No code change. |
| F5 sequential revalidation outside the recall deadline (optional) | Not taken. It runs only on a claim restoring earlier memory (after an interruption or park), never on a fresh attempt or per tool round; its failure is already the retryable `memory.context_unavailable`, and each call stays bounded by `MEMORY_SERVICE_REQUEST_TIMEOUT`. A multi-scope validate operation would need a new adapter method for a rare path. | — |

Docs: `docs/workflow-operations.md` "Agent memory tools" now describes automatic recall (bound,
deadline, continuing without it) and resume revalidation (`memory.context_revoked` /
`memory.context_unavailable`); `docs/deploy.md` U5 paragraph adds that recall stops and a resuming
run with earlier memory retries `memory.context_unavailable`.

Evidence (R2), manifest `logs/MEM-P05-B4-R2.evidence.json`:
- Focused selection widened to every `WorkflowAgentRuntime` caller (adds `mcp_agent workflow_tool
  message_agent agent_recovery agent_model_step classify decision` to R1's): 931 listed
  (`MEM-P05-B4-R2-list.txt`), 929 passed / 0 failed / 2 pre-existing ignored
  (`MEM-P05-B4-R2-focused.txt`), private cluster `open_bots_root_test`.
- 2 MiB stack: `memory_tools:: knowledge_search agent_runtime mcp_agent agent_recovery`, 161 passed.
- clippy `-D warnings` (lib + tests), `cargo fmt --check`, `git diff --check` clean.
- Mutations H, I, J, K: isolated copy `tmp/memory-service-run/mut-b4r2` (own target, deleted
  after), each run on the final tree with `cargo test --lib -- memory_preparation`: exactly its
  target test fails, 12 of 13 pass (`MEM-P05-B4-R2-mutation-{H,I,J,K}.txt`, patches
  `…-mutation-<name>.patch.txt`).
- Not exercised: unchanged from R1 (live service, browser, full suite and stack-budget script at the
  05 gate; `skipped_budget` end to end).

### p05-B4 correction round R3 (root; recheck MEM-P05-B4-R2: F6 Minor, F7 optional)

| Finding | Resolution | Tests / evidence |
| --- | --- | --- |
| F6 deleted tool item untested | `memory_preparation_resumed_attempts_revalidate_recorded_tool_items` now runs three cases: corrected (revision 2), deleted (the fake no longer returns the id), unchanged. Revoked cases: (Terminal, `memory.context_revoked`), 0 provider entries, one revalidation fetch. | B's mutation L patch applied unchanged in an isolated copy (`mut-b4r3`, deleted): the test fails on "deleted" (`agent.unresolved` ≠ revoked), 31 of 32 pass (`logs/MEM-P05-B4-R3-mutation-L.txt`). |
| F7 docs: disable then resume (optional, taken) | `docs/deploy.md` (disable paragraph) and `docs/workflow-operations.md` (recall paragraph): a run already shown memory fails with `memory.context_revoked` when it resumes after memory was disabled. Verified against the code: `scopes_on` → `ready_scopes_on` yields no scopes for a disabled tenant, so the pack's or a tool item's scope is missing → revoked. | Docs only. |

Evidence (R3), manifest `logs/MEM-P05-B4-R3.evidence.json`: digest
8490e2e6c999eea62a7738e2e762b18c1fbda91cbec21f106612bc761d4f78ee, freeze tree 2a0d2550 (delta vs R2
tree ff1607fd: that test file and the two docs). `memory_preparation memory_tools::` 32 passed;
clippy -D warnings, fmt, diff --check clean. R2's widened selection (929/0) stands for unchanged code.

## p05-B5 — capture, generation intents, lifecycle races, phase-05 gate (author: root, Opus/xhigh)

Requirements: 05-openbot-integration.md 05B.5–7; decisions E2 (tenant FOR SHARE, epoch), E3
(ceiling, consumed), E8 (capture policy, generation intents, delivery), E9 (lifecycle order), E11
(`memory_capture_`), U3/U4. E8/E9 passed the p7a architecture gate (r4); no new gate unless a
decision below deviates from them. Reviewer B abdd866fb428eea05.

### Design (B5, from E8/E9; routine choices marked R)
- Schema `memory_agent_capture` (init schema, reset policy): PK (company_id, agent_id, kind), kind
  `private_sessions`|`threads` (E8 "each with"), `enabled`, `allowance_tokens` (R: 1–1,000,000),
  `max_output_tokens` 1–8,192, `expiry_hours` 1–168, `revision`, `changed_by` (user), `reason`
  (R: 1–500 chars), `changed_at`; FK agents ON DELETE CASCADE (agent final delete removes it).
  `the_memory_provider_schema_is_gone` allowlist gains it.
- Settings (owner/admin, company memory section): per agent and kind, enable/disable with the
  numbers and a reason, revision-checked; the cross-channel text shown on the page and on turning
  `threads` on; "generation off (operator cap 0)" already shown for the tenant.
- Capture at acceptance (`agent_history::accepted_on`, run row already locked by `lock_owner` →
  `lease::lock_fence`): only `RunState::Completed`; conversation `thread` → `threads` switch →
  agent scope; `private_session` with an unattached session → `private_sessions` switch → the user
  scope of `user_scope_owner_on` (E2/U4); `child_branch` and attached sessions never. Then tenant
  FOR SHARE (enabled, epoch, `generation_cap` > 0), policy row FOR SHARE (enabled), namespace FOR
  SHARE (ready); otherwise skip (no intent, traced reason). Request: events user = logical input,
  assistant = accepted output (canonical JSON text), access_level = run content level ≤ producing
  ceiling = run `knowledge_ceiling`, consumed = `consumed_for` (thread input inexact → unknown);
  key `ob.g.<exchange simple>`; expires_at = now + expiry, allowance and max output frozen; no
  session_id, no description, summary false; over the window bound → skipped (`too_large`).
  Excluded by construction: tool arguments/results, failed/invalid/cancelled runs, samples (to
  verify: samples never reach acceptance), internal notes, ineligible scopes.
- Provisioning (R): capture cannot provision inside acceptance (own transaction, P7b), and a
  non-ready namespace fails delivery (`namespace_unavailable`), so the capture target is
  provisioned at preparation: `MemoryRecall::step` asks persistence for a capture target whose
  switch applies and whose namespace is not ready, and provisions it (own transaction) before the
  recall plan; failures there are logged and never fail the run.
- Delivery: the claim decodes the frozen request by intent kind (save | generation);
  `MemoryClient::request_generation` (`generations/request`); `Forbidden{generation_disabled}` →
  terminal `failed(generation_disabled)`; otherwise the save classification, same key and body on
  every retry, the policy expiry kept. Contract fake gains `generations/request`.
- E9: disable fails pending intents of both kinds; agent delete / member removal cancel them
  (verify kind-agnostic); a switch change locks its policy row FOR UPDATE, so capture either
  commits first or sees it off.

### Verification plan (B5)
`memory_capture_`: per-agent switches (off → nothing; each kind only its own conversations);
eligible windows only (thread, unattached private session; not child, attached, failed, invalid);
request content (input/output only, levels, consumed unknown for threads, key, expiry, frozen
numbers); no duplicate (replayed acceptance, unique key); promised delivery (worker sends once,
retry replays the same body, generation_disabled terminal, expiry); competing delivery claimants;
delete vs delivery; settings form (owner/admin only, revision check, cross-channel text);
`memory_integration_disable_races_save_and_capture_then_reenable` (competing connections, both
orders, re-enable). Phase-05 gate: live-service tests, browser check of the settings section,
full suite + stack budget (or recorded as at 06C), each recorded.

### Verification mapping (B5)
| Requirement | Test | Decisive assertions |
| --- | --- | --- |
| E8 frozen generation request, one intent, replay, promised delivery | `memory_capture_freezes_one_generation_intent_and_delivers_it_once` | key `ob.g.<exchange>`; two events (user/assistant texts) at the content level; ceiling 6; consumed unknown (inexact) / exact(2); allowance 5,000, output 300, expiry 2 h; a replayed freeze returns the same id, one row; one `generations/request` call whose body equals the frozen body; result has the operation id |
| E8 retry is a replay | `memory_capture_a_retry_replays_the_frozen_request` | dropped connection → pending, attempts 0; next pass delivered `replayed: true`; one generation at the service; no contract violation |
| E8 terminal refusal, expiry | `memory_capture_generation_disabled_is_terminal_and_an_expired_intent_is_never_sent` | service cap 0 → `failed(generation_disabled)`, body dropped; past expiry → `expired` at claim, no call |
| U3/E8 gates | `memory_capture_switch_cap_readiness_and_bounds_gate_the_freeze` | never set / other kind / switched off → `off`; cap 0 or unknown → `generation_off`; unready scope → `not_ready`; content above ceiling → `above_ceiling`; over 512 KiB → `too_large`; disabled → `not_enabled`; no rows, no calls |
| E8 competing claimants | `memory_capture_competing_delivery_claimants_send_once` | two workers racing one intent: one `generations/request`, held peak 1, delivered |
| E8/E9 switch-off race | `memory_capture_switch_off_races_the_freeze_in_both_orders` | freeze first: the switch-off waits (blocked), the intent stays pending and delivers; switch first: the freeze waits and answers `off`, no rows |
| E9 agent delete race | `memory_capture_agent_delete_races_the_freeze_in_both_orders` | freeze first: delete waits, then `cancelled(owner_removed)`, body dropped; delete first: freeze waits, `not_ready`; policies cascade away; nothing sent |
| E9 lock order, interleaved | `memory_capture_agent_delete_interleaved_with_a_freeze_does_not_deadlock` | delete holds agent + retired namespace; the freeze blocks on the namespace; the delete's `principals` demotion and `DELETE FROM agents` (the rest of `finish_agent_delete_on`, run as its statements) complete without waiting; the freeze answers `not_ready`; no rows. Mutation M2 (policy locked before namespace) fails it: `deadlock detected` |
| E9 disable races save and capture, re-enable | `memory_integration_disable_races_save_and_capture_then_reenable` | order 1: both intents `failed(disabled)`; order 2: save `NotEnabled`, capture `not_enabled`; after re-enable nothing delivered, no save or generation call |
| E8 end to end, private session | `memory_capture_an_accepted_private_session_exchange_is_captured_for_its_creator` | production runtime + acceptance: one intent into the creator's user namespace; event 0 = the exchange's frozen `context`, event 1 = the accepted output; ceiling/consumed/level from the run row; the tool round's argument marker absent; delivered once, body equal |
| E8 provisioning at preparation | `memory_capture_a_switched_on_target_is_provisioned_at_preparation` | agent without memory tools or recall: user namespace requested (provisioning) by preparation; no intent; no agent namespace |
| U3 off by default | `memory_capture_an_agent_without_the_switch_captures_nothing` | ready user namespace, cap > 0, no switch → no intent |
| E8 thread capture level | `memory_capture_a_thread_exchange_is_captured_into_the_agent_memory_at_the_thread_level` | customer email thread: one intent into the exchange agent's namespace; producing ceiling 0, consumed unknown, events level 0; frozen policy numbers |
| E8 child branch never | `memory_capture_a_child_branch_is_never_captured` | both switches on: exactly the two private-session exchanges of the root captured (user scope); no child text in any request |
| E8 settings | `memory_capture_settings_form_is_owner_admin_only_revision_checked_and_states_its_reach` | refused before enable; page lists the agent's two policies and both reach texts and the idle note; member/stranger/cross-company denied; threads on repeats the cross-channel text in the status; stale, blank reason, out-of-range and unknown kind refused; recorded actor and reason |
| Classification | `memory_integration_delivery_classifies_every_service_answer` | `Forbidden{generation_disabled}` → `Failed(generation_disabled)` |
| Schema | `the_memory_provider_schema_is_gone` | exact table/column allowlist incl. `memory_agent_capture` |

Stand-ins: the contract fake for the service (generation acceptance only; no generation worker);
the thread and child tests seed tenant/namespace/policy rows directly (acceptance reads only those);
the freeze-level tests use synthetic run ids with explicit run facts (as the save tests do).
Not exercised: a live service generation (05 gate live tests), browser rendering (05 gate).

### Mutation probes (B5)
Isolated copy `tmp/memory-service-run/mut-b5` (own target, deleted after), `cargo test --lib --
memory_capture_ memory_integration_delivery_classifies`, logs `logs/MEM-P05-B5-R1-mutation-M<n>.txt`,
patches `…-mutation-<name>.patch.txt`:
- M1 a switched-off policy still captures → the gate test and the switch-off race fail.
- M2 policy row locked before the namespace → the interleaved delete test fails (`deadlock
  detected`). It first survived every test (both-orders races never interleave); the interleaved
  test was added for it, and M1/M2 were rerun on the final tree.
- M3 child branches captured like threads → the child-branch test fails (a `child_branch` → agent
  capture appears).
- M4 capture targets never provisioned at preparation → the provisioning test fails.
- M5 `generation_disabled` retried → the terminal test (row stays pending) and the classification
  unit test fail.
M3–M5 ran on the tree before the interleaved test was added (their targets unchanged since).

### Evidence (B5)
Manifest `logs/MEM-P05-B5-R1.evidence.json` (6 PASS / 2 NE): digest
be48350f22f124546d81d9d9abc175039d0c971b8580c817ac32fe103629a5c3, freeze tree 8f5f0f97 (delta vs B4
tree 2a0d2550: 29 files, `git diff 2a0d2550 8f5f0f97 -- . ':(exclude)plan/memory-service/phases'`;
`src/main.rs` and COORDINATION unchanged). Focused 1180 listed: 1173 passed, 1 failed
(pre-existing `rendered_markup_carries_no_inline_javascript`, `workflow_authoring/sample.rs:153`,
unchanged since base), 6 pre-existing ignored; the pre-fix run's `no_page_reaches_for_a_class_daisyui_5_removed`
failure was this batch's `input-bordered`, fixed. 2 MiB stack 173 passed; clippy -D warnings, fmt,
diff --check clean; mutations M1–M5 each fail their targets. Test DB `open_bots_root_test` reset
after the schema edit. Not exercised (B6 gate): live service, stack-budget script, boundary
scripts, credential rotation, `sqlx prepare --check`, browser; full suite at 06C.

### p05-B5 correction round R2 (root; review MEM-P05-B5-R1, findings F1–F4)

| Finding | Resolution | Tests / evidence |
| --- | --- | --- |
| F1 docs said message-triggered and scheduled runs are never captured | `docs/workflow-operations.md` and the settings page now say private-session capture never covers scheduled, message-triggered or child runs, while thread capture covers every completed exchange of the agent in a channel thread, whatever started the run (customer messages included); child branches, failed/cancelled runs and samples are never captured. | Settings test asserts the three corrected sentences. |
| F2 capture kept queuing with the URL removed | User ruling U7 (2026-11-10): skip capture while unconfigured. `PreparedAgent.capture` (serialized only when true) is armed by `with_capture` at the first freeze from `MemoryPreparation { recall, capture }`; the agent runtime passes `capture: self.recall.configured()`; `restore_on` re-binds the saved flag before comparing; `accepted_on` captures only when the frozen preparation carries `capture: true`. Residual (in U7): a run prepared while configured and accepted after removal can still queue one. Recorded in E12 U7, COORDINATION r4, `docs/deploy.md` (U5 paragraph and capture paragraph), operations guide. `capture_targets` now checks readiness by state only (no credential opened). | `memory_capture_nothing_is_captured_without_a_configured_service` (plain runtime: frozen preparation has no `capture`, no intent, everything else ready); the private-session test asserts `capture: true` frozen by the configured runtime; the thread test now runs `Deployment::with_memory` (configured engine; its own delivery worker sends to the fake; asserts the received body); the child test composes a configured recall in `AgentSetup.recall` and prepares its direct preparation with `capture`. |
| F3 `validate`'s doc comment on `provision` | Moved back; `provision` documented. | — |
| F4 delete vs delivery for capture intents | Added a generation case. | `memory_capture_agent_delete_during_an_in_flight_delivery_loses_its_completion`: agent delete while the capture is held at the service → `cancelled(owner_removed)` at once, body dropped; the worker's completion is fenced out (stays cancelled, no result). |

Evidence (R2), manifest `logs/MEM-P05-B5-R2.evidence.json` (6 PASS / 2 NE): digest
6041e394d913523eb06c8aaac8348c675134643e8947fe1281a6a20eca9e4f1e, freeze tree 8e18b451 (delta vs R1
tree 8f5f0f97: 22 files). Focused 1182 listed: 1175 passed, 1 pre-existing unrelated failure, 6
pre-existing ignored; capture/recall/child rerun on the final tree 30 passed; 2 MiB stack 174
passed; clippy -D warnings, fmt, diff --check clean. Mutations M6 (acceptance ignores the flag) and
M7 (every runtime arms) each fail the U7 test (copy `mut-b5r2`, deleted).

## p05-B6 — phase-05 gate (E11) (author: root, Opus/xhigh)

Scope (E11 05 gate): `memory_integration_live_` `#[ignore]` tests run by `scripts/memory-integration.sh`
with a CI step; all five prefixes on the final tree; `scripts/stack-budget.sh`; boundary scripts;
credential rotation; `sqlx prepare --check`; browser check of the settings memory section; B5
optional F5 (doc comment).

### Implementation (B6)
- `scripts/memory-integration.sh`: builds the service in `services/memory`, provisions a throwaway
  `memory_live_*` database and role with `provision-db.sh` on `MEMORY_INTEGRATION_ADMIN_URL`
  (required; no default server), `admin bootstrap` → operator, `admin-credentials/issue` →
  provisioner, `serve` on 127.0.0.1 with `scripts/memory-integration-fake-messages.py` as the
  Messages API, then `cargo test --lib memory_integration_live_ -- --ignored` requiring a nonzero
  count and that the stand-in was called; the EXIT trap (`set +e`) stops both processes and drops
  the database and role.
- `src/adapters/persistence/memory/live_tests.rs` (2 tests): (1) enable → reconciler provisions
  tenant + company/agent namespaces with real credentials; a frozen save delivered; search (once
  indexed), fetch (absent under a lower ceiling), pack and validate; the operator raises the cap,
  the documented disable/enable refreshes the cached cap; a capture delivered as a generation
  request, the service's worker calls the stand-in and the generated memory (found by search,
  fetched) is labelled at the producing ceiling; disable → tenant disabled remotely, reads refused
  `tenant_disabled`, a new freeze `NotEnabled`. (2) two delivery workers racing four intents of one
  company: each delivered exactly once (`replayed: false`), one observation each.
- CI: new `memory-integration` job in `.github/workflows/ci.yml` (pgvector image pinned as in
  `memory-service.yml`, model cache + `prepare-model.py --download`, `scripts/memory-integration.sh`).
- Live findings fixed: (a) **a defect in accepted B5 code**: both capture events shared one
  `(source_ref, revision)` with different texts, which the service refuses (409, "one source
  revision carries different content"); every real capture would have failed. Each part now has
  its own source (`openbot.exchange.<id>.input|output`); the contract fake enforces the same rule;
  the freeze test asserts distinct sources. (b) the live test's company-scope label expectation was
  wrong (U2: company scope is inexact → ceiling). (c) the script's EXIT trap aborted under `set -e`
  on a killed child's `wait`, leaving the database; fixed and verified (nothing left behind).
- F5: `configured()` / `unconfigured` doc comments in order.
- Browser check: `scripts/tests/memory-settings-browser.mjs` with fixtures from
  `company_memory::browser_fixtures::dump_memory_settings_browser_fixtures`; CI step in the verify
  job. The first screenshot showed labels detached from their fields (inline flow); the capture
  fields are now labelled blocks in a two-column grid (classes already in `app.css`, so the
  generated CSS stays current).

### Verification mapping (B6, E11 05 gate)
| Gate item | Evidence | Decisive result |
| --- | --- | --- |
| Live tests + script + CI | `memory_integration_live_provisioning_save_recall_capture_and_disable`, `memory_integration_live_competing_delivery_workers_deliver_each_intent_once` via `scripts/memory-integration.sh`; CI job `memory-integration` | 2 passed on the final tree; save labelled 6 (company scope inexact, U2); fetch absent under ceiling 1; pack item `Valid`; cap refreshed to 100,000 by disable/enable; capture delivered, generated memory labelled 6 contains the stand-in's marker; after disable `tenant_disabled` and `NotEnabled`; racing workers: each of 4 intents delivered once; nothing left behind |
| Five prefixes | `logs/MEM-P05-B6-R1-prefix-counts.txt`, full suite | listed 43/25/53/13/16 (+2 live); all pass in the 2 MiB lib suite (live ignored there) |
| Stack budget | `scripts/stack-budget.sh` | 3689 passed, 3 pre-existing unrelated failures, no overflow |
| Boundaries | five boundary scripts | pass except the pre-existing transport offender (unchanged since base) |
| Credential rotation | rotation script + DB tests in the suite | pass |
| `sqlx prepare --check` | fresh `open_bots_mem_prepare` | migrate and check pass |
| Browser | `memory-settings-browser.mjs` | escaped name, all fields labelled, blank reason not posted, posted policy fields exact, saved section swapped in, both themes readable and distinct |

### Evidence (B6)
Manifest `logs/MEM-P05-B6-R1.evidence.json` (8 PASS / 2 NE): digest
88808f5ad06857e86e417fe9593fd6b72185bdc01b42ad52871054c77e81b0a1, freeze tree 7388d1c2 (delta vs B5
tree 8e18b451: 11 files). Not exercised: CI execution of the new job/step (no pushes), the
all-target full suite (06C). Pre-existing unrelated failures unchanged: inline script in
`workflow_authoring/sample.rs:153`, `security_auth_limits_*` (C locale), transport offender
`channel_reply_continuation_views_support.rs:59`.

## Phase 05 status
**Phase 05 accepted in phase (2026-11-10)**: batches p05-B1…B6 each accepted by reviewer B with
root-recomputed digests (final 88808f5a…b0a1, tree 7388d1c2). Not integrated (no commits are
authorized). Carried into phase 06: B6-F1–F3 (optional), CI execution of the
`memory-integration` job and the browser step, the all-target full suite and Docker checks (06C),
earlier deferred F10/F16, and the 06A items recorded in implementation-decisions.md.

