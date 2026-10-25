# Workflow foundation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Replace hardcoded message-to-agent dispatch with one durable workflow engine in Rust and
PostgreSQL. Company admins author YAML workflows, publish immutable versions, and bind them to
channels and schedules. Agents, human decisions, context loading, tools, HTTP requests,
MCP tool calls over HTTP, and message delivery become explicit steps. Memory is out of this
release by user decision 2026-10-21 (06-F032).

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
- `agent.run` uses saved tools and skills from the frozen agent specification, subject to current
  authorization. Classifier-selected profiles and explicit capability overrides are excluded by
  06-D5; malformed or explicit selection fields fail validation instead of broadening defaults.
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
| [06](06-agent-context-memory-and-capabilities.md) | Rig integration, explicit context/classification (memory out of this release) |
| [07](07-child-workflows-and-review-revisions.md) | Child calls, workflow tools, bounded feedback rounds |
| [08](08-triggers-and-application-replacement.md) | Channels, schedules, manual starts, removal of old orchestration |
| [09](09-authoring-and-operations-ui.md) | Authoring, binding, runs, decisions, sample execution |
| [10](10-verification-and-cutover.md) | Acceptance matrix, CI, operational readiness, fresh cutover |

[NOT_PLANNED.md](NOT_PLANNED.md) records exclusions and future extensions separately.

Phases 01–09 are implemented. The phase08/09 combined artifact was committed as
`5d4aab1d44e209fbb46bae275be77c86cd3fef22`; retained phase records distinguish implementation,
independent review and verification. Phase10 implementation, full verification and independent acceptance status are recorded in
[its task record](phases/p10/docs/TASK.md). Local acceptance does not constitute deployment.

Current operator guidance is [workflow operations](../../docs/workflow-operations.md), with
[reconciliation safety](../../docs/workflow-reconciliation-safety.md) documenting the precise
historical SQL diagnostic limitation. [REPLACEMENT-MAP.md](REPLACEMENT-MAP.md) is the historical
phase01 removal inventory; its old source spans are not the current architecture. Prior phase
records are retained as evidence, not current setup instructions.

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
