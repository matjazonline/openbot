# Call a channel workflow from an agent

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed implementation, 2026-11-08; amended 2026-11-09 with a closure envelope and a
per-call deadline so department leads can use the call as their dispatch primitive
([01-departments](../01-departments/README.md)). These documents do not implement the feature.

## Intended behavior

An agent executing `agent.run` calls one built-in, `channel.call`, with a destination channel and
message content. The runtime resolves that channel's selected responder, captures its configured
workflow and dependencies, runs it as a child, and returns its response to the pending tool call.
The calling model then continues within the same logical `agent.run` execution. No workflow-tool
registration or separate tool definition is required for each destination.

Proposed model-visible call:

```json
{
  "channel_id": "<destination-channel-uuid>",
  "content": {
    "subject": "Order review",
    "body": "Review order 42 and explain whether it can be approved."
  }
}
```

The agent must explicitly have the `channel.call` built-in in its published capabilities. That
grant does not authorize every channel: current caller, company, channel, workflow, resource,
knowledge, and agent authority are checked on each invocation.

## Contract decisions

- **Address the selected responder.** A channel can have other observing bindings. Call only the
  binding selected in channel reply settings; never pick the first binding or broadcast to all.
- **Use the channel's message contract.** Selected responders are message-triggered. The existing
  `MessageTriggerInput` contains `source_message`, `message`, `subject`, and `sender`. The caller
  supplies content; the runtime creates the other facts. This refines the earlier illustrative
  `input: {question: ...}` proposal to fit actual channel workflows. Do not accept model-supplied
  source IDs, sender identities, workflow IDs, resource assignments, or `run_as` overrides.
- **Changing values are request content.** Destination-specific fixed `params` and resources come
  from the selected binding. Per-call content changes freely within the canonical message bounds.
  Arbitrary non-message workflow input schemas remain the existing workflow-tool use case; this
  plan does not introduce a generic unregistered workflow executor.
- **Return the channel response.** Apply the selected response contract and any configured final
  review, then return approved subject/body content. Do not return unreviewed raw workflow output
  alongside it. `channel.no_reply` yields a typed `no_reply` result. Explicit unrelated effects
  inside the child retain their own existing authorization and delivery behavior.
- **Preserve the caller's authority.** Use the parent run's actor. The destination binding's
  `run_as` is configuration provenance, not permission to impersonate another user. Reauthorize
  target resources and agent selections for the caller; narrower authority produces refusal.
- **Snapshot once per saved call.** Resolve the current selection on first dispatch and freeze
  binding/configuration revision, version, response policy, resources, and agent dependencies.
  Retries resume that invocation. A later distinct call can select a newer binding.
- **Keep lineage and dependency ownership distinct.** A channel child remains in the parent's
  cancellation/deadline tree but starts an immutable dependency scope of its own. Ordinary static
  children continue inheriting their nearest dependency scope. Never mutate a sealed manifest.
- **Use an isolated conversation branch.** Allocate a fresh destination-channel thread and source
  message for the call. Do not reuse the caller's thread or read unrelated target-channel history.
  Channel visibility still governs the stored request; perform content/audience checks before
  storing it. Private agent history remains a child branch.
- **Closure says why.** A `replied` result carries a closure: `done`, `denied` with a bounded
  reason, or `handed_off` with the run id the destination delegated to. The destination's response
  contract produces it; a child that declines completes successfully at the run level and closes
  as `denied` at the work level. The caller never has to infer refusal from prose.
- **A call may carry its own deadline.** Optional, and it must be earlier than the parent run's
  deadline; the child's deadline is the earlier of the two. Expiry is the existing typed deadline
  error. A lead can therefore give a member less time than its own run has left.
- **Leave the trail in the threads.** The approved response is written to the destination thread
  as the child's reply, and the parent's thread gets an internal note naming the desk called and
  the closure, so supervisors see delegated work where they see everything else (04, Batch C).
- **Return through the pending tool call.** Do not enqueue ordinary message ingress, observer
  runs, or automatic email delivery. Human review and human-input waits keep their normal gates;
  review notifications are distinct from automatically publishing the final channel response.

## Implementation order

