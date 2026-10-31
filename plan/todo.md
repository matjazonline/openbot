#TODO ideas for future 

- main /ui dashboard uses server-side db polling for every SSE stream - use db events hybrid
- convert to HTMX 4

## Workflow foundation follow-ups

Left open when [workflow-foundation](.archived/workflow-foundation/README.md) was completed
(phase 10 handoff, `docs/workflow-operations.md`):

- Run the fresh-database production cutover with its controlled pilot
  ([deploy](../docs/deploy.md), [operations](../docs/workflow-operations.md#deliberate-fresh-cutover)).
  No target was authorized or tested; local verification is not pilot evidence, and live SMTP
  readiness is unproven.
- Install a production reconciliation evidence verifier. Reconciliation is only an annotation
  today; a run waiting on it recovers by cancel and rerun.
- Live reviewer reassignment for human decisions. Today an unavailable reviewer means cancel,
  publish corrected reviewers, and start replacement work.
- Workflow memory: `memory.load`/`memory.save` are registered but unhandled (06-F032), and the
  memory-provider infrastructure was deleted in phase 08 (P08-Q1). `memory_management/` and
  `hydradb/` predate that deletion.
- If a run ever allows several live executions (e.g. parallel branches), add an owner-created
  duplicate-candidate fixture and real competing-worker coverage; keep the adversarial SQL test.
- Add the missing historical reconciliation SQL diagnostic test (replay safety in the current
  schema). Reassess whether supported writers can produce authentic evidence from an earlier
  transaction and a fresh insertable claim-episode identity. If reachable, exercise the actual
  deferred trigger on the public table, assert the exact historical-transaction rejection,
  include a genuine current-transaction positive control, and verify complete rollback.
  Currently, reusing an old identity hits uniqueness constraints first, while new commands create
  fresh evidence and their binding atomically. Revisit when reconciliation or scheduling writers
  change; preserve constraints and avoid fabricating protected history. See
  [the recorded obstruction and existing coverage](../docs/workflow-reconciliation-safety.md).

Not planned in the foundation ([NOT_PLANNED](.archived/workflow-foundation/NOT_PLANNED.md);
its "Deliberately excluded" list holds architectural boundaries, not work items):

- Parallel branches, joins and dynamic fan-out (join, cancellation, budget and data-merge
  semantics). Graphs are sequential today.
- Visual drag-and-drop workflow editor that round-trips through the same YAML compiler.
- Slack and other transport adapters on the provider-neutral contracts; see `slack-integration/`.
- Workflow package installer/marketplace: dependencies, setup provisioning, ratings, billing,
  public submissions. Today: basic templates and company copies.
- Template upgrades and merge proposals for customized company workflows.
- Named business-provider suites (HubSpot, Stripe, CRM, payments) beyond generic HTTP, MCP over
  HTTP and registered tools.
- More MCP transports and client features: stdio subprocesses, legacy HTTP+SSE, resource and
  prompt browsing.
- Voting/quorum approval policies. Today: listed member reviewers, first valid response.
- Email-only (non-member) reviewers and email-link decision credentials (dropped 2026-10-18;
  decisions are member-only).
- Multi-response outreach aggregation: completion, partial results, deadlines, cancellation.
  Today: one matching response per correlated wait.
- Live collaborative editing and richer discussion (e.g. comment reactions).
- Automatic compensation and distributed transactions.
- Run rewind/fork/time travel across external effects.
- Self-modifying workflows and autonomous publication.
- Recursive workflow dependencies (child calls are acyclic today).
- Multi-region orchestration or an external workflow service.
- Export or conversion of business data from the old database (cutover is fresh-database).
