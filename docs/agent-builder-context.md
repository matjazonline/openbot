# Agent Builder context: agents, channels, and workflows

Use this reference to design company agents and workflow steps. Check the company's current agents, models, skills, tools, channels, and published workflows before naming dependencies. A design or YAML draft is not deployed until the corresponding application commands succeed.

## How the pieces fit

| Piece | Owns | How it runs |
| --- | --- | --- |
| **Agent** | Name, slug, description, system prompt, enabled provider/model, workflow-tool grants, and attached prompt-only skills. | A workflow model step selects it. `agent.run` gets its saved tools and skills; `ai.classify` and `decision.agent` use its instructions and model without them. |
| **Channel** | Company address, aliases, access (`team`, allowlist, or public), and independent threads. It needs no agent. | Active workflow bindings decide what accepted messages start. A channel may have multiple bindings. |
| **Workflow** | A typed, routed sequence of steps with input, parameter, and output schemas. | Publish an immutable version, configure a binding to that version, then activate it. The binding supplies parameters, resource references, run-as member, and an optional message trigger. Schedules and manual starts can also admit runs. |

No active binding means messages are stored without starting that workflow. A draft, a published version, and an active binding are separate states. Agent edits require republishing a workflow version to affect new runs.

## What Agent Builder can create today

The built-in `agent-builder` has `list_company_agents` and `create_agent_channel`. It can list existing agents and enabled models, then create **one agent plus one team-only channel** bound to the shipped autonomous or reviewed reply workflow. Autonomous sends the agent's answer; reviewed asks a human to approve, edit, or reject it before sending. Creation requires an owner or admin writing in the app, after explicit confirmation of the full specification. A library copy of Agent Builder itself creates only an agent; give that copy a channel/binding to converse with it.

Agent Builder has no workflow-draft, publication, or arbitrary channel-creation tool. It can propose workflow steps or YAML for the **Workflows** editor. Use that editor to save, validate, publish, bind, and activate a custom workflow. Report those actions as completed only after they succeed.

## Design an agent and its channel

1. Look for an existing agent that already owns the job. Define the outcome, users, example requests, responsibilities, non-goals, escalation conditions, needed facts, output format, and tone.
2. Choose a distinct name and lowercase hyphenated slug, a short description, a complete operational system prompt, and an explicit enabled provider/model. Request only needed workflow tools. Attach only existing skills identified by exact company skill slug.
3. Decide who may use the channel, what messages should trigger work, and whether sending needs review. The Builder's creation tool supports a team-only channel and the two reply modes; other channel access or workflow designs need separate authoring.
4. Show the final agent, channel, and reply-mode specification before asking for creation confirmation. Never imply a design is live merely because it was drafted.

## Design workflow steps

Start from the incoming event and the required outcome. Give each step a single purpose, explicit input bindings, a schema for model or mapped output, and a route. Use deterministic steps for mapping and routing; use a model step where language understanding or writing is needed. Put a `decision.human` before a send when review is required. Give effects an error route when the workflow needs a defined failure path.

The registered step types are:

| Purpose | Types |
| --- | --- |
| Read context and reason | `context.history`, `ai.classify`, `agent.run`, `decision.agent` |
| Transform and decide | `data.map`, `decision.rule`, `decision.human` |
| Act | `http.request`, `tool.call`, `mcp.call`, `message.send`, `message.reply` |
| Compose and wait | `workflow.call`, `flow.repeat`, `wait.event`, `wait.timer` |
| Reserved for unavailable memory | `memory.load`, `memory.save` — publication refuses them in this release |

Workflow source is YAML v1. Its root requires `format_version`, a fresh UUID `workflow_id`, `input_schema`, `parameter_schema`, `output_schema`, `resources`, `entry`, `steps`, and `limits` (`max_steps`, `max_context_bytes`). Each step requires `type`, `with`, and `routes`. Every `with` value is a binding such as `{literal: ...}`, `{ref: /input/message}`, or `{object: {key: {ref: ...}}}`. A step route declares `success` **or** `choices`, optionally `error`; targets are step IDs or `$end`. References to step output must be available on every incoming route. `decision.rule` also needs ordered `rule.cases` and a `rule.default`.

This small shape follows the shipped autonomous reply template. Replace the UUID and agent slug, and keep the input contract compatible with the binding that will admit messages:

```yaml
format_version: 1
workflow_id: 00000000-0000-0000-0000-000000000099
input_schema:
  type: object
  properties:
    message: {type: string}
    source_message: {type: string, maxLength: 128}
    subject: {type: string, maxLength: 998}
    sender: {type: string, maxLength: 320}
  required: [message, source_message]
  additionalProperties: false
parameter_schema: {type: object, additionalProperties: false}
output_schema: true
resources: []
entry: draft
steps:
  draft:
    type: agent.run
    with:
      agent: {literal: assistant}
      context: {object: {untrusted_message: {ref: /input/message}}}
      output_schema:
        literal:
          type: object
          properties: {body: {type: string, maxLength: 16384}}
          required: [body]
          additionalProperties: false
    routes: {success: reply}
  reply:
    type: message.reply
    with:
      source_message: {ref: /input/source_message}
      content: {ref: /steps/draft/output}
    routes: {success: $end}
limits: {max_steps: 10, max_context_bytes: 65536}
```

For a reviewed reply, place `decision.human` between `draft` and `reply`, supply explicit company-user reviewers and a deadline such as `{ref: /run/deadline}`, then route approval to `reply` and rejection to `$end`. `message.reply` needs the source message and replies only to its sender from the source channel; `message.send` names explicit destinations. A model step's `output_schema` is its answer contract. Tools and skills belong to the agent selected by `agent.run`; the step cannot add or narrow them. Publication freezes their definitions, while current revocation still applies.

In **Workflows**, start from a shipped template or blank draft, save and validate the exact source, resolve named agents/tools/resources, publish a version, configure its channel binding and trigger, then activate it. A binding with no message trigger can still serve a schedule or manual start. Message triggers are `direct` (To or mention), `any` (To/Cc or mention), `mentioned`, and `mentioned_or_reply`. A quiet message is filed without admitting a new run.

Source of truth: [agent provisioning](../src/application/workflow/provisioning.rs), [step registry](../src/application/workflow/registry/mod.rs), [workflow language](workflow-language-v1.md), [autonomous template](../src/application/workflow/fixtures/autonomous-response.yaml), and [channel triggers](../src/domain/workflow/trigger.rs).