| Phase | Deliverable | Depends on |
| --- | --- | --- |
| [01 — Contracts and authority](01-contracts-and-authority.md) | One built-in contract, typed results, ownership and authority decisions | None |
| [02 — Target snapshots and dependency scopes](02-target-snapshots-and-dependency-scopes.md) | Atomic target capture, immutable invocation records, bounded child dependency capture | 01 |
| [03 — Child execution and agent continuation](03-child-execution-and-agent-continuation.md) | Isolated request context, durable child launch, same-step resume and recovery | 02 |
| [04 — Response ownership and review](04-response-ownership-and-review.md) | Channel response projection/review to a tool result, without automatic external delivery | 03 |
| [05 — Authoring, inspection, and samples](05-authoring-inspection-and-samples.md) | Capability selection, truthful previews, traceable call/response views | 01–04 |
| [06 — Integration and final verification](06-integration-and-final-verification.md) | End-to-end, competing-worker and fault tests; complete verification gate | 01–05 |

Execute phases in order. Each phase has cohesive implementation batches and focused checks. The
feature is complete only after phase 06; an intermediate successful child launch is insufficient.

## Existing owners and important gaps

Source spans were inspected while planning; refresh graph queries before editing because this
checkout has concurrent work in workflow persistence, memory integration, and `src/main.rs`.

| Owner | Starting point | Existing behavior / required change |
| --- | --- | --- |
| Built-in binding | `src/domain/entities/agent_capability.rs:77` | Published built-ins exist; add the single capability |
| Selected channel responder | `src/adapters/persistence/workflow/channel_reply.rs:233` (`selected_on`) | Checks active channel-associated binding and response contract |
| Channel message input | `src/application/workflow/triggers.rs:20`; `src/adapters/persistence/workflow/message_intents.rs:380` | Reuse canonical normalization, not fabricated ingress |
| Registered workflow calls | `src/adapters/persistence/workflow/workflow_tool_calls.rs:148` (`call_on`) | Already parks/resumes a saved tool call; requires registration and a parent manifest selection |
| Child configuration | `src/adapters/persistence/workflow/children.rs:159` (`configured_child`) | Currently uses parent resources, binding, actor, and association; channel calls need a distinct typed target |
| Child creation | `src/adapters/persistence/workflow/children.rs:230` (`create_on`) | Existing atomic run/job/wait linkage should remain the lifecycle owner |
| Manifest capture/attachment | `src/adapters/persistence/workflow/agent_manifest/mod.rs:46`; `src/adapters/persistence/workflow/agent_manifest/storage.rs:30` | Existing scope is sealed at admission and inherited by children; add explicit nested scope ownership |
| Authorization | `src/application/workflow/authorization.rs:150` | Current workflow admission requires company operations authority and channel visibility; do not silently broaden it |
| Continuation proof | `src/adapters/persistence/workflow/workflow_tool_tests.rs:21` | Existing database test preserves one call/child and consumes the result in the same agent execution |

Run `graft callers call_workflow_tool --direction out --depth all` and trace any owner being changed.
Graph edges are incomplete for ambiguous Rust method names; use `graft grep` for exhaustive uses.

## Verification and evidence

Use a disposable database unique to this checkout/task. Never run tests or edited migrations
against another session's development or test database. Follow `src/AGENTS.md` and
`src/adapters/persistence/AGENTS.md`, regenerate SQLx metadata after SQL edits, and keep the init
schema as the only migration. No backfills or compatibility paths belong in this work.

For each focused filter, first run `cargo test --locked --offline --lib <filter> -- --list` and
confirm a nonzero selection, then run it with the isolated `TEST_DATABASE_URL` exported. New
`channel_call_*` prefixes below are required test naming conventions, not tests that exist today.
Record the command, selected/executed count, result, and unavailable dependencies in its phase.
Never use `ALLOW_MISSING_DATABASE_URL=1` as verification evidence. Use `own_database` for unscoped
claims, sweeps, or whole-table assertions. Run the complete gate once after integration, as in 06.

Keep this README and all six phase paths in `NO_BACKWARD_COMPATIBILITY_NOTES` in `src/main.rs`.

Planning validation, 2026-11-08: `cargo fmt --all -- --check` and
`git diff --check -- src/main.rs plan/agent-workflow-call` passed. A local Python consistency check
passed for all seven files: sequential phase numbering, phase dependencies/check sections, relative
links, policy notes, whitespace, and exactly one startup-list entry per file. Feature/database tests
were not run: this change adds the implementation plan and its required startup path registrations.
All implementation evidence remains pending in the numbered phases.
