# 04 — Shared actions, HTTP, MCP, tools, and delivery

## Outcome and dependencies

Depends on phases 1–3. Give workflow steps and direct agent tools one effect protocol. Phase 5
implements human authorization waits; protected actions must remain undispatched until then.

## Action contract

Add an application-owned action service accepting capability, scoped connection/target,
schema-validated arguments, logical execution identity, and policy context. Both workflow
handlers and agent tool bridges must use this service. Reuse existing tool implementations where
they satisfy the contract; remove routes that bypass it.

An action invocation freezes its operation before authorization or dispatch. Store its canonical
argument digest, required capability, policy decision, stable idempotency key, attempt records,
and effect receipt. Approval is bound to the invocation and digest. A changed argument set creates
a new invocation and requires a fresh authorization decision.

Agent model tool-call IDs map durably to invocations. A workflow step's action identity derives
from the logical step execution, never the current worker attempt. Receipts own effect truth;
agent conversation checkpoints reference or copy accepted results for replay.

## Execution and uncertainty

1. Authorize with current company/resource access and the run's capability ceiling. A frozen
   definition is not a grant to bypass revocation. Fail closed on policy lookup errors.
2. Persist intent before dispatch. Commit local database effects and their receipt in one
   transaction where they share a database; otherwise use the durable invocation protocol.
3. Use provider idempotency only where the provider contract supports it. HTTP method names and
   a client-generated header alone do not prove a write is safe to repeat.
4. Record successful receipts before advancing the step or returning tool output to the agent.
5. A timeout/crash after dispatch can be ambiguous. If safe replay or provider reconciliation is
   unavailable, mark the invocation as needing reconciliation and park the owning execution.
   Never blindly retry a potentially applied non-idempotent write.
6. Add authorized reconciliation commands to attach evidence and record applied/not-applied.
   Only a proven not-applied result permits retry. Unknown remains unknown; wait deadlines can
   fail the run without discarding the unresolved invocation.
7. Cancellation suppresses undispatched work. It cannot retract accepted remote effects, and late
   receipts remain auditable even when they no longer advance the cancelled run.

## HTTP and registered tools

`http.request` uses company-owned connections with fixed permitted origins and scoped encrypted
credentials. Parameters supply paths, methods, bounded headers and bodies. Do not let a model
replace the connection origin or introduce credential-bearing arbitrary URLs.

Validate resolved addresses and reject private, loopback, link-local, and metadata destinations
by default; guard against DNS rebinding at connection time. Disable redirects. Bound time,
response size, and retained output. Redact credentials and sensitive headers from traces and
context. Server operators own any explicitly required private-network exception.

`tool.call` exposes registered capabilities through the same service. Unknown or unsupported
write/recovery contracts cannot be registered as safely retryable. Remote tools must also obey
these rules; their metadata is not an authority to grant access.

## MCP tool calls over HTTP

Add `mcp.call` as a first-class workflow step. It invokes one explicitly selected MCP tool over
Streamable HTTP without requiring an agent or model tool selection. Its `with` configuration
contains `connection` (a declared MCP resource slot), `tool` (a fixed tool name), and `arguments`
(a typed object supporting the phase 2 binding vocabulary). Resolve the slot through the run's
binding revision to a company-owned MCP connection with a fixed endpoint and credential reference.
Never accept endpoint URLs, raw credentials, or arbitrary authorization headers from step inputs.

```yaml
lookup_customer:
  type: mcp.call
  with:
    connection: customer_service
    tool: lookup_customer
    arguments:
      customer_id: { ref: "/input/customer_id" }
  next: $end
```

The containing workflow declares the `customer_service` MCP resource slot and the input schema;
its binding selects the company connection. Reuse/refactor the existing MCP client port and HTTP
adapter for initialization, discovery, calls, cancellation, and session cleanup. The workflow
handler invokes the shared action service; transport/session code stays in the adapter. Direct
agent MCP tools use that same service and enforce the same authorization and recovery policy.

Validate the selected tool and argument schema during publication/binding readiness checks. Freeze
the approved tool contract in the published bundle and reject incompatible discovery at execution
before dispatch; server metadata cannot silently broaden capabilities or establish retry safety.
Resolve current credentials and enforce connection/tool revocation at use time. Apply the HTTP
destination protections, redirect restrictions, deadlines, size bounds, and secret redaction above
to initialization, discovery, and calls, including JSON and SSE responses.

Expose a bounded result envelope containing `content`, optional `structuredContent`, and `isError`
(default false when absent). Validate structured output against the frozen output schema when one
is declared. Downstream steps reference the committed envelope through `steps.<id>.output`.
Do not automatically fetch resource links returned in tool content. A tool-level `isError: true`
is a classified action failure, not success; persist the bounded error result for diagnostics.
Neither tool errors nor output-validation failures prove that a remote effect was not applied.

Persist the frozen connection identity, tool, arguments, and invocation identity before dispatch.
MCP request/session IDs are transport correlation, not durable idempotency keys. A timeout, lost
response, worker crash, or session expiry after possible dispatch follows the unknown-effect
reconciliation protocol; reconnecting must not silently repeat `tools/call`. Cancellation attempts
protocol cancellation and session cleanup but does not prove rollback. Only the approved operation
contract or reconciliation evidence can establish that replay is safe.

## Provider-neutral messaging

Implement `message.send` and `message.reply` using canonical content, scoped destinations, and
the existing delivery engine. Replies name an explicit source message and preserve provider
threading; never target the latest message in the thread by inference.

Atomically create the canonical message, required delivery intents, action linkage, and wait.
Durable receipt events resume the step when all required deliveries reach provider acceptance.
Represent queued, accepted, failed, and uncertain states accurately. Acceptance does not mean
the recipient received or read the message. Preserve stable outbound email Message-IDs.

For multiple delivery destinations, retain each receipt and retry only unresolved safely
retryable work. Never resend a successful sibling when another destination fails. Purely
in-app publication completes at the database commit. Implement email now; keep provider adapters
outside workflow handlers.

## Acceptance

- Direct tools and step handlers enforce identical grants, approval, and idempotency behavior.
- Competing dispatchers, stale workers, crash-after-send, partial delivery, and duplicate callbacks
  cannot silently create new logical effects.
- Tests cover uncertain writes, reconciliation, revocation, cancellation, and argument changes.
- HTTP tests cover redirect, private-address, DNS-resolution, timeout, size, and secret leakage paths.
- MCP fixtures exercise initialization/discovery and explicit calls with JSON and SSE responses,
  typed argument/result references, tool errors, schema drift, and unavailable or revoked tools.
- MCP tests cover cross-company connections, secret redaction, response limits, session expiry,
  cancellation cleanup, and crash/timeout after dispatch without unsafe replay. Competing claimants
  cannot dispatch the same invocation concurrently; committed results survive restart without a call.

## Design references

- [Transactional outbox](https://docs.aws.amazon.com/prescriptive-guidance/latest/cloud-design-patterns/transactional-outbox.html): database/dispatch consistency still requires duplicate handling.
- [OWASP SSRF prevention](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html): destination validation and redirect restrictions.
