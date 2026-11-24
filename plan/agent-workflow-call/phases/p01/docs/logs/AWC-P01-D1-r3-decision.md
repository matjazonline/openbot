# AWC-P01-D1 r3 — architecture gate: authority, call contract, intake/result fields

Author: root/A (Claude Code `1e84a7af`, Opus 5.5/xhigh), 2026-11-22, anchors at `fe7791d3`.
Supersedes [r2](AWC-P01-D1-r2-decision.md) and [r1](AWC-P01-D1-r1-decision.md), both kept
unchanged. Answers review `AWC-P01-D1-R1-N1`, findings F1–F8 (dispositions at the end).
**r3 changes only D1.4 and its verification row, following user message U4**; everything else is
r2's text.

## User decisions (verbatim)

- **U1** (2026-11-22). Q: "When an agent calls another desk with channel.call, whose authority
  does the child run use? … Plan 11 proposal 1 (unsettled) says the destination's binding runs as
  configured, with its own run_as, and the caller is recorded only as the requester." Option
  chosen: **"Destination's run_as now"**, whose description read: "Adopt plan 11 proposal 1 now.
  Without request rules, the only gate would be today's operations authority plus desk
  visibility, so a caller can cause work under someone else's identity."
- **U2** (2026-11-22, after F4). Q: "With the destination's run_as decided, which check decides
  whether an agent may call a desk? In every option, the person behind the calling run must still
  be a live company member, as for any tool call today." Option chosen: **"Agent's write right
  (Recommended)"**: "The calling agent may call a desk it may already message: any public desk,
  or a members-only desk it holds a grant on. The person behind the run isn't checked further,
  and nobody is exempt, the owner included. This matches plan 11, whose request rules later
  replace this check." Rejected: write right plus the actor seeing the desk; operations
  authority plus desk view (the U1 description).
- **U3** (2026-11-22, after F2). Q: "How high may a called desk's run read while answering a
  channel.call, until plan 11 adds a per-unit cap?" Options were "Level 0 until plan 11" and
  "Lower of caller and run_as". Answer (free text): **"we respect plan 11 which will be
  implemented"**. Interpretation recorded and reported to the user: use plan 11 03C's formula,
  lowest of (caller ceiling, allowance, run_as level). The allowance term does not exist until
  plan 11, so the code takes it as an explicit input that is currently absent.
- **U4** (2026-11-22, while r2 was under review, verbatim): "it's ok if it's easier to respect
  plan 11 even if app doesn't function properly until it's implemented". This settles U3 strictly:
  plan 11 admits nothing above what a rule allows, and no rule exists yet, so the allowance
  today is `PUBLIC`.

## D1 — Authority model

### D1.1 Admission order

Every check reads current facts in one transaction at first dispatch; 5–8 are re-read at launch
(03). Each check runs only if all earlier ones passed, so a later answer never reveals an
earlier refusal. An **error** means the caller must fix the call. A **denied** closure means
the destination declined it (D2).

1. **Shape** (no target facts): exact input fields, canonical content, `fields` is an object,
   `deadline` parses and is strictly earlier than the calling run's deadline. Errors:
   `channel_call.invalid_input`, `channel_call.deadline_invalid`.
2. **Capability and caller liveness.** The saved call is a call of the `channel.call` built-in
   in the calling agent's frozen capability snapshot, validated by the origin owner
   (`agent_origin`, as `workflow_tool_on`). It runs under the live fence, with the run's actor
   still live through `action_authority::prelock_on`, the same as every tool call (U2). The
   agent passes `agent_authority::check_execution_on(.., AgentUseMode::Tools)`. A failure is
   the existing origin/authority refusal, never a tool result.
3. **Desk and write right (U2).** The channel exists in the same company, and the calling
   agent's principal may write to it. The predicate is the one an internal relay applies to that
   agent as sender: `IngressOrigin::Internal { agent_principal }.internal_context()` evaluated
   by the desk's participant access, as in `routing.rs` `authorize_channel`. A public desk
   accepts any company agent; a members-only desk needs the agent principal's grant. The run's
   actor is not consulted, and no actor is exempt. If the desk is missing, foreign or
   unwritable, the caller gets one error, `channel_call.target_unavailable`.
4. **Cycle.** The desk is not in the calling run's active channel-call ancestry, including a
   self-call. Error: `channel_call.cycle`.
