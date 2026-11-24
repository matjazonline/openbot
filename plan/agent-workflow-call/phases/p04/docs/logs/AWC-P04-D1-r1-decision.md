# AWC-P04-D1 r1 — architecture gate: call response ownership, closures, review and trail

Author: root/A (Claude Code `1e84a7af`, Opus 5.5/xhigh), 2026-11-23. Covers doc 04 batches A–C and the
AWC-C002 p04 placements (`channel.deny`, `channel.hand_off`). Builds on p03 (accepted `b9271b7d…6dbb`).
Applicable notices: AWC-C001 r4, AWC-C002 r1 and AWC-C004 r1.

## Facts (code as of `b9271b7d…6dbb`)

In this document, `schema` means `migrations/20260817000000_init_schema.sql` and `wf/` means
`src/adapters/persistence/workflow/`. TASK's investigation notes carry the full map.

- **Ownership is keyed on ingress.**
  - `channel_reply_contexts` is captured from an admissible `workflow_message_intents` row (schema:13091).
  - `channel_reply_runs` links a **root** run to it (`link_run`, guard schema:13176-13215).
  - `channel_reply_proposals` and `channel_reply_decisions` carry `intent_id`, with FKs to contexts.
  - The guards `require_channel_reply_proposal_links` (:13509) and `channel_reply_publication_matches` (:13596)
    read facts from the context.
- **The Rust root.** `channel_reply_completion::root_on` (:37-47) builds `RootReply { intent_id, message_id,
  channel/principal, thread_id, actor_id, contract, approval_policy, source, deadline, binding }` from
  runs ⋈ contexts. A call record holds every one of these except `intent_id` and `source` (the intent JSON).
  `source` is read only into the proposal's integrity document (:272).
- **Shared final path.** Terminal handling, approval, resume and publication all go through `root_on`. A run
  without it is `Unowned`: no projection, no review and no decision. A call child is `Unowned` today.
- **Publication target.** The final reply is a reserved `message.reply` (origin `channel_final`) to the context's
  source message. For a source with `audience='internal_only'` the planner gives `ReplyInternal`: a note filed in
  the source thread with no delivery (`message_planner.rs:358-392`, `wf/message.rs:510-580`). A call's source
  message is `MessageWrite::internal`.
- **Review.**
  - Approve or reject only, with `data == {}`. **There is no edited content**: the approved content is the
    immutable `proposal.content`.
  - Reject and expiry settle the decision wait with `failure_code` `approval_rejected` / `approval_expired`
    and fail the step with `message.approval_*`, which is routed or fails the run.
- **Output validation.** Every terminal output is validated against the workflow's output schema
  (`batch.rs:144`). Only `channel.no_reply` → `$end` gives `Suppressed` (`batch.rs:166-177`).
- **Gaps.** `channel.deny` and `channel.hand_off` do not exist. p03's success arm projects committed output and
  ignores `response_disposition`: a `channel.no_reply` child closes `done` today.

## D1 — Two explicit response owners

`channel_reply_runs` owns a run's final response for exactly one of:

- an **ingress** context (`intent_id`, as today);
- a **call** (`call_source_message_id`, referencing `workflow_channel_calls(company_id, source_message_id)`).

Schema:

- **`channel_reply_runs`.**
  - `intent_id` becomes nullable, and `call_source_message_id uuid` is added, with
    `CHECK (num_nonnulls(intent_id, call_source_message_id) = 1)`.
  - FK `(company_id, call_source_message_id) → workflow_channel_calls(company_id, source_message_id)`.
  - `UNIQUE(company_id, call_source_message_id)`.
  - `guard_channel_reply_run` keeps its ingress arm unchanged. It gains a call arm: the run is the record's
    `child_run_id`, and the run's binding, revision, channel, thread and actor (= `run_as_user_id`) equal the
    record's. A call child is not a root, so the root-only rule stays on the ingress arm.
- **The owner view.** `channel_reply_owners(company_id, run_id, intent_id, call_source_message_id, message_id,
  channel_id, principal_id, thread_id, actor_id, delivery_policy, binding_id, binding_revision,
  settings_revision, approval_policy, contract, source)` is a `UNION ALL` of:
  - runs ⋈ contexts (ingress);
  - runs ⋈ `workflow_channel_calls` ⋈ the child run (call). For a call, `principal_id` is null,
    `delivery_policy` is the child's, and `source` is `null`.
