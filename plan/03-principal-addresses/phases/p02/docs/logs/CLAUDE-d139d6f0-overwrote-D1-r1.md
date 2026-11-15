# 03PA-P02 D1/r1 (immutable copy of TASK "D1 — architecture gate (revision r1)")

## D1 — architecture gate (revision r1, 2026-11-14)

The evidence is `logs/D1-inventory.md`. It is a read-only inventory of the current tree with the accepted p01 changes applied. It counts about 86 schema objects on or depending on the thread tables, 22 `workflow_bindings` queries, the whole ingress and commit path, the access functions, the public-mode surfaces and the C004 producers.

### D1.1 Batches and phase boundaries

The phase is too large for one thorough review, so it splits at stable contracts. Each later batch gets its own gate when it introduces a decision costly to unwind.

- **B1, direct threads.** The thread target schema, ingress resolution and the commit for principal targets, direct access (decision 5) with the Direct lists, one intent per principal recipient (moved here by C006), the "People and agents" directory heading (C006), internal relay delivery to a principal, and the hop trace.
  - Intents for principal targets are written, but nothing admits them until B2. They behave like an intent on a desk with no binding.
- **B2, principal bindings and admission.**
  - `workflow_bindings.principal_id` is agent-only and `company_wide` covers both null cases.
  - Admission gates on the principal row.
  - Runs, history and refusals on direct threads.
  - A role binding on an agent resolves to its department; several departments are refused as ambiguous.
  - The C004 re-inventory belongs here.
- **B3, the public flag and principal routing revisions.**
  - The `principals.open_to_public` API and settings form, with the spam interlock.
  - `principals.settings_revision`, bumped by slug and flag edits.
  - The `public_desk_private_agent_` checks.
  - The column itself lands in B1 (default false), because direct access reads it.
- **B4, direct-message notifications for people.**
  - README: "a person reads theirs in the product and in notifications".
  - No plan document assigns it. Today's notifications have no message kind (delivery, decision_reviewer and escalation only), so it is new work.
- **Not in p02:**
  - In p03:
    - an agent answering from its direct thread: outbound sender identity, `message_deliveries` attribution, and the relay `From` check per 03B;
    - agent address registration;
    - notice rerouting (C001/C003);
    - rerunning the agent-target checks.
  - In p04: the documentation.

### D1.2 Target pair: the same shape as p01's `addresses`

- `threads` gets `channel_id uuid` (now nullable) and `principal_id uuid`, with `CHECK (num_nonnulls(channel_id, principal_id) = 1)`. There are two composite target keys, `UNIQUE (company_id, channel_id, id)` (existing) and `UNIQUE (company_id, principal_id, id)`, and FKs to `channels(company_id,id)` and `principals(company_id,id)`. A principal target must be a person or an agent; the guard follows p01's `guard_address_principal`, using `FOR KEY SHARE`.
- **Every table that pins a thread's target** (`thread_messages`, `thread_principals`, `external_threads`, and in B1 also `internal_notes` and `workflow_message_intents` with the tables whose FKs carry their target) gets the same nullable pair and the exactly-one check. It also gets **three FKs to `threads`**:
  - `(company_id, thread_id) → threads(company_id, id)`. This one is always enforced.
  - `(company_id, channel_id, thread_id) → threads(company_id, channel_id, id)`.
  - `(company_id, principal_id, thread_id) → threads(company_id, principal_id, id)`.
  - Under `MATCH SIMPLE`, a composite FK is checked only when none of its columns is NULL. So each target FK pins exactly the rows of its kind, and the first FK guarantees the thread exists for both. This is the reason for the extra plain FK: without it, a direct row would reference its thread through no checked key.
- **`thread_principals`' primary key** `(company, channel, thread, principal, role)` cannot hold a nullable column. It becomes `UNIQUE NULLS NOT DISTINCT (company_id, channel_id, principal_target_id, thread_id, principal_id, role)` with no primary key. A plain `PRIMARY KEY (company_id, thread_id, principal_id, role)` would also be equivalent, because a thread has exactly one target. The plain key is chosen as the smaller one.
- **`thread_messages_channel_message_key UNIQUE (channel_id, message_id)`** keeps its meaning of one row per target per message. It becomes `UNIQUE NULLS NOT DISTINCT (company_id, channel_id, principal_id, message_id)`. `thread_messages_workflow_history_key` and the six FKs that reference it get the principal twin, `(company_id, principal_id, thread_id, message_id)`.
- **Only the B1 path is generalized.** Tables first reached by B2 (`workflow_runs`, `workflow_history_members`, `agent_history_*`, `workflow_admission_refusals`) or by p03 (`channel_reply_contexts`, `message_deliveries`) are generalized in their own batch. `notifications.channel_id` belongs to B4.
- In B1 they stay channel-only. That is sound because nothing reaches them from a direct thread until B2 admits a run.
- **Desk-only guards stay desk-only** (inventory §1, items 22–29). They key on runs or replies, which a direct thread does not have in B1. NOTIFY payloads keyed on `channel_id` gain a `principal_id` field (null for desks).

### D1.3 Transport interface: direct mail has no endpoint row (inventory open question 1)

