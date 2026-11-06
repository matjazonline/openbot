# 04 — Live models and integrations

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on 03. Measure real model behavior using existing provider/harness execution while keeping
business operations simulated. Real external effects belong only to a separately provisioned
disposable integration environment. The original sample interpreter remains offline.

## Batch 04A — Explicit live inference with isolated tools/state

Add an application-owned live-eval execution operation that uses the frozen workflow/agent/model
contracts and existing provider adapter. Reuse the responsible execution/input-preparation owners;
extract a small common policy/traversal helper where necessary rather than cloning the workflow
engine or pretending a scripted response is a real provider response.

Every HTTP/MCP/message/knowledge/memory/child-agent surface must pass through the evaluation
dispatcher. Permit model inference only to the selected authorized provider destination; intercept
external reads and writes using validated case fixtures, isolate history/memory stores, and keep
child workflows in the same mode. Unknown/unsupported operations fail closed with no production
fallback. A workflow/tool result cannot change mode, select another company's connection, call
an arbitrary URL, or escape through an imported child workflow.

Reuse operation-schema, tool-grant and workflow-policy checks where applicable. Record proposed
actions, rejected actions and simulated approvals distinctly from actual effects/real human
approval. A supplied approval fixture never creates a production approval record or capability.
Freeze retrieval/history context; do not silently query changing production knowledge during a
supposedly matched comparison. Do not save eval exchanges as customer conversation history.

**Focused checks:** `eval_live_boundary_` and `eval_live_tools_` through scripted local model
servers and counted fake services. Prove nested workflows, direct agent tools, HTTP/MCP, reply,
memory/history and unexpected operations cannot hit production paths. Inspect prepared provider
requests for absence of references/labels/expected answers, credentials and foreign context.

## Batch 04B — Budgeted execution and honest recovery

Require existing company model access, execution permission, and an explicit run budget before
paid dispatch. Grading shares that budget. Reserve bounded worst-case input/output capacity before
each provider call using existing budget authority, including concurrent attempts and retries;
observed token counters after the call are not a pre-call budget. Enforce case/run deadlines,
provider timeouts, tool rounds, prompt/output bounds and company/global concurrency.

Store actual model/parameters, provider request identity when available, usage, timing and attempt
outcome. Unknown billing or an interrupted remote response remains unknown/uncertain. Never
silently reissue a potentially charged inference as though request IDs guarantee idempotency.
Use existing recovery contracts; any allowed retry consumes attempt/spend budget and remains
visible. Lease loss or cancellation reaches the provider work and prevents stale settlement.

**Focused checks:** `eval_live_budget_` and `eval_live_recovery_` database-backed races for one
remaining budget reservation, duplicate worker dispatch, revoked model access, dropped responses,
provider errors, cancellation and lease loss. Assert counted calls, budget accounting and explicit
unknown outcomes. A scripted provider proves orchestration, not real-inference compatibility.

## Batch 04C — Disposable integration acceptance

Run the normal application against isolated databases, companies, mail sinks and test HTTP/MCP
accounts/services. The environment owns explicit test-only credentials and destination allowlists;
ordinary eval requests cannot switch to production credentials or authorize real customer writes.
Integration tests inspect actual remote/database state and effect counts rather than trusting
the agent's final text. Cover approvals, tenant isolation, idempotency/concurrent claimants and
recovery with the actual production dispatch protocol.

Record source/image revisions, fixture identity/digest, actor/resource scope, assertions and
results as integration evidence, with provenance distinguishing CI/operator evidence from a
user-imported claim. Do not add a public API that accepts arbitrary signed-off success blobs as
authoritative release evidence. Attach only bounded redacted observations through trusted tooling.

For [Doop](../doop-canvas/README.md), use disposable workspaces/accounts and complete the
[private-file fix](../fix-file-access/README.md) before real confidential document acceptance.
Other workflows use their own controlled service fixtures; there is no dependency on Doop for
the general eval runner.

**Focused checks:** `eval_integration_` with real application workers and disposable counted
HTTP/MCP/mail services, including negative cross-company access and competing claims. Separately
record any real service/model probes and unavailable credentials; no silent skipping or claim
that fixture acceptance establishes a real external service's behavior.

## Exit criteria

Live inference measures actual model decisions without a route to production business effects.
Budgets and recovery hold across workers; actual integration guarantees have separate, attributable
disposable-environment evidence.
