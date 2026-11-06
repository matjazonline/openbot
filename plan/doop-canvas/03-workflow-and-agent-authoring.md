# 03 — Workflow and agent authoring

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependencies

Depends on 02. Ship a runnable document workflow/sample using existing MCP resources, operation
contracts, agent publication, and workflow execution. Match the current source format at
implementation time rather than inventing a second tool-binding DSL.

## Batch 03A — Fixed-workspace creation

Validate a bounded title/brief and the approved source-data audience before the first external
write. Bind the workflow's MCP resource to its company's reviewed connection. Pass the workspace
ID from trusted published configuration into a deterministic `create_canvas` action; model/user
text cannot override it. Parse and validate the created canvas identity before invoking an agent.

Persist creation's outcome through the existing durable action/result owner so resuming an agent
uses the same known canvas. Preserve any required guide/profile initialization from 01. Do not
create a new remote canvas merely because the agent retries or a workflow worker restarts.

Canvas/frame creation is not assumed idempotent. If a connection drops after a possible write,
use existing action uncertainty and explicit reconciliation: list/read candidate canvases as
authorized, resolve unambiguous identity, or require operator reconciliation. Do not infer identity
from a non-unique title or silently reissue a possibly completed create. Add a provider-owned
idempotency mechanism only if the pinned Doop version actually supports its claimed semantics.

**Focused checks:** `doop_create_` and `doop_recovery_` database-backed tests with a counted MCP
server: successful resume, rejected workspace override, wrong result identity, error envelope,
timeouts before/after remote acceptance, and cancellation. Race two real workflow workers for
one action and assert the external call count and honest uncertain state.

## Batch 03B — Edit and review through a published agent

Give the editing agent the created canvas ID, bounded brief, existing approved document guidance,
and the selected company MCP operations. Read the guide/canvas first, create or edit HTML frames,
stream bounded sections if supported, inspect the final frame/canvas, and return the canvas ID
and a concise completion summary. Use existing agent instructions/skills; do not add a new runtime.

Treat canvas HTML, comments, tool output, and imported text as untrusted content. They cannot
expand tool grants, select credentials, request an unrelated workspace, or authorize publication.
Do not put bearer keys in HTML or reuse a human/admin session as a rendering credential.

Keep current limits for tool rounds, wall time, input/output bytes, rendered frame dimensions,
screenshots, and model context. Use compact reads and existing truncation/refusal behavior. A
failed/unsupported visual inspection must not be reported as reviewed. Preserve user edits; use
Doop's proven conflict controls and surface a conflict instead of a blind destructive overwrite.

Retain existing action authorization, approvals, access-level propagation, live final-dispatch
checks, leases, cancellation, sample isolation, and provider budgets for every MCP call. Synthetic
sample runs must never reach the production Doop instance.

**Focused checks:** `doop_agent_` and `doop_sample_` integration tests through real workflow and
agent dispatch with a scripted provider/MCP server. Cover ungranted tools, prompt injection,
revoked authority, streaming failure, human edit conflicts, bounded results, and honest completion.
Run existing MCP-agent and workflow-sample boundary regressions.

## Exit criteria

A counted, resumable workflow creates one known canvas in its fixed workspace and a published
agent edits it through normal authorization. Failed, cancelled, conflicted, and uncertain work
remain distinguishable and cannot produce a false success or automatic duplicate creation.
