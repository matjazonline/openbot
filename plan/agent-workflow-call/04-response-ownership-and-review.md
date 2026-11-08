# 04 — Response ownership and review

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on: 03. Outcome: the destination channel produces one response for the pending tool call,
with its existing response mapping and approval requirements intact.

## Batch A — Share final-response preparation with a tool destination

1. Trace channel reply terminal handling, contract projection, response disposition, review,
   publication, and delivery ownership. Extend response ownership with explicit channel-ingress
   and agent-call destinations. Share the existing projection/review decisions; do not create a
   second approval engine or forge `channel_reply_contexts` as if external ingress occurred.
2. Tie the agent-call destination to phase 02's frozen binding/settings revision and saved call.
   Database guards must prove company, child, parent execution, and response ownership agree.
   A normal channel observer, manual run, or different call cannot publish this response.
3. Validate final child output against its workflow output schema, then evaluate the captured
   channel response contract. Produce canonical bounded response content. Raw output is internal;
   return only the response projection the selected channel policy permits.
4. `channel.no_reply` returns `no_reply` to the tool caller without suppressing the parent run's
   own eventual response. Child failure, invalid projection, or oversized response is a typed
   error, not an empty body or fabricated successful reply.
5. Automatic final publication for this destination commits a tool-response receipt and wakes
   the parent. It creates no outbound transport delivery or external response obligation. Existing
   explicit `message.send` and non-source `message.reply` calls retain their own effect policies;
   do not misrepresent a channel call as a sandbox that disables all workflow effects.
6. A reply to the call's synthetic source must be routed to its call response owner, not guessed
   from an external sender address. Trace `message.reply`/`message.ask` and native templates;
   support their existing human-input/continuation semantics without a fake email recipient,
   deadlock against the waiting parent, or more than one final call result. Unsupported interactive
   shapes must fail explicitly before creating a wait that no authorized participant can settle.

Focused checks: `channel_call_response_`, affected channel response terminal/contract tests, and
`channel_reply_hitl_resume_child_question_`. Cover native and custom workflows, empty/no-reply,
invalid output/projection, suppression isolation, direct reply-to-source behavior, and a child
with an explicitly authorized independent effect. Assert exact response-owner and delivery rows.

## Batch B — Review, confidentiality, and final result delivery

1. Reuse the destination's configured final review, reviewers, deadline, edits, and artifact/revision
   checks. Child workflow completion must not release an unapproved response to the model. Display
   that approval will return content to a calling agent, not send an email to a channel customer.
2. Only the approved response version becomes the tool result. Rejection/expiry/cancellation
   terminates the call with a typed outcome and wakes the eligible parent. Replayed approval or a
   stale review submission cannot publish a different response or consume approval twice.
3. Keep ordinary configuration edits from replacing captured content/policy. Apply existing live
   reviewer and binding revocation rules when approving and releasing. Pending review must have a
   bounded deadline and remain visible/recoverable through existing decision operations.
4. Propagate the child's consumed content classification to the parent atomically before storing
   or exposing the result to the model. Revalidate caller read authority and return audience; no
   lower label is justified merely by projection or summarization. Preserve simulation restrictions.
5. Maintain separate private history branches: the child receives its own call source/history and
   selected agent context; the parent receives the approved tool response, not the child's private
   transcript, system prompt, tool secrets, or unrelated target-channel conversations.
6. Retain call source, approved response, invocation snapshot, and pending review dependencies while
   either side needs them. Include deletion, cancellation during review, and retention sweep races.

Focused checks: `channel_call_review_` and `channel_call_privacy_`. Race competing approvals,
approval versus cancellation/revocation, and result consumption versus access removal. Verify edited
approved text is exactly what the resumed model sees and that raw output never leaks through a
tool result, sample preview, notification, run view, or logging path.

## Acceptance and record

- Automatic and reviewed responders both return through the pending call, with no duplicate final
  response and no automatic external delivery.
- Approval, no-reply, human-input, authority loss, and classification have positive and negative
  end-to-end controls, not just sequential policy mocks.
- Record focused commands/counts/results and any explicitly unsupported interactive workflow shape.

Implementation evidence: pending.
