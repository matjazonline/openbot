# AWC-P04-D1 r2 — delta on r1

Author: root/A (Claude Code `1e84a7af`, Opus 5.5/xhigh), 2026-11-23. Answers review `AWC-P04-D1-R1-N1`
(F1–F7).

**r2 is [r1](AWC-P04-D1-r1-decision.md) with the replacements and additions below.** Everything not replaced
stays in force.

r2 replaces:

- D1's schema bullets and its "Rust" paragraph;
- D3's "Failed" and "Invalid projection" items;
- D5's "Retention" item;
- D6's "Caller note" item;
- r1's verification rows A4, B5 and C1.

It adds:

- D2a (where the response steps may run);
- D7 (consumption facts);
- D8 (policy and interactive shapes);
- the reader table.

In this document, `schema` means `migrations/20260817000000_init_schema.sql`.

## D1 schema, replaced (F1)

The owner key is made explicit and non-null, so that every equality guard keeps working.

**`channel_reply_runs`.**

- New columns:
  - `owner_kind text NOT NULL CHECK (owner_kind IN ('ingress','call'))`;
  - `intent_id uuid` (ingress) and `call_source_message_id uuid` (call), with a check that exactly the kind's
    column is set;
  - `owner_id uuid GENERATED ALWAYS AS (COALESCE(intent_id, call_source_message_id)) STORED NOT NULL`.
- Keys: `UNIQUE(company_id, owner_id)` and `UNIQUE(company_id, run_id, owner_id)`.
- FKs:
  - ingress: the existing FK to contexts (MATCH SIMPLE);
  - call: `(company_id, call_source_message_id) → workflow_channel_calls(company_id, source_message_id)`.
- The guard keeps its ingress arm. Its call arm is as in r1.

**`channel_reply_proposals` and `channel_reply_decisions`.**

- `intent_id` is renamed `owner_id` (NOT NULL). Their context FK becomes
  `(company_id, run_id, owner_id) → channel_reply_runs(company_id, run_id, owner_id)`.
  - The context is still reachable, for ingress, through the run row.
- `UNIQUE(intent_id)` becomes `UNIQUE(company_id, owner_id)`.
- The integrity document's `intent` key becomes `owner`, plus `owner_kind`.

**The owner view.** `channel_reply_owners` is as in r1, keyed by `owner_id`. Every former context join on a reply
table reads it.

### Readers

The reader table was built by search: `grep -rn channel_reply_contexts src` (non-test), `intent_id` in every
reply-table reader, and every schema object at :13006-13645 that names `intent_id`.

