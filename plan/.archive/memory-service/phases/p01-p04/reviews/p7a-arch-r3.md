# Review p7a-arch-r3: phase 05 architecture recheck (round 3, text only)

**Verdict: PASS for the phase-05 architecture gate. Two items carry into P7b's assignment (R3-m1,
R3-n1); neither changes the architecture.**

Reviewer B, generation 4 (agent `a5916c4a63f198c6f`, nonce `run=8e7c1e85 role=B gen=4`).
Date 2026-11-08. Read-only: no project file was edited and nothing was committed.

## Artifact identity

| Artifact | sha256 (recomputed; all match) |
| --- | --- |
| `tmp/memory-service-run/frozen/p7a-arch-r3.diff` | `bd8d2082f6f2a1740b71dc0c746a9a3786c539f42b1a9968e9d95944f15b64e7` |
| `tmp/memory-service-run/frozen/p7a-r3-incremental-from-r2.diff` (+115/−47) | `35b587fda44b191623aafced1e78c501bc7b52cf1155e0956fcee5ec41a898c1` |

- The tree, recomputed with a temporary index (base `a6bed48a`, `tmp/` excluded), is
  `4bbd84f19011e701a23c72a74fb04145242a40af` and matches the worktree.
- `git diff --no-index --check` on the record is clean.

## Coverage

- I read the whole r2→r3 incremental diff and the parts of the record it changes.
- I checked the new anchors against the code:
  - `batch_commit.rs:11` is `complete`.
  - `checkpoint_updates.rs:4` is `update_on`.
  - `action_authority.rs:42` is `prelock_accounts_on`.
  - `selection.rs` has the `selected_level` aggregate.
  - `history_reader.rs:144` is the history read.
- I re-derived lock interactions for disable, save, capture and lazy provisioning, using the
  round-2 traces (`prelock_accounts_on`, `authorize_company`, `remove_member`,
  `delete_user_account`, agent delete).

## Round-2 findings

| r2 | Status | Evidence |
| --- | --- | --- |
| N-M1 | Resolved | See the breakdown below the table. |
| N-m1 | Resolved | Hook 1 is inside `batch_commit::complete`, named as the single writer for every caller. Hook 2 is at `update_on` `InvocationResult`. The coverage test drives every caller and every `InvocationResult` path. |
| N-m2 | Resolved | Order stated as users → company → membership/principal → agent → run. Membership is locked after the prelocks. Enable and lazy provisioning run in their own short transaction, never inside the fenced run. |
| N-m3 | Resolved | One recall is one interactive operation. A test runs two concurrent recalls in one company. |
| N-m4 | Resolved, option (a) | `content_exact` on agent-authored messages and exchanges, stamped with their level. `context.history` and the history freeze lower the reading run's flag (`bool_and`). A test reads an inexact run's message through both paths. |
| N-m5 | Resolved | The compromise procedure is in E1 (operator revokes, client credentials keep serving, refusals show in settings, operator completes deletions by hand). 06A reassignment is required before the first production deploy. |
| Nits | Resolved | `provisioner_credential_id` is also on `memory_lifecycles`. The cap is cached from `TenantView`, with settings text. Capture-switch changes are audited, and the cross-channel text is shown when `threads` is turned on. `child_branch` conversations and attached sessions are never captured. Header line 8 is wrapped; the long lines left are pre-existing. |

How N-M1 is resolved:

- **Disable** takes the users → company prelocks, then `memory_tenants FOR UPDATE` with
  `revision + 1`. It then fails the pending intents and sets the lifecycle to disabled.
- **Saves, tool saves and capture enqueue** take `memory_tenants FOR SHARE` after the run row and
  before the namespace. They require `enabled`, plus the agent's switch for capture.
- **Why no deadlock:** both sides hold the company prelock in the same order, so they serialize and
  cannot form a cycle.
- **Backstop:** each intent carries the tenant revision, and a claim refuses a stale one.
- **Test:** a competing test runs both orders and then re-enables.

## New findings

### R3-m1 (Minor, carry into P7b): the revision backstop can fail valid saves unless only disable bumps it

- *Location:* E2 (`memory_tenants.revision`) and E8 (a claim requires the intent's revision to equal
  the current one).
- *Problem:* The record says disable bumps `revision`. It does not say that nothing else does. The
  same row is also rewritten:
  - when the `generation_cap` cache is refreshed after a create;
  - when `provisioner_credential_id` is recorded;
  - on settings edits.

  If any of these bumps a general row version, every legitimate pending intent becomes "stale" and
  is refused. The record also gives no disposition for a refused stale intent. If it is merely
  skipped, it lingers until it expires.
- *Required:*
  - Use a dedicated `disable_epoch` (or state that only disable increments `revision`).
  - A stale claim settles the intent as `failed(disabled)` and wakes its wait, as disable does. It
    is never skipped.
  - Add a poison-batch case for stale intents.

### R3-n1 (Nit, carry into P7b): give the new `content_exact` columns no DEFAULT

- *Problem:* A message or exchange writer that P7c misses (system notices, non-workflow agent
  paths) would silently write "exact" if the column has a default.
- *Required:* Declare `content_exact` on `messages` and `agent_history_exchanges` as `NOT NULL`
  with no `DEFAULT`, so a missed insert fails loudly. The runtime queries there are not checked at
  compile time, so the DB-backed tests are what catch it.

## Opinion on the pending U2 proposal

The gate was judged on the current text, where both classes are an exact 0. This is an opinion on
the proposed change only.

The proposal: `ConversationalInput` (human messages, manual and interactive input,
`decision.human` feedback, `message.ask` replies) becomes inexact, while `AuthoredConfiguration`
stays an exact 0.

**It is sound.** The fallback is the run's existing admitted ceiling: the lowest-reader level for
message runs, the actor's level for manual runs. So it adds no parallel clearance system and does
not touch `messages_human_public` or release.

- **Class membership.** `message.ask` replies and `decision.human` feedback belong in
  `ConversationalInput`: both are free text typed at runtime by a reader. The `decision.human`
  *choice* is an authored enum and can stay exact; only its feedback and data fields are
  conversational.
- **Consequence.** Every message-triggered and manual run becomes inexact at admission. Exact
  "consumed 4 saves at 4" then survives only in runs with no conversational input (schedules, and
  their children).
- **If it is adopted, the record must:**
  1. State that message stamps and release are unchanged; only memory accounting changes.
  2. Have the N-m4 bit for human messages follow `human_input_exactness`, preferably derived from
     the message role at read time rather than stored.
  3. Rebase the E11 `memory_access_level_` "consumed 4 saves at 4" cases on schedule runs, and add
     a case where a message run with readers 6/8/9 saves at 6.
  4. Note a side effect in E8. Thread-capture windows then send `consumed: unknown`, so captured
     thread memories are labelled at the thread's lowest-reader level. That narrows the
     cross-channel exposure the user accepted for U3 to readers at that level or higher.
