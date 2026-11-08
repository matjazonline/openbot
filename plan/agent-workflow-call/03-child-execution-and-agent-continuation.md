# 03 — Child execution and agent continuation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on: 02. Outcome: a channel call starts one child and durably parks/resumes the original
agent tool call, using the existing worker, child, and wait lifecycle.

## Batch A — Isolated channel context and atomic launch

1. Extend child-start configuration with typed alternatives for inherited static/tool calls and
   captured channel calls. Keep shared creation of runs, jobs, causal links, budgets, and waits in
   the existing child owner. Do not bolt on boolean flags or copy `create_on` into another engine.
2. For a channel call, use the selected binding's params/resources, the caller actor, and a fresh
   thread under the destination channel. Record the original calling agent and parent execution
   separately from the canonical message author so audit never falsely attributes a user action.
   No caller-thread IDs, existing target-thread IDs, or arbitrary sender fields come from the model.
3. Create the canonical internal source message and derive `MessageTriggerInput` through the
   existing normalizer. Its storage must not enqueue normal ingress, observers, or an automatic
   transport response. Preserve source-message hashes, author/participant authority, classification,
   retention, and fresh-thread history semantics. Do not impersonate an external email sender.
4. Commit invocation snapshot, thread/source context, target scope, child run/first job, child wait,
   response ownership, and parent park atomically. Roll back all of them on refusal or lease loss.
   Child provenance remains `ChildCause::ToolCall`; having a source message is not message ingress.
5. Preserve parent deadline, tree depth/child count, continuation count, correlation, simulation
   delivery restrictions, and knowledge bounds. Reject a destination channel already present in
   the active channel-call ancestry, including direct self-calls. Separate later calls after a
   completed invocation remain valid; no global blacklist or time-based deduplication.

Focused checks: `channel_call_launch_`, affected child-schema tests, and focused message/history
identity tests. Include a native template using thread history, a parent without a channel, two
different target channels with different params/resources, foreign source-message spoofing, a
classified request that cannot enter the target audience, and zero observer/delivery intent rows.

## Batch B — Resume, cancellation, and durable replay

1. Route `channel.call` from published agent capabilities through the actual saved tool-call
   origin. Reuse pending-call persistence, suspension, agent history, and continuation. A worker
   restart must reconstruct the same call; never restart the model conversation or agent step.
2. Preserve the distinction between child execution termination and callable response readiness.
   After phase 04, success is consumable only when projection/review has settled. Human review,
   child human-input steps, and timers may park without occupying an executing worker.
3. Reuse bounded wakeups, claim fencing, sweeps, terminal-child handling, and cancellation cascade.
   A cancelled parent cannot acquire a new child; a late child/result cannot resurrect it. Record
   typed errors for failed/rejected/expired outcomes and resume the model only when its parent is
   still eligible to continue.
4. Consume a saved result once logically. A crash after child completion, after response approval,
   or after tool-result persistence must replay the same receipt without creating another child,
   repeating completed side effects, or reselecting the destination configuration.
5. Keep caller/target authority live at protected operations and result consumption. Deactivation,
   membership removal, agent retirement, resource revocation, or deletion must settle the affected
   call or follow the existing terminal recovery path, never leave an immortal parent wait.
6. Keep async chains bounded. Box the existing substantial I/O seams when necessary; measure frame
   growth and retain the stock-stack regression gate rather than increasing stack limits.

Focused checks: `channel_call_resume_` and existing `workflow_tool_` continuation/race tests affected
by shared code. Compete two launchers and two resumers; race cancel with launch and completion;
expire a lease at commit; restart after every durable boundary. Assert one child, one saved tool
result, unchanged parent execution ID, monotone continuation accounting, and no stranded wait.

## Acceptance and record

- A scripted parent model calls a channel, suspends, receives its response, and performs a second
  model turn in the same `agent.run` execution and private history exchange.
- Ordinary `workflow.call` and registered workflow-tool calls retain existing behavior.
- Record focused commands/results and before/after stack measurements where the chain changed.

Implementation evidence: pending; successful response integration additionally requires phase 04.
