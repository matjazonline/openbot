# AWC-P05 — Authoring, inspection, and samples

## Identity

- Plan `plan/agent-workflow-call`, phase `p05`, doc [05-authoring-inspection-and-samples.md](../../../05-authoring-inspection-and-samples.md).
- Workflow implement-opus-reviewed-2_4; records at
  `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/agent-workflow-call/phases/p05/docs`.
- Root/A: Claude Code `1e84a7af`, Opus 5.5/xhigh (user-confirmed). B: Opus substitute B' `a4f44ca532412935f`
  (Opus 5.5/medium) for all of p05, by user decision U7. Earlier note: The
  Fable B `a8d0fa1fbd0b8b88e` is rate-limited until 21:00 Europe/Berlin. The Opus substitute B' `a4f44ca532412935f`
  was user-authorized for p04's rounds only.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD `fe7791d3`.
  - Preserved pre-existing work: p01–p04, accepted in phase and uncommitted. p04 was accepted at
    `46e25d95…995c`.
  - Starting digest, with p05 records excluded: `10d8f823826817f5a0651ff5addd4672bccc87f8aac9e64fe8307c84e212422b`.
- Prerequisites: p01–p04 accepted in phase (see COORDINATION phase directory).
- Coordination `plan/agent-workflow-call/COORDINATION.md` r20. Applicable:
  - AWC-C001 r4 (the authority model);
  - AWC-C002 r1: p05 A owns the authoring catalogue entry and directory discovery, real channel desks only and
    never principal inboxes (p01 F9). Acknowledged r20.
  - AWC-C005 r1: a call child is a fresh personal scope, and the publisher accepts call owners. p05's views
    must not label a child's scope as the caller's. Acknowledged r20.
  - Plan-wide r1, r2 and r15 (U1–U5).
- Hand-over from p04 (`phases/p04/docs/TASK.md`):
  - editor labels for `channel.deny` / `channel.hand_off` (`graph.rs:219`, `catalogue.rs:54`);
  - `BUILDER_STEP_TYPES`;
  - the native template's response-step note.
- Resources: scratch DBs `openbot_awc_mac02`, `openbot_awc_mac02_test`.
- Snapshot exclusions: `plan/agent-workflow-call/phases/p05/docs`, `.trail/learnings`.

## Status

- **2026-11-23: B2 accepted in phase.**
  - B' PASS AWC-P05-B2-R2-N1 at `905b73f8f75ac938e26cb397b96e61108afd5478c951deb7b393c8d8f23c8d5a`, after R1
    (F1 low, fixed in code; F2 optional, declined and accepted). Root recomputed the digest and it matches.
  - The R2 manifest validates. Coordination is unchanged at r22.
  - With B1 and B2 accepted, **p05 is complete in phase**. Next: p06, the final full gate.
