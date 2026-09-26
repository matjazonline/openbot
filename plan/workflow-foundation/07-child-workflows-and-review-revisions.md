# 07 — Child workflows and bounded human-feedback revisions

## Outcome and dependencies

Depends on phases 1–6. Reuse workflows as steps or agent tools and implement iterative human
feedback without confusing a new revision with a technical retry.

## Child calls and workflow tools

`workflow.call` invokes a pinned company workflow version with explicit schema-validated inputs.
Persist child creation and parent wait together. A logical call has one child identity across
worker retries. The child receives only its explicit inputs and allowed runtime resource bindings,
not the parent's entire context. It returns declared output mappings.

Children inherit root authorization ceilings, deadlines, budgets, and cancellation. They cannot
gain access by selecting a more privileged workflow. Freeze the dependency graph at publication
and reject recursion. Child results/failures create durable parent continuation events; the parent
does not occupy a worker while waiting.

Expose selected published workflows as agent tools using their input/output schemas. Map the saved
model call to the child identity before starting it. The child may perform human decisions or
effects; the parent agent resumes the exact pending call with the child's result. Rejected/failed
children yield typed results or errors according to the declared contract, not fabricated success.

## Retry versus revision

| Operation | Identity and input behavior |
| --- | --- |
| Technical retry | Same logical operation, frozen inputs and effect idempotency identity |
| Human-requested revision | New child round with prior artifact, explicit feedback and edited data |
| Rerun with changed definition/input | New top-level run, linked to the original for inspection |

Never reset a completed agent step to pending to implement feedback. That would reuse the wrong
inputs, obscure history, and risk repeating unrelated effects. Human revision creates new work
while preserving the original run's causal history and limits.

## Reusable draft-and-review round

Implement the representative review process as a parent with a `flow.repeat` step invoking a
draft-and-review child:

```text
load context
  -> repeat draft-and-review round
       child input: request, context, prior artifact, feedback, edited data
       child: agent draft -> human review -> declared child output
       child output: { choice, artifact, feedback, data }
       revise: map output into the next round's explicit input
       accept / escalate / cancel / timeout: leave repetition
  -> parent routes final choice
       accept: send the accepted artifact
       escalate: specialist workflow with artifact and feedback
       cancel: end with cancelled business outcome
       timeout: declared timeout route
```

The child completes after the decision. The parent owns repetition and final delivery, so no
iteration sends a draft accidentally. The accepted artifact is the exact reviewed/edited version.
Each round has its own agent execution, decision, artifact, and action identities.

`flow.repeat` requires a child reference, initial input mapping, next-iteration input mapping,
exit predicate, maximum iteration count, and exhaustion route. Evaluate the exit predicate after
each child completion, before starting another round. An acceptance on the last permitted round
still succeeds. On exhaustion without an exit, follow the declared escalation/failure route;
never auto-approve. The final output exposes the final child result and round count, with prior
rounds linked in the trace rather than copied into every prompt.

Default the shipped support template to three rounds and route exhaustion to human escalation.
Allow workflow authors to choose a lower bound within enforced platform limits. Deadlines and root
budgets apply across all rounds and cannot be reset by requesting revision.

## Feedback to a different next step

Repeated review is optional. Any successor can explicitly map the human result:

```yaml
specialist:
  type: agent.run
  with:
    agent: { ref: "/params/specialist_agent" }
    context:
      request: { ref: "/input/message" }
      proposal: { ref: "/steps/draft/output" }
      feedback: { ref: "/steps/review/output/feedback" }
      human_data: { ref: "/steps/review/output/data" }
  next: $end
```

This is a successor-step excerpt; its containing workflow declares the preceding draft/review,
parameter schemas, and review-choice route to `specialist`.

## Acceptance

- Revision feedback reaches the next round, while the previous artifact and decision stay intact.
- Accepting an older draft cannot authorize a newer one; late links cannot settle the current round.
- Only the final accepted artifact is sent, once; context loads and earlier effects are not replayed.
- Test child-creation retries, duplicate completion, parent cancellation, child timeout, and restart.
- Test last-round acceptance, exhausted revision requests, and inherited budget enforcement.
- Agent workflow tools resume the original checkpoint after a child human decision.
