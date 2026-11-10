# 05 — Authoring, inspection, and samples

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on: 01–04. Outcome: users can enable and understand channel calls, and inspect or simulate
them without extra workflow-tool registrations or hidden production effects.

## Batch A — Capability authoring and channel contract visibility

1. Add the built-in to the existing agent capability catalogue, draft validation, publication,
   schema/name adaptation, resolution, exactness checks, and protected-tool approval selection.
   An agent without the grant must neither see nor invoke it. Avoid silent automatic grants.
2. Explain in tool help that `channel_id` addresses the selected responder, content varies per
   call, fixed configuration comes from that binding, and the result arrives in the same agent
   step after completion/review. Show one native-channel example and a typed `no_reply` outcome.
3. Extend existing authorized channel detail/selection views to expose callable status and a
   copyable channel ID, selected responder label, and whether final review is required. Do not
   reveal params, resource secrets, prompts, or inaccessible channel metadata. Reuse existing
   channel lookup; this feature does not require a new global discovery service or tool registry.
4. Keep the public request contract fixed. There is no per-channel tool-schema registration and no
   schema discovery round trip because channel calls use the canonical message contract. Explain
   that direct arbitrary workflow input is a different invocation contract.
5. Update operations documentation with actual revocation, retry, cancellation, approval, limits,
   and retention behavior. Do not promise startup validation that runtime alone performs.

Focused checks: `channel_call_authoring_`, existing agent authoring/exactness tests, and relevant
channel response authoring browser tests. Cover draft/published separation, stale saves, malicious
labels, reserved aliases, and inaccessible target details. Preserve keyboard/form accessibility.

## Batch B — Inspection and effect-free sample execution

1. Extend call/run/step views with the caller agent and execution, destination channel/binding
   revision, child link, selected workflow version/dependency scope, response state, and review
   link. Show the difference between child execution complete and response awaiting approval.
   Use existing pagination, bounded projections, and authorized links.
2. Update dependency views to follow explicit scopes. Parent and child can legitimately have
   different manifests; do not label the child's selected agent as the caller's root selection.
   Preserve original causal tree links for cancellation and tracing.
3. Extend sample capture/execution with a frozen, authorized channel target and declared simulated
   call result. Samples create no production invocation, thread, message, child run, delivery,
   review, or model/network request. Required tool/final-response approvals must be simulated
   explicitly; a missing response fixture is a visible incomplete sample, not an empty success.
4. Preserve untrusted-data labeling, redaction, knowledge levels, and retention in both live and
   sample views. Log IDs and typed transitions, not raw request/response content or secret config.
5. Add bounded operational signals for target refusal, launch, child wait, review wait, response
   readiness, parent continuation, terminal error, and deadline. Use existing durable event state;
   do not create a new telemetry service or rely on process counters to find stuck calls.

Focused checks: `channel_call_views_` and `channel_call_sample_`, affected dependency/history view
tests, and `python3 scripts/workflow-sample-boundary.py`. Verify sample production-table counts
remain unchanged, foreign scopes never render links/content, and a waiting review is distinguishable
from a stuck worker. Run relevant existing browser protocols with real generated fixtures.

## Acceptance and record

- A user enables one built-in and calls two differently configured channels without creating two
  workflow-tool registrations.
- Authoring, samples, and run inspection describe the same invocation and response semantics.
- Record exact focused and browser commands, fixture generation, counts/results, and omissions.

Implementation evidence: pending.
