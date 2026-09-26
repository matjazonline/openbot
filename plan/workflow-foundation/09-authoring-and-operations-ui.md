# 09 — Authoring and operations UI

## Outcome and dependencies

Depends on phases 1–8. Complete the user journey using the existing server-rendered UI/HTMX
architecture. Application commands already exist; pages do not introduce parallel business logic.

## Authoring and setup

- Workflow library: select a shipped template, create a company copy, or create a draft.
- Editor: YAML source, source-located validation, step catalogue reference, resource requirements,
  input/output schemas, and a read-only graph generated from the same compiled representation.
- Versions: compare changes and publish an immutable version. Publishing is distinct from binding
  activation; show which channels/schedules will remain on older versions.
- Bindings: select version, resources and schema-driven parameters; validate readiness and activate.
  Show paused/unconfigured bindings plainly rather than silently using a default workflow.
- MCP steps: select a company MCP connection for the declared resource slot, inspect the selected
  tool's frozen schema, and validate argument bindings. Show tool/connection identity, bounded
  results, and reconciliation status in run details; keep credentials out of source and previews.
- Agents: instructions, model settings, skills and grant ceilings. Provide an explicit assistant
  channel setup action, not implicit channel creation on every agent save.
- Capability configuration: show “Use agent tools and skills” as the default for an agent step;
  choosing a profile is optional. Display the resolved tools/skills and whether they came from
  agent defaults or an explicit profile in validation previews and run details.

Use optimistic revisions to detect concurrent edits and preserve unsaved source when validation
fails. Company administrators author/publish/configure; existing membership and channel rules
govern who can start and inspect runs. Operator template management grants no company-content access.

## Runs and conversations

Show run state, triggering message, published version, graph, and chronological timeline. Expand
steps to show authorized input/output data, context sources, selected skills/tools, model usage,
action receipts, child runs, and retry/reconciliation history. Keep internal worker attempts under
diagnostics rather than making them the primary business object.

Conversation entries link to their producing run. Show multiple active/waiting runs independently;
do not reduce the conversation to one “current task.” Reply and review links must point to exact
message/artifact/decision identities.

Offer explicit cancellation, safe retry, and reconciliation controls with visible operation status.
Queued delivery, provider acceptance, human waiting, failure, and uncertainty must be distinguishable.
Retries cannot masquerade as new revisions, and cancellation must explain already accepted effects.

## Human work queue

Provide one queue for assigned business decisions and protected-action approvals, with clear subject
labels. Detail pages show the reviewed artifact version, context, discussion, deadline, choice
buttons, feedback field, and schema-driven editable data.

“Add comment” leaves the decision open. “Submit decision” validates choice-specific feedback and
data before settling. Show draft/revision history and the feedback that produced each revision.
Closed or stale decisions remain readable but cannot submit a new outcome. Late links explain
which decision was already settled rather than applying to a newer round.

## Sample execution

Sample runs use the same compiler/engine in an isolated execution mode enforced below the UI.
Use scripted model/decision responses and intercepted actions. Use fixtures for external reads;
do not contact production providers by default. Capture intended messages and writes for inspection.
Never send customer messages, execute write tools, or persist production memory.

Keep sample records and resource bindings isolated from production admission. A sample cannot
be converted into production by resuming it; production starts a new authorized run.

## Acceptance

- A company admin can copy, edit, validate, test, publish, and activate a reviewed support workflow.
- A reviewer can comment, request a revision with feedback, inspect the next draft, and accept it.
- Readers can distinguish two simultaneous runs on one thread and follow their separate replies.
- Test cross-company IDs, private-thread visibility, hostile rendered content, and CSRF boundaries.
- SSE is a wakeup followed by reconciliation; test reconnect, event-before-subscribe, and stale
  partial-page responses. Keep the event-stream owner mounted across swaps.
- Provide progress feedback for accepted actions and verify both themes and keyboard interaction.
- Sample execution proves zero production effects, including through direct agent tools and children.
