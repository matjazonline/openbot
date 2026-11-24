# AWC-P06 — Integration and final verification

## Identity

- Plan `plan/agent-workflow-call`, phase `p06`, doc [06-integration-and-final-verification.md](../../../06-integration-and-final-verification.md).
- Workflow implement-opus-reviewed-2_4; records at
  `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/agent-workflow-call/phases/p06/docs`.
- Root/A: Claude Code `1e84a7af`, Opus 5.5/xhigh (user-confirmed). B: a Fable/medium reviewer, per **U9**
  (2026-11-23, verbatim: "continue and use fable for review"). The p05 substitute B' `a4f44ca532412935f` is
  superseded for p06. The `fable` alias may resolve to Fable 5.1, so the reviewer reports its exact model, and
  that model is recorded in REVIEW.md.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD `fe7791d3`.
  - Preserved pre-existing work: p01–p05, accepted in phase and uncommitted. p05 was accepted at
    `905b73f8…8d5a`.
  - Starting digest, with p06 records excluded:
    `5d818a95864dc2f2271f12edaaa51fa70ca87713868c089be5c4e63aa7f7ef70`.
- Prerequisites: p01–p05 accepted in phase (COORDINATION r23).
- Coordination r23. Applicable:
  - plan-wide r1, r2, r15 (U1–U5) and r22 (U8);
  - AWC-C005 r1 targets p06 (pending).
- Resources: scratch DBs `openbot_awc_mac02`, `openbot_awc_mac02_test`. The final gate (doc 06 B) uses fresh
  disposable databases.
- Snapshot exclusions: `plan/agent-workflow-call/phases/p06/docs`, `.trail/learnings`.
- Startup plan paths: `src/main.rs` `NO_BACKWARD_COMPATIBILITY_NOTES` already lists all seven plan files.

## Status

- 2026-11-23: phase started. Coordination checked at r23. Investigation: a read-only mapping of doc 06 A scenarios
  1–9 to existing p01–p05 tests is in progress.

## Investigation (2026-11-23)

A read-only agent mapped doc 06 A scenarios 1–9 to the p01–p05 tests. The full report is retained in the session
transcript. Its fixture findings:
- the default destination always replies with the literal "Done";
- no call test inspects the scripted model's request (`scripted_llm` checks method, path and model only);
- the caller agent also holds a workflow-tool binding (the destinations are not registered tools).

Gaps, by priority (each becomes a test, or a recorded disposition):