- **`channel_reply_proposals` and `channel_reply_decisions`.**
  - Each takes the same `intent_id` / `call_source_message_id` pair with exactly-one checks.
  - The existing context FKs stay for ingress (MATCH SIMPLE skips nulls). A call FK is added.
  - `UNIQUE(company_id, call_source_message_id)` is added to decisions beside `UNIQUE(intent_id)`.
  - The proposal integrity check compares `integrity_document->'intent'` and `->'call'` with
    `IS NOT DISTINCT FROM`.
- **Guards.** `require_channel_reply_proposal_links` and `channel_reply_publication_matches` read
  `channel_reply_owners` instead of contexts. Each fact they compare has the same meaning for both owners.

**Linking.** `children::write_call_on` inserts the call child's `channel_reply_runs` row inside p03's savepoint,
after the record. The guard proves the record, the child and ownership agree. An observer, a manual run, another
call or a rerun cannot own it: the FK names one record, the record names one child, and the call arm requires
the run to be that child.

**Rust.**
- `RootReply.intent_id` becomes `owner: ReplyOwner { Ingress(intent) | Call(source_message) }`, and `root_on`
  reads the view.
- Every writer of proposals and decisions binds the owner pair.
- The integrity document carries `intent` or `call`. `source` stays (null for a call).

**Consequence.** Projection, final review, approval, resume, publication and retry run unchanged for a call
child, with the destination's captured contract and policy from p02's record:

- the reply is a `message.reply` to the call's source message;
- because the source is internal-only, the plan is `ReplyInternal`: one reply note in the call's desk thread,
  with no delivery (Batch C.1);
- no context is forged and no second approval engine exists.

## D2 — Closures from the destination's terminal

The response kind is fixed by the terminal step:

| Terminal (→ `$end`) | Response | Over a call |
| --- | --- | --- |
| any other step | the captured contract's projection of the output (as today) | `done` |
| `channel.no_reply` | suppressed, as today | `no_reply` (no content) |
| `channel.deny` (new) | subject `Re: <source subject>` (bounded), body `reason` | `denied`, code `channel.denied`, `reason`, `content`, `fields?` |
| `channel.hand_off` (new) | the nested call's projected reply | `handed_off`, `run_id`, `content`, `fields?` |

- **The two new registry steps.** Both are success-only to `$end`, like `channel.no_reply`. Their output is typed
  by the step:
  - `channel.deny`: `with.reason` (string, ≤ the subject bound) and an optional `with.fields`, validated against
    the version's result-field sub-schema (`output_schema.properties.fields`, AWC-C002 / p01 D3). Its output
    `{reason, fields?}` is validated by that step contract.
  - `channel.hand_off`: `with.run_id` (uuid). Output `{run_id}`.
  - **Output schema exception.** The workflow `output_schema` validates `done` outputs only. For these two
    steps, `batch.rs` validates the step contract instead of the workflow schema. Without this, every responder
    that may deny would have to widen its business output schema to admit `reason`.
- **Hand-off runtime.** At the terminal, the run must have recorded a `channel.call` whose `child_run_id` is
  `with.run_id`, and that call must have closed `done`, read from its settled decision. Otherwise the step fails
  with `channel_call.hand_off_invalid` (routed or failing the run). The hand-off's content is that nested call's
  published content. A `handed_off` nested call is not `done` and is refused (AWC-C001).
- **Proposal kind.** The proposal stores `closure` (`done` / `denied` / `handed_off`) and, for `handed_off`,
  `handed_off_run_id`, both in the integrity document. Review therefore approves the kind as well as the content.
- **Over mail.** `denied` and `handed_off` publish their content like `done`, under the same review.

## D3 — The call result (replaces p03's interim success arm)

At consumption, after p03's live-authority check, the child's outcome is read as follows.

