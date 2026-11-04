# 05 — Human input, approval, and workflow continuation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [04](04-channel-publication-and-delivery.md). One workflow can ask several questions,
wait for decisions, resume, and eventually produce one automatic final response. Each prompt has
its own publication/delivery outcome; prompt delivery never resolves the human wait.

| Event | Workflow state | Channel/message responsibility |
| --- | --- | --- |
| Human input requested | Waiting | Publish the declared prompt or reviewer notice |
| Prompt accepted by transport | Waiting | Record delivery; keep the human wait open |
| Authorized response accepted | Queued/running continuation | Resolve that wait once and resume the same run |
| Ordinary execution pause | Waiting | No implicit message without a declared communication |
| Successful finalization | Succeeded | Publish the final response, unless suppressed |
| Prompt failure or uncertainty | Waiting until resolved/deadline | Show delivery problem; do not rerun completed business work |

## Batch 05A — Integrate prompt and approval publication

1. Reuse [workflow decisions](../../src/application/workflow/decisions.rs), existing decision
   notices, action approvals, and durable waits. A declared human-input request supplies its
   prompt/proposal, response schema or choices, permitted respondent/reviewers, deadline, and
   exact continuation identity. A generic timer/event/child wait does not create a question.
2. For a channel-managed question, atomically register its wait and durable prompt intent before
   the question can be externally published. Associate each published prompt with that wait
   occurrence and expected respondent. Deduplicate reviewer notices by decision and reviewer;
   deduplicate conversation prompts by wait occurrence and recipient.
3. Consolidate existing decision-notice delivery with the shared publication machinery only where
   ownership fits. Existing private reviewer notices must remain private: never post approval
   tokens, confidential proposals, or reviewer-only context into a public channel/customer thread.
   Runs without a channel keep their existing authorized standalone reviewer-notice path.
4. Preserve the existing explicit `message.reply`/`message.send` followed by reply-wait pattern.
   Its bounded early-reply deferral remains necessary because publication precedes registration
   of the later wait. Do not silently make every agent-tool message resumable; correlation must
   be backed by a declared wait and the appropriate authorization.
5. Complete the automatic-final-response approval integration from phase 03. Approval parks and
   reopens the same terminal execution through existing continuation accounting. On acceptance,
   finalize the exact approved artifact and durable final intent; on rejection/timeout use the
   established outcome/error route. Approval notices are distinct from the final customer reply.
6. Resolve decisions before response publication when policy requires it. Notifications of an
   approval request cannot recursively require the very decision they are asking for: preserve
   the existing trusted notice boundary without broadening it to arbitrary outbound content.

Focused check: `channel_reply_hitl_prompt_` and `channel_reply_final_approval_` covering atomic
park/intent creation, duplicate park, private recipients, frozen artifact acceptance, edited
artifact rejection, approved finalization, rejection, and repeated human rounds.

## Batch 05B — Correlate responses and resume once

1. Extend [reply correlation](../../src/adapters/persistence/workflow/reply_waits.rs) to recognize
   channel-managed prompt origins through the same trusted source/destination model. Use tenant,
   run, prompt identity, wait occurrence, responder, and deadline; never thread ID alone.
2. Preserve the current ordering in [message admission](../../src/application/workflow/message_admission.rs):
   offer an incoming response to its eligible live wait before ordinary trigger admission. A
   matched response is consumed once and does not also create another run/automatic reply for
   that association. Independent channels and independent messages remain independent.
3. Accept a valid response even if the provider acknowledgement has not yet been recorded: the
   durable prompt/wait identity establishes correlation. Do not require a `delivered` flag that
   can lag the actual message. Preserve durable bounded deferral for the existing split-step path.
4. Keep formal decision submission on its authorized, revision/digest-checked path. Plain reply
   text or a comment cannot approve a protected action. A model cannot manufacture a decision,
   broaden the reviewer set, or resume another execution.
5. Preserve first-valid-submission ownership, duplicate replay, stale/closed decision rejection,
   existing second/unmatched-reply admission policy, and producer/consumer lock order. Late
   responses cannot reopen an expired/cancelled run. Repeated waits must have distinct identities.
6. Retry a safely retryable failed prompt under the same wait and deadline. Do not reset deadlines
   indefinitely or count a delivery retry as a workflow continuation. Expiry/cancellation stops
   unsent prompt work through existing delivery controls; uncertain in-flight sends remain
   honestly uncertain. Human rejection/timeout never fabricates success.
7. Maintain parent/child behavior: a child HITL decision resumes its child, then the existing
   child-completion path resumes the parent. Only the designated root owns its final automatic
   reply; child questions and final replies cannot steal that ownership.

Focused check: `channel_reply_hitl_resume_` with two competing human responses; reply versus
timeout/cancel; response arriving before send acknowledgement or split-step wait registration;
duplicate input; foreign/unauthorized responder; two live runs in one thread; repeated waits;
parent/child decisions; and reconstruction after a committed decision before worker wakeup.

## Acceptance

- Sent prompt, accepted human decision, workflow success, and final delivery are separately
  observable facts. A sent prompt alone advances no business or approval step.
- A human continuation retains the original run/result contract and cannot regenerate an old
  prompt or final response accidentally.
- `channel.no_reply` affects only automatic final response; it neither suppresses required HITL
  communication nor cancels published intermediate messages.
- Run the existing `message_reply_resumes_its_wait_once_and_a_second_reply_is_ordinary`,
  `workflow_action_approval_message_then_effect_wait_preserves_both_histories`, decision race,
  reply-ingress, wait-deadline, and child-continuation tests alongside focused new coverage.
  Follow [07](07-integration-and-verification.md) for database isolation and evidence.