| # | Scenario | Gap |
| --- | --- | --- |
| G1 | 1 | One execution: B `done`, then C (another version/params) `done`, then the final answer. Distinct bodies asserted inside the model's later requests. Neither destination registered as a workflow tool. |
| G2 | 3 | Edited approval reaches the caller as the edited body; target `decision.human` resolved through to the caller; parent ingress reply still published while its call child answers `no_reply`; source reply followed by settlement. |
| G3 | 6 | Self-call and A → B → A through the real port (code and zero rows); nested calls cannot reset the tree budget, and depth/count limits; a poison wakeup does not monopolize a poll batch. |
| G4 | 4 | Two consumptions of one result racing; launch racing revocation through `call_channel`; launch racing a competing worker after lease loss. |
| G5 | 5 | Restart after review settlement and after the stored fact; `retry_count` asserted. |
| G6 | 2 | Target with agent plus static/registered child settling the caller; skills/resources/agent revision capture; a deterministic later call selecting current configuration; agent republished while waiting. |
| G7 | 7 | Non-operations caller; resource revocation; a foreign-company desk through `call_channel`. |
| G8 | 8 | Company teardown; stored-result cascade; waits and manifests at retention. |
| G9 | 9 | Rerun `workflow_tool_`, `workflow_child`, `channel_reply_`, `message_`, `agent_history`, `sample` at the integrated state (the final gate's full suite covers this). |

Batches: **A1** = G1–G3, **A2** = G4–G5, **A3** = G6–G8, each reviewed as one batch by B'. Then **B** = the final gate.
No architecture gate is planned: Batch A adds tests, and any production fix found will be gated if it changes a
contract.

## Batch A progress (checkpoint 2026-11-23; implementation in progress, not frozen or reviewed)

Plan revision: Batch A is test-only, so A1–A3 get **one** batch review after all of them (fewer, larger
artifacts), instead of three. The reviewer is Fable (U9).

Done (all pass on `openbot_awc_mac02_test`: `cargo test --offline --lib -- channel_call_integration_
channel_call_resume_restart_at_each_boundary channel_call_response_reply_to_source` → 7 passed):

- New `src/adapters/persistence/workflow/channel_call_integration_tests.rs` (mounted in `message_tests.rs`):
  - `channel_call_integration_two_desks_in_one_execution` covers G1 plus G2's edited approval and its human
    decision resolved through to the caller. One agent execution calls desk B, whose `decision.human` step the
    reviewer edits and whose contract takes `/input/data/body`, then desk C (another version, params and intake).
    It consumes both and finishes. The model's 2nd and 3rd requests carry B's edited body (never the draft) and
    C's body. There are no workflow-tool registrations, two `done` results with distinct bindings, and 3 model
    turns.
  - `channel_call_integration_cycles_and_nested_budget` covers G3. A self-call answers `channel_call.cycle` with
    0 records. A → B → A: B's agent, which is the caller's own agent workflow as B's responder, sees `cycle` in
    its next request, and nothing is recorded for A. B's legal nested call to C has its budget root equal to the
    caller's root.
  - `channel_call_integration_restart_after_review_settlement` covers G5. Fresh persistences between approval and
    finalization and before the wakeup: the poller finds the Wait, the model sees the approved body, and there is
    1 record, 1 result, 1 child and `retry_count` 0.
  - `channel_call_integration_competing_consumptions` covers G4: two `call_channel` replays of one result race;
    one stored result, every answer is `done(child)`, absorbed once.
  - `channel_call_integration_launch_races_revocation` covers G4 over 3 rounds of launch against grant revocation
    through the port: either Parked with a whole record and child, or `target_unavailable` with no rows.
- Extended `channel_call_response_reply_to_source` (G2, source reply then settlement), with a new helper
  `committed_step` that claims, executes and `complete_io`s one step.
- Extended `channel_call_resume_restart_at_each_boundary`: `retry_count` = 1 after the one lost attempt (G5).

Finding: a final-reply review accepts only approve/reject with `{}` data (`decision_approvals::record_choice`). An
"edited approval" for a call is therefore a destination's own `decision.human` edit, as in the native template.
G1 proves that path.

Planned dispositions, for B' to judge:
- G2 "parent response suppression independent": building an ingress caller with its own reply settings is a large
  fixture. Suppression is held per run (`channel_reply_decisions` keyed by run).
- G3 "poison wakeup": call waits use the generic wait poll, which
  `workflow_fairness_real_loop_mixes_full_unsupported_poison_wait_hung_and_refill` covers; a poisoned response
  settles as `channel_call.response_invalid` (resume outcomes) rather than looping.
- G4 "launch vs competing worker after lease loss": covered by `launch_competing_claimants_converge` and
  `launch_lease_expired_at_commit`.

A3 (done 2026-11-23), in `channel_call_integration_tests.rs`:
- `channel_call_integration_pinned_while_waiting_later_call_current` covers G6. While call 1 waits:
  - the desk's responder is reselected to a version whose run calls a static `workflow.call` leaf, under another
    contract ("Delegated");
  - the calling agent is republished with a new prompt, keeping its bindings.

  Call 1 keeps its binding, settings revision 1, agent revision, version and contract, and consumes
  `done(child)` with body "Done". Call 2 records the new binding, settings revision 2 and the new agent revision.
  Its child runs the new version and its grandchild the leaf version. The leaf's terminal fact completes the
  static step (`resume_wait` → `Completed`), and the caller consumes the body "Delegated".
- `channel_call_integration_member_caller_and_unavailable_desks` covers G7:
  - a foreign desk, a missing desk and a members-only desk without a grant each answer byte-identical
    `target_unavailable` through `call_channel`, with no desk rows and no call rows;
  - a non-operations caller (a run an admin admitted, demoted to member by `update_member_role` before its step)
    is refused by its agent step with `agent.rejected` before the model is asked: 0 model requests, no call rows;
  - the positive control is the owner's run: it launches, its child runs as the desk's `run_as`, and it consumes
    `done`.
- `channel_call_integration_retention_and_teardown` covers G8.
  - Retention: a tree whose caller consumed its result has 1 record and 1 stored result, a call wait and the
    child's scope manifest. After age and `retire_tree`, all call rows, manifests (caller and child) and waits
    are 0.
  - Company teardown with a call still waiting (`CompanyPersistence::delete`): call records, results, waits,
    manifests, reply runs, runs, the desk thread and the source message are all 0. The immutability guard never
    blocks it.
- `channel_call_response_tests.rs`: `call_rows`, `age`, `retire` and `publish_all` became `pub(super)` for reuse.
  There is no behavior change.

Findings while writing A3 (no production change):
- **A binding cannot run as a plain member** ("The binding's run-as must currently manage its unit"). A run's
  actor is its admitting person, and admission needs operations authority. A non-operations caller therefore
  exists only by demotion after admission, and the agent step rejects it.
- **Republishing an agent with regenerated binding IDs revokes a waiting call**: publication revokes grants on
  removed bindings, and the consumption check (`still_authorized_on` → `check_grants_on`) answers
  `channel_call.revoked`. That is the intended capability-revocation path (`channel_call_resume_revoked`
  `Capability`). An edit keeping its bindings stays pinned (G6).

More dispositions, for the reviewer to judge:
- G6 "skills/resources capture across dependency scopes": the call record pins `agent_manifest_id` and
  `agent_revision_id`, and the child's own `scope_manifest_id` (FK to `run_dependency_manifests`). Skill and
  resource capture inside manifests is the existing `autonomous_agent_storage_manifest_` contract (final gate,
  G9). A destination with its own agent is G3's A → B → A.
- G7 "resource revocation": p02 V6 found it is not a separate case. The destination's resources are checked by
  admission of the call child under the desk's `run_as`, and a refused admission leaves no rows
  (`channel_call_launch_refusal_leaves_no_thread_or_message`).
- G7 "restrictive knowledge audience, hidden history": p02/p03 tests `channel_call_launch_history_is_the_fresh_thread`
  with `channel_call_snapshot_audience_refused_above_ceiling` and
  `channel_call_resume_destination_audience`, cover them.

A checks (2026-11-23, `openbot_awc_mac02_test`):
- `cargo fmt --check`: clean.
- `cargo +1.95.0 clippy --offline --locked --all-targets -- -D warnings -A clippy::redundant_field_names -A
  clippy::nonminimal_bool`: exit 0 (`logs/a-clippy.txt`).
- `cargo test --offline --locked --lib -- channel_call_`: 119 passed (`logs/a-channel-call-tests.txt`).
- `bash scripts/stack-budget.sh --offline -- channel_call_`: 119 passed at the stock 2 MiB
  (`logs/a-stack-budget.txt`).

Frozen for review AWC-P06-A-R1: base `fe7791d3`, digest
`d3bee2a425e9d3508223741ca231608e8c9336715dbece7ca497f5d061c8576f`. Exclusions:
`plan/agent-workflow-call/phases/p06/docs`, `.trail/learnings`. Manifest `logs/AWC-P06-A-R1.evidence.json` is
validated (3 PASS).

Batch A changed files since p05 acceptance:
- `src/adapters/persistence/workflow/channel_call_integration_tests.rs` (new);
- `src/adapters/persistence/workflow/message_tests.rs` (mount);
- `src/adapters/persistence/workflow/channel_call_response_tests.rs`: `committed_step`, the
  `channel_call_response_reply_to_source` extension, and the `pub(super)` helpers;
- `src/adapters/persistence/workflow/channel_call_resume_tests.rs` (`retry_count`).

## AWC-P06-A-R1 corrections (2026-11-23)

Review AWC-P06-A-R1 (Fable 5.2 `claude-fable-5-2`, agent `a34d3fab3f32abacc`, note AWC-P06-A-R1-N1) found
F1–F3 actionable and F4 optional. All are handled in one round:

- **F1 (G2 parent response suppression), fixed with a test.**
  `channel_call_integration_nested_no_reply_keeps_parent_response`:
  - desk B's responder is the `agent.run` example workflow, and its agent calls desk C, whose run answers
    `channel.no_reply`;
  - C's decision is `suppressed`, and B's agent receives `{"closure":"no_reply","child_run_id":C}`;
  - B's agent step commits and B's run ends `succeeded` with its own decision `automatic`/`done`;
  - the caller consumes `done(B)` with B's contract body.
- **F2, fixed.** The "neither destination is a workflow tool" query now names B (`DESTINATION + 2`) and C
  (`DESTINATION + 1`). A positive control asserts that the same query counts the agent's own registered tool
  (`CHILD`) as 1.
- **F3, record corrected.** These are separate NOT EXERCISED entries in manifest R2; the reviewer accepted each in
  substance:
  - G3 poison wakeup: no test poisons a call wait's wakeup. The earlier citation
    (`workflow_fairness_real_loop_mixes_full_unsupported_poison_wait_hung_and_refill`) poisons a job's input with a
    healthy timer wait, so it does not show the claim; the citation is withdrawn. Mechanism: a call wait is an
    ordinary `workflow_waits` row resumed by the shared poll, and a malformed or oversized answer settles once as
    `channel_call.response_invalid` / `tool_result_too_large` (`channel_call_resume_outcomes`) rather than
    retrying.
  - G3 depth bound: `MAX_ANCESTRY` (1000, `channel_call_target.rs`) is a fail-closed cap: a chain that deep is
    treated as a cycle. Reaching it needs a forged 1000-run chain, so it has no test. The tree limit that does
    bound real trees is exercised at launch (`channel_call_launch_continuation_limit`, `TOOL_LIMIT`).
  - G6 skill/resource capture at a call scope: no call test holds a skill or a resource. The call pins
    `agent_manifest_id`/`agent_revision_id` and the child's `scope_manifest_id` (G6 test). Capture inside
    manifests is the kind-independent `autonomous_agent_storage_manifest_` contract (final gate).
  - G7 resource revocation: no call test holds a resource. The destination's resources are checked when the call
    child is admitted under the desk's `run_as`, and a refused admission leaves no rows
    (`channel_call_launch_refusal_leaves_no_thread_or_message`).
- **F4 (optional).** The `cargo fmt --check` output is now retained (`logs/a2-fmt-check.txt`). Not taken:
  - the race-order observation, because both outcomes are valid and asserted whole, and forcing an order would
    need a production hook;
  - desk B's intent/delivery counts, because `DeskRows` counts those company-wide, and the reply publisher is not
    run in that test.

Helpers `no_reply_destination` and `settled` in `channel_call_response_tests.rs` became `pub(super)`, with no
behavior change.

Checks (2026-11-23, `openbot_awc_mac02_test`):
- `cargo fmt --check`: exit 0 (`logs/a2-fmt-check.txt`).
- clippy 1.95 (same flags): exit 0 (`logs/a2-clippy.txt`).
- `cargo test --offline --locked --lib -- channel_call_`: 120 passed (`logs/a2-channel-call-tests.txt`).
- `bash scripts/stack-budget.sh --offline -- channel_call_`: 120 passed at 2 MiB (`logs/a2-stack-budget.txt`).

**Batch A accepted in phase (2026-11-23)** at digest `c7fba3400cd27117204379ea11848105f5d96846018aa35d3b27a3bef9d97431`.
- Fable 5.2 PASS, AWC-P06-A-R2-N1, with no open findings.
- Root recomputed the digest and it matches. COORDINATION r25 is unchanged during review.
- Manifest R2 is valid. Its four NOT EXERCISED dispositions are accepted by B.
- Reviewer's caveats:
  - the parent in F1 is a call owner, not an ingress owner;
  - B's agent step there is committed with a literal output, not a model turn.

Remaining:
1. ~~Review/recheck AWC-P06-A~~ (accepted).
2. ~~fmt, clippy, focused rerun, stack budget, freeze, manifest~~ (done above).
3. Batch B: in progress, see "Batch B — final gate" below. Then fill in doc 06's completion
   record.

## Batch B — final gate (2026-11-24)

### B1. Full local gate

Command: `DATABASE_URL=postgres://mac02@localhost:5432/postgres bash scripts/check.sh`. The script creates and
drops its own `openbot_check_<uuid>` database, migrates it fresh and checks the SQLx metadata. Output:
`logs/b-check-sh.txt`, exit 1. Each gate's result, as the script printed it:

| Gate | Result | Classification |
| --- | --- | --- |
| Runner test coverage; local-agent plugin tests; generated CSS; frontend protocols; deployment scripts; knowledge preparation tests; Doop runner boundaries; Doop probe protocols; formatting; patch whitespace; workflow sample boundary tests and gate; integration boundary tests, gate and docs; evidence validator fixtures; retained workflow evidence; locked fetch; offline compilation; test database; migrations; SQLx metadata; compile tests; conformance families; ignored browser fixture producers; browser protocols (19/19); lease budget (139 passed); memory model preparation; memory backup/restore; OpenBot and memory integration (7 passed) | PASS | — |
| Reproducible plugin package | FAIL | Environment: `package.mjs` requires Node `v24.21.0` exactly; the machine has v26.13.1 (and v24.22.3 via Homebrew). Unavailable. |
| Transport abstraction boundary | FAIL | Environment: `rg` is a shell function of the Claude Code shell, not a binary, so the `bash` gate could not find it. **Rerun with `rg` on PATH: PASS** (`logs/b2-transport-boundary.txt`). |
| Clippy (`cargo clippy --locked --all-targets -- -D warnings`, local default toolchain rustc 1.99.1) | FAIL | Pre-existing: 9 `redundant_field_names` errors, all in files this plan never changed (`git status` is clean for every one): `routes/knowledge/scope_tests.rs` ×2, `knowledge/review_scope_tests.rs`, `ui_department_knowledge_tests.rs`, `delegated_administration_tests.rs`, `knowledge/scope_test_support.rs`, `workflow/knowledge_scope_fixture.rs`, `run_scope_child_tests.rs`, `run_scope_native_tests.rs`. The plan's own clippy runs (pinned 1.95 with the repository's two `-A` lints) are recorded per batch. See B3 for the 1.99 run over this plan's code. |
| Query statistics prerequisites | FAIL | Environment: the local server does not preload `pg_stat_statements` (`pg_stat_statements.track` unrecognized). The gate reads server settings and never changes them; changing the user's server was not done. Unavailable. |
| Database-backed suite (isolated network), `--all-targets` | FAIL: 5146 passed, 4 failed, 30 ignored | See B2. |
| Stack budget (2 MiB, isolated network) | FAIL: 5147 passed, 3 failed, 30 ignored; no stack overflow in the log | See B2. |
| Knowledge, memory, attribution production images | FAIL | Environment: no Docker daemon. Unavailable, not passed. |
| Memory extracted service suite; memory standalone journey; attribution extracted service; attribution browser integration | FAIL | Environment: `/usr/bin/python3` (CommandLineTools) has no `tomllib`. See B3 for the rerun. |
| Managed Doop service and private integration | FAIL | Environment: requires `DOOP_TEST_UI_CHROME` and `docker pull`. No Docker: unavailable. |

