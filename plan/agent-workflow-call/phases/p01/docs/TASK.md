# AWC-P01 — Contracts and authority

## Identity

- Plan: `plan/agent-workflow-call` (call a channel workflow from an agent). Phase `p01`, doc
  [01-contracts-and-authority.md](../../../01-contracts-and-authority.md).
- Workflow: implement-opus-reviewed-2_4. Record root:
  `/Users/mac02/orca/workspaces/openbot/plan08_ph1/plan/agent-workflow-call/phases/p01/docs`.
- Root/A: Claude Code session `1e84a7af`, Opus 5.5 / xhigh (user-confirmed 2026-11-22).
- B: Fable / medium general-purpose reviewer, agent id recorded in HANDOFF once spawned.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`.
- Starting commit `fe7791d3` (Plan08 p02, unpushed). No pre-existing uncommitted changes.
- Coordination: `plan/agent-workflow-call/COORDINATION.md` r1 (this session is writer);
  notice AWC-C001 r1 acknowledged. Plan08 coordination r4, notice 08G-C001 r1 (this plan
  implements Plan08 02B.3's call clause).

## Requirements (original sources)

- `plan/agent-workflow-call/README.md` (contract decisions, as amended by U1),
  `01-contracts-and-authority.md` Batches A, B, C.
- User decision U1 (2026-11-22): destination binding runs as configured (own `run_as`).
- Plan08 `02-unit-lifecycle.md` 02B.3: a call to an archived unit's desk returns `unit.archived`.
- Plan 11 README proposal 1 (now settled by U1) and 03B/03C as the later replacement.
- `AGENTS.md`, `src/AGENTS.md`, `src/adapters/persistence/AGENTS.md` and any nested guide
  for touched directories.

## Architecture gate

AWC-P01-D1 r3: [logs/AWC-P01-D1-r3-decision.md](logs/AWC-P01-D1-r3-decision.md) —
D1 authority model, D2 call/result contract, D3 intake/result fields. r1 not passed
(AWC-P01-D1-R1-N1, findings F1–F8); r2 answers all and records user decisions U2 (gate = agent
write right) and U3 (ceiling follows plan 11 03C); r3 applies U4 (allowance is level 0 until
plan 11). Status: **PASSED** (B note AWC-P01-D1-R3-N1, 2026-11-22, digest `6b2e98c3…eed4`).
No dependent edits before B passes the revision.

## Batch plan

One review batch, AWC-P01-B1, covering 01 Batches A, B and C:

1. **A** — `AgentBuiltinTool::ChannelCall` (`channel.call`) with its trusted snapshot and
   dispatch target; name reserved across tool registries; application request/result/error
   types (D2); the pure input adapter (canonical content, `MessageTriggerInput` with `fields`).
2. **B** — pure authority policy (D1): the admission predicate over typed facts (write rule,
   archived ordering, cycle), the child-ceiling function, release check on the request level.
   Database-backed controls need the target read (02) and launch (03); they are planned there
   (D1 verification table) and listed below as deferred, not as passed.
3. **C** — intake/result fields (D3): `MessageTriggerInput.fields`; the `fields` sub-schema bound
   and the activation refusals in `check_message_trigger`; call-side intake validation. As
   implemented (addendum A1): the bound runs at activation, `schema()` is unchanged, and
   `channel.deny`, `channel.hand_off` and result-field projection belong to p04 (notice AWC-C002).

Until 03 lands, a granted `channel.call` dispatches to a typed `channel_call.target_unavailable`
tool error. This is an intermediate state on an unmerged branch; the feature is complete only at
p06.

## Focused checks (per batch)

- `cargo test --locked --offline --lib channel_call_contract_` (list then run)
- `cargo test --locked --offline --lib channel_call_authority_`
- `cargo test --locked --offline --lib channel_call_intake_`
- existing built-in exactness / capability tests touched by the new variant
  (filters recorded after tracing), registry step-list test, binding activation tests
- `SQLX_OFFLINE=true cargo check --locked --offline --all-targets`, `cargo fmt --all -- --check`
- DB-backed tests only where SQL changes (none expected in p01)

Final full gate: plan 06 Batch B (once, after integration).

## Acceptance mapping

Tests are in `src/application/workflow/channel_call_tests.rs` unless noted. Round AWC-P01-B1-R1,
artifact `66d3e78f…e17e`, manifest `logs/AWC-P01-B1-R1.evidence.json`.

| Criterion | Production path and case | Decisive assertion | Check | Status |
| --- | --- | --- | --- | --- |
| 01A.1 built-in | `AgentBuiltinTool::ChannelCall`, `builtin_channel_call_snapshot`, `AgentToolTarget::builtin`, reserved names | trusted snapshot equals the built-in; a widened schema or another effect is untrusted; name reserved; target mapping | `channel_call_contract_built_in_snapshot_is_exact_and_its_name_reserved`; `exactness_tests` (every builtin/target) | PASS |
| 01A.2 request | `ChannelCallRequest::parse` | exactly `channel_id`/`content`/`fields`/`deadline`; spoofed identity keys refused; content bounds in bytes at char boundaries; `fields` object ≤ 16 KiB; deadline strictly between now and run deadline | `channel_call_contract_accepts_…`, `…refuses_identity_spoofing…`, `…bounds_content…`, `…deadline_must_fall…` | PASS |
| 01A.3 envelope (D2) | `ChannelCallOutcome::to_value`, `ChannelCallError::to_value` | every closure and every error code validates against the trusted output schema; refused closure has no child; reason cut; oversize refused | `…every_result_matches_the_trusted_output_schema`, `…denied_reason_is_cut…` | PASS |
| 01A.5 input adapter | `ChannelCallRequest::trigger_input` → `MessageTriggerInput` | exact value with runtime-supplied source/sender, fields only when present | `channel_call_intake_call_becomes_the_childs_message_input` | PASS |
| 01B D1.1 order | `decide_target` | every unwritable combination → `target_unavailable`; cycle before archived before disabled before no responder; positive control proceeds | `channel_call_authority_target_checks_run_in_order_behind_the_write_rule` | PASS (pure); DB controls NOT EXERCISED (p02/p03) |
| 01B D1.4 levels | `child_ceiling`, `release_request`, `ALLOWANCE_WITHOUT_REQUEST_RULES` | min of three in all orders; allowance = PUBLIC forces 0; raised allowance lets caller/run_as decide; above-ceiling request refused | `channel_call_authority_child_ceiling…`, `…request_above_the_ceiling…` | PASS (pure) |
| 01C.1 fields input | `MessageTriggerInput.fields` | mail/representative carry none; canonical schema declares no intake; responder schema with intake accepts call value | `channel_call_intake_mail_input_…` | PASS |
| 01C.2 bound + activation | `check_fields_schema`, `check_message_trigger` | each violation names its path; required `fields` → `binding.intake_fields_must_be_optional`; bad intake/result → `binding.intake_fields_invalid`/`binding.result_fields_invalid` with full path | `channel_call_intake_schema_bound_…` (2), `binding::tests::channel_call_intake_activation_refuses_required_or_unbounded_fields` | PASS |
| 01C.3 call validation | `check_intake` | no intake → `intake_not_declared`; failing paths and schema returned and schema-valid | `channel_call_intake_call_fields_are_checked_against_the_captured_intake` | PASS (pure; snapshot capture in p02) |
| 01C.4 directory | — | — | — | moved to p05 A (gate D3, F9) |
| 01C.5 result fields / `channel.deny` | — | — | — | moved to p04 (see placements) |
| no regression | consumers of changed contracts | 796 affected tests pass | `logs/AWC-P01-B1-affected.txt` | PASS (attempt 1 FAIL, corrected) |

Other checks, same round: `cargo fmt --check` + `git diff --check` PASS; `cargo sqlx prepare --check
-- --all-targets` PASS; `cargo test --test source_modules` PASS (8); `cargo clippy --all-targets -D
warnings` FAIL on the 9 pre-existing `redundant_field_names` lints only (files outside this diff;
known since Plan08 p02).

### Round R2 corrections (findings of AWC-P01-B1-R1-N1)

- **F1:** init-schema constraints `agent_binding_target_shape` and `agent_binding_reserved_alias` now name
  `channel_call` / `channel.call`, so the binding can be stored. New DB test
  `channel_call_contract_built_in_binding_is_stored_and_published_exactly`. Scratch DBs were recreated.
- **F2:** addendum `logs/AWC-P01-D1-r3-addendum-A1.md` and notice AWC-C002 r1 (coordination r5);
  Batch plan and Status updated.
- **F3:** empty body judged after canonical sanitization; whitespace-only and zero-width bodies are
  refused (test extended).
- **F4:** named struct in `check_call_fields`; `failing_paths(.., 0)` returns at once; replay and
  `allOf` notes carried into AWC-C002 and the addendum.

R2 evidence (`logs/AWC-P01-B1-R2.evidence.json`, artifact `2ebbc0cd…feaa`):
- 18 focused tests and 797 affected tests pass;
- sqlx check, source_modules and fmt pass;
- clippy fails on the same 9 pre-existing lints only (`logs/AWC-P01-B1-R2-clippy.txt`).

The SQL surface touched in p01 is the two binding constraints above; there are no new queries.

## Contract impact

Known before implementation: `MessageTriggerInput::schema()` is also the reserved
`message.reply` / `wait.event` payload (registry `actions.rs`, `mod.rs`, `waits.rs`); adding the
optional `fields` changes that payload schema (F8).

Carried from the gate review:
- F9 (p05 A): today's `list_company_agents` entry gives `address: {principal_id, address}`, a
  direct principal inbox with no channel id (`provisioning_directory.rs:101`). A principal inbox
  never gets `callable`. p05 adds real channel desks (`channel_id`, address, `callable`, `intake`)
  for department desks and agents' shared desks.
- F10:
  - 08G-C001 r2 tells Plan08 p03 that calls are level 0 until plan 11, and that a denied-before-
    launch closure leaves no rows.
  - p03 of this plan must publish a Plan08 notice when it extends cancel-and-archive.
  - AWC-C001 r4 names 01 A.3 and 01 C.5 as replaced by D2/D3.
  - p04 refuses a `channel.hand_off` whose nested call did not close `done`.

| Changed contract | Producers / consumers / equivalent surfaces | Disposition | Check |
| --- | --- | --- | --- |
| `AgentBuiltinTool::ChannelCall` (`channel.call`) | `name`/`parse`; publication snapshot `agent_publication/tools.rs::builtin`; `AgentToolTarget::builtin`; `agent_action_target` (resource kind); authoring catalogue `agent_authoring/catalog.rs::builtins`; exhaustive test `exactness_tests::every_builtin` | snapshot = trusted built-in; target `ChannelCall`; `agent_action_target` refuses ("Channel calls use the child runtime"), as for workflow tools; **not** added to the authoring catalogue (05 A.1 owns that), so no author can grant it from the UI before p05 | `channel_call_contract_built_in_snapshot_…`, exactness tests |
| `AgentToolTarget::ChannelCall` | `prepared_tool` consistency arm; `exactness::tool_accounting` (ChildControl, like workflow tools); `agent_context` description; `workflow_agent::invocation`/dispatch | dispatch returns `channel_call.target_unavailable` until p03 adds the launch (intermediate, branch-only) | exactness every-target test |
| `ToolKind::ChannelCall` | `ToolSnapshot::kind`; `authority.rs` publication/run check; `resources.rs` run readiness; `sample/tools.rs` (fixture-only, like other built-ins); `preview.rs` label | trusted only when equal to the built-in (`is_trusted_channel_call_snapshot`) | contract snapshot test |
| reserved tool names | `provisioning::is_reserved_tool_name` (HTTP operations, workflow-tool registration, `tool.call` literals) | `channel.call` reserved | contract snapshot test |
| `MessageTriggerInput.fields` (not `schema()`) | producers: `MessageTriggerInput::new` (mail/relay, `message_intents.rs::trigger_input`) → `None`; `ChannelCallRequest::trigger_input` → the call's fields. Consumers: `to_value`; `schema()` used verbatim as many responders' input schema and as the exact `message.reply`/`message.ask`/`wait.event` payload (`registry/actions.rs`, `registry/mod.rs`, `waits.rs`); `binding::check_message_trigger` (representative) | `schema()` is **unchanged** (deviation from 01 C.1's "schema() includes it as optional", see below); `fields` is omitted from `to_value` and `representative()` when absent | `channel_call_intake_mail_input_…`, `channel_reply_`/`message_reply` affected runs |
| `compiler::Schema::failing_paths` | new, used by `channel_call::check_intake` | additive | intake tests |
| `registry::message_content_schema` | new accessor over `schemas::content()` (message.send content) | additive; the call's `content` is the same schema | contract tests |
| `ConfiguredBinding::check_message_trigger` | callers: `lifecycle/bindings.rs` (configure, activate), `lifecycle/provisioned.rs` (two) | adds `binding.intake_fields_must_be_optional`, `binding.intake_fields_invalid`, `binding.result_fields_invalid`; versions without `properties.fields` unaffected | `channel_call_intake_activation_…` |

Design placements made while implementing (B to check):
- **Fields bound at activation, not compile.** 01 C.2 says "a publication error", but a version does not
  know it is message-triggered until a binding says so; bounding every version's `fields`
  property would constrain unrelated workflows. The bound runs in `check_message_trigger`, which
  configure and activate already call, naming the full schema path.
- **`channel.deny` and `channel.hand_off` move to p04.** They are closure producers in the response
  pipeline (D2), and a deny over mail is a final-response publication. Adding steps in p01 that
  finalization cannot yet handle would make mail runs ending in them fail. Gate D2 text is
  unchanged; only the phase that delivers them moves.
- **`schema()` declares no `fields`.** Attempt 1 put an optional `"fields":{"type":"object"}` in
  `MessageTriggerInput::schema()`, as 01 C.1 says. That schema is the literal input schema of many
  responders, so every such desk then "declared" an unbounded intake, and the activation bound
  refused it: 192 affected tests failed (`logs/AWC-P01-B1-affected-attempt1-failed.txt`, all
  `binding.intake_fields_invalid` plus the target-count assertion). Correct reading: the canonical
  schema has no intake; a responder declares one in its own `input_schema.properties.fields`, and a
  call with `fields` to a desk without one is `intake_not_declared` before any child exists.
- **DB-backed authority controls** of the D1 verification table run in p02/p03, where the target
  read and launch exist. p01 proves the pure ordering, ceiling and release policies.

## Status

- Gate AWC-P01-D1 r3: PASSED (AWC-P01-D1-R3-N1). F9 deferred to p05 A; F10 dispositions in "Contract impact".
- Implementation of AWC-P01-B1: done. R1 at `66d3e78f…e17e` needed changes; R2 at `2ebbc0cd…feaa` PASSED (AWC-P01-B1-R2-N1).
- Review: R1 changes required (AWC-P01-B1-R1-N1: F1–F3 actionable, F4 optional); R2 PASS (AWC-P01-B1-R2-N1).
- **Accepted in phase, 2026-11-22**, at digest `2ebbc0cd1c41a6241a85eb15f6a1bd508e826d40fd54e32f2079fd569ea1feaa`
  (base `fe7791d3`, uncommitted; exclusions `plan/agent-workflow-call/phases/p01/docs`, `.trail/learnings`).
  - Root recomputed the digest and validated manifest `logs/AWC-P01-B1-R2.evidence.json`.
  - Coordination r5: AWC-C001 r4 acknowledged and satisfied for p01; AWC-C002 r1 targets p03–p05.
  - Clippy fails only on 9 pre-existing `redundant_field_names` lints in files outside this diff
    (unrelated, unchanged).
  - NOT EXERCISED, owned by p02/p03: the DB-backed authority controls.
- Integrated: no. Not committed, not pushed.