- **2026-11-23: batch B2 implemented and frozen** (gate AWC-P05-D2 r2 with F8, F9).
  - Digest `24b290b23293e66b2e534d4dfbd2546ffd6f9fa180917d8859313890f48e9da4`; base/HEAD `fe7791d3`; exclusions
    `plan/agent-workflow-call/phases/p05/docs`, `.trail/learnings`.
  - Records: `logs/AWC-P05-B2-R1-snapshot.txt` and manifest `logs/AWC-P05-B2-R1.evidence.json` (validated: 9 PASS,
    1 FAIL, which is run1's gauge-count failure before its expectation update).
  - Coordination unchanged at r22. Changes, mapping, deviations and checks are under "B2 implementation" below.
    Sent to B' as AWC-P05-B2-R1.
- **2026-11-23: gate AWC-P05-D2 r2 PASSED** (AWC-P05-D2-R2-N1; r2 sha256 `4fd2383d…94ae`, a delta on r1
  `de0e8612…78ca`). Carried into batch B2:
  - **F8 (low).** Rule 3 and the gauges must not count a call the caller already consumed without a stored result
    (`revoked`). The agent checkpoint's completed tool invocation (schema ~12408-12415) distinguishes it. Add a V1
    row and a V3/V9 case.
  - **F9 (low).** Live passes `fields` through whenever it is an object: `done` via
    `channel_reply_completion::project_on`, `channel.deny` via `families.rs::result_fields`. Apply live's
    effective rule in samples, or record a deliberate stricter rule with a test.
  - B2 is in implementation.
- 2026-11-23: gate AWC-P05-D2 r1 needed changes (AWC-P05-D2-R1-N1: F1, F2 medium; F3–F6 low; F7 optional). r2
  (`logs/AWC-P05-D2-r2-decision.md`, sha256 `4fd2383d0214bc9f4c616c8b7616ecde3ff14d24f1fc4abd4f53701bb40494ae`, a delta on r1) answers all seven and was sent to B'.
- **2026-11-23: gate AWC-P05-D2 r1** (`logs/AWC-P05-D2-r1-decision.md`, sha256 `de0e861270efe6f8b0c6f7f00a627e761dda8806e594c10502248e01ae7778ca`) sent to B' for batch
  B2. Pre-gate digest `22fdcacd89aa94c2b7d60e432a336910df017210e970d3eb0ba0abd471ecba13`. Decisions D1–D5; D3 (sample contract, identity v3, authorized target capture), D1
  (cross-scope links) and D5 (metric names) are gated.
- **2026-11-23: B1 accepted in phase.**
  - B' PASS AWC-P05-B1-R2-N1 at `22fdcacd89aa94c2b7d60e432a336910df017210e970d3eb0ba0abd471ecba13`. Root recomputed
    the digest and it matches.
  - The R2 manifest validates.
  - Coordination is unchanged at r22.
  - The known unrelated failure `doop_agent_current_preparation_reaches_ready_before_model_io` (asserts version 7)
    stays recorded.
  - Next: gate AWC-P05-D2 for batch B2 (doc 05 Batch B).
- **2026-11-23: batch B1 implemented and frozen** (gate AWC-P05-D1 r2 with F8–F10).
  - Digest `fb860437a159df93045d7975ed0b3c2a6cc104a9958c21edb4cf9559f5e0fdfd`; base/HEAD `fe7791d3`.
  - Exclusions: `plan/agent-workflow-call/phases/p05/docs`, `.trail/learnings`.
  - Records: `logs/AWC-P05-B1-R1-snapshot.txt` and manifest `logs/AWC-P05-B1-R1.evidence.json` (validated: 7 PASS,
    2 FAIL, both the known doop test).
  - Mapping, deviations and checks are under "B1 implementation" below.
  - R1: changes required (F1 medium, F2/F3 low, F4 optional). All are dispositioned below. R2 recheck at digest
    `22fdcacd89aa94c2b7d60e432a336910df017210e970d3eb0ba0abd471ecba13` (manifest `logs/AWC-P05-B1-R2.evidence.json`).
- **2026-11-23: gate AWC-P05-D1 r2 PASSED** (AWC-P05-D1-R2-N1; r2 sha256 `541203da…28e2`, a delta on r1
  `24c9f9ef…70bd`; pre-gate digest `d9e156a5…6a2e`). Carried into batch B1:
  - **F8 (low).** The written r2 lost its escape strings. The exact rule: in the serialized JSON list, every
    backtick (U+0060) becomes the six characters `\u0060`, and every `<` becomes `\u003c`. C5 asserts both
    sequences appear for hostile names, and that no raw backtick or `<` from listed data remains.
  - **F9 (low).** "External" is judged from what the run can answer, not only from the root trigger. In a call
    subtree the run is internal. Otherwise it is external when any of these holds:
    1. the root's trigger message audience is not `internal_only`;
    2. the run's (or its root's) thread holds any message whose audience is not `internal_only`;
    3. the run input's `source_message` names a message whose audience is not `internal_only`.
    C3a adds a manual rerun of an ingress run on the customer thread (listed nothing).
  - **F10 (optional).** Taken:
    - the "n more" count is computed by a separate `count(*)` of public candidates, not from the 40-candidate
      window;
    - the per-candidate `target_on` share locks are held until preparation commits, so desk-settings writes wait
      on an agent preparation. This is stated and accepted.
  - B1 is in implementation.
- 2026-11-23: gate AWC-P05-D1 r1 needed changes (AWC-P05-D1-R1-N1: F1, F2 medium; F3–F6 low; F7 optional). r2
  (`logs/AWC-P05-D1-r2-decision.md`) answers all seven, with U8 for F2, and was sent to B'.
- 2026-11-23: gate AWC-P05-D1 r1 (`logs/AWC-P05-D1-r1-decision.md`, sha256 `24c9f9ef…70bd`) sent to B'. Pre-gate digest
  `b2c2b594…5976`. Batches:
  - B1 = doc 05 A plus the p04 hand-overs (this gate);
  - B2 = doc 05 B (its own gate after B1 is accepted).
- 2026-11-23: phase started. Coordination checked at r19 and acknowledged at r20. Investigation of the existing
  state against doc 05 A/B is in progress.

