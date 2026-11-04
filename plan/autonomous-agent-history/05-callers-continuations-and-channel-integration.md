# 05 — Callers, continuations, and channel integration

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [04](04-automatic-context-and-frozen-preparation.md). All eligible agent producers use
the same history contract. Human waits and children preserve execution ownership, and channel
publication remains independent from the agent's accepted output.

## Batch 05A — Integrate producers and execution variants

1. Inventory every producer of `agent.run`: message admission, manual/API starts, scheduled runs,
   repeated steps, dynamic agent selection, explicit child workflows, workflow-tool children,
   provisioning/templates, samples, and tests. Use graft callers plus exhaustive indexed searches;
   do not stop after the ordinary channel entry point compiles.
2. Carry trusted conversation/session identity through admission, run snapshots, activation,
   preparation, and retries. Select the stream for the actual resolved agent, not its source alias.
   Preserve the recorded agent revision and current authorization rules from the checked-out
   implementation and [autonomous-agent plan](../autonomous-agent/03-workflow-references-and-run-snapshots.md).
3. Sequential calls to the same agent in a root conversation share completed exchanges, even
   across different workflow runs. Different agents have separate histories. Repeated activations
   in one workflow are distinct exchanges; a retry of an activation is the same exchange.
4. Standalone calls use the private session contract from phase 01. Validate reuse through existing
   caller authority and tenant checks. A guessed ID or copied context JSON cannot join a session.
   Preserve newly allocated identity across admission replay and worker reconstruction.
   Reusing a session never raises a caller's ceiling. A level-0 caller who is authorised to reuse a
   session containing level-5 exchanges loads only its level-0 exchanges. Scheduled runs take
   their ceiling from the binding's `run_as` actor, and a run with no actor is at level 0
   (knowledge plan 03A).
5. Allocate children a private branch at their existing durable admission boundary. Calls inside
   that child can share the branch, but the parent transcript is not inherited automatically.
   Parent context can reach the child through explicit input; child results enter parent history
   only as part of input/output the parent actually receives/produces. Do not copy child internal
   exchanges into the parent or allow same-stream reentrant acquisition while the parent waits.
   A child branch inherits its parent's knowledge ceiling and starts at the parent's content level
   when the call is made, so its exchanges are stamped at least that high. The child's result
   raises the parent's content level when the parent consumes it (knowledge plan 03B).
6. Make sample history sealed and ephemeral within the existing sample policy. A sample run may
   demonstrate repeated calls in its own session; it cannot load production history, create a
   production exchange, or advance a production conversation queue. Extend structural boundaries
   and fixtures as well as runtime checks.

Focused checks: `agent_history_callers_`, `agent_history_children_`, and `agent_history_samples_`
for every producer, standalone reuse/refusal, dynamic selection, sequential/repeated calls,
private branch recovery, same-agent parent/child progress, and production/sample separation.
Add `agent_history_level_` cases for every producer's ceiling, session reuse at a lower level,
and child inheritance of ceiling and content level.
Run affected admission, child-tool, provisioning, template, and sample-boundary regressions.

## Batch 05B — Preserve human input and channel semantics

1. Keep human/approval continuation within the original active exchange when it resumes the same
   agent activation. Accepted human answers and tool results remain in its existing durable
   checkpoint/decision path. A subsequent `agent.run` activation saves whatever structured input
   the workflow supplies, including an accepted human response where explicitly bound.
2. Route correlated authorized replies to live waits before treating them as ordinary new work,
   using [message admission](../../src/application/workflow/message_admission.rs) and
   [reply waits](../../src/adapters/persistence/workflow/reply_waits.rs). A human response needed by
   the active exchange cannot wait behind that same exchange in the conversation queue.
   Do not turn ordinary text into approval or let a continuation bypass its original authority.
3. Preserve checkpoint history for repeated human rounds, private decision visibility, deadlines,
   early replies, first-valid-submission ownership, and current revocation. A resume neither
   refreshes the selected seed history nor inserts another copy of the original logical input.
4. Keep agent output and channel message identities separate and link them where publication has
   real provenance. The channel may project/transform the output or choose not to publish it.
   Neither publication nor delivery writes another agent exchange. Published content is not
   substituted for the saved agent output, and suppression never removes a valid agent output.
   Every publication and every human-input prompt is an egress through the knowledge plan's
   release check (03D), with the destination audience resolved at publication time. A refused
   release (the audience is below the exchange's level) leaves the exchange completed and stamped.
   Only later calls whose ceiling admits it can select it. The refusal surfaces as
   `knowledge.audience_below_content`, never as a substituted or truncated reply.
5. Coordinate with [channel publication](../channel-handles-send/04-channel-publication-and-delivery.md)
   and [human continuation](../channel-handles-send/05-human-input-and-continuations.md) if those
   features have landed. Test raw output versus projected reply, explicit sends, `channel.no_reply`,
   and delivery failure independently. If that plan is still pending, test existing messaging and
   application boundaries and record its future integration cases as unexecuted dependencies;
   do not claim channel auto-send/no-reply coverage before those features exist.
6. Keep `context.history` available as explicit thread data. Remove redundant history-loading
   steps from default templates only where they were providing agent continuity and no other
   business data. Do not deduplicate arbitrary workflow context by content guessing. Workflow-
   supplied replacement conversations remain a later explicit contract, not a hidden behavior
   triggered by a JSON field name.

Focused checks: `agent_history_continuation_` and `agent_history_channel_` for two human rounds,
two competing respondents, reply-before-acknowledgement, deadline/cancel races, private approvals,
output retained after workflow failure, and unchanged exchange counts across publication/retry.
Also cover a CC added between output acceptance and publication: the release is refused and the
exchange is retained at its level.
Run existing message-reply, approval-continuation, provider-recovery, and child-decision tests.

## Acceptance

Default agent continuity works without workflow history assembly. A human can resume an exchange
that owns its stream; a child cannot deadlock behind its parent. Neither output suppression nor
delivery failure changes the recorded agent exchange. Record available and pending integration
evidence explicitly under [07](07-integration-and-verification.md).