5. **Destination state.** Denied closures: an archived unit gives `unit.archived`; a disabled
   desk gives `channel_call.desk_disabled`; no selected responder, or one that is inactive or
   not message-triggered, gives `channel_call.no_responder`.
6. **Intake** (against the captured snapshot, 02). `channel_call.intake_not_declared` and
   `channel_call.intake_invalid` (with `schema` and the failing `paths`) are errors.
7. **Levels** (D1.4). A request above the child ceiling is the error
   `channel_call.audience_refused`.
8. **Destination admission** (03, at launch, using the same owners a message admission uses):
   - `run_as` authority (`management::authorize_association_on`), `bindings::verify_selection`,
     resource readiness at the admission stage, and role resolution. A failure is the denied
     closure `channel_call.destination_unavailable`, with one fixed reason that names no
     configuration.
   - The destination unit's spend cap (07, `admission_write.rs`) is the denied closure
     `budget.unit_exhausted`.
   - The caller's tree limits (depth, child count, continuations) are the existing error
     `workflow.tool_limit`.

Nothing is stored for a refusal at any step: no child, thread, message, job or invocation row.
The tool result is the only output, and a denied closure before launch has no `child_run_id`.

### D1.2 Which facts come from where

The child is admitted the way a message on the destination desk would be, and stays inside the
caller's tree:

| Fact | Source |
| --- | --- |
| binding, revision, params, resources, escalation policy | destination's selected binding (snapshot, 02) |
| `actor_id` | destination binding's `run_as` |
| unit scopes (memory, knowledge, tools) | computed for the destination desk as a message-admitted run's; **not** `run_scopes::copy_child_on` |
| agent/workflow selections | the child's own dependency scope captured at first dispatch (02 B) |
| role resolution and escalation | destination desk's chain, as for its own runs |
| spend | charged to the destination unit chain (07) |
| lineage, cancellation, correlation id | caller's tree (`ChildCause::ToolCall`, `parent_run_id`) |
| run deadline | earlier of the caller's run deadline and the call's `deadline` |
| depth, child count, continuation limits, root budgets | caller's tree, unchanged (a nested scope never resets them) |
| `delivery_policy` | **inherited from the caller**, as `children.rs` does: a simulated caller cannot cause real effects under the destination's identity |
| knowledge ceiling | D1.4 |
| goal (Plan08 p03) | out of scope here; p03 adds it to the invocation path |

### D1.3 The requester

The invocation row (02 A) names company, calling run, execution, saved call id and calling agent;
`parent_run_id` keeps lineage. The caller's actor gains no grant, no thread read beyond the call's
own result, and no resource access on the destination.

### D1.4 Knowledge levels (U3)

- **Child ceiling** = lowest of (calling run's current ceiling, allowance, `run_as` person's
  level from `actor_level_on`). The allowance is one explicit `AccessLevel` input. Until plan 11
  03C supplies it from a request rule it is `AccessLevel::PUBLIC` (U4), so today the child reads
  public data only. This is the same as an agent-to-agent message today and is no new exposure.
  The consequence is accepted: a caller that has consumed anything above level 0 gets
  `audience_refused`.
- **Child destination audience.** A call child's output goes back to the calling run and is
  projected onto its fresh destination thread. There is no transport delivery, and viewers below
  a message's level see a placeholder (`knowledge_e2e_level_five_team_reply_…`). So the child's
  destination level is its own ceiling, lowered by the least-cleared human reader that later
  joins its thread, if any. `knowledge::destination_level_on` gains this case, keyed on the
  child's invocation row (03). Without it, an agent-only thread would force level 0 and the
  ceiling above would never take effect (F2).
- **Request release.** The request message is stamped with the calling run's current content
  level and exactness. It is stored only when that level is ≤ the child ceiling; otherwise the
  error `audience_refused` comes back before anything is stored. No other release check applies,
  because a desk destination has none today.
- **Result.** The child's level and exactness are absorbed into the caller with
  `knowledge::absorb_content_on`, as for workflow tools. The level is ≤ the child ceiling ≤ the
  caller's ceiling.

### D1.5 Live authority after admission

The snapshot freezes ordinary configuration. These stay live and are re-read when the result is
consumed (03 B.5):

- the calling agent's capability and execution authority (D1.1.2);
- its write right on the desk (D1.1.3);
- the desk enabled;
- the destination binding not deactivated or deleted.

Losing any of them settles the call with `channel_call.revoked` and discloses no result. The
parent is woken through the child-terminal path and is never left parked.

