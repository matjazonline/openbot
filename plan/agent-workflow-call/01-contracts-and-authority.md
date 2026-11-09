# 01 — Contracts and authority

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on: none. Outcome: a precise channel-call contract that can be implemented without
weakening tool grants, channel access, or existing response approval.

## Batch A — Define the call and result

1. Add `ChannelCall` / `channel.call` to the existing built-in capability owner. Its trusted tool
   snapshot uses the same canonical content parser and limits as messaging, but has an explicit
   child-call effect and durable continuation semantics. Reserve the name across tool registries.
2. Define application-owned request, resolved-target, invocation identity, and result types.
   Use existing channel/binding/run/call identifiers and typed revisions. Avoid untyped strings
   and a collection of behavior flags. The call accepts exactly `channel_id`, `content` and an
   optional `deadline` (RFC 3339), refused unless earlier than the parent run's deadline.
3. Define a bounded result envelope: `replied` with approved canonical response content, child
   run ID and a `closure` of `done`, `denied { reason }` (reason bounded like a subject) or
   `handed_off { run_id }`; `no_reply` with child run ID; or an error with a stable code and
   bounded safe detail. The closure is produced by the destination's response contract (a
   `channel.deny` built-in beside `channel.no_reply`, and `handed_off` when the final response
   came from a nested channel call), never parsed from the body.
   No successful `accepted` result while the child or its review is still pending. Do not expose
   unreviewed workflow output, hidden policy, credentials, or inaccessible channel metadata.
4. Define errors for unavailable/unauthorized target, invalid input, dependency/recursion limit,
   revoked authority, child failure, cancellation, deadline, rejected/expired review, and result
   overflow. Ordinary domain failures reach the model as tool errors. Ownership loss is a worker
   lifecycle outcome, not a successful tool result or permission to repeat effects.
5. Specify the shared input adapter: create canonical source facts from the caller, normalize
   body/subject with `MessageTriggerInput`, and validate the resulting value against the frozen
   destination workflow schema. No raw JSON-to-`params` merge or arbitrary workflow-ID override.

Focused checks: `channel_call_contract_` tests for valid/invalid/unknown fields, identity spoofing,
canonical bounds including Unicode and oversized input, exact built-in snapshot identity, and
every success/error variant. Run the existing built-in exactness tests. Compile all targets to
find enum matches and mocks; do not run the full suite for this batch.

## Batch B — Authority, response, and dependency invariants

1. Model the call as a capability of the selected published agent, tied to its actual saved model
   tool call, parent execution, and live attempt fence. A string naming `channel.call` or a forged
   call ID must not grant authority. Reuse action-origin validation and tool approval machinery.
2. Resolve only the selected active responder in the same company. Use existing workflow admission
   authority and destination-channel visibility. The current admission policy requires company
   operations authority; changing that policy is outside this feature. Missing and inaccessible
   targets must have indistinguishable public failures.
3. Execute as the parent actor. Check target binding resources, destination agent grants, models,
   skills, connections, and knowledge access for that actor. Do not adopt the target's `run_as`
   identity. Reuse existing permission owners rather than duplicating member/channel checks.
4. Snapshot selection at the first committed dispatch; later ordinary configuration edits affect
   later calls only. Revocation/deactivation/deletion and caller-access loss remain live authority
   events: refuse new calls and prohibit further protected effects/result disclosure on existing
   calls. Specify how refusal wakes the parent so it cannot remain parked indefinitely.
5. Separate causal root from dependency-scope root. A channel child is a permitted new scope;
   ordinary child workflows inherit their nearest scope. Its descendants still count against the
   original tree's depth/count/deadline/cancellation budgets. No sealed manifest may be extended.
6. Define tool-response ownership as a destination of the channel response pipeline. The selected
   contract projects child output, configured review still applies, and approved response content
   goes to the tool call. Child `channel.no_reply` does not suppress the parent channel response.
   Existing explicit message steps keep their independent semantics and controls.