- A desk's email interface (`channel_bindings`) is its provider endpoint. Decision 7 says "no per-address mailbox, DNS record or new credential", so principals get no endpoint row, and their mail rides the company domain.
- `ThreadAssociation` gets a target enum, `AssociationTarget::{Desk { channel_id, binding_id }, Principal { principal_id }}`.
- `verify_association_bindings` checks desks only.
- `bind_conversations` writes `external_threads` only for desk associations. Provider thread keys are a desk-interface concept, and direct threads have no `external_threads` rows. That keeps `external_threads` channel-only, so the pair in D1.2 is not needed there and D1.2's list drops it.
- Direct-thread correlation follows the user's decision ("like desk mail"):
  - by `Message-ID`, `In-Reply-To` and `References` within the principal target (`find_thread_for_message`);
  - by ancestor (`find_ancestor_thread`);
  - by thread index (`find_thread_by_thread_index`);
  - each scoped by target instead of by channel.
- What this leaves unproved: correlation by provider thread key for direct mail. For email that key is derived from the same headers, so the header rules cover it.

### D1.4 The hop trace names targets (inventory open question 2)

- `trace_channels uuid[]` becomes `trace_targets uuid[]`, and the domain and wire carry the ids of the desks and principals passed through. Channel and principal ids are both v4 UUIDs, so cycle detection compares ids and needs no kind tag.
- The `X-MailAgents-Trace` header carries the same ids. The bound of 5 entries is unchanged.
- **Deviation from 02B.3's wording** ("the principal's address in the trace"): an id is the stable identity, while an address can be renamed mid-trace. The loop refusal applies to principals as the plan intends.

### D1.5 One direct thread per conversation under concurrency (inventory open question 3)

- Resolution stays in the application, before the commit. For a principal association, the commit takes `pg_advisory_xact_lock` on a key derived from `(company_id, principal_id)`. It takes these locks in sorted order, after the existing gates and before resolving threads.
- Under that lock it re-runs the header lookup (`Message-ID`, `In-Reply-To`, `References`, scoped to the target) for each `Create` target. A thread committed by a racing message of the same conversation is joined instead of forked.
- Desk behaviour is unchanged.
- Competing test, `direct_competing_`: message B references message A, and both commit concurrently on two connections. The test proves exactly one direct thread holds both.

### D1.6 Decision 5 access (B1)

- `Principal::direct_access(&self, sender: PrincipalAccessContext) -> ParticipantAccess` and `Principal::direct_viewer_access(&self, viewer: PrincipalAccessContext) -> bool` in `src/domain/entities/participant.rs` are the only definitions:
  - **Person target:** read and write only for that person and the company owner. Everyone else is refused, including other teammates (decision 5: "nobody else").
  - **Agent target:** read for the company team. Write for anyone who may write to a team desk, which is the team. An external sender may write only when `open_to_public`; such a sender is untrusted and gets the desk-public spam scoring.
  - Department membership and roles do not affect either rule.
- Refusals reuse p01's stranger rule. A non-team sender always gets the unknown-address answer, so a member's address stays unconfirmable. A refused team sender gets an `Unauthorized`-class refusal naming the address. `ContextSuffix` stays refused.
- `notification_principal_can_view` gets a principal-target branch, used by B4.
- The thread pages get a "Direct" list for each principal whose direct threads the viewer may read: their own, the owner's view of every person, and the team's view of agents.

### D1.7 Invariants and failure modes

- Every thread-keyed row has exactly one target, and it equals its thread's target (D1.2 FKs). Deleting a principal cascades as for desks.
- A message is associated at most once per target.
- A direct thread's target is a person or an agent. Demoting the principal while it has direct threads keeps the threads, as history, and the target FK remains valid. Access then requires the principal to still be a person or agent, so a demoted target's threads are visible to the owner only.
- The relay sender check stays ahead of delivery to a principal (COORDINATION revision 10).
- Errors on authorization lookups propagate (`src/AGENTS.md`).

### D1.8 B1 verification plan

- **`direct_thread_`:**
  - a person's direct thread is invisible to a teammate and visible to the owner;
  - an agent's (fixture stand-in, C006) is visible to the team;
  - an outsider's mail to the agent gives the unknown answer while `open_to_public` is false;
  - a reply joins its thread by headers, and a new subject starts a new thread;
  - one intent per principal recipient;
  - a desk-plus-person message commits both associations;
  - the "People and agents" heading lists what the sender may write to;
  - the trace refuses a loop through a principal;
  - an internal relay delivers to a direct thread while keeping the sender check.
- **`direct_competing_`:** the D1.5 race on two connections, `own_database`, proving one thread.
- **Schema (rollback-only):**
  - a row with both or neither target is refused;
  - a direct row pointing at a desk thread, or the reverse, is refused by the target FKs;
  - a cross-tenant target is refused;
  - an `external`/`system` thread target is refused.
- **Regressions:**
  - every thread, commit, ingress and relay suite;
  - the p01 `address_*` tests;
  - plan02 escalation and channel-reply suites;
  - internal notes;
  - the mailbox and thread routes.
- **Gates:** fmt, whitespace, diagnostic clippy, all boundary scripts with a real `rg`, source modules, a fresh migrate plus `.sqlx`, and test names before and after.

### D1.9 Affected contracts

- the `Thread` entity (`channel_id`, which becomes a target);
- the thread port lookups (inventory §3, 5 methods);
- `ThreadAssociation`, `InboundAdmissionIntent`, `ChannelCandidate` and `PreparedChannel`, which gain the target;
- `trace_channels`, which becomes `trace_targets`, in the wire header, the domain and the schema;
- the NOTIFY payloads;
- the thread pages;
- 28 non-test reads of `thread.channel_id`.

B2–B4 contracts are listed by their own gates.