- **Succeeded.** The child's `channel_reply_decisions` row decides:
  - `suppressed` → `{closure: no_reply, child_run_id}`;
  - `pending` / `claimed` / `published` / `failed` → the approved proposal's `closure` and `content`, with
    `fields` from the committed output `/fields` (`done`), the deny output (`denied`) or the nested call's
    result (`handed_off`).

  Publication state does not gate the result. The proposal is already approved and immutable when the decision
  row exists, and the thread reply is a projection (C.4). A succeeded call child without a decision row is a
  defect: `invalid()`.
- **Failed.**
  - When the child's final-reply decision wait settled with `approval_rejected` / `approval_expired`, the result
    is `channel_call.review_rejected` / `channel_call.review_expired`.
  - Otherwise it is `workflow.child_failed`.
- **Cancelled.** `workflow.child_cancelled` (unchanged), including cancellation during review.
- **Too large.** `TOOL_RESULT_TOO_LARGE`, unchanged.
- **Invalid projection.** The projection runs at the child's terminal, through the existing final path, before
  any review. A failure there fails the child's step with the existing refusal (`message.final_response_refused`).
  At consumption that reads as `child_failed`, unless the child routed it. `channel_call.response_invalid` stays
  for a decision whose stored content does not decode, which is a defect guard. **This narrows AWC-C004's
  "reuse for invalid projections": the projection error now surfaces inside the child.** p03's corrupted-output
  test is rewritten to expect the child-side refusal.
- **Absorption.** Absorb as p03 does, before returning, and only for a disclosed closure (`done`, `denied`,
  `handed_off`). `no_reply` discloses no content and absorbs nothing.

## D4 — Review for a call

