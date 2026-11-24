# AWC-P01-D1 r1 — architecture gate: authority, call contract, intake/result fields

Author: root/A (Claude Code, Opus 5.5/xhigh), 2026-11-22. Code anchors on `plan08_ph1` at
`fe7791d3`. This gate covers decisions every later phase (02–06) builds on. It changes the
plan where the user decided on 2026-11-22 (U1 below); everything else refines the plan text
against current code.

## U1 — user decision (2026-11-22)

> When an agent calls another desk with `channel.call`, the child runs **the destination's
> binding as configured, with its own `run_as`**; the caller is recorded only as the requester
> (plan 11 proposal 1), adopted now, before plan 11's request rules exist.

This replaces README "Preserve the caller's authority", 01 Batch B items 2 and 3 and 03 Batch A
item 2 ("the caller actor"). The user was told that, without request rules, the only gate is the
existing rule for an agent writing to a desk, so a caller can cause work under the destination's
identity. Plan 11 03B/03C later add request rules and the allowance to this same gate.

## D1 — Authority model

### D1.1 Who may call (admission gate)

A call is admitted only when all of these hold, read in one transaction at first dispatch and
re-read at launch (03):

1. **The capability is real.** The saved model tool call is a call of the `channel.call`
   built-in in the calling agent's frozen capability snapshot, validated through the existing
   origin owner (`agent_origin`, as `workflow_tool_on` does), under the live attempt fence. The
   calling agent passes `agent_authority::check_execution_on(.., AgentUseMode::Tools)`.
   A forged call id or a string naming the tool grants nothing.
2. **The destination is a live desk with a responder.** An enabled channel of the same company
   with a selected responder in `channel_reply_settings` that is active, message-triggered and
   inbox-associated with that desk (the facts `settings_on` / `selected_on` read).
3. **The calling agent may write to that desk.** The same predicate an internal relay applies to
   an agent sender: `IngressOrigin::Internal { agent_principal }.internal_context()` evaluated by
   the desk's participant access (`routing.rs` `authorize_channel` /
   `Channel::participant_access`). A members-only desk needs the agent principal's grant; a
   public desk accepts any company agent. The parent run's actor is **not** consulted, and no
   actor is exempt, the owner included.
4. **No cycle.** The destination desk is not already in the active channel-call ancestry of the
   calling run, including a direct self-call. Separate later calls stay legal.

Failure behavior: 2 or 3 failing (missing, foreign, disabled, no responder, inactive responder,
no write right) returns one indistinguishable error `channel_call.target_unavailable`. A desk
whose unit is archived returns `unit.archived` (plan 08 02B.3) **only when 3 would otherwise
pass**; otherwise it is `target_unavailable`. 1 failing is an origin refusal, as for any
forged tool call. 4 is `channel_call.cycle`. A refusal creates no child, thread, message,
job or invocation row.

### D1.2 Whose identity the child runs under

- The child runs the destination's selected binding **as configured**: its configuration
  revision, params, resources, escalation policy and `run_as`. The child run's `actor_id` is
  that binding's `run_as`; its binding identity is the destination binding (not provenance
  copied from the parent, unlike `children::configured_child`).
- `run_as` is rechecked as a message admission would check it: association authority for the
  desk (`management::authorize_association_on`), `bindings::verify_selection`, and resource
  readiness at the admission stage. Loss of that authority is `target_unavailable`.
- Unit scopes are computed for the destination desk the way a message-admitted run on that desk
  gets them; they are **not** copied from the parent (`run_scopes::copy_child_on` is for
  ordinary children only). Memory, knowledge and tools therefore follow the destination's unit.
- The child's agent and workflow selections come from its own dependency scope captured at first
  dispatch (02 Batch B), never from the caller's manifest.
- Lineage is unchanged: the child is `ChildCause::ToolCall` in the caller's tree, inside its
  cancellation, deadline, depth and count budgets, with the caller's correlation id.
- Spend is charged where the run executes (07): to the destination desk's unit chain.

### D1.3 The requester

The invocation record (02 Batch A) names company, calling run, execution, saved call id and
calling agent; the child's `parent_run_id` keeps lineage. The caller's actor gains nothing on the
destination: no desk grant, no thread read beyond the call's own result, no resource access.

### D1.4 Knowledge levels

- Child ceiling = lowest of (calling run's current ceiling, the `run_as` person's level from
  `actor_level_on`). The child therefore never reads above what the caller could receive.
- The request message is stamped with the calling run's current content level and exactness.
  It is released to the destination only when that level is ≤ the child ceiling **and** passes
  the existing release check for a message written to that desk's audience
  (`AppError::KnowledgeRelease`); otherwise `channel_call.audience_refused`, before anything is
  stored.
- The result is absorbed into the caller with `knowledge::absorb_content_on` (child level,
  exactness), as the workflow-tool result is today. It cannot exceed the caller's ceiling.

### D1.5 Live authority after admission

The snapshot (02) freezes ordinary configuration. These remain live and are rechecked when the
result is consumed and before any protected child effect already guarded today: the calling
agent's capability and execution authority (D1.1.1), its write right on the desk (D1.1.3), the
desk enabled and its unit not archived, the binding not deactivated or deleted. Losing one
settles the call with `channel_call.revoked` and no result disclosure. The parent is woken
through the existing child-terminal path (03 Batch B item 5), never left parked.

