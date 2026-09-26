# 06 — Agent runtime, context, memory, and capabilities

## Outcome and dependencies

Depends on phases 1–5. Make agent preparation visible and reusable, retain durable Rig execution,
and remove hidden orchestration from the harness boundary.

## Rig integration

Standardize on Rig and its existing durable model-turn/tool-call checkpoints. Remove the
`ai-agents` adapter, dependency, runtime selector, and adapter-specific configuration when callers
move to the new contract. Preserve model-provider choice through Rig.

`agent.run` receives a frozen agent specification, explicit context/history, resolved tools and
skills, output schema, deadline, and remaining root budgets. It returns validated output or
suspends through durable waits. Structured output repair is bounded and charged to the same
budgets. Invalid output never becomes a customer reply.

Refactor the agent runner so it does not implicitly load/save memory, choose channel agents,
publish messages, or grant tools through skills. System policy checks remain enforced outside
prompts; optional classifier/guardrail behavior can be an explicit workflow step where appropriate.
Delimit inbound text, retrieved content, and feedback as data rather than trusted instructions.

Agent direct tools use the phase 4 action service. Persist a pending call before invoking it,
record its receipt before resuming model generation, and resume saved calls after approval rather
than asking the model to recreate them. Process effectful calls sequentially within an agent run
in this release. Independent workflow runs can execute concurrently.

## Explicit preparation steps

- `context.load`: retrieve only selected context and bounded history up to the admission cutoff.
  Retain source IDs and token accounting. No implicit lookup of a later conversation state.
- `memory.load`: accept explicit query and authorized company/agent/user scope; return bounded
  memories with provenance. Freeze the successful result for subsequent retries.
- `memory.save`: persist explicitly selected facts through an idempotent write operation; do not
  infer that every generated draft or internal reviewer comment belongs in long-term memory.
- `ai.classify`: make a bounded model call without tools and validate the output against declared
  labels/schema. Persist the result and usage before routing or capability selection.
- `decision.agent`: compute eligible declared choices deterministically, ask the model to choose
  among them, validate and persist the choice, then let the workflow engine advance. The agent
  cannot invent node IDs, mutate the graph, or authorize itself to execute a disabled choice.

## Skills and capability profiles

Simplify skills into reusable instructions with declared capability requirements. Remove
executable ordered tool instructions from skills; their procedures become workflows. Skill
selection never silently grants tools.

A workflow may define named profiles listing tools and skills. A classifier may select a profile,
and `agent.run` can receive that selection by reference; a static profile works without a classifier.
Selection is optional and applies to the particular agent step, not implicitly to every later
agent because a classifier appeared somewhere in the graph.

Resolve capabilities as follows:

1. If `with.capability_profile` is omitted, start with all tools and skills saved on the selected
   agent in the run's frozen specification. Do not require a classifier, an extra context-loading
   step, or duplicate tool/skill lists in YAML.
2. If a profile is supplied, use its explicit tool and skill lists as the selection instead of
   adding the agent defaults. Profiles declare both lists; an empty list deliberately selects none.
   A malformed/unknown profile, null selection, or unresolved required reference is an error, not
   a reason to fall back to broader capabilities.
3. Apply the agent's grants, any explicit workflow capability ceiling, and current company policy.
   An omitted workflow ceiling adds no restriction to the agent defaults; it does not mean an
   empty grant. Neither a profile nor default inheritance can add unauthorized tools or skills.
4. Validate selected skill requirements against the effective tools. Fail with an actionable error
   if a required tool is absent instead of silently changing the procedure or granting that tool.

Provide effective tool declarations to the harness and selected skill instructions to prompt
composition. The default is operational capability loading, not merely listing tool names in text.
Use the same frozen selection across retries and resumptions; later agent edits cannot change it,
while current revocations remain enforced.

For example, this complete step configuration inherits the agent's saved tools and skills:

```yaml
answer:
  type: agent.run
  with:
    agent: { ref: "/params/support_agent" }
    context:
      message: { ref: "/input/message" }
    output_schema:
      type: object
      required: [body]
      properties:
        body: { type: string }
  next: $end
```

The containing workflow supplies the referenced parameter/input schemas. No classifier or
capability profile is required for this step.

Record selection source (`agent_defaults` or `profile`), any selected profile, skill snapshots,
effective tool IDs, model settings, and actual context sources in the run trace. Credentials never
appear in these records. Apply token/spend reservations
before model calls and retain uncertain provider usage reservations across recovery.

## Acceptance

- Without a classifier/profile, the agent receives its saved tools and skills in their saved order.
- A classifier used only for routing does not replace agent defaults unless its selection is wired
  into that agent step. Static explicit profiles work without classification.
- Explicit empty selections remain empty; malformed or unresolved selections never load defaults.
- Defaults and profiles both obey workflow ceilings and revocation; agent edits do not change a
  frozen selection on retry/resumption. An agent with no saved capabilities receives none.
- A classifier changes the visible profile, and an out-of-profile model tool request is rejected.
- Skills cannot widen grants; revocation after publication remains effective.
- Memory retrieval is visible, reusable, scoped, and not repeated after a committed success.
- Agent restarts after approval resume the saved call/result without repeating completed actions.
- Structured-output repair, token limits, provider deadlines, and cancellation are enforced.
- Agent outputs and internal feedback are not automatically sent or saved to memory.
- Tests and configuration no longer require or expose the alternate runtime.
