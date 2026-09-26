# Not planned in the workflow foundation

These items are excluded from this implementation. Some are plausible later extensions; others
are deliberate architectural boundaries. Listing an item here does not promise its delivery.

## Future product and execution extensions

| Item | Current boundary / prerequisite for future work |
| --- | --- |
| Parallel branches, joins, dynamic fan-out | Sequential branches and independent concurrent runs only. Parallel graph execution needs join, cancellation, budget, and data-merge semantics. |
| Visual drag-and-drop workflow editor | YAML editor and read-only graph ship first. A future builder must round-trip through the same compiler. |
| Slack and additional transport adapters | Retain provider-neutral contracts; implement email now. Slack needs connection setup, authenticated events, threading, and delivery contracts. |
| Rich workflow package installer/marketplace | Basic templates and company copies only. No package dependency installer, setup provisioning framework, ratings, billing, or public submissions. |
| Automatic template upgrades and merge proposals | Company workflows adopt changes through explicit edits/publication. No automatic replacement of customized definitions. |
| Named business-provider integrations such as CRM/payment suites | Generic HTTP, MCP tool calls over HTTP, and registered tools are foundation capabilities; implementing a HubSpot, Stripe, or other product suite is separate work. |
| Additional MCP transports and general MCP client features | Foundation includes explicit tool calls over Streamable HTTP. Stdio subprocesses, legacy HTTP+SSE transport, and general resource/prompt browsing are separate work. |
| Sophisticated human voting/quorum approval policies | One assigned reviewer or eligible group with first valid response. Multi-party voting and quorum business decisions require explicit policies and tests. |
| Multi-response outreach aggregation | One matching response per correlated wait. Aggregating several respondents needs explicit completion, partial-result, deadline, and cancellation semantics. |
| Live collaborative editing and rich discussion | Append-only comments, optimistic edits, and explicit submission suffice. No collaborative document editor or comment reactions. |
| Automatic compensation and distributed transactions | Explicit workflows may invoke compensating actions, but the engine does not invent rollback or guarantee remote atomicity. |
| Arbitrary run rewind/fork/time travel | Safe retry preserves logical identity; changed inputs start a new run. Rewinding across external effects needs a separate design. |
| Self-modifying workflows and autonomous publication | Agents select declared routes or call authorized workflows. They cannot rewrite the executing graph or publish new authority. |
| Recursive workflow dependencies | Child call dependencies are acyclic. Bounded repetition is provided explicitly. |
| Multi-region orchestration or external workflow service | Rust/PostgreSQL in the existing operating model; new deployment topology needs its own consistency design. |

## Deliberately excluded from this redesign

- Backward-compatible APIs, channel configuration, task payloads, alternate-runtime settings, or
  legacy orchestration shims. This is a clean redesign.
- Business-data migration from the old database. Cutover targets a fresh database; exports or a
  future conversion project are separate work.
- Retaining the `ai-agents` runtime or supporting multiple agent execution engines. Rig is the
  single runtime for this foundation.
- A separate SOP execution engine or prerequisite implementation of older workflow-library plans.
- Arbitrary JavaScript, Python, shell, executable YAML tags, remote schema loading, or downloaded
  workflow plugins. YAML is a bounded declarative format.
- Shared mutable context between independent message runs or conversation-wide execution locks.
- Implicit tool grants from skills, hidden memory side effects, or automatic message sending by
  the agent harness. Explicit tools remain supported through the shared action service.
- Arbitrary next-step IDs supplied by a model or reviewer. Routing remains declared and validated.
- Exactly-once remote-effect guarantees for providers without the necessary idempotency/reconciliation
  contract. Unknown outcomes are exposed and recovered explicitly.
- Recipient read/delivery confirmation as the default meaning of a successful send step. Foundation
  completion means provider acceptance, or local publication for an in-app message.
- Turning authentication, credential management, transport retries, leases, and maintenance into
  company-editable business workflows.

## Included work that must not be deferred by mistake

Human comments, structured feedback, revised drafts, alternate human-selected routes, and bounded
review rounds are part of the foundation. So are direct agent write tools, workflow-as-tool calls,
action-specific authorization, uncertain-effect recovery, independent same-thread message runs,
provider-neutral sending/replying, explicit memory steps, classifier-selected capabilities, and
explicit `mcp.call` steps over Streamable HTTP through the shared action service.
The phases must implement these contracts before the foundation is considered complete.