### D1.6 Hand-off to plan 11

Plan 11 03B/03C replace D1.1.3 with the request-rule check (asking unit = calling run's unit)
and add the allowance to the D1.4 ceiling. Recorded in plan 11's README, proposal 1.

### Invariants

- I1. A call never lets the child act with more authority than a message from the calling agent
  to that desk would have given the desk's own responder.
- I2. The caller learns nothing about a desk it may not write to (one public failure).
- I3. Nothing the child read above the caller's ceiling reaches the caller.
- I4. No row exists for a refused call.

## D2 — Call and result contract (model-visible)

Input (`deny_unknown_fields`):

```json
{"channel_id": "<uuid>", "content": {"subject": "...", "body": "..."},
 "fields": {"...": "..."}, "deadline": "<RFC 3339>"}
```

`fields` and `deadline` are optional. Subject and body use the same canonical parser and bounds as
`message.send` (`CanonicalContent::parse`, `MESSAGE_INPUT_BODY_BYTES`, `MAX_SUBJECT_BYTES`).
`deadline` must be strictly earlier than the calling run's deadline (`channel_call.deadline_invalid`).

Result (a tool result value, at most `MAX_TOOL_RESULT_BYTES`):

```json
{"status": "replied", "child_run_id": "<uuid>",
 "content": {"subject": "...", "body": "..."}, "fields": {},
 "closure": {"kind": "done"} | {"kind": "denied", "reason": "..."} |
            {"kind": "handed_off", "run_id": "<uuid>"}}
{"status": "no_reply", "child_run_id": "<uuid>"}
{"error": {"code": "<stable code>", "message": "<bounded safe text>"}}
```

`fields` appears only when the reply has them. `denied.reason` is bounded like a subject. Errors use
the existing `tool_error` shape. Stable codes:
`channel_call.target_unavailable`, `unit.archived`, `channel_call.invalid_input`,
`channel_call.intake_invalid` (adds `schema` and failing `paths`),
`channel_call.intake_not_declared`, `channel_call.deadline_invalid`, `channel_call.cycle`,
`workflow.tool_limit` (existing, depth/count/continuation), `channel_call.audience_refused`,
`channel_call.revoked`, `workflow.child_failed`, `workflow.child_cancelled` (existing),
`channel_call.deadline_exceeded`, `channel_call.review_rejected`, `channel_call.review_expired`,
`workflow.tool_result_too_large` (existing).

There is no `accepted` result. Ownership loss is a worker lifecycle outcome, never a tool result.
The closure comes from the destination's response pipeline (04): `done` for a projected final
output, `denied` from a new terminal step `channel.deny` (beside `channel.no_reply`, success to
`$end`, `reason` required), `handed_off` when the final response came from a nested call.

## D3 — Intake and result fields

- `MessageTriggerInput` gains `fields: Option<Value>`: absent for mail and relays, present only
  for a call. `schema()` declares it optional; `representative()` omits it.
- A message-triggered version may declare `input_schema.properties.fields` (the intake) and
  `output_schema.properties.fields` (the result fields). Both are bounded by one compiler rule:
  at most 32 properties, depth 2, types string/number/boolean/enum/array of strings, each with a
  `description`, under 16 KiB serialized; anything else is a publication error naming the path.
- Binding activation keeps validating `MessageTriggerInput::representative()`, so a version that
  makes `fields` required is refused with `binding.intake_fields_must_be_optional`.
- The call validates `fields` against the intake captured in the target snapshot before any row
  exists (`intake_invalid` / `intake_not_declared`). It reaches the child as `/input/fields`,
  never in `params`.
- There is no `reply` step in the current registry: the channel response is the final output
  projected by `ChannelReplyContract`. The result fields are the final output's `/fields`,
  validated by the output schema and returned unchanged; `channel.deny` takes optional `fields`
  validated against the same sub-schema. A version without result fields answers in prose.
- The agent directory entry (`list_company_agents`) carries each desk's intake schema beside its
  address.

## Verification plan (accepted at gate; executed by phase)

| Decision | Proof | Phase |
| --- | --- | --- |
| D1.1 1–4, I2, I4 | `channel_call_authority_` pure matrix (write-rule predicate, archived ordering, cycle) + DB controls: authorized agent, members-only desk without grant, foreign company, disabled desk, no responder, forged origin, owner without exemption; row counts zero on refusal; one positive control | p01 (pure), p02/p03 (DB) |
| D1.2 identity | child `actor_id` = destination `run_as`, different from parent actor; unit scopes = destination desk's; resources = destination binding's | p03 `channel_call_launch_` |
| D1.4 levels | ceiling = min(caller, run_as) both orders; request above ceiling or desk audience refused before rows; absorbed result | p01 pure, p03/p04 DB |
| D1.5 live authority | revoke grant/disable desk/archive unit while waiting → `revoked`, no disclosure, parent woken | p03 `channel_call_resume_`, p06 |
| D2 contract | `channel_call_contract_` parse matrix (unknown fields, spoofed ids, Unicode bounds, oversize, deadline), envelope serialization for every variant | p01 |
| D3 fields | `channel_call_intake_` compiler bounds, activation refusal, mail without fields, `channel.deny` validation | p01; snapshot capture in p02 |
