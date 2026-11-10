# Review p7a-arch-r2: phase 05 architecture recheck (E1–E12, round 2)

Reviewer B, generation 4 (agent `a5916c4a63f198c6f`, nonce `run=8e7c1e85 role=B gen=4`).
Date 2026-11-08. Read-only: no project file was edited and nothing was committed.

## Artifact identity

- Full diff `tmp/memory-service-run/frozen/p7a-arch-r2.diff`: sha256
  `efb52e1cec5d7d04b001ff38d25ad40556275daa4827859a05dc38c9407eb30d` (recomputed, matches).
- Incremental from round 1, `tmp/memory-service-run/frozen/p7a-r2-incremental-from-r1.diff`: sha256
  `bb5f29002d3e2e57a0f90b3c11868370c49865d1d572be2d29b172b849a0bc17` (recomputed, matches).
- Tree: recomputed with a temporary index (base `a6bed48a`, `tmp/` excluded):
  `aaed319c4be2431184d482adb54b6f765c3c56c4`. It matches the worktree.
- `git diff --no-index --check` on the record is clean.

## Scope

The user decisions U1–U3 are fixed and were not reopened. That includes thread capture into the
agent scope, which the user chose over round 1's M6 recommendation. For that choice, this round
judges only whether its gating and documentation are adequate.

## Coverage

I read the whole rewritten section (E1–E12, record lines 1159–1537) and checked it against
OpenBot code:

- **Output and tool-result commit paths:** `batch_commit.rs` (`complete`, the single
  `committed_output` writer) and every caller of it (`completion.rs`, `wait_commit.rs`,
  `decisions.rs`, `channel_reply_resume.rs`, `recovery.rs`, `batch.rs`). I also traced
  `checkpoint_updates.rs` (`InvocationResult`) and `harness/rig/workflow.rs` (where tool results
  are committed after a park), and `wait_commit.rs` (`continue_delivery`).
- **Step kinds and tool targets:** `registry::TYPES` (21 kinds) and the `AgentToolTarget` variants.
- **Lock order:** `action_authority.rs` (`prelock_accounts_on`: users, then company
  `FOR NO KEY UPDATE`, then the member row) and `checkpoints.rs` (`authorize_on`, whose comment
  says account and company come before agent rows). The owners: `agent_preparation.rs`
  (`current_on`), `agent.rs` (delete), `company_invite.rs` (`remove_member`) and
  `user_lifecycle.rs` (`delete_user_account`).
- **Service:** tenant state handling (`change_tenant_once`, where deleting a tombstone is a no-op
  200) and the `TenantView` schema, which carries `generation_daily_token_cap`.

## Round-1 findings

| r1 | Status | Evidence |
| --- | --- | --- |
| M1 | Resolved | E2 always sends delete after create; only a 200 completes; a 404 is `tenant_unmanageable`. The service returns a no-op 200 for its own tombstone (`change_tenant_once`, `(Delete, Deleted)`). The provisioner id is recorded and a mismatch refuses work (but see N-n1). |
| M2 | Resolved | User scope only when the root run is bound to an unattached `private_session` created by the actor (`guard_workflow_agent_session` guarantees a root run and creator = actor). Children inherit; message runs, schedules, manual runs outside a session and their children are never eligible; steps have no user scope; tool save defaults to the agent scope. |
| M3 | Resolved in substance; placement is incomplete (N-m1) | The allowlist plus the lowering list covers all 21 `TYPES` and every `AgentToolTarget`, and a coverage test is specified. |
| M4 | Resolved | Child admission copies `content_exact` (workflow-tool children included), and an inexact child lowers its parent. |
| M5 | Resolved | Explicit `memory` arm, reachable only from the memory target, with both tests. Read queries are stated not to be release-checked egress. |
| M6 | Adequate given the user's choice | Per-agent `threads` switch, off by default, set by an owner or admin, with settings text naming the cross-channel consequence (E8). The optional audit addition is N-n3. |
| M7 | Resolved in substance; lock wording and placement need fixing (N-m2) | Company `FOR UPDATE` first, upserted `absent` rows, present → absent when the owner is gone, three competing tests. |
| m1 | Resolved | E6 states the real order (prelock, `lock_fence`/`lock_scope` run row `FOR UPDATE`, execution, job), re-reads from the locked row, adds no extra lock, and adds mutations A and B. |
| m2 | Resolved | Worker writes the receipt only; save takes the namespace `FOR SHARE` and requires `ready`; removal and cancellation never take namespace → run; disable fails pending intents. The same-transaction tenant check is still missing (N-M1). |
| m3 | Resolved | Tool save has a scope, key `ob.t.<invocation>`, and at most 64 deterministic `external_sources`. |
| m4 | Resolved | No `session_id` in the first release; deadlines unified (save 6 days, generation at most 168 h). |
| m5 | Resolved | One U2 rule for all human-authored input. |
| m6 | Resolved with a sizing defect (N-m3) | Separate pools, per-company caps of 2 interactive and 1 background, and a 2 s recall deadline. |
| m7 | Resolved | Validation once per resumed attempt; a ceiling change discards the pack as `unavailable{ceiling_changed}`. |
| m8 | Resolved | `memory.not_enabled` at publication and at runtime; a startup warning lists pending lifecycles when the URL is unset. |
| m9 | Resolved | Private-cluster prepare and test URLs; tables go into the init schema in place. |
| n1–n6 | Resolved | Full paths, defense-in-depth wording, conservative `exposed_level` stated, generation-status client dropped, header updated, retire is the same final delete (`agent.rs`: deferred FKs refuse deleting an agent that has run history). |

