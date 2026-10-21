# 06 — Agent runtime, context, memory, and capabilities

## Outcome and dependencies

Depends on phases 1–5. Make agent preparation visible and reusable, retain durable Rig execution,
and remove hidden orchestration from the harness boundary.

## Schema and data

Pre-release: nothing is deployed, so existing data is not preserved. Schema changes in this phase
edit the squashed init migration (`migrations/20260817000000_init_schema.sql`) in place, and the
development and test databases are dropped and recreated rather than upgraded. Do not add additive
migrations, backfills, compatibility shims, or populated-upgrade tests; verify against a freshly
migrated schema.

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
*User decision 2026-10-21 (06-R024, 06-F030): the legacy Stage-3 LLM spam guardrail and its
per-company toggle are removed entirely, not moved into a workflow step; the heuristic and
Rspamd/SpamAssassin stages remain.*

Agent direct tools use the phase 4 action service. Persist a pending call before invoking it,
record its receipt before resuming model generation, and resume saved calls after approval rather
than asking the model to recreate them. Process effectful calls sequentially within an agent run
in this release. Independent workflow runs can execute concurrently.

## Explicit preparation steps

- `context.load` (renamed `context.history` by user decision 2026-10-20, 06-D6: thread history
  frozen at admission, bounded by `max_tokens`): retrieve only selected context and bounded history up to the admission cutoff.
  Retain source IDs and token accounting. No implicit lookup of a later conversation state.
- `memory.load`: accept explicit query and authorized company/agent/user scope; return bounded
  memories with provenance. Freeze the successful result for subsequent retries.
- `memory.save`: persist explicitly selected facts through an idempotent write operation; do not
  infer that every generated draft or internal reviewer comment belongs in long-term memory.

  *Memory is out of this release by user decision 2026-10-21 (06-R024, 06-F032), confirming the
  2026-10-20 deferral (06-D6): `memory.load` and `memory.save` stay registered but unhandled, and
  no phase of this release implements them. The legacy automatic recall and persistence leave with
  the legacy dispatch (phase 08) and are not replaced.*
- `ai.classify`: make a bounded model call without tools and validate the output against declared
  labels/schema. Persist the result and usage before routing or capability selection.
- `decision.agent`: compute eligible declared choices deterministically, ask the model to choose
  among them, validate and persist the choice, then let the workflow engine advance. The agent
  cannot invent node IDs, mutate the graph, or authorize itself to execute a disabled choice.

## Skills and capability profiles

> **Superseded by user decision 06-D5, 2026-10-19** ([decision](docs/logs/06/decision-D5.md),
> TASK.md "User capability-ownership decisions"). Tools and skills are defined on the agent and
> `agent.run` uses exactly them, frozen at publication with current revocation enforced. There is
> no step-level narrowing: `capability_profile`, workflow profiles, classifier profile selection
> and the workflow `capability_ceiling` are removed. Skills are prompt text plus required tools;
> legacy executable skill tool steps are refused. The text below up to "Usage analytics" is kept
> as history only; where it conflicts with 06-D5, 06-D5 governs (including the profile/ceiling
> acceptance bullets).

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
   Declare the ceiling at the workflow top level as
   `capability_ceiling: {tools: [...], skills: [...]}`; both lists are required when present.
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
appear in these records. Apply token and model-call reservations before model calls and retain
uncertain provider usage reservations across recovery. There are no monetary caps.

## Usage analytics and token continuation — user revision, 2026-10-19

Measure agent and workflow model calls and tokens, including retries and structured-output repair.
Display analytics grouped by provider/model or workflow, with agent and workflow-run attribution.
Keep provider-reported usage distinct from estimates and unresolved reservations; missing usage
must not silently become zero or be reported as measured tokens.

Optional provider/model prices per million tokens enable displayed cost calculations. Prices are
for analytics only, never permission to run a model. Unpriced models remain usable; their cost is
unavailable, not zero. No monetary cap or monetary approval threshold is required.

A workflow may optionally set a maximum token allowance. Exhaustion pauses automated progression
and creates a manual approval to continue; it is not an ordinary retry or an automatic allowance
reset. Approval must be authorized and durable, retain cumulative usage, and resume saved state
without repeating completed model calls or effects. Denial, cancellation and deadline expiry must
leave no runnable continuation. Existing bounded model-call and execution safety limits remain.

Verify analytics attribution and pricing arithmetic, missing prices/usage, duplicate settlement,
uncertain provider usage, threshold behavior, and competing approval/cancellation/resumption.
These user requirements supersede the previous monetary-cap decision question.

*User decision 2026-10-21 (06-R024, 06-F030), narrowing 06-D2's "every production model entry must
persist usage": the spam guardrail's model call is removed with the guardrail, and the
system-prompt generator's one-shot completion stays unmetered. It writes no `model_usage_calls`
row (that table requires a task and an agent) and is disclosed as unmetered in `docs/rig.md` and
`docs/deploy.md`.*

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
  *Out of this release by user decision 2026-10-21 (06-F032; deferred 2026-10-20 by 06-D6): no
  phase implements memory steps in this release.*
- Agent restarts after approval resume the saved call/result without repeating completed actions.
- Structured-output repair, token limits, provider deadlines, and cancellation are enforced.
- Agent outputs and internal feedback are not automatically sent or saved to memory.
- Tests and configuration no longer require or expose the alternate runtime.

## 04.10 handler composition obligation

Every handler advertising `mcp.call`, `http.request` or `tool.call` must route
effects exclusively through ActionService's marker-first local/remote protocol.
The shared `marker_owned_effect_kind` recovery classifier relies on this invariant.
A new tool bridge must preserve it or change the classifier before integration.
See [BRIEF-04.10](BRIEF-04.10.md) and finding04.10-F011.
