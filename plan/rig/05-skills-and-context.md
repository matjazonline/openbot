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

Implement attached skills as bounded executable recipes exposed through adapter-generated skill
tools, with descriptions/triggers helping the model select an attached recipe. Use deterministic,
collision-checked names satisfying each supported provider's tool-name restrictions. Map names
back to existing typed skill IDs/slugs; never accept an arbitrary skill body from model arguments.

After selection, execute stored `SkillInstruction` items in order:

1. `Prompt { text }`: perform the recipe's prompt step using the same resolved model and run
   context, charging its call/output to the shared budget.
2. `Tool { tool, args, output_as }`: resolve arguments according to the characterized existing
   template semantics, invoke the same guarded tool dispatcher, and retain a bounded named output.
3. Make prior outputs available to later steps with explicitly defined missing-variable and
   non-string behavior. Treat tool output as data, never as new trusted system instructions.
4. Stop immediately on pending approval, native suspension, cancellation, or a terminal error.

Use the baseline fixtures from step 1 to settle argument interpolation, prompt history, and named
output semantics. Reject unsupported constructs during preflight. Plain Markdown descriptions
alone do not satisfy executable tool steps. Do not permit recursive skill-tool calls in V1; prompt
steps must not reopen an unrestricted skill router or reset the run's budgets.

Continue using existing outreach/directory tools for sub-agent communication. Rig's ability to
wrap an agent as a tool must not bypass persisted sub-agent allowlists, tenant checks, approval,
quorum, or task ownership. There is no second nested-agent execution service in this change.

Resume through the application's durable task/approval/outreach protocol. A fresh process cannot
rely on an in-memory Rig conversation or recipe cursor. Verify approved-step replay and completed
side-effect deduplication; if a recipe needs durable progress absent from the current protocol,
add that through an application-owned port with a migration before advertising resumable support.

## Verification and acceptance

- A scripted model selects an attached recipe whose tool output feeds a later step correctly.
- Step ordering, arguments, output names, empty/malformed variables, and instruction/size bounds
  have deterministic fixtures rather than natural-language output comparisons.
- Missing skill/tool grants, name collisions, recursion, and missing runtime context fail clearly.
- Suspension halfway through a recipe prevents subsequent tool calls; resume does not repeat an
  already committed effect. Unrestricted company-agent invocation is impossible.
- Full prompt/history and intermediate tool results count toward the same token budget.