## New findings (one pass)

### Major

**N-M1. Disable does not stop capture or saves enqueued after the disable, so re-enabling sends
them.**

- *Location:* E2 (disable semantics); E6 (save transaction); E8 (capture enqueue in `accepted_on`,
  and "claims only for an enabled tenant").
- *Problem:* Disable fails the intents that are pending *at that moment*. But neither the save
  freeze nor the capture enqueue is specified to check `memory_tenants` enabled under a lock in the
  same transaction. The only gate named is the worker's claim-time "enabled tenant" check. So:
  - An exchange that succeeds after disable still enqueues a generation intent.
  - So does a save that raced the disable.
  - Either intent sits pending (up to 168 h for generation, 6 days for saves) and is delivered as
    soon as an owner re-enables. That would send the window to the service and, with a cap above
    0, to Anthropic.

  This contradicts the fixed U1 semantics ("Disable stops new … saves and capture … enable revives
  nothing"). It also leaves the save's effect wait hanging until expiry.
- *Required:*
  - Every intent-creating transaction (save step, tool save, capture enqueue) takes `memory_tenants`
    `FOR SHARE` after the run row and before the namespace, and requires `enabled` and the matching
    per-agent switch. Otherwise the save fails with a terminal `memory.not_enabled` and capture is
    skipped.
  - Disable takes `memory_tenants` `FOR UPDATE` and then fails the pending intents, as E9 says. The
    two then serialize: a racing save or capture either commits first (and disable fails it) or
    waits and sees the tenant disabled.
  - The claim also refuses intents created before the latest disable. Store the tenant revision on
    each intent and compare it at claim, as a backstop.
  - Add a `memory_integration_` competing test: disable against capture enqueue and against a save
    freeze, then re-enable; nothing is delivered.

### Minor

**N-m1 (M3 placement). Name the funnels where the exactness hooks go.**

- *Problem:* Hook (1) is named at `complete_io` and the wait-resume completions. But every step
  output is written by one statement in `batch_commit::complete` (`batch_commit.rs:51`), and that
  function is also called by `decisions.rs:579`, `channel_reply_resume.rs:96`, `recovery.rs:312`
  and `batch.rs:183`. Every kind those callers commit today is allowlisted, or writes a `null`
  output on `final_error`, so nothing leaks now. A future kind committed through one of them would
  bypass a hook placed only at the two named sites, which defeats "fail closed".
- *Required:*
  - Place hook (1) inside `batch_commit::complete`, keyed by the execution's step kind.
  - Place hook (2) at `checkpoint_updates.rs` `update_on` for `WorkflowCheckpointUpdate::InvocationResult`.
    That is the single funnel for direct calls, approved or parked continuations (re-dispatch after
    `continue_delivery`), and refusals.
  - Have the coverage test assert those two placements.

**N-m2 (M7 lock wording and placement). Follow the existing lock order.**

- *Problem:*
  - "Member removal and account deletion lock the membership row `FOR UPDATE` first" contradicts
    the established order. That order is users → company → membership/principal → agent rows → run
    (`action_authority::prelock_accounts_on`, `remove_member` comment, `delete_user_account`,
    `checkpoints.rs` `authorize_on`). Locking the membership row before users or company would
    invert against workflow prelocks.
  - "Lazy provisioning locks the same owner rows `FOR SHARE`" is unplaced. Inside a fenced run
    transaction, an agent row `FOR SHARE` taken after the run row inverts against agent delete.
    Agent delete holds agent `FOR UPDATE` and then runs commit-time FK checks on the referencing
    rows.
- *Required:*
  - Say "membership row `FOR UPDATE` after the existing users → company prelocks".
  - Run lazy namespace provisioning in its own short transaction, in the order users → company →
    member or agent `FOR SHARE` → namespace and lifecycle upsert. It must never run inside the
    fenced run transaction; the run continues with `not_ready`.

**N-m3 (m6 sizing). The per-company interactive cap is below one recall's demand.**

- *Problem:* A recall runs up to three `context.pack` calls concurrently (company, agent, user), but
  the per-company interactive cap is 2. One recall therefore waits on itself, and two concurrent
  agent runs in a company push most recalls past the 2 s deadline. They freeze `unavailable`, and
  that is permanent for those preparations.
- *Required:* Either count a recall as one permit (its scopes share it) or set the per-company cap
  to at least 3. Add a `memory_preparation_` case with two concurrent recalls in one company that
  both succeed.

**N-m4. Exactness does not cross runs through stored history.**

- *Location:* E3 audit, `context.history` and history freeze, which are allowlisted as raising
  reads.
- *Problem:* An agent message or exchange stamped by an *inexact* run carries only `access_level`.
  A later run that reads it through `context.history` or the history freeze raises by that stamp
  and stays exact. The earlier run's fallback is bypassed by one hop through the thread.
  - This matches OpenBot's existing labels: the message is already visible at its stamp.
  - But 05 lists "frozen history" as a path to account for.
- *Required:* Choose one and record it:
  - (a) carry an `exact` bit on agent-authored messages and exchanges, and lower exactness when a
    selected item has it false; or
  - (b) state explicitly that stored OpenBot labels are authoritative and that exactness does not
    cross runs, as a documented limitation beside the U2 declassification note.

  (a) is the stricter reading of 05.

**N-m5. "The provisioner cannot be rotated yet" is acceptable for this release only with an
incident path recorded now.**

- *Problem:* Rotation is blocked because `managed_by` is the credential id. The provisioner also
  holds transitive full data authority (D3), so a leaked token is the worst-case secret. The record
  defers everything to 06A without saying what happens on compromise.
- *Required:* Add to E1 the procedure:
  - An operator revokes the provisioner credential.
  - Client credentials keep serving.
  - The lifecycle worker's refusals show up in settings (`tenant_unmanageable` and the
    credential-id mismatch).
  - The operator, whose scope can manage every tenant, completes pending deletions by hand.
  - 06A must add reassignment before the first production deploy.

  With that, I accept the deferral.