## Investigation (2026-11-23)

A read-only survey mapped doc 05 to the code. Root/A verified the directory and grant rule.

| Item | State | Evidence | Gap |
| --- | --- | --- | --- |
| A.1 built-in | partial | enum and name `agent_capability.rs:92,109,126`; publication `agent_publication/tools.rs:256`; resolution `workflow/agent.rs:116,506`; exactness `exactness.rs:120` | Not offered in `agent_authoring/catalog.rs:272-278`, yet draft validation (`choices.rs:39,58`) accepts any non-provisioning built-in, so a crafted save binds it. No `channel_call_authoring_` tests. No auto-grant. |
| A.2 help | partial | `CHANNEL_CALL_DESCRIPTION` `channel_call.rs:26`, wired at `agent_context.rs:89` | No native-channel example; no `no_reply` example. |
| A.3 channel views | missing | `workflow_authoring/channel_response.rs:51-60`; `channel_settings.rs:342-423` | No callable status, copyable id, responder label or review-required indicator. |
| A.4 directory | missing / contrary | `provisioning_directory.rs:89-101` emits each agent's principal `address`. `list_company_agents` is a provisioning built-in ("kept or removed; cannot be added", `choices.rs:39-40`). | No callable desks, no intake, no check-3 filter. An ordinary calling agent cannot have the directory at all. |
| A.5 ops docs | missing | `docs/workflow-operations.md` has none; `docs/departments.md:134,143`, `docs/agent-builder-context.md:19` and `docs/inter_channel_agent_communication.md:42` are stale | Revocation, retry, cancel, approval, limits and retention are undocumented. |
| B.1 run/step views | generic only | `workflow_runs/detail.rs:276-290`, `step.rs:136-160`, `provenance.rs:140-170` | Nothing reads `workflow_channel_calls` / `_results`. |
| B.2 dependency view | wrong text | `provenance.rs:35-36` says children keep the root's selection | A call child has its own manifest and a fresh scope (AWC-C005). |
| B.3 samples | partial | `sample/tools.rs:121-131`: generic fixture or refusal | No frozen target; a missing fixture is an error, not an incomplete sample; no simulated review. |
| B.4 labels | missing | results carry `content_level` / `content_exact` | nothing to project yet |
| B.5 signals | missing | `workflow_run_events.event_kind` is free text (schema ~5963) | no call event kinds |
| p04 labels | missing | `workflow_authoring/graph.rs:218-223`, `catalogue.rs:54` | deny / hand_off labels |
| `BUILDER_STEP_TYPES` | missing | `provisioning.rs:38-44` | decision needed |
| template note | missing | `templates/agent_workflow.rs:147,236,446` special-case only `no_reply` | deny / hand_off rejected as custom ends? |

Browser suites are in `scripts/tests/` (Playwright; `node --test`). Relevant: `agent-authoring-browser.mjs` (fixtures via
`dump_autonomous_agent_authoring_browser_fixtures`), `channel-response-authoring.mjs`, `workflow-runs.mjs`,
`workflow-samples.mjs`.

Blocking product decision: **how an ordinary calling agent discovers callable desks** (A.4 vs the provisioning-only
directory). It goes to the user before gate AWC-P05-D1.

## User decisions

- **U6 (2026-11-23, desk discovery):** "List desks in the call tool". The `channel.call` description, prepared
  per agent turn, lists the desks the agent passes check 3 on: id, name and intake, capped. No second tool. The
  directory stays builder-only. Coordination r21.
- **U7 (2026-11-23):** "Opus substitute for p05" reviews all p05 rounds.
- **U8 (2026-11-23, list audience, from gate finding AWC-P05-D1-F2):** "Public desks; none for external". Only
  public desks are listed, and nothing is listed when the run answers an external correspondent. Coordination
  r22.

## B1 implementation (2026-11-23)

### Changes

- **D1 catalogue.** `agent_authoring/catalog.rs::builtins` offers `AgentBuiltinTool::ChannelCall` to every agent.
  Nothing grants it: no default binding, template or migration adds it.
