# 02 — Terminal outcomes and `channel.no_reply`

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-contracts-and-response-ownership.md). Add a terminal `channel.no_reply` node
that means: complete this workflow run successfully and suppress its automatic final response.
Represent this as typed completion metadata separate from the workflow's business output.

## Batch 02A — Compile the terminal contract

1. Extend the existing workflow source/compiler/catalogue with `channel.no_reply`. It is a pure
   control node, not a transport call or an agent tool that mutates a channel setting.
2. Give it an explicit typed business value to return using the existing value-binding pattern.
   Validate that value against the workflow's output schema. A workflow that needs a richer
   result passes that result through the node; suppression must not replace it with a control
   object such as `{"no_reply": true}`.
3. Require terminal success routing; reject a success successor step or choice routes on this
   node. Normal failure/error routing still applies. Normal completion has the default response
   disposition; this node has a suppressed disposition. Derive that metadata from the compiled
   step contract, never an untrusted output field.
4. Keep failure distinct from silent success. Invalid input/output, exhausted limits, cancellation,
   and deadline expiry retain normal workflow handling rather than becoming silent success.
5. Update catalogue revision/serialization, registry metadata/help/examples, source diagnostics,
   published identity, and exhaustive type matches together. Do not add legacy decoders.

Anchors: [transition](../../src/domain/workflow/transition.rs),
[registry](../../src/application/workflow/registry/mod.rs),
[pure execution](../../src/application/workflow/batch.rs), and
[I/O completion](../../src/application/workflow/completion.rs).

Focused check: `channel_reply_terminal_contract_` covering valid terminal nodes, rejected routes,
business-output preservation, source round trips, publication identity, and forged output flags.
Run the affected existing catalogue/schema/compiler tests because this changes a shared contract.

## Batch 02B — Carry disposition through completion and replay

1. Add the typed disposition to the shared prepared/committed terminal completion representation.
   Persist it with the terminal result. Read and replay the exact committed disposition.
2. Trace all paths into [batch commit](../../src/adapters/persistence/workflow/batch_commit.rs):
   pure advancement, fenced I/O, decisions, waits, child completion, recovery, and operator retry.
   A node's successful terminal disposition must not be lost by an adapter or reconstructed from
   incidental history such as whether any earlier message was sent.
3. Scope suppression to this run. Child/repeat outputs carry their normal business result back
   to their parent; the child's silent disposition cannot suppress a parent response. A parent
   must reach its own `channel.no_reply` to finish silently.
4. For runs without an automatic-response obligation, record the terminal disposition normally
   and create no response. Scheduled/manual/child runs do not acquire a destination implicitly.
5. Preserve explicit sends and replies already published. A workflow can send a final explicit
   reply, then terminate with `channel.no_reply`; earlier intermediate replies followed by normal
   completion still allow the channel's final reply.

Focused check: `channel_reply_terminal_` database tests for pure and I/O predecessors, human and
child continuations, duplicate completion, restart/replay, cancellation races, and independent
parent/child dispositions. Check preserved output and disposition after persistence round trips.

## Acceptance

- Ordinary terminal success and silent terminal success are distinct, typed, replayable outcomes.
- Suppression is terminal and local to its run; there is no mutable channel-wide or run-wide
  switch that an intermediate step flips and later branches inherit accidentally.
- `channel.no_reply` creates no message or delivery and does not cancel earlier messages.
- Execute and record focused checks under [07](07-integration-and-verification.md); SQL/schema
  changes require fresh migration checks and regenerated offline metadata.
