# 05 — Openbot management and workflows

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-company-machines-and-pairing.md) through
[04](04-cloudroom-plugin-and-session-recovery.md). Users manage local agent sessions in Openbot,
and workflows can submit work without holding a worker lease for a native agent's lifetime.

## Batch 05A — Company management and local profiles

Add company machine settings using existing layout, authentication, CSRF, and company-management
use cases. Show pair/revoke/enable/disable, last seen, compatible/incompatible/offline status,
advertised projects/providers, current workload and actionable connection failures. A connection
test establishes plugin reachability/catalog exchange only. An explicit agent smoke task is what
establishes actual provider execution; the UI must not conflate them.

Add local agent profiles with name, machine, advertised local project, exact provider/model and
Cloudroom permission/workspace choices supported by the compatibility manifest. Project-directory
reuse is the default. Worktrees are optional when the existing Cloudroom environment provider
supports them; Openbot does not implement Git checkout management. Validate all selections against
the current authoritative company binding and catalog, then freeze them into accepted operations.

Profiles are separate from existing published native `Agent` definitions. Give users a clear
local execution choice without making CLI agents pretend to support Openbot-native bindings,
structured model decisions, tool policies, or managed provider credentials. Native tool access
remains under Cloudroom/local-account policy.

Session views show prompt/turn status, bounded final output, known usage, pending interactions,
connection gaps, uncertain submissions and the external thread identity. Support start, follow-up,
inspect, explicit thread-wide stop and archive. Unknown usage is unavailable, not zero. Never
construct a public URL from a local path; use a supported Cloudroom deep link when proven, otherwise
show a copyable thread ID and instructions to open it locally. Opening Openbot in a remote browser
does not grant that browser access to the machine's localhost UI.

All read/control requests resolve the session's company and original Openbot association. Reuse
existing conversation/workflow access rules for output; don't expose hidden run data through a
company-wide machine list. Unrelated local UI threads are not imported automatically.

## Batch 05B — Durable workflow handler

Register one explicit proposed step, `local_agent.run`, through the current workflow catalogue.
Its input selects an authorized local profile, a bounded prompt and optionally an already
authorized local session for continuation. A new call without a session creates a fresh thread;
do not silently reuse a session across conversations, channels, agent profiles or samples.
Return bounded text and typed operation/session references, with measured usage only when known.

Declare the step's external effects, admission budgets and required approval behavior in registry
metadata and existing workflow admission. Choosing local execution does not bypass Openbot
approval or action authority. Recheck live dispatch permission as specified in phase 03.

Implement through `WorkflowHandler` and current handler routing in `src/infra/workflow.rs`.
Keep native `agent.run`, `ai.classify`, `decision.agent` and Rig preparation unchanged. Do not add
the historical `AgentExecutionDriver` abstraction or route a full CLI agent through a custom-tool
script backend. Existing workflow tool publication may expose a workflow containing the new step
after its normal publication/approval checks; no separate direct-tool catalog is required.

At admission, transactionally resolve live authority, freeze selection/input, save the external
operation keyed by the stable logical workflow invocation, and park the workflow on a durable
wait. Repeated claims reuse that operation; lease/attempt IDs are not new external command IDs.
No socket call occurs inside the transaction. The connection owner delivers the saved command.

At outcome acceptance, commit operation receipt/output and the corresponding workflow wake-up
atomically. A fresh claim consumes the result under the current fence and completes once. New
output from the local machine is never a continuation capability by itself. Cancellation,
deadline, changed authority or a stale claimant prevents workflow continuation, even if the local
agent later succeeds. Record late evidence without reviving the run or publishing it.

Offline machines can retain bounded queued work until its absolute deadline, visibly waiting for
the selected machine. Never silently fail over to another machine or replay uncertain work. If
the handler cannot safely resume after a gap, return an explicit uncertain/external-recovery state
using the existing workflow failure/reconciliation vocabulary, not a retryable network error that
creates a new thread. Claim-loss recovery must reuse the same saved operation and wake-up identity.

## Batch 05C — Interaction and publication boundaries

Native CLI permission requests remain in Cloudroom. Openbot displays the blocked operation and
where to respond; this is separate from any Openbot approval required to admit the workflow or
publish a message. Do not translate an agent-generated question/result into an approval grant.

Openbot outputs continue through existing message/response-intent/publication paths. A local
assistant message is not automatically an email, Slack reply, or tool receipt. Agent sessions
created by one authorized workflow do not acquire arbitrary channel history or Openbot tools;
send only that invocation's authorized bounded prompt/context. Samples may neither start local
effects nor contact paired machines; extend the existing sample boundary checks.

For simultaneous Openbot/UI use, retain Cloudroom's queue and show the actual observed turn.
Manual thread-wide stop explicitly affects current local work. Automated timeout behavior follows
phase 04's targeted-stop capability boundary. No second Openbot prompt queue may reorder already
accepted local messages or treat a human's completed turn as its own completion.

**Focused checks:** `cloudroom_management_`, `cloudroom_workflow_`, `cloudroom_publication_`, and
`cloudroom_samples_`. Cover foreign IDs, private conversation output, frozen selections, queued
offline deadlines, competing workflow claims, duplicate/late result wake-ups, lease loss, human
turn interleaving, permission waits, and cancellation versus completion. Run affected registry,
native-agent, workflow-wait and sample-boundary regressions and relevant browser form checks.