- **D2 discovery.**
  - New `application/workflow/channel_call_listing.rs`: `DeskListing` is `Unlisted | External | Listed {desks,
    more}`. It holds the bounds, the budget fill, the renderer and the F8 escapes (```, `<`).
  - `AgentToolTarget::ChannelCall { desks }`. `PreparedAgent::with_desk_listing` refuses after history is set.
    `PREPARED_AGENT_VERSION` is 9.
  - The DB pins on the prepared version move to '9' in two places:
    - `agent_history_preparation_valid`;
    - `agent_history_checkpoint_matches`. This second pin was found when every call test failed with "agent
      exchange level/checkpoint agreement failed".
  - `channel_call_target.rs`:
    - `desk_listing_on` takes at most 40 public candidates by name then id, runs `target_on` and
      `decide_target == Proceed` on each, and counts "more" separately (F10);
    - `answers_external_on` implements the three F9 sources; a call subtree is internal;
    - `target_on` and `agent_participates_on` take the `AgentId`.
  - `agent_history/preparation.rs::candidate_on` lists only when the prepared agent calls desks.
    `frozen.rs::restore_on` re-applies the saved list, so restore stays byte-equal.
  - `agent_context.rs` appends `desks.render()` to the description.
- **D3 desk card.**
  - New port read `AuthoringReads::desk_call_facts`: access mode, enabled, unit archived.
  - `channel_response.rs` adds the "Calls from agents" block:
    - status by access mode, or "Agents cannot call this desk: <first reason>";
    - a read-only, labelled id input with a Copy button;
    - the responder label;
    - "Final review required".
  - The shared `copyTextFrom` (`layout.rs`) copies an input's value. It selects the input when the clipboard API
    is absent or refuses, and shows "Copied" only on success.
- **D4.**
  - `graph.rs::response_step_label` is shared by the graph and the catalogue: "Deny request", "Hand off to
    another run", "Finish without reply".
  - `BUILDER_STEP_TYPES` adds `channel.deny` and `channel.hand_off`.
  - The native template (`agent_workflow.rs::ends_response`) accepts any `registry::RESPONSE_STEPS` as the
    terminal custom step, and its refusal names all three.
- **D5 docs.**
  - `docs/workflow-operations.md` gains "Channel calls" and the response-step sentence.
  - The stale lines in `departments.md`, `agent-builder-context.md` and `inter_channel_agent_communication.md`
    are corrected.
  - Doc 05 A.4 is amended for U6/U8.

### Verification mapping

| ID | Test | Decisive assertions |
| --- | --- | --- |
| C1 | `channel_call_authoring_catalogue_offers_opt_in` (unit), `channel_call_authoring_catalogue_save_and_publish` (DB) | Offered with or without memory. A new agent has no binding. A saved binding is in the draft while the active revision stays the earlier one without it. A stale save → `Conflict`. A reserved alias `channel.call` on another built-in → `BadRequest`. Publish → the active definition has it. |
| C2 | `channel_call_authoring_ungranted_never_sees` | A prepared spec without the binding: `calls_desks()` is false and no `channel_call` declaration. Dispatch refusal of an undeclared name is existing behaviour (not re-asserted). |
| C3 | `channel_call_authoring_discovery_lists_callable_public_desks` | Fixture desks: public; the caller's own desk (ancestry); members+grant; members; disabled; archived unit (production `archive_department`); no responder; plus the agent's principal inbox. Only the public desk is listed. Listed == `start_on` launched for each desk, except members+grant, which launches but is not listed (U8). |
| C3a/b | `channel_call_authoring_discovery_omitted_for_external` | An internal thread lists, and the run's `content_level` is unchanged by preparation (C3b, folded in here instead of a separate `_level_zero` test). A thread holding an `external_conversation` message → `External` and the omission line. A call child of that external run lists. |
| C4 | `channel_call_authoring_discovery_frozen` | After a desk is added and the listed desk disabled, the resume returns the identical list; the disabled desk is refused at launch; a new step lists afresh. |
| C5 | `channel_call_authoring_discovery_bounds` | 25+ desks, more count, truncation on char boundaries, ≤ 6 KiB, `more_fields`, ```/`<` escapes with no raw backtick or `<` from data, parses as JSON. |
| C6 | `channel_call_authoring_help_examples` | Native example, typed `no_reply` example, direct-input sentence. |
| C7 | `channel_call_authoring_desk_card` (unit HTML); browser `channel-response-authoring.mjs` "calls-from-agents block …" | Public/members texts. Reason order: disabled, no responder, paused, archived. Id input readonly + labelled, Copy wired. Responder label escaped. Review Yes/No. No params in HTML. No block without facts. Browser: Tab from the id reaches Copy, Enter copies the exact id to the clipboard; without the clipboard API the id is focused and fully selected. |
| C8 | `channel_call_authoring_template_accepts_response_ends`, `channel_call_authoring_builder_offers_response_steps`, `channel_call_authoring_response_step_labels` | Deny and hand_off end the composed template (compiled), with no `finish_response`. A classify step ending `$end` is refused, naming all three labels. `BUILDER_STEP_TYPES` ⊇ `RESPONSE_STEPS`. Graph labels. |
| C9 | existing suites | See checks. Expectation updates: the catalogue builtin list now includes `channel.call` (`catalog_tests.rs`); the prepared version is 9 (`agent_history_preparation_tests.rs`). |

### Deviations and notes

1. **D1 label.** The catalogue model (`AgentToolOption`) has no label or help for any built-in, so `channel.call`
   is listed by its name. "Call another desk" and the help sentence are not shown. Adding a label model for all
   built-ins is out of B1's scope.
2. **D3 status.** A fourth reason, "the selected responding workflow is paused", matches launch's `active` check.
   The status is a configuration summary; launch re-judges.
3. **C3 fixture.** Each launch claim is cancelled after its check, because the company dispatch cap
   (`fairness::available`) refuses a third live claim.
4. **D4.** The builder cannot add `workflow.call`. A builder-authored `channel.hand_off` names its run literally or
   by a reference, for example to an agent step's output. Launch refuses one that is not a consumed `done` call
   (`channel_call.hand_off_invalid`, p04). Corrected per AWC-P05-B1-F4.
5. The shared copy handler change also affects the onboarding and Resend copy buttons: "Copied" now appears only
   after a successful write. No test covered the old behaviour.
6. `doop_agent_current_preparation_reaches_ready_before_model_io` still asserts version 7 (now 9). It is the
   known unrelated failure, unchanged.

### B1 checks (2026-11-23; scratch DBs `openbot_awc_mac02`/`_test`, reset after the version-9 schema edits)

| Check | Command | Result | Log |
| --- | --- | --- | --- |
| Focused | `cargo test --offline --lib -- channel_call_authoring_` | 13/13 | — |
| Affected DB suites | `cargo test --offline --lib -- channel_call_ channel_reply_ agent_history exactness agent_publication agent_authoring workflow_tool provisioning workflow_authoring catalogue templates channel_response channel_settings registry` | 802 passed, 1 failed (known `doop_agent_current_preparation_reaches_ready_before_model_io`, asserts version 7), 3 ignored. The C1 DB test was added after this run and passes in the focused run. | `logs/b1-affected-run2.full.txt` |
| Stack budget 2 MiB | `bash scripts/stack-budget.sh --offline -- channel_call_ channel_reply_ agent_history workflow_tool` | No overflow. 596 passed, 1 failed (the same known doop test); the script reports "failed for a reason unrelated to the stack budget". | `logs/b1-stack-budget.txt` |
| Browser | `node --test scripts/tests/channel-response-authoring.mjs` | 2/2 | — |
| Clippy 1.95.0 | `cargo +1.95.0 clippy --offline --locked --all-targets -- -D warnings -A clippy::redundant_field_names -A clippy::nonminimal_bool` | exit 0 | `logs/b1-clippy.txt` |
| fmt | `cargo fmt --check` | clean | — |
| CSS | `bash scripts/check-css.sh` | clean | — |
| sqlx | `cargo sqlx prepare --check -- --all-targets` | exit 0 | `logs/b1-sqlx.txt` |
| source_modules | `cargo test --offline --locked --test source_modules` | 8/8 | — |

### AWC-P05-B1-R1 corrections (2026-11-23)

- **F1 (medium), fixed.** `doop_agent_mcp_error_policy_is_frozen_in_preparation_and_checkpoint_identity`
  (`agent_tests.rs:91`) and `agent_history_contract_preparation_retains_original_json_and_one_untrusted_prompt` now
  assert `PREPARED_AGENT_VERSION` instead of a literal. A `doop_agent_` run is added to the checks.
- **F2 (low), fixed.** The new `channel_call_authoring_discovery_external_sources` covers F9 source 1 (an
  `external_conversation` trigger message, linked only through a message intent and its admitted binding; the
  message sits on a thread the run is not on) and source 3 (the run input's `source_message` names a customer
  message on no thread). A sibling plain run still lists.
  - Mutation check: replacing source 1's or source 3's audience predicate with `false` fails the matching assertion
    (lines 352 and 368).
  - The C3a comment now states that the thread case is the manual rerun on a customer thread: a run admitted with
    no trigger message, on the customer's thread.
- **F3 (low), fixed.** The docs say: "Only the approved response becomes the tool result; reviewers approve or
  reject it."
- **F4 (optional).**
  - Listing on resume: declined. `candidate_on` already recomputes the department context fresh and
    `restore_on` replaces it with the frozen one; the list follows the same pattern. Skipping it would mean finding
    the exchange before the candidate is built. The cost is bounded (≤ 40 candidates), and the locks are accepted
    under F10.
  - Deviation 4's note is corrected.
  - The deadline claim (expiry → caller sees `workflow.child_failed`) is p03 L12
    `channel_call_launch_shorter_deadline_fails_child`.

## B2 implementation (2026-11-23; gate AWC-P05-D2 r2 with F8, F9)

### Changes

- **Phase and signal (pure).** New `application/workflow/channel_call_views.rs`: `CallPhase` from
  `CallPhaseFacts` (rules 1–4, with F8: rule 3 needs `!caller_consumed`), labels, `ChannelCallItem` /
  `ChannelCallPage` (`restrict()` clears desk names) / `ChannelCallOrigin`, `STALLED_AFTER_SECONDS = 300`,
  `outcome_label` and `MonitoredChannelCalls` (wired in `infra/workflow.rs`), which counts
  `workflow_channel_call_outcomes_total{outcome}`.
- **Read.** New `persistence/workflow/run_view_calls.rs`: `CALLS_SQL` (latest call wait, stored result, final-review
  decision via `reply_proposal_id IS NOT NULL`, child and caller stall predicates, `caller_consumed` from the caller
  execution's checkpoint invocation with `state='completed'`), `channel_call_page` (limit + 1 for `truncated`) and
  `channel_call_origin`. Readability comes from `run_levels_on(...).is_visible()`.
- **Views.** `RunView.channel_calls` / `called_by`, `StepView.channel_calls`; `project()` restricts them. Pages:
  new `pages/workflow_runs/calls.rs` (section, "Called by", `CallAccess`), `detail.rs`, `step.rs`, and provenance's
  call-scope wording (`data-scope="call"` / `"root"`). Route: `Ui::call_access` authorizes runs (readable and
  `Visibility::allows`), desks (`Visibility::allows(Channel)`) and responders (`channel_reply_settings`: Ok means
  shown; NotFound/Forbidden means hidden; other errors propagate).
- **Gauges.** `metrics.sql` `call_facts`/`call_phases`/`calls` CTEs and seven `workflow_channel_call*` gauges.
- **Samples.**
  - `SampleRequest.call_targets` (≤ 10), `FrozenCallTarget`/`FrozenResponder`, `ScriptedToolCall.approval`,
    `SampleEffectKind::SimulatedCall`, `SampleEffect.label`;
  - identity kind `sample-only-v3` with the frozen targets;
  - `PreparedSample::new_with_targets` (refuses duplicates and > 10; requires frozen = declared);
  - the form's call-targets field.
  - Execution: `sample/tools.rs::channel_call` runs `ChannelCallRequest::parse` → declared target →
    `decide_target` over frozen facts → `check_intake` → review → fixture (output schema, success closure, F9
    fields).
  - Capture: `sample_capture.rs::call_targets_on` authorizes each desk through `authorize_association_on(inbox)`
    (the authority behind `channel_reply_settings`), reads `channel_call_target::desk_state_on`, computes
    `writable_agents` for closure agents granted `channel.call` with `agent_writes_on`, and freezes responder
    binding/revision, `review_required`, intake and `result_fields` via `declared_fields`.
  - `channel_call_target.rs`: `target_on` is split into `desk_state_on` (desk share lock, settings, pinned
    responder) and `agent_writes_on`. Live keeps the same row locks in the same order (desk, then principal and
    grants). The settings and binding reads, which take no row locks, now come before the grant locks.
- **Boundary.** `scripts/workflow-sample-boundary.py` names `channel_call::{ChannelCallRequest, ChannelCallError,
  ChannelCallOutcome, DeskFacts, TargetDecision, decide_target, check_intake, REVIEW_REJECTED}`.
- **Test entry.** `#[cfg(test)] routes::workflows::run_page_for_tests` renders the production pane/step handlers
  for a user over a persistence (`runs_tests.rs`: `ui_over`, `text`, `pane_query` made `pub(super)`).
- **Browser.** `scripts/tests/workflow-run-calls.mjs` consumes `WORKFLOW_CALLS_BROWSER_FIXTURES` written by
  `channel_call_views_section_draws_only_authorized_references`, wired into `scripts/check.sh` (export and the
  native-markup gate) and a CI step "Run call section in a browser".
- **Docs.** `docs/workflow-operations.md`: three gauge rows, "Inspecting calls" (phases, review vs stalled, access)
  and "Samples" (targets, members-only desks the author can manage, rules, what is not simulated).

### Verification mapping

| ID | Test | Decisive assertions |
| --- | --- | --- |
| V1 | `channel_call_views_phase`, `_phase_named_cases` (unit) | Every combination against an independent `expected()`, including F8 (consumed with execution open → closed) and all eight phases reachable. |
| V2 | `channel_call_views_caller_and_child` (DB) | Item ids/desk/responder/child/readable; the origin names the caller; the child's scope differs from the caller manifest; `wait_parked` then the `wait_completed` pair. Pages through the production routes: the call row links the child; the step lists the call; the child links its caller; no argument text on caller and step pages, nor in "Called by". Phases ChildRunning → ResponseReady → AwaitingCaller → AwaitingCaller{stalled} → Settled done. |
| V2/F5 | `channel_call_views_section_draws_only_authorized_references` (render unit) | No access: "a desk", no responder, no child or review link. Desk only: name, no responder. Full: escaped name, responder and revision, child and decision links. "Only the newest 50" when cut. Writes the browser fixtures. |
| V3 | `channel_call_views_review_vs_stalled` (DB) | Overdue child job → ChildRunning{stalled}. Final review → AwaitingFinalReview{decision} plus the review link. `decision.human` → ChildWaiting(decision), and the review gauge stays 0. F8: a revoked consumption followed by a second call in the same step → the first is ClosedWithoutResult{Completed}, the second ChildRunning; response_ready 0, open 1; a page cut at 1 is `truncated`, newest first. Mutation: dropping `!caller_consumed` fails line 375. |
| V4 | `channel_call_views_foreign_scope` (DB) | Admin at level 0 with a tainted child: `child_readable=false`, no child link in the call row (the owner has one). Tainted caller: the restricted pane keeps the call, shows "a desk" and no "finance"; the child page's "Called by" has no caller link. Another company's user → NotFound for both runs. |
| V5 | `channel_call_views_dependency_scope` (DB) | Child: `data-scope="call"`, "Selected when the call from", caller link. Caller: `data-scope="root"`. |
| V6 | `channel_call_sample_simulated_result` (unit) | Fixture result with the label; the report never prints the responder binding or revision. Done/handed_off/denied/no_reply accepted. F9: with no declared result fields, any fields object passes. Review true → fixture; false → `review_rejected` with its label. Launch answers: target_unavailable, unit_archived, desk_disabled, no_responder, invalid_input, intake_invalid, intake_not_declared. |
| V7 | `channel_call_sample_incomplete` (unit) | Stops with `sample_target_undeclared`, `sample_review_undeclared`, `sample_result_undeclared`, approval on a desk without review, approval on another tool, a non-closure or schema-invalid answer, fields outside the frozen result schema, a scripted answer on a launch-answered call. No SimulatedCall effect on any stop. |
| V8 | `channel_call_sample_capture` (DB) | Capture + execution through `SampleCapture`: the fixture answer comes back and production counts (runs, calls, threads, messages, checkpoints) are unchanged. An undeclared target stops. A foreign or missing desk → NotFound. A new responder changes the configuration identity. Frozen members-ungranted → target_unavailable, disabled → desk_disabled, cleared responder → no_responder. |
| V9 | `channel_call_views_metrics` (DB, own database) | Six gauges exact at each state: parked; child stalled; past deadline; succeeded with the wait open and then completed (response_ready); caller stalled; consumed (all 0); final review (review_waiting 1). `metrics_tests` counts 31 gauges. |
| V10 | `workflow-run-calls.mjs`; `workflow-runs.mjs`, `workflow-samples.mjs`, `channel-response-authoring.mjs` | Heading, phase, review link, past-deadline badge, escaped name, cut note, no horizontal scroll at 390 px; restricted: only the step link. |
| V11 | `workflow-sample-boundary.py` | Passes. A copy of the sample tree naming `ChannelCallDispatch` fails (recorded manually; the script has no fixture mode). |

### Deviations and notes

1. **F9.** An undeclared result sub-schema accepts any `fields` object, as live does. r2's "must be absent" is
   superseded. The declared case is still enforced (V7).
2. **Arguments.** A sample judges call arguments only with `ChannelCallRequest::parse`, as live does (live applies
   no schema pre-check). A schema-invalid call gets live's typed `invalid_input`, not a sample stop.
3. **Unused fixture.** A scripted `output` on a call answered at launch or by a rejected review stops the sample. The
   desk never gives that answer.
4. **Boundary.** `ChannelCallOutcome`, `DeskFacts` and `decide_target` are added beside the gate's list; the frozen
   decision needs them. `declared_fields` and `channel_call_schemas` are not named, because sample code does not
   use them (capture uses `declared_fields` in persistence, outside the boundary). `DeniedCode` is not named either:
   it is used only through `TargetDecision`.
5. **V10 file.** The call section has its own browser file, `workflow-run-calls.mjs`, because `workflow-runs.mjs`
   runs in a CI step without fixtures. It follows the decision-forms pattern.
6. **Stand-ins, not proved end to end.**
   - V2/F5: run pages admit only owners and root admins (`managed_company`), and they manage every unit, so no DB
     fixture separates desk visibility from responder authority. The render unit covers the split; the route maps
     `channel_reply_settings` NotFound/Forbidden to "hidden", which was checked by reading the code.
   - V8: no actor can manage the root-bound definition without managing every unit, so "may view but not manage
     the desk" is not constructible with this fixture. V8 proves NotFound for foreign and missing desks.
     Authorization uses `authorize_association_on(inbox(desk))`, the same call `channel_reply_settings` makes.
   - V5: the root wording is checked on the caller, not on an ordinary workflow-tool child.
7. **Bug found in batch.** `run_levels_on` returns an entry for every run. The first cut used `contains_key`, which
   made every child and caller "readable". It now uses `is_visible()`, and V4 failed until that fix.
8. **Expectation.** `metrics_tests` gauge count 24 → 31, with the seven names asserted.

### B2 checks (2026-11-23; scratch DB `openbot_awc_mac02_test`)

| Check | Result | Log |
| --- | --- | --- |
| Affected suites (`--lib -- channel_call sample workflows::ui::runs metrics run_view knowledge_view provenance autonomous_agent mcp_agent`) | run1: 399 passed, 1 failed (`metrics_tests` gauge count, expectation updated); run2: 400 passed, 1 ignored, 0 failed | `logs/b2-affected-run1.txt`, `logs/b2-affected-run2.txt` |
| Stack budget at 2 MiB (`-- channel_call sample workflows::ui::runs metrics`) | 215 passed | `logs/b2-stack-budget.txt` |
| `cargo +1.95.0 clippy --all-targets -D warnings` (repo allows) | clean | `logs/b2-clippy.txt` |
| `cargo fmt --check` | clean | `logs/b2-fmt.txt` |
| `scripts/check-css.sh` | current (no asset change) | `logs/b2-css.txt` |
| `cargo sqlx prepare --check -- --all-targets` | ok | `logs/b2-sqlx.txt` |
| `--test source_modules` | 8 passed | `logs/b2-source-modules.txt` |
| `workflow-sample-boundary.py` | passed; the negative copy fails as expected | `logs/b2-boundary.txt` |
| Browser: `workflow-runs`, `workflow-samples`, `channel-response-authoring`, `workflow-run-calls` | 18/18 | `logs/b2-browser.txt` |
| `scripts/tests/check-runner.py` | OK | — |

### AWC-P05-B2-R1 corrections (2026-11-23)

- **F1 (low), fixed in code** (no deviation).
  - `CallAccess.decisions` holds the final-review decisions the viewer may open. `Ui::call_access` fills it through
    the decision page's own authority, `decisions().read_decision`: Ok means linked; NotFound/Forbidden means label
    only; other errors propagate. That is at most one read per listed call in `AwaitingFinalReview`.
  - `calls.rs` draws the review link only from that set.
  - The render unit adds "child readable, decision not openable → child link, no review link". V3 still asserts
    the owner's link through the real authority.
  - Probe (removed): a second admin, even at level 0 with a tainted child, may open the decision. So, as with F5,
    no DB fixture separates run access from decision access today. The render unit covers the split.
  - Doc: "linked to the decision when the viewer may open it".
- **F2 (optional), declined.** `call_facts` scans unfinished executions as the existing `steps` CTE does, so a
  sample costs the same as before. A partial index is a schema change across the shared migration and belongs
  with the existing CTE's cost, not this batch.
- Checks:
  - fmt and clippy 1.95 clean (`logs/b2r2-fmt.txt`, `logs/b2r2-clippy.txt`);
  - `--lib -- channel_call_views workflows::ui::runs`: 38 passed (`logs/b2r2-focused.txt`);
  - stack budget over the same filters: 38 passed (`logs/b2r2-stack-budget.txt`);
  - `workflow-run-calls.mjs`: 1/1 (`logs/b2r2-browser.txt`).
