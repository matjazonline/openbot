# 06 — Authoring, status views, and operations

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [05](05-human-input-and-continuations.md). Authors can configure the channel's response
contract and use silent completion. Participants and operators can distinguish work completed,
human input needed, publication pending, and delivery trouble without interpreting internal rows.

## Batch 06A — Configure and author the behavior

1. Extend existing channel settings and workflow binding APIs/forms with responding-binding
   selection and the bounded output projection. Use normal authorization and revision conflict
   handling. Show which workflow responds and which bindings merely observe; expose contract
   errors at configuration/activation before a user sends a test message.
2. Add `channel.no_reply` to the existing step catalogue, help, editor/source validation, graph
   rendering, and examples. Show it as a terminal node with a business value; enforce its route
   restrictions on the server as well as in the editor. Use an accessible label such as
   "Finish without reply" and explain its effect on the automatic final response.
3. Provide examples for: ordinary automatic reply; conditional silent completion; explicit reply
   followed by silent completion; intermediate question/HITL then automatic final reply; and a
   protected final response requiring review. Examples must be executable against the registry.
4. Update new-channel/binding defaults and relevant shipped templates so a default responding
   workflow does not both explicitly send a final reply and cause an accidental automatic copy.
   Where explicit final sending is intentional, finish with `channel.no_reply`. This is a fresh
   schema/template change, with no rewriting/backfilling old stored workflow versions.
5. Samples preview the intended final response or suppression and HITL state using existing sealed
   sample execution. They must never queue a production delivery or send a real reviewer notice.

Focused check: `channel_reply_authoring_` route/rendering tests and the affected authoring/sample
browser protocol. Include unauthorized changes, stale forms, invalid projection, terminal-node
editing, keyboard interaction, escaped result content, and no-production-effects assertions.

## Batch 06B — Present linked outcomes and safe controls

1. Extend existing run/thread/delivery readers with bounded tenant-authorized joins connecting
   incoming message, owning run, prompt wait/decision, final reply, and delivery. Use stored source
   identities even when the original message is outside the currently paginated thread window.
2. Keep workflow execution state and message status distinct. Derive the user-facing summary from
   those records; do not maintain a competing `overall_success` flag. Examples:

   | Execution | Response | Display |
   | --- | --- | --- |
   | Running | None yet | Processing |
   | Waiting for decision/input | Prompt sent | Waiting for your response / waiting for reviewer |
   | Waiting for decision/input | Prompt failed/unconfirmed | Waiting; request needs attention |
   | Succeeded | Publication/delivery pending | Work completed; reply pending |
   | Succeeded | Provider accepted or locally published | Completed; reply sent / posted |
   | Succeeded | Final delivery failed/unconfirmed | Work completed; reply needs attention |
   | Succeeded | Explicitly suppressed | Completed without reply |
   | Failed/cancelled | Earlier messages may exist | Failed/cancelled; preserve message history |

3. Use the actual transport semantics: accepted/sent does not promise a read receipt or confirmed
   human receipt. Show uncertain outcomes honestly. A private review request is visible only to
   authorized readers, even when its source run is associated with a public channel.
4. Provide links from failure to the owning intent/delivery and existing authorized recovery
   controls. Safe delivery retry must target the same message and never rerun business work.
   An unknown provider outcome must not expose a blind resend button. Cancel pending delivery
   only through existing cancellation semantics; never imply recall after provider acceptance.
5. Emit bounded structured events for final response disposition, publication, approval waiting,
   delivery linkage, and failures. Reuse monitoring categories for retry, lease loss, dead letter,
   unknown outcome, and stuck work. Logs/metrics contain IDs and status, not message bodies,
   recipient lists, approval tokens, or unbounded identifiers as metric labels.
6. Update [workflow operations](../../docs/workflow-operations.md) and
   [verification guidance](../../docs/workflow-verification.md) with actual supported contracts,
   status meanings, fresh-database requirements, authorization behavior, and recovery commands.
   Do not document configuration keys or startup validation that the implementation does not own.

Focused check: `channel_reply_views_` and `channel_reply_controls_` for independent state
combinations, pagination, tenant/participant visibility, source lookup, safe versus uncertain retry,
and SSE/navigation updates. Run affected conversation/run/decision browser tests and CSS checks.

## Acceptance

- A user can tell whether work is still running, human input is needed, a reply is intentionally
  absent, or a completed result has a delivery problem.
- Default examples require no explicit final send step. Custom communication still works through
  explicit message actions, with clear suppression where appropriate.
- UI/API projections reveal no foreign/private data and confer no new decision/send authority.
- Record the focused checks under [07](07-integration-and-verification.md). Rebuild generated CSS
  only when source changes require it, and verify the generated asset is current.