### Nits

- **N-n1.** Keep `provisioner_credential_id` on the `memory_lifecycles` row as well. `memory_tenants`
  cascades away on company delete, so the mismatch check is vacuous exactly on the delete path.
  M1's delete-200 rule still makes that path safe.
- **N-n2.** Say where OpenBot reads the operator cap: `TenantView.generation_daily_token_cap` from
  the last create, cached on `memory_tenants`. A stale value only produces a terminal
  `generation_disabled`. Settings should show "generation off (operator cap 0)".
- **N-n3.** Record who changed each `memory_agent_capture` switch and when (typed actor and reason,
  per persistence AGENTS). Show the cross-channel text at the moment `threads` is enabled, not only
  on the settings page. This is adequate gating for the user's choice; the audit makes it
  accountable.
- **N-n4.** State that `child_branch` conversations and private sessions attached to a channel are
  never captured. They fall outside both switches implicitly today.
- **N-n5.** Record line 8 (the updated header) is over 100 characters.

## Interplay checks requested by root

- **Fail-closed hooks versus approval, continuation and inherited child results.**
  - Approved or parked agent tool calls re-dispatch after `continue_delivery` and commit through
    `InvocationResult`, so hook (2) at `update_on` covers them.
  - Child and workflow-tool results are propagated explicitly (M4).
  - `channel_reply_resume` commits an agent.run output (allowlisted), and recovery commits a `null`.
  - Covered once the hooks sit at the funnels named in N-m1.
- **Owner locks versus existing deletion lock orders.**
  - Company `FOR UPDATE` first is compatible. Workflow transactions hold company
    `FOR NO KEY UPDATE` before run rows, so delete waits for them and no cycle forms.
  - Agent delete (agent `FOR UPDATE` first) is existing behaviour and stays compatible if lazy
    provisioning moves out of the run transaction (N-m2).
  - Member removal and account deletion are compatible once "first" means "after users → company"
    (N-m2).
- **Per-agent capture policy versus per-company opt-in, daily cap and disable.**
  - The per-agent capture policy, the per-company opt-in and the operator cap all gate capture
    (N-n2 covers where the cap is read).
  - Disable is not enforced at enqueue (N-M1). Fix that and the three gates compose correctly.
  - Enable revives nothing, provided N-M1's revision backstop is added.
- **Provisioner not rotatable.** Acceptable for this pre-release, with N-m5's incident path.

## Verdict

**Changes required: one Major (N-M1) and five Minors.** Every round-1 finding is resolved or
superseded by the user's decisions, and the thread-capture gating is adequate for the choice the
user made. N-M1 conflicts with the fixed U1 disable semantics and must be fixed before P7b. The
Minors are text-level fixes that can go in the same correction round. I expect to pass the next
round on a text recheck of N-M1 and N-m1 to N-m5.
