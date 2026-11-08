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
   and a collection of behavior flags. The call accepts exactly `channel_id` and `content`.
3. Define a bounded result envelope: `replied` with approved canonical response content and child
   run ID; `no_reply` with child run ID; or an error with a stable code and bounded safe detail.
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

## Acceptance and record

- Contract examples work for a normal native channel responder and a message-compatible custom
  workflow; each can call without a workflow-tool registration.
- Publication, execution authority, binding configuration, dependency scope, and response ownership
  are named facts with clear owners. No alternate general-purpose workflow service is introduced.
- Record exact focused commands/results and any design changes here during implementation.

Implementation evidence: pending; no feature tests have been run for this planning document.
