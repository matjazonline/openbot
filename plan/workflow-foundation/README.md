# Workflow foundation

## Outcome

Replace hardcoded message-to-agent dispatch with one durable workflow engine in Rust and
PostgreSQL. Company admins author YAML workflows, publish immutable versions, and bind them to
channels and schedules. Agents, human decisions, context loading, memory, tools, HTTP requests,
MCP tool calls over HTTP, and message delivery become explicit steps.

This is an implementation plan, not documentation of shipped features. It incorporates the
architecture discussion and the subsequent human-comment, feedback, and revision design.
`../workflows/configurable-workflows-architecture.md` supplied inspiration, not constraints.
Earlier SOP and workflow-library plans are not prerequisites: implement one execution model,
not another engine alongside those proposals.

## Agreed decisions

- Company-owned YAML editor, templates, validation, immutable publication, and a read-only graph.
- Sequential graphs with branches, bounded repetition, child workflows, and agent-selected routes.
- Every incoming message has an independent run, context, decisions, and replies. A waiting run
  never blocks another message in the same conversation.
- Direct agent tools and workflow tools are both supported, including writes, through one shared
  action service with authorization, approval, idempotency, and effect recovery.
- Explicit `mcp.call` steps invoke company-bound MCP tools over Streamable HTTP through that same
  action service, without requiring an agent step.
- Standardize agent execution on Rig; remove the alternate `ai-agents` runtime.
- Capability selection is optional: without an explicit selection, `agent.run` uses the agent's
  saved tools and skills from the run's frozen specification, subject to authorization limits.
- Use Rust/PostgreSQL and existing durable infrastructure; no external orchestration service.
- Provider-neutral message steps, implemented against the existing email transport. Slack is future work.
- Fresh-database cutover, with no legacy API/configuration compatibility or business-data migration.
- Human decisions support comments, feedback, structured edits, revision requests, and routing;
  approve/reject is only one configuration.

## Implementation order

Execute the numbered files in order. Each phase names its acceptance gate. Backend commands and
fixtures ship with the owning subsystem; phase 9 completes the user journey rather than delaying
all interfaces until then. Build the representative workflow fixtures from phase 2 onward.

| Phase | Deliverable |
| --- | --- |
| [01](01-architecture-and-domain.md) | Ownership, boundaries, domain contracts, replacement map |
| [02](02-workflow-language-and-publication.md) | YAML compiler, registry, context, versioning |
| [03](03-durable-runtime-and-persistence.md) | Admission, independent runs, jobs, transitions, recovery |
| [04](04-actions-http-and-delivery.md) | Shared action protocol, HTTP, MCP calls, tools, message effects |
| [05](05-human-decisions-and-waits.md) | Human tasks, comments, feedback, authorization decisions, waits |
| [06](06-agent-context-memory-and-capabilities.md) | Rig integration, explicit context/memory/classification |
| [07](07-child-workflows-and-review-revisions.md) | Child calls, workflow tools, bounded feedback rounds |
| [08](08-triggers-and-application-replacement.md) | Channels, schedules, manual starts, removal of old orchestration |
| [09](09-authoring-and-operations-ui.md) | Authoring, binding, runs, decisions, sample execution |
| [10](10-verification-and-cutover.md) | Acceptance matrix, CI, operational readiness, fresh cutover |

[NOT_PLANNED.md](NOT_PLANNED.md) records exclusions and future extensions separately.

Phase01 implementation is complete. [PROGRESS.md](PROGRESS.md) records verified evidence and
[RESUME.md](RESUME.md) records the requested stop boundary and next task: phase02, Definition and
context contract. The foundation is not yet a production workflow runtime.
[REPLACEMENT-MAP.md](REPLACEMENT-MAP.md) inventories the current entry points, callers,
projections, owners, and removal gates for phase 01 item 6.

## Working rules

Follow repository and subsystem `AGENTS.md`. Use graft before source exploration and trace callers
before replacing existing symbols. Prefer pure domain decisions, application-owned narrow ports,
and adapter-owned SQL/provider code. Introduce domain identifiers as newtypes. Keep async chains
shallow and preserve the stock-stack regression check.

All table, type, route, and YAML names in this directory describe implementation targets. Do not
advertise them as working configuration until implemented. Do not preserve an old path merely
to keep old tests green: replace obsolete behavioral assumptions with the new contract while
retaining authorization, recovery, and delivery guarantees.

Documentation creation does not authorize resetting a running database. The implementation
targets a fresh schema; actual cutover identifies the deployment/database and follows the
environment's execution permissions. Do not reset anything as part of implementing these documents.
