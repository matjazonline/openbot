# 02 — Workflow language, context, and publication

## Outcome and dependencies

Depends on phase 1. Compile company-authored YAML into a validated typed graph and publish
immutable versions. Build the compiler and fixtures before real execution handlers.

## Definition and context contract

A definition declares a format version, identity, input/parameter/output schemas, resource
requirements, entry step, step map, explicit transitions, and execution limits. Store source,
compiled representation, and a content hash. Published content is immutable; editing creates a
draft and publishing creates a new version. Archiving stops future selection without deleting history.

Expose `input`, `params`, `steps.<id>.output`, and safe `run` metadata. A step writes only its own
output. References such as `{ ref: "/steps/memory/output/items" }` preserve JSON types. Resolve
inputs once at activation and persist them before execution; technical retries reuse them.

Implement a small typed binding vocabulary: literal values, references, object/array construction,
concatenation, explicit defaults, comparisons, membership, existence, and boolean composition.
Do not introduce an executable template language. Missing required data is an error. The compiler
rejects references that cannot exist on a path unless the definition supplies an explicit default.
Validate resolved inputs and handler outputs at runtime as well as performing static checks.

Use explicit success/choice/error transitions and `$end` as the terminal target. No implicit
fall-through by YAML ordering. A decision emits a declared choice, and the engine maps it to a
target. Deterministic rules use ordered cases and a required default. A declared error route runs
only after applicable safe retries are exhausted; uncaught errors fail the run.

Graphs are acyclic. Repetition is a `flow.repeat` control step around a child workflow, not an
arbitrary backward edge. Reject recursive child-workflow dependencies. See phase 7 for iteration
state and budget behavior.

## Registered step types

| Type | Contract |
| --- | --- |
| `context.load` | Bounded conversation/application context |
| `memory.load`, `memory.save` | Explicit scoped retrieval/persistence |
| `ai.classify` | Schema-validated labels or named profile selection, without tools |
| `agent.run` | Explicit agent, context, output schema; optional capability selection defaults to saved agent tools/skills |
| `decision.rule`, `decision.human`, `decision.agent` | Declared choice plus validated data |
| `data.map` | Pure data construction and transformation |
| `http.request`, `tool.call` | Shared action-service invocation |
| `message.send`, `message.reply` | Canonical message and provider-neutral delivery |
| `workflow.call`, `flow.repeat` | Pinned child execution and bounded sequential repetition |
| `wait.event`, `wait.timer` | Correlated durable suspension with a deadline |

Each registration supplies input/output schemas, configuration validation, capabilities, effect
classification, and execution/retry constraints. These registrations also supply authoring help.
Unknown types and fields fail validation. Limit YAML size/nesting and reject duplicate keys and
custom tags; do not allow aliases to evade document size limits or resolve remote schema references.

`agent.run.with.capability_profile` is optional. Omission uses the saved tools and skills from
the frozen agent specification; a workflow does not need a classifier or duplicate capability
lists. A supplied profile, whether static or referenced from classifier output, explicitly
selects tools and skills. Empty lists select none. Invalid selections or unresolved required
references fail rather than falling back to the agent's broader defaults. See phase 6 for resolution.

## Publication and binding

1. Support operator-supplied templates and company-owned copies. Template updates never change a
   company copy automatically. This is a simple catalogue, not a package installation platform.
2. Publication freezes agent instructions, skills, capability profiles, child versions, and their
   dependency hashes. Resource slots declare compatible company-owned runtime resources.
3. Binding revisions select one published workflow and provide schema-validated parameters and
   resource selections. Activation validates tenant ownership, provider capabilities, and readiness.
   Updating a binding affects future admissions only.
4. Runs snapshot the binding revision and parameters. Connections remain credential references;
   resolve current secrets and enforce revocation at use time.
5. Add application commands for save draft, validate, publish, archive, configure binding, and
   activate/deactivate binding. Use expected revisions for edits and idempotent publication commands.
6. Build valid fixtures for reviewed support, autonomous response, triage/routing, and repeated
   human revision. The autonomous fixture omits classification and capability selection to exercise
   saved agent defaults. Expand the same fixtures as later handlers become available.

## Acceptance

- Reject malformed graphs, unknown routes/types, unavailable required references, invalid schemas,
  recursive dependencies, missing repetition bounds, and unauthorized resource bindings.
- Tests round-trip typed values without string coercion, including null versus missing values.
- Publishing or editing a dependency cannot alter an existing published bundle.
- Validation returns source locations and actionable errors usable by the editor.
