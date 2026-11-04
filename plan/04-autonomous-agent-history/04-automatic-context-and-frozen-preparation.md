# 04 — Automatic context and frozen preparation

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [03](03-turn-ordering-and-recovery.md). Each eligible agent call automatically receives
a bounded sequence of prior completed input/output exchanges followed by its current input.
Retries and continuations reproduce the originally selected context.

## Batch 04A — Select and freeze eligible history

1. Extend the history-selection boundary in [agent preparation](../../src/adapters/persistence/workflow/agent_preparation.rs).
   Once the exchange owns its turn, read prior completed exchanges in its authorized stream.
   Exclude the current exchange, queued/failed/cancelled exchanges, foreign/private branches,
   samples from another scope, and diagnostic/tool-only records. Exclude in SQL every exchange
   whose `access_level` is above the run's `knowledge_ceiling`
   ([knowledge access levels 03A](../01-knowledge-access-levels/03-run-levels-history-and-egress.md)).
   Such an exchange is not a candidate at all. It does not stop the newest-to-oldest walk, it is
   not counted as omitted, and nothing in the prompt, the preparation record or the diagnostics
   marks its absence. The agent then answers "I don't have that" rather than "that is restricted".
2. Select complete input/output pairs from newest to oldest, then present the chosen recent
   suffix chronologically. Stop at the first eligible pair that does not fit; never emit an
   orphan input or answer, silently truncate structured JSON, or skip a large recent pair to
   include older ones. Record omission counts and the selected exchange identities/order.
3. Account for current instructions, tool definitions, output-schema instructions, current input,
   and reserved output capacity before allocating the remaining model context budget to history.
   Use existing model budget/accounting facilities with an explicit conservative fallback where
   exact token counts are unavailable. Keep token estimates distinct from byte/item limits.
4. Bound query rows, payload bytes, serialization work, pair count, and final request size. If
   mandatory current-call content cannot fit, return the existing controlled budget failure;
   silently dropping current input or increasing context limits is not an acceptable fallback.
   Keep proof of these bounds in CI, including stack/checkpoint growth checks where affected.
5. Persist selection, ordering, content integrity, and actual immutable replay material with
   preparation before provider entry. Either reference immutable payloads whose lifetime is
   pinned or store a bounded snapshot; choose one ownership model and document it. A live query
   on retry or a hash without recoverable content does not satisfy frozen preparation.
   In the transaction that freezes the preparation, call `raise_content_level` with the highest
   selected exchange level (knowledge plan 03B), and record that level on the preparation. A
   retry replays the same selection, so it neither lowers nor re-raises the level.
6. Recheck current history/conversation authority before dispatch and resume. If access is revoked,
   refuse or cancel according to existing policy; do not regenerate a different prompt under an
   already opened checkpoint. Cleanup cannot prune payloads needed by a live preparation.
   The recheck includes level. On dispatch and on every resume, recompute the audience level:
   the thread's current participants, or the acting member's current level. If it is below the
   frozen preparation's recorded level, refuse through the same revocation path, before provider
   entry, with `knowledge.audience_below_content`. Example: a customer CC'd while a level-5
   exchange waits on human input means the resumed call never runs. The release check would also
   stop the reply, but by then model tokens would have been spent and the failure would be less
   clear.

Focused checks: `agent_history_selection_` for empty history, chronological pair order, exact
limits, oversized newest pair, incomplete exchanges, JSON scalar/object preservation, omission
counts, current-input exclusion, revocation, retention pinning, and bounded queries. Add
`agent_history_level_` cases:

- a ceiling-0 call selects no level-5 exchange, and its omission count is unchanged by them;
- the walk continues past a level-5 exchange to older level-0 pairs;
- selection raises the run's content level;
- a retry replays the same level;
- a resume after the audience dropped is refused before provider entry (counted provider calls
  stay unchanged). Use meaningful
boundary cases rather than tests that only restate the selection implementation.

## Batch 04B — Seed the model protocol and preserve replay

1. Extend [PreparedAgent](../../src/application/workflow/agent.rs), the
   [checkpoint specification](../../src/application/workflow/checkpoints.rs), and
   [checkpoint matching](../../src/adapters/persistence/workflow/checkpoints.rs) coherently.
   Matching currently assumes the first message is the initial prompt; update that invariant to
   include the frozen seed conversation and current input without weakening immutable preparation.
2. Represent historical inputs as untrusted user/task data and outputs as prior assistant data,
   followed by current input. Preserve canonical structured serialization and current prompt
   boundaries. The current agent instructions, capabilities, and output contract remain authoritative;
   an old input/output containing role names or instructions cannot become a system message.
3. Keep prior logical exchanges distinct from the current execution's protocol messages. The
   [provider history adapter](../../src/adapters/harness/rig/protocol.rs) currently associates each
   checkpoint assistant message with a saved model turn. Do not insert synthetic history into
   that sequence as though it had provider turn metadata. Add an explicit bounded seed segment
   or equivalent representation that composes with existing live tool/repair replay.
4. Never replay historical tool-call IDs, reasoning/provider tokens, or tool invocations as new
   actionable messages. Existing checkpoint recovery still preserves the current execution's
   exact provider/tool protocol. Historical outputs are ordinary data, not provider continuations.
5. Update request fingerprints, input counts, context/token admission, checkpoint byte accounting,
   and diagnostics to include the selected history exactly once. Do not reload history on each
   model turn or count retained history again as a newly saved logical input.
6. Keep model-only steps unchanged and run supported provider protocol tests. No model call is
   needed to summarize history in this scope; add summarization later only with its own durable
   semantics and evidence.

Focused checks: `agent_history_preparation_` and `agent_history_protocol_` using counted scripted
providers to inspect the actual messages for two separate calls, provider changes, output-schema
changes, repaired output, tool continuation, checkpoint reopen, and worker restart. Prove historical
assistant messages do not consume current-turn metadata or trigger old tools. Run existing agent
preparation, provider replay/repair, budget, and checkpoint matching regressions.

## Acceptance

A second agent call sees the prior completed input and output without workflow-authored history
steps. Retrying that call sends the same selected conversation. The logical input record contains
only that call's supplied input, so history does not grow recursively. Record evidence under
[07](07-integration-and-verification.md).