Archive interaction (F7): archiving the destination unit while a call child is open is refused
as open work. Plan08 p02's cancel-and-archive cancels root runs on the subtree desks; 03 extends
it to cancel open call children on those desks. Their callers then receive the existing
`workflow.child_cancelled`.

### D1.6 Plan 11 hand-off

- 03B: replaces check 3 with the request rule; the asking unit is the calling run's unit.
- 03C: supplies the allowance in D1.4.

### Invariants

- **I1.** The child holds exactly the authority its destination's responder has for a message on
  that desk. It reads no higher than its ceiling.
- **I2.** A caller that may not write to a desk learns nothing about it beyond the single
  `target_unavailable` error. Every later, more specific answer requires check 3 to pass.
- **I3.** Nothing above the caller's ceiling reaches the caller.
- **I4.** A refused call leaves no rows.
- **I5.** A simulated caller produces only a simulated child.

## D2 — Call and result contract (model-visible)

Input (`deny_unknown_fields`):

```json
{"channel_id": "<uuid>", "content": {"subject": "...", "body": "..."},
 "fields": {"...": "..."}, "deadline": "<RFC 3339>"}
```

`fields` and `deadline` are optional. Subject and body use `message.send`'s canonical parser and
bounds.

The result is a tool value of at most `MAX_TOOL_RESULT_BYTES`. Its top-level `closure` says how
the work ended (F1):

```json
{"closure": "done", "child_run_id": "<uuid>", "content": {"subject": "...", "body": "..."}, "fields": {}}
{"closure": "handed_off", "child_run_id": "<uuid>", "run_id": "<uuid>", "content": {...}, "fields": {}}
{"closure": "denied", "code": "<stable code>", "reason": "<bounded>", "child_run_id": "<uuid>", "content": {...}}
{"closure": "no_reply", "child_run_id": "<uuid>"}
{"error": {"code": "<stable code>", "message": "<bounded safe text>"}}
```

- `fields` appears only when the reply has them.
- `denied` has two producers:
  - the destination's procedure, through `channel.deny`: code `channel.denied`, with
    `child_run_id`, `content` and optional `fields`;
  - destination admission (D1.1 steps 5 and 8), before any child: no `child_run_id` and no
    `content`.
- `reason` is bounded like a subject.

This replaces the README's "`replied` carries a closure" with "the closure is the result kind". It
is the shape Plan08 02B.3, 03C.3 and plan 11 03B.3 call a denied closure. The README is updated
in the same change.

Error codes (caller-fixable or lifecycle), in D1.1 order:

- `invalid_input`, `deadline_invalid`
- origin refusal (not a tool result)
- `target_unavailable`, `cycle`
- `intake_not_declared`, `intake_invalid`
- `audience_refused`
- `workflow.tool_limit`
- after launch:
  - `revoked`
  - `workflow.child_failed`, `workflow.child_cancelled`
  - the existing child-deadline outcome (F6; 03 confirms whether the stored terminal fact
    distinguishes deadline from failure, and adds `workflow.child_deadline` only if it does)
  - `review_rejected`, `review_expired` (04)
  - `workflow.tool_result_too_large`

The `channel_call.` prefix applies to the new codes. There is no `accepted` result, and ownership
loss is a worker lifecycle outcome.

**Closure producers (decidable, F6):**

- `done`: the final output projected by the selected `ChannelReplyContract`.
- `denied`: the new terminal step `channel.deny`, the sibling of `channel.no_reply`. It requires
  success to `$end`. Its output is `{reason: string ≤ subject bound, fields?: object}`. Its
  response content is the subject `Re: <source subject>` (bounded) with the body `reason`.
  - Over mail, that content is published as the reply, under the same review as any final
    response.
  - Over a call, it is the denied closure with that content.
- `handed_off`: the new terminal step `channel.hand_off`, success to `$end`, whose `with.run_id`
  must resolve to the `child_run_id` of a `channel.call` invocation made by this same run.
  Runtime validation reads the invocation rows (04). Its response content is the projected reply
  of that nested call. Over mail it publishes that content like `done`.
- `no_reply`: `channel.no_reply`, unchanged.

## D3 — Intake and result fields