- p03's interim rule (an approval policy declines) is removed (AWC-C004). The destination's captured
  `approval_policy` drives the existing `channel_reply_approvals` path, with its reviewers, deadline (bounded by
  `review_timeout_seconds` and the run deadline, so it is never longer than the call's deadline), clearance check
  and artifact/revision checks.
- **Display.** The decision view for a call-owned proposal says that approval returns the content to the
  calling agent, and names the caller run. It never says the reply will be mailed. The text comes from the
  owner kind in `channel_reply_views`.
- **Edits.** Channel finals have no edit path. Approving releases exactly `proposal.content`, so "edited
  approved text" in 04 B is satisfied trivially and recorded as such. No edit path is added.
- **Exactly once.** A replayed approval or stale submission is refused by the existing revision/artifact checks,
  and the decision row is unique per owner. Approval versus cancellation and approval versus revocation use the
  existing locks (decision wait, run row). B2's race tests exercise them for a call owner.
- **Live authority at release.** p03's consumption check (origin, write right, desk enabled, binding active)
  still runs at the caller. The existing publication re-validation (`validate_on`) still runs for the reply note.

## D5 — Confidentiality and history

- **Classification.** The child's content level and exactness are absorbed into the caller before the result is
  stored by the harness (p03 D4, unchanged). Projection does not lower the label.
- **Private branches.**
  - The child's history is its fresh thread (p03).
  - The parent receives only the closure value. Never the child's committed output beyond `/fields`, its
    transcript, its prompts or its tool results.
  - B2's privacy tests read every parent-visible surface for a marker that appears only in the child's raw
    output: the tool result, the run view, notifications and the parent thread.
- **Retention.** The call record's `NO ACTION` keys (AWC-C004) keep the source and thread while the caller
  exists. The proposal and decision FK to the owner row, which cascades only with the child run. Deleting a
  caller during review is refused while the child exists: the child holds `parent_run_id`, and run retention
  deletes leaves first. B2 asserts the ordering and the cancellation-during-review outcome.

## D6 — Thread trail

- **Destination reply (C.1).** It is the existing publication of the call child's final reply: a `ReplyInternal`
  note in the call's desk thread, written by the publisher (its own transaction, exactly once per decision). No
  new writer is added.
  - **Deviation from C's "both writes in the transaction that settles the call":** the destination reply settles
    with its decision's publication, as every final reply does.
  - Its role is the existing final author (`message_author_on`: the destination's agent, else the system). C.1's
    `MessageRole::Agent` therefore holds only for agent destinations, and B3's test records which.
  - `denied` publishes its reason as the body, and the closure is readable from the proposal for the row
    renderer. `no_reply` publishes nothing.
- **Caller note (C.2).** One internal note on the caller's thread, when the caller run has a thread association.
  - It names the destination desk, the child run and the closure, and never quotes content.
  - It is written in the consumption transaction that first discloses the result, keyed by the call
    (`UNIQUE` on a new `workflow_channel_call_notes(company_id, run_id, execution_id, call_id)`), so a replay
    writes nothing twice.
  - Its author is the system principal, with the calling agent named in the body, stamped `Public`. It carries
    no classified content.
- **Not read back.** Nothing resumes from either write (C.4). Read access is each desk's existing grants.

## Invariants

1. A run has at most one response owner: ingress or call, never both. A call child's owner names its record.
2. One call yields at most one decision and one released closure, and releases it only after review when the
   destination requires it.
3. No call path writes an intent, a reply context, a delivery or an external row.
4. The caller sees only the closure value, after absorption and after live authority.
5. Ingress behaviour is unchanged: same rows, same guards, read through the view.

## Failure modes

| Failure | Behaviour |
| --- | --- |
| Review rejected / expired | Child step fails (routed or run fails); the caller gets `review_rejected` / `review_expired`. |
| Cancel during review | Decision wait cancelled with the child; the caller gets `child_cancelled`. |
| Projection refused | The child fails at its terminal; the caller gets `child_failed`. |
| Hand-off to a run not called, or not `done` | `channel_call.hand_off_invalid` in the child. |
| Publication fails or retries | No effect on the call result (projection only). |
| Replay after release | The same closure is read from immutable rows; the note is unique. |

## Verification plan

| ID | Test | Assertion |
| --- | --- | --- |
| A1 | `channel_call_response_owner_is_the_call` | The call child has a `channel_reply_runs` call row; forged rows (another run, an ingress run naming a call, both keys) are refused by name; ingress rows unchanged. |
| A2 | `channel_call_response_automatic_done` | Native and custom destinations: `done` with the projected content; one decision; one reply note in the desk thread; zero deliveries, intents or contexts. |
| A3 | `channel_call_response_no_reply` | `no_reply`, no content, no absorption; decision `suppressed`; no thread reply; the parent's own later response is unaffected. |
| A4 | `channel_call_response_deny_and_hand_off` | `denied` with its reason and fields; `handed_off` to a nested `done` call; hand-off to an uncalled run, or to a non-`done` call, gives `hand_off_invalid`; over mail, deny publishes its reason. |
| A5 | `channel_call_response_projection_refused` | An unprojectable output fails the child; the caller gets `child_failed`; nothing is published. |
| A6 | `channel_call_response_independent_effect` | A destination's explicit `message.send` keeps its own effect policy alongside the call result. |
| A7 | `channel_call_response_reply_to_source` | `message.reply` to the call source inside the child files an internal note on the call thread (no recipient guessed); `message.ask` either works or refuses before creating a wait (recorded either way). |
| B1 | `channel_call_review_approve_releases` | A policy desk: the child waits for review; the caller stays parked; approval releases exactly `proposal.content`; the view text names the calling agent. |
| B2 | `channel_call_review_reject_and_expire` | `review_rejected` and `review_expired`, no content, caller continued. |
| B3 | `channel_call_review_races` | Competing approvals; approval versus caller cancel; approval versus revocation; stale submission. Exactly one release, no deadlock. |
| B4 | `channel_call_privacy_no_raw_leak` | A child-only marker is absent from the tool result, the run views, notifications and the parent thread. |
| B5 | `channel_call_privacy_retention` | Caller and child deletion ordering; cancellation during review; the retention sweep keeps source and thread while needed. |
| C1 | `channel_call_trail_reply_and_note` | One reply on the desk thread, one note on the caller thread; `no_reply` leaves only the note; `denied` shows its reason; a caller without a thread gets no note; a replay writes nothing twice; a reader of one desk sees nothing of the other. |

Affected suites: `channel_reply_` (all terminal, publication, approval and views tests, since the
ownership change touches every one), `channel_call_`, `message_`, `workflow_decision`, `knowledge_`,
`stack-budget.sh`.

## Batches

- **B1 — ownership, closures, result and review:** D1–D5 and A1–A7, B1–B5. Review reuse is automatic once
  ownership is generalized, so the two are verified together.
- **B2 — trail:** D6 and C1.