7. Treat request content, target workflow responses, and any target history as untrusted data.
   Preserve knowledge classification on request admission and on return before the model resumes.
   Target channel visibility may widen the request's audience: reject before persistence if the
   caller's consumed content cannot be shared with that audience.

Focused checks: `channel_call_authority_` pure policy matrices and database-backed controls for a
valid authorized caller, a non-operations member, private/foreign channels, forged origins, differing
`run_as`, restricted resources, and audience/knowledge rejection. A positive control must prove
the contract is usable, not merely that all calls fail.

## Batch C — Intake and result fields

A desk's procedure can declare the structure it wants in and the structure it gives back, so a
calling agent prepares the request in that shape and reads the answer without parsing prose. Mail
and `message.send` are unchanged: they carry subject and body only.

1. `MessageTriggerInput` (`src/application/workflow/triggers.rs:20`) gains `fields: Option<Value>`,
   absent for mail and present for a call. Its `schema()` and `representative()` include it as
   optional. A message-triggered version's `input_schema` may declare a `fields` sub-schema, the
   intake; binding activation (`src/application/workflow/binding/mod.rs:107`) still validates the
   representative input, so a version whose `fields` is required is refused with
   `binding.intake_fields_must_be_optional`, because mail cannot supply them. Required properties
   inside `fields` are allowed.
2. The compiler (`src/application/workflow/compiler/schema.rs`) bounds the intake sub-schema the
   way tool contracts are bounded: at most 32 properties, depth 2, types string, number, boolean,
   enum and array of strings, each with a `description`, total under 16 KiB. Anything else is a
   publication error naming the path. The same rule bounds the result sub-schema in item 5.
3. The call accepts `fields` beside `content`. It is validated with `jsonschema` against the intake
   captured in the target snapshot ([02](02-target-snapshots-and-dependency-scopes.md), Batch A
   item 4) before any child, thread or source message exists. A mismatch is the typed error
   `intake_invalid`, carrying the intake schema and the failing instance paths, so the model
   corrects the call and retries. `fields` on a target with no intake is `intake_not_declared`.
   The validated object is stored as part of the canonical source message and reaches the child as
   `/input/fields`, never merged into `params`.
4. The agent learns the intake before calling. The agent directory entry
   ([01-departments](../01-departments/03-leads-and-coordination.md), Batch 03B item 1) carries each
   desk's intake schema beside its address, and the `channel.call` tool description tells the model
   to read it there. `fields` never selects the procedure: the desk's selected binding does.
5. The symmetric half. The `reply` step and `channel.deny` accept `fields`, validated at the step
   against the version's `output_schema` `fields` sub-schema (same bounds; a `reply` without
   `fields` on a version that declares them is valid, since a procedure may answer in prose). The
   response contract projects them unchanged, and the `replied` envelope returns `fields` beside the
   body and closure. Review (04, Batch B) shows and may edit them with the body; an edit is validated
   against the same schema before approval. Over mail the fields are not rendered; the body is the
   answer.
6. Both schemas are part of the frozen version and the target snapshot. Changing either republishes
   the destination; an in-flight call keeps the schema it was validated against.

Focused checks: `channel_call_intake_` for schema bounds at publication, the optional-`fields` rule
at activation, valid and invalid calls with exact error paths, a target without intake, mail
admission with absent fields, `/input/fields` reaching the child unchanged, and result fields
validated at `reply`, surviving review edits, and returned in the envelope. Unit tests for the pure
validators; one database test for the snapshot capture.

## Acceptance and record

- Contract examples work for a normal native channel responder and a message-compatible custom
  workflow; each can call without a workflow-tool registration.
- Publication, execution authority, binding configuration, dependency scope, and response ownership
  are named facts with clear owners. No alternate general-purpose workflow service is introduced.
- Record exact focused commands/results and any design changes here during implementation.

Implementation evidence: pending; no feature tests have been run for this planning document.