- `MessageTriggerInput` gains `fields: Option<Value>`. It is absent for mail and relays and
  present only for a call. `schema()` declares it optional; `representative()` omits it.
  Contract impact (F8): `schema()` is also the reserved `message.reply` / `wait.event` payload
  (`registry/actions.rs`, `registry/mod.rs`, `waits.rs`). Those payloads never carry `fields`,
  and their exact-equality checks keep comparing against the one function.
- A message-triggered version may declare `input_schema.properties.fields` (the intake) and
  `output_schema.properties.fields` (the result fields). One new compiler rule bounds both: at
  most 32 properties, depth 2, types string/number/boolean/enum/array of strings, each with a
  `description`, under 16 KiB serialized. Anything else is a publication error naming the path.
  No existing tool-contract bound matches the plan's wording; this rule is new.
- Binding activation keeps validating `representative()`. A version that makes `fields` required
  is refused with `binding.intake_fields_must_be_optional`.
- A call's `fields` is validated against the snapshot's intake before any row exists. It reaches
  the child as `/input/fields`, never in `params`.
- The registry has no `reply` step. Result fields are the final output's `/fields`, validated by
  the output schema and returned unchanged. `channel.deny` validates its optional `fields`
  against the same sub-schema.
- **Discovery (F3).** `list_company_agents` today gives an agent's own desk (`desk: {id,
  address}`) and its departments without their desks. Phase 05 A extends it:
  - each agent's desk and each department's desks carry `callable: bool` and `intake` (when
    declared);
  - both are present only for desks the calling agent passes check 3 on, so I2 holds.

  The `channel.call` tool description points there. "No schema discovery round trip" in 05 A.4
  means no per-channel tool registration; the directory is the discovery. The plan text is
  amended to say so.

## Verification plan

| Decision | Proof | Phase |
| --- | --- | --- |
| D1.1 order and I2/I4 | `channel_call_authority_` pure matrix over the ordered checks, in which each step fails alone and an earlier failure hides a later one. DB controls: an authorized agent on a public and on a members-only desk (positive); a members-only desk without a grant; foreign; disabled; archived unwritable (gives `target_unavailable`) vs archived writable (gives `unit.archived`); no responder; a forged origin; the owner's run with an unwritable desk (no exemption); zero rows on every refusal | p01 pure; p02/p03 DB |
| D1.1.8 denied closures | destination `run_as` loses authority; unit cap exhausted; caller at depth limit | p03 |
| D1.2 facts | child `actor_id` = destination `run_as` ≠ caller actor; scopes = the destination desk's; resources = the destination binding's; simulated caller gives a simulated child with no delivery rows | p03 `channel_call_launch_` |
| D1.4 levels | pure: ceiling = min of the three in every order, and the allowance below the others decides it. Today's production allowance is `PUBLIC`: a request at level 0 is admitted; a request above 0 is refused with zero rows; the child's effective read ceiling is 0. The destination-audience case is unit-tested with a raised allowance, so plan 11 inherits it. A human joining at a lower level lowers the destination; result absorbed | p01 pure; p03/p04 DB |
| D1.5 live authority | grant removed / desk disabled / binding deactivated while waiting gives `revoked`, no disclosure, parent woken; cancel-and-archive cancels the call child, and the caller gets `child_cancelled` | p03, p06 |
| D2 contract | `channel_call_contract_`: parse matrix (unknown fields, spoofed ids, Unicode bounds, oversize, deadline equal/later/earlier); serialization of every closure and error | p01 |
| D2 producers | `channel.deny` routing/validation and mail reply; `channel.hand_off` refusing a run id not made by this run | p01 (compile), p04 (runtime) |
| D3 | `channel_call_intake_`: compiler bounds naming paths; activation refusal; mail input without `fields`; `message.reply` payload unchanged; directory filtering (p05) | p01, p05 |

## Finding dispositions (r1 → r2)

| Finding | Disposition |
| --- | --- |
| F1 | D2: closure-first envelope; denied before launch with no child; destination-admission codes D1.1.8 |
| F2 | D1.4: destination audience defined; nonexistent desk release check removed; I1 rewritten |
| F3 | D3 Discovery, filtered by check 3; plan contradiction resolved and amended |
| F4 | U2 asked and recorded verbatim; D1.1.2 states the actor liveness that remains |
| F5 | D1.2 table, `delivery_policy` inherited (I5) |
| F6 | D1.1 order; closure producers `channel.deny` / `channel.hand_off`; deadline code note |
| F7 | D1.5 archive interaction; reachable revocation cases listed |
| F8 | D3 contract-impact note |
