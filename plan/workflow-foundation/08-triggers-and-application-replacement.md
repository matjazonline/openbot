# 08 — Triggers and replacement of application orchestration

## Outcome and dependencies

Depends on phases 1–7. Route all supported business execution through workflow admission and
remove the old dispatch/configuration model without a compatibility layer.

## Channels and ingress

Keep channel identity, provider bindings, access policy, participants, and message/thread storage.
Replace channel-owned agent lists, automatic agent ownership, memory toggles, and embedded response
orchestration with an explicit workflow binding and validated parameters/resources.

Retain transport authentication, spam/abuse controls, hop limits, sender authorization, canonical
message deduplication, and thread resolution before workflow admission. Company workflows cannot
turn off these boundaries. Trigger eligibility such as direct-address versus CC selection belongs
in explicit binding admission configuration, not a hidden dispatch branch.

Atomically persist inbound acceptance and the workflow-admission intent. Resolve each eligible
channel binding independently. A multi-address message may create one run per eligible binding;
redelivery creates none beyond the original admissions. Each reply names its own triggering
message and producing run. Do not combine agent outputs from independent runs into one answer.

A channel can retain messages without an active automatic workflow binding. Disabling a binding
stops future admissions; cancelling existing runs is a separate command. Existing runs retain
their pinned binding snapshot, subject to current access and connection revocation.

## Other entry points

- Manual starts validate input against a published workflow and current caller access; use a
  client command idempotency key and return the run identity immediately.
- Schedules bind to workflow versions/parameters and deduplicate by materialized schedule slot.
  Reuse the scheduling infrastructure; remove the scheduled-agent-dispatch special case.
- Internal delegation uses child workflows for run-local work and explicit message steps for
  conversation-level communication. Preserve causal links and authorization at the target.
- Outreach requiring a response uses explicit sends and correlated event waits. Matched replies
  resume their intended wait; unrelated messages create independent runs. The foundation supports
  one matching response per wait. Remove the legacy quorum-specific execution loop; multi-response
  aggregation is a separately scoped extension rather than another agent execution path.
- Agent creation creates an agent only. An explicit “Create assistant channel” setup action binds
  a selected agent to an autonomous/reviewed workflow template.

## Public interfaces

Expose company-scoped HTTP routes over the owning application commands for workflow draft
save/validate/publish/archive, binding revision/activation, run start/read/cancel/retry, comments,
decision submission, and action reconciliation. Keep command semantics identical between browser,
API, agent-tool, and notification-link entry points.

Writes use idempotency keys where they create work and expected revisions where they edit state.
Detail, nested, artifact-download, and mutation routes authorize the actual referenced resource,
including its channel/thread. Apply normal CSRF and webhook authentication rules.

## Removal work

Delete obsolete dispatch and scheduled-dispatch branches, task-payload context snapshots,
channel response-review tables/state owners, executable skill instructions, alternate runtime
configuration, and automatic personal-channel coupling after their replacement contracts pass.
Retarget approval, outreach, task-monitor, message-lookup, and simulation callers to the new owners.

Retain useful provider adapters and invariants rather than preserving their old function names.
Update bootstrap fixtures, seed templates, configuration documentation, and examples together.
Do not build a second SOP engine or install the older workflow-library architecture as a prerequisite.

## Acceptance

- Inbound email, scheduled and manual starts, and child calls reach the same run engine.
- Parallel message runs in one conversation retain separate decisions, artifacts, and replies.
- Duplicate schedule slots and provider redeliveries do not start extra work.
- Existing ingress/access/threading protections remain exercised against the new routes.
- No runtime or UI path depends on removed channel execution settings or alternate harness selection.
- Exhaustive caller searches identify no remaining legacy execution entry points.