### B2. Test failures in the full suite

1. `desk_wording_page_sources_keep_channel_out_of_text_nodes` (both runs) — **regression from this plan, fixed.**
   p05 put the word "channel" in page text: the run page's `<h3>Channel calls</h3>`
   (`pages/workflow_runs/calls.rs`) and the authoring label `Desk id for <code>channel.call</code>`
   (`pages/workflow_authoring/channel_response.rs`). The product word is desk (plan 01-departments,
   decision 2). Fix:
   - the heading now reads "Desk calls";
   - the label now reads "Desk id for the call tool", matching the help line below it ("An agent with the call
     tool …");
   - ids, `for=` and `data-*` are unchanged;
   - the browser checks `scripts/tests/workflow-run-calls.mjs` and `channel-response-authoring.mjs` now look
     for the new names;
   - `docs/workflow-operations.md` "Inspecting calls" names the heading **Desk calls**. Its section title
     "Channel calls" is operator documentation and stays, like the `channel_calls_*` metrics.
2. `doop_agent_current_preparation_reaches_ready_before_model_io` (both runs) — **pre-existing, unrelated,
   left unchanged.** Recorded as unrelated by p02, p03 and p04 already.
   - It asserts the prepared document's `version` is the literal 7.
   - At base `fe7791d3` the constant was already `PREPARED_AGENT_VERSION = 8` (raised in `2edd0a73`), and the
     test is unchanged since `cb46c6fd`. So the test failed before this plan (8 ≠ 7).
   - This plan raised the constant to 9 (p05, D2 desk listing), which changes only the left side of an already-failing
     assertion.
3. `doop_agent_observe_policy_keeps_error_observation_available_to_the_model` ("document timed out") and
   `doop_agent_published_editor_approvals_and_worker_restart_reuse_one_canvas` ("scenario lifetime
   exceeded") — **timing under load.**
   - They failed only in the 57-minute full run.
   - They passed in the stack-budget full run and in the isolated rerun (`logs/b2-focused-rerun.txt`).
4. `agent_history_e2e_process_crash_before_successor_wakeup_recovers_by_poll_without_releasing_successor`
   failed only in the stack-budget full run: the child process reported FAILED after `AH_PROCESS_PREPARED`,
   with the child's panic "the actual worker must reach the required crash barrier" (`agent_history_process_driver.rs:117`, log `b-check-sh.txt:9861`): the child returned without reaching the barrier, so neither a crash nor a stack overflow.
   - It passed in the main full run (16 MiB), in the isolated rerun, and twice alone at 2 MiB
     (`logs/b2-crash-stack-budget.txt`).
   - Not reproduced. Because this plan changed agent-history preparation (`agent_history/preparation.rs`,
     `frozen.rs`), it is **not** claimed unrelated. It stays an unresolved intermittent verification gap.

### B3. Reruns of the environment-blocked gates (scratch PATH)

The PATH adds `rg` (a wrapper around Claude Code's bundled ripgrep), `python3` → Homebrew 3.13.18, and `node` →
Homebrew node@24 v24.22.3. Each gate ran with `check.sh`'s own variables (`SQLX_OFFLINE=true`, admin URLs for
memory and attribution). Runner: the session's `scratchpad/env-gates.sh`.

| Gate | Result | Evidence |
| --- | --- | --- |
| Transport abstraction boundary | PASS (regression tests + gate) | `logs/b2-transport-boundary.txt` |
| Memory extracted service suite | PASS (198 s) | `logs/b2-env-gates.txt` |
| Memory standalone journey (stand-in provider) | PASS (58 s) | same |
| Attribution extracted service and contracts | Not completed: 120 service tests passed, then `scripts/verify.py` needs the Python module `jsonschema`, which is not installed. Unavailable; nothing was installed. `services/` is untouched by this plan. | same |
| Attribution browser integration | Its own 2 journeys PASS. The OpenBot-side `attribution_sso_browser_real_openbot_password_and_existing_session` FAILS reproducibly at stage "independent secure cookies" (`services/attribution/browser/sso.mjs:30–36`: `__Host-attribution` cookie attributes on `127.0.0.1`). **Unrelated by code path:** this plan's diff from `fe7791d3` has no cookie, `__Host`, SameSite or SSO line, and `services/` is unchanged. | same |
| Reproducible plugin package | Unavailable: requires Node v24.21.0 exactly. | `logs/b-check-sh.txt` |
| Query statistics prerequisites | Unavailable: the server lacks `pg_stat_statements` preloading. | same |
| Knowledge/memory/attribution images; managed Doop | Unavailable: no Docker daemon (Doop also needs `DOOP_TEST_UI_CHROME`). | same |

Other Batch B checks, after the wording fix:

- **Focused rerun** (`logs/b2-focused-rerun.txt`): `cargo test --offline --locked --lib -- desk_wording workflow_runs
  channel_response doop_agent_observe_policy… doop_agent_published_editor_approvals… doop_agent_current_preparation…
  agent_history_e2e_process_crash_before_successor`. 19 passed, 1 failed (the pre-existing version-7 test).
- **Browser checks for the renamed text** (`logs/b2-browser-wording.txt`): the calls fixture was regenerated by
  `channel_call_views_section_draws_only_authorized_references` (ok). Then `node --test
  scripts/tests/workflow-run-calls.mjs scripts/tests/channel-response-authoring.mjs`: 3/3 pass.
  `cargo test -- channel_response workflow_runs::calls`: 7 passed.
- **Crash test at 2 MiB, twice** (`logs/b2-crash-stack-budget.txt`): `bash scripts/stack-budget.sh --offline --
  agent_history_e2e_process_crash_before_successor`. 1 passed each time.
- **Clippy** (`logs/b2-clippy.txt`):
  - rustc 1.99.1 `--all-targets -D warnings -A clippy::redundant_field_names`: exit 0;
  - 1.95.0 with the repository's two `-A` lints: exit 0.
- **Formatting:** `cargo fmt --all -- --check`, exit 0 (`logs/b2-fmt-check.txt`).
- **Graph refresh:** `trail build`, exit 0 (`logs/b2-trail-build.txt`).
- **SQLx metadata** (doc 06 B.1): the full gate's "SQLx metadata matches migrated schema" passed on a fresh
  database. This batch made no schema change, so regeneration is not needed.
- **AWC-C005 (p06):** the full-suite run passed the named suites. By test-name match: `memory_scope` 80/80,
  `run_scope` 4/4, `channel_call_` 120/120, and `agent_history` 158 of 159. The one failure is the pre-existing
  version-7 test.

### B4. Freeze for AWC-P06-B-R1 (final integration review)

- Digest `bc8b9841d9f9346152acdbd78e3a093597f554357b61c900e5639ed81486764f`, base/HEAD `fe7791d3`; exclusions
  `plan/agent-workflow-call/phases/p06/docs`, `.trail/learnings`.
- Change since Batch A acceptance (`c7fba340…7431`): only the wording fix in B2.1:
  - `src/adapters/http/pages/workflow_runs/calls.rs` (heading text and module doc);
  - `src/adapters/http/pages/workflow_authoring/channel_response.rs` (label text);
  - `scripts/tests/workflow-run-calls.mjs`, `scripts/tests/channel-response-authoring.mjs`;
  - `docs/workflow-operations.md`, line 331.
- Manifest `logs/AWC-P06-B-R1.evidence.json` is valid: 5 PASS, 3 FAIL, 4 NOT EXERCISED. The FAIL entries are the
  full-suite and stack-budget runs (classified in B2) and the attribution SSO browser test (B3).

### AWC-P06-B-R1 corrections (2026-11-24)

Fable 5.2 returned changes required for the record only (AWC-P06-B-R1-N1). No source changed, so the digest stays
`bc8b9841…764f`.

- **F5 (medium), fixed.** Source-module reachability did not run in the full gate: both `--all-targets` runs
  stopped at the failing lib target. R1 declared it PASS anyway. It has now been run separately
  (`logs/b3-source-modules-bins.txt`):
  - `cargo test --offline --locked --test source_modules`: 8 passed;
  - `cargo test --offline --locked --bins`: `main.rs` 6 passed.
- **F6 (low), fixed:**
  - The Linux-only CI self-test "Network isolation preserves the runner control plane" is NOT EXERCISED. On
    macOS, `check.sh` printed "Linux network-isolation scope: NOT EXERCISED locally; required in CI". It is
    unavailable here.
  - Clippy now has separate entries. The CI/`check.sh` command on stable 1.99.1 FAILS with 9 pre-existing
    errors. The scoped runs (1.99 with `-A redundant_field_names`; 1.95 with the repository's flags) PASS.
  - The crash test's child panic text is now quoted in B2.4.
- **Manifest:** `logs/AWC-P06-B-R2.evidence.json` is valid (6 PASS, 4 FAIL, 5 NOT EXERCISED). R1 is kept as it
  was reviewed.

### Batch B accepted in phase (2026-11-24)

- **Review:** Fable 5.2 PASS, AWC-P06-B-R2-N1, at `bc8b9841d9f9346152acdbd78e3a093597f554357b61c900e5639ed81486764f`.
  Root recomputed the digest and it matches. Manifest R2 is valid. There are no open findings.
- **Edits after the review (records only):**
  - doc 06's completion record was filled in;
  - COORDINATION moved to r26 (p06 row; AWC-C005 satisfied for p06).

  Both files are inside the snapshot, so the post-record digest is `62ad5b790e2139913f4476f6b3badbcc003b5fd84c161339ddb47f7e4c0cd8b5`. No source, test or script changed.
- **Plan status:** implemented and accepted in phase, but **not declared complete** (doc 06 rule):
  - unavailable here: Docker images, managed Doop, plugin package, query statistics, attribution extracted service,
    Linux isolation self-test;
  - unresolved: the intermittent crash-test gap;
  - pre-existing failures, recorded separately: the version-7 test, clippy on stable, attribution SSO.