| Reader | Today | Disposition |
| --- | --- | --- |
| `capture_channel_reply_context`, `guard_channel_reply_context`, `guard_channel_reply_intent` (:13091-13174) | context ⇄ intent | unchanged (ingress only) |
| `guard_channel_reply_run` (:13176) | ingress | ingress arm unchanged; call arm added |
| `guard_channel_reply_proposal` (:13393, `intent_id` equality at :13410) | proposal ↔ run | compares `owner_id` (renamed column, same equality) |
| `guard_channel_reply_decision` (:13432, immutability list) | | `owner_id` in the immutable list |
| `require_channel_reply_decision` (:13474, :13489) | run ↔ decision | compares `owner_id` |
| `require_channel_reply_proposal_links` (:13509) | proposal ↔ context | reads the owner view |
| `channel_reply_publication_matches` (:13596) | decision ↔ context | reads the owner view |
| `channel_reply_completion::root_on` (:37-47) | runs ⋈ contexts | owner view; `RootReply.owner: ReplyOwner` |
| `channel_reply_completion` writers (8 uses), `channel_reply_proposals.rs` (3), `channel_reply_publish_authority.rs` (1), `channel_reply_publish_read.rs` (2) | `intent_id` binds and reads | `owner_id` |
| `approval_accounts::load_on` (:31) | policy via contexts | owner view (the call arm gives the record's policy) |
| `agent_preparation::owned_agent_desk_on` (:81-91) | requires a context and a root run | owner view; the call arm requires the run to be the record's child on the record's desk and thread, with actor = `run_as`; the root rule stays on the ingress arm. Without this, an agent-run destination of a call cannot prepare at all; p03's tests used `data.map` destinations only |
| `channel_reply_views.sql` (:12), `channel_reply_views.rs` (:201) | views by context | owner view; the call arm renders the D4 text |
| `channel_reply.rs::frozen_revision` / `link_run` (:407, :432) | ingress admission | unchanged (ingress only; a call child is linked by `write_call_on`) |
| `message_intents.rs` (:143) | intent → response | unchanged (ingress only) |
| `notification/projection.rs` (:117) | a delivery-failure notice joins the published decision's context | unchanged: call owners create no deliveries, so the left join never matches a call; the join key is renamed `owner_id` |
| escalation objects (schema :18503-19466, `agent_notice_message.rs`) | `workflow_message_intents.intent_id` | not reply tables; unchanged |

B1 runs the full `channel_reply_` suite as the ingress regression for every row.

## D2a — Where the response steps may run (F3, added)

- `channel.deny` and `channel.hand_off` complete only in a run that has a response owner.
- At the terminal, before any output is committed, an unowned run fails the step with
  `channel.response_step_unowned`, routed or failing the run. Unowned runs include a `workflow.call` child, a
  workflow-tool child, a manual run and an observer.
- So no consumer that assumes "a succeeded output fits its schema" ever reads a deny or hand-off output:
  - static and `workflow.call` parents (`child_completion_plan.rs:93`);
  - workflow-tool callers (`workflow_tool_calls.rs:313`).
- `require_channel_reply_decision` compares the decision with the proposal, whose `business_output` is the step
  output. That holds for both steps.

## D3 items, replaced (F4)

- **Failed.** Let `E` be the child's terminal execution (`terminal_execution_id`). If the proposal of `(child, E)`
  has an `approval_decision_id`, and that decision's wait in `E` settled with `failure_code` `approval_rejected` /
  `approval_expired`, the result is `channel_call.review_rejected` / `channel_call.review_expired`. Otherwise it
  is `workflow.child_failed`.
  - A rejection routed to another step leaves no failed terminal at `E` with that proposal. A later failure
    therefore reads `child_failed`.
  - Other approval waits (action approvals) are not the proposal's and are never read.
- **Invalid projection.** As r1: it fails inside the child, and the caller reads `child_failed`.
  `channel_call.response_invalid` remains the defect guard. **AWC-C004 is revised to r2 with this narrowing.**

## D5 "Retention", replaced (F5)

- The caller and its call child share one budget tree (p02). `storage_retention.rs:47-49` deletes a whole budget
  tree in one statement, so the two go together. There is no leaves-first order.
- The record's `NO ACTION` keys and its immutability guard are checked at the end of that statement, by which
  point the caller is gone, so the guard admits the delete.
- The proposal and decision cascade with the child's run. Eligibility (a terminal tree) means no review is
  pending.
- A tree with a pending review is not eligible. A cancelled call during review is eligible once terminal.
- B5 runs the production retention (`storage_retention`) on a finished tree and on a tree whose review was
  cancelled, and asserts that the record, thread, source, proposal and decision rows go exactly when the tree goes.

## D6 "Caller note", replaced (F2 link); destination reply (F6)

- **Caller note.** Written in the consumption transaction that inserts the call's consumption fact (D7), only on
  that insert and only when the caller has a thread association. Content and author are as in r1. Idempotence
  comes from the fact's primary key. The `workflow_channel_call_notes` table is dropped from r1.
- **Destination reply.** As in r1: the existing final publication. **The user confirmed this on 2026-11-23
  ("reuse the publisher")**, and doc 04 C is amended in this change. The alternative (writing the reply at
  settlement) is not taken.
  - The desk's view when publication is refused or exhausts its attempts after the caller already has the
    answer: the decision row shows `failed`, and the run's final-response panel shows "Final response failed",
    the existing ingress rendering. The desk thread lacks the reply. The caller's result stands, because it was
    read from the approved proposal (C.4).

## D7 — Consumption facts (F2, added)

New table `workflow_channel_call_results`:

| Column | Notes |
| --- | --- |
| `company_id`, `run_id`, `execution_id`, `call_id` | PK, FK → `workflow_channel_calls` |
| `closure` | `done` / `denied` / `handed_off` / `no_reply`, or `error` with `code` |
| `content` | jsonb or null |
| `fields` | jsonb or null |
| `content_level`, `content_exact` | |
| `created_at` | |

- It is immutable.
- It is inserted by the first consumption that discloses a result: closures and errors, but not `revoked`, which
  is re-judged on each replay (p03).
- A later replay returns this row, so the model sees one value however often the saved call replays.
- `channel.hand_off` validates against **this run's** fact: a row for the call whose `child_run_id` is
  `with.run_id`, with `closure = done`. Its content and fields are the hand-off's. A call that was revoked, never
  consumed, or closed otherwise is refused with `channel_call.hand_off_invalid`. The run has already absorbed what
  it consumed.

## D8 — Policy and interactive shapes (F7, added)

- **Captured policy.** For a call owner, `policy_current_on` compares against the record's captured
  `approval_policy` (the owner view), not the desk's current settings. A mid-call edit therefore never fails or
  replaces the captured policy (04 B.3). Ingress is unchanged. Live reviewer membership and clearance are still
  checked at approval, and binding activity is checked at release (p03 consumption).
- **`message.ask` in a call child** targets the call's requester (an agent), which cannot answer. It is refused
  at dispatch, before any wait exists, with `channel_call.interactive_unsupported`.
  - `message.reply` to the call source is supported (a `ReplyInternal` note, A7).
  - Human decisions inside the destination (`decision.human` by the desk's reviewers) are unaffected.

## Verification rows, replaced and added

| ID | Test | Assertion |
| --- | --- | --- |
| A4 | `channel_call_response_deny_and_hand_off` | `denied` with reason and fields; `handed_off` to a consumed `done` call; hand-off to an uncalled run, a revoked/unconsumed call, or a non-`done` call gives `hand_off_invalid`; over mail, deny publishes its reason |
| A8 | `channel_call_response_steps_require_owner` | `channel.deny` / `channel.hand_off` in a `workflow.call` child, a workflow-tool child and a manual run fail with `response_step_unowned` and commit no output |
| A9 | `channel_call_response_agent_destination_prepares` | an `agent.run` (native template) destination prepares and answers over a call |
| A10 | `channel_call_response_captured_policy` | a policy edit mid-call neither fails nor replaces the captured review |
| A11 | `channel_call_response_ask_refused` | `message.ask` in a call child fails with `interactive_unsupported`; no wait exists |
| B2a | `channel_call_review_rejected_then_routed` | a rejection routed to a later failing step gives `child_failed`, not `review_rejected` |
| B5 | `channel_call_privacy_retention` | production `storage_retention` on a finished tree and a review-cancelled tree; all call rows go with the tree, and none before |
| C1 | `channel_call_trail_reply_and_note` | as r1, with "a replay writes nothing twice" asserted through the consumption fact |
| D7 | `channel_call_response_consumed_once` | replays return the stored fact; `revoked` writes no fact |

## Notices

- **AWC-C004 r2** (on this gate's pass): `channel_call.response_invalid` is a defect guard only; invalid
  projections fail inside the child.

## Finding dispositions

| Finding | Disposition |
| --- | --- |
| F1 | Explicit non-null `owner_id`; reader table by search, with a disposition per reader; the agent-preparation arm added |
| F2 | D7 consumption facts; hand-off reads this run's consumed `done` |
| F3 | D2a: owned runs only; A8 |
| F4 | D3 "Failed" keyed on the terminal execution's proposal decision wait; B2a |
| F5 | D5 restated (one statement per tree); B5 runs production retention |
| F6 | User confirmed the publisher reuse (2026-11-23); doc 04 C amended; the failure-time view is stated |
| F7 | D8 captured policy and the `message.ask` refusal; AWC-C004 r2 |
