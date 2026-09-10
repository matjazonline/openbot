# Step 5: Skills and runtime context

Dependencies: step 4. Main files: proposed `src/adapters/harness/rig/skills.rs` and `compile.rs`;
existing `src/domain/entities/skill.rs` remains the source of recipe types and limits.

## Changes

Compile the resolved capability spec directly into Rig instructions, context, tools, and skill
definitions. Do not round-trip through ai-agents YAML. Preserve the application's untrusted-input
fencing and leave history/memory selection in the existing prompt composer.

Render the runtime facts currently supplied through ai-agents context: current date/time with an
explicit timezone, agent name, recipient role, and primary/CC flags. Use an injectable clock for
tests. Do not leave `{{ context.* }}` placeholders in a Rig preamble. Define refresh timing rather
than letting a long tool loop claim an obsolete timestamp is current.

Build a compact catalog of available attached skills in the system prompt. Each entry contains
the skill's `slug`, `description`, and `uri`; keep full instructions out of the initial prompt.
Tell the model to load a relevant skill before following it:

```text
compact skill catalog in system prompt (slug, description, uri)
  → model selects a relevant skill and calls read_resource(skill_uri)
  → tool returns the full concatenated skill steps
  → model follows those instructions in the current run
```

Expose an application-owned `read_resource` tool taking `skill_uri`. Resolve only catalog URIs
back to existing typed skill IDs/slugs, with deterministic, collision-checked mappings. Recheck
attachment, tenant scope, and effective grants at read time; reject unknown or unavailable skills.
This is a scoped skill-content loader, not a general filesystem or network reader. Never accept
an arbitrary skill body from model arguments. Loading a skill does not execute its steps.

Render stored `SkillInstruction` items into one bounded instruction document in their original
order. Include each `Prompt { text }` verbatim and represent each `Tool { tool, args, output_as }`
as an explicit instruction to call the named tool with the stored argument template and retain
its result under the specified output name. Preserve step boundaries, argument types, and output
references so concatenation does not lose recipe content. Include the skill's trigger as usage
guidance in the loaded document.

The model follows the loaded steps through the normal conversation and guarded tool dispatcher;
do not generate a callable tool per skill or introduce a separate recipe executor. Use baseline
fixtures from step 1 to define how argument templates and named outputs are expressed, including
missing-variable and non-string behavior. Reject unsupported constructs during preflight and
document compatibility differences: model-followed instructions do not guarantee deterministic
recipe execution. Tool results referenced by a skill remain data, not trusted instructions.
Catalog text, loaded content, subsequent calls, and outputs share the run's existing budgets;
loading another skill must not reset them. Preserve existing instruction and size bounds.

A skill may include `Tool { tool: "request_approval", args: { title, proposal }, output_as }` to
ask a human to approve concrete proposed work before continuing. Render it as an explicit call in
the loaded instructions and include its tool grant. The custom tool parks the task; positive
approval restores the saved context and supplies the approved result before later calls proceed.
See [step 6](06-execution-and-approvals.md#explicit-checkpoints-through-request_approval) for its
contract. Skill instructions are model-followed; mandatory approval for a protected action must
also be enforced by application tool policy.

Sub-agent invocation must use our custom application-owned tools: `list_company_agents`,
`create_agent_channel`, `outreach_and_await_quorum`, and `transfer_or_release_task`, as applicable.
Do not register sub-agents as Rig agent-as-tool calls or invoke them through Rig's nested-agent
execution. The custom tools use the step 4 bridge and retain persisted sub-agent allowlists,
tenant checks, approval, quorum, and task ownership. There is no second nested-agent execution
service in this change.

Stop tool dispatch immediately on pending approval, native suspension, cancellation, or a terminal
error. Resume through the application's durable task/approval/outreach protocol. A fresh process
cannot rely on an in-memory Rig conversation: restore the loaded skill content and relevant tool
results from durable run history. Verify approved-step replay and completed side-effect
deduplication; add any missing durable state through an application-owned port with a migration
before advertising resumable support.

## Verification and acceptance

- The initial system prompt lists only available skills with slug, description, and URI, without
  embedding their full instruction bodies.
- A scripted model reads a catalog URI, receives all steps in order, and follows them through the
  guarded dispatcher, including a tool output referenced by a later step. Reading alone has no
  recipe side effects.
- Catalog and content rendering, arguments, output names, empty/malformed variables, and size
  bounds have deterministic fixtures rather than natural-language output comparisons.
- Unknown/unattached/cross-tenant URIs, missing skill/tool grants, URI collisions, and missing
  runtime context fail clearly.
- Sub-agent requests route only through our custom tools; no Rig agent-as-tool is registered.
  Existing allowlist, tenant, approval, quorum, and ownership checks remain covered.
- Suspension prevents subsequent tool calls; resume restores loaded context and does not repeat
  an already committed effect.
- Full prompt/history, resource reads, and intermediate tool results count toward the same token
  budget, including repeated skill loads.
