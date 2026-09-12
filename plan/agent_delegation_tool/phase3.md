# Phase 3 — Make it reachable: a timeout that asks the agent first

Read [`general_plan_instructions.md`](general_plan_instructions.md) first, especially "The blocker".
This phase is what turns phases 1-2 from grantable-but-unreachable into working behaviour, and it is
the only phase that changes what the platform *does* on its own.

**Goal.** When an outreach passes its deadline and the task's owning agent created it and holds the
`adjust_delegated_outreach` grant, the agent gets one chance to decide — extend, drop a silent
internal target, reassign it — before a human is asked. If it does nothing useful, or the bound is
spent, the existing human `quorum_timeout` approval is raised exactly as today.

## What happens now, and the one line that changes

`TaskWorker`'s outreach sweep calls `request_quorum_timeout_decision`
(`src/application/services/task_worker.rs:798-910`). It claims the timeout first —
`mark_outreach_timeout_pending` (L828-L840), whose boolean answer is the "only the worker that
flipped the state may raise the approval" lock — then builds an `ApprovalSubject` with
`TaskSuspension::AlreadySuspended` and mails the channel's approver. If the approval cannot be sent
it calls `restore_outreach_waiting` (L898-L908) so the outreach is not stranded.

The claim stays. What changes is what the claiming worker does with it: ask the agent, or ask the
human.

`list_due_outreaches` (`src/adapters/persistence/task/operations.rs:700-731`) selects only
`outreach.status = 'waiting' AND task.status = 'waiting_for_third_party_reply'`, so the moment the
claim moves the outreach to `timeout_pending_approval` the sweep cannot pick it up again. That is
already the re-entry guard this phase needs; do not add a second one.

## The decision to settle before writing code: how the agent gets a turn

`apply_operation` accepts these operations while the outreach is `waiting` **or**
`timeout_pending_approval` (`controls.rs:495-502, 548-555, 627-634`), and requires only that the task
is not terminal. So the claimed-timeout state is already an actionable one. What is missing is an
agent run. Two shapes, both possible without touching `controls.rs`:

### Option A — wake the parked task, then re-park it (recommended)

Set the parked task to `pending` (`run_at = now`) so an ordinary claim runs its owning agent, with
an instruction note saying the deadline passed and what the current progress is.

- **In practice:** one task per delegation, which is what every existing read assumes. The activity
  badge, the collaboration summary, `list_thread_work_summary`'s one-row-per-thread projection and
  the Tasks board all keep showing a single piece of work.
- **Under load:** one extra claim per timed-out outreach, on the existing queue with the existing
  per-company fairness. Nothing new competes for a worker slot.
- **Caveat, and the work this option costs:** the run ends with the task `processing` and its
  outreach `waiting` again (if the agent extended) — nothing re-parks it. `apply_operation`'s extend
  arm only updates `background_tasks` `WHERE status IN ('waiting_for_third_party_reply',
  'pending_approval')` (L527-L529), so while the task is `processing` its `wait_expires_at` is
  deliberately left alone. This phase therefore adds one persistence operation,
  `repark_awaiting_outreach(lease, outreach_id)`, which moves the task back to
  `waiting_for_third_party_reply` with `wait_expires_at` read from the outreach row, fenced on the
  same lease predicate `create_outreach_and_pause` uses (`operations.rs:572-607`) — same worker, same
  execution generation, same ownership version, live lease. The dispatcher calls it when a run
  finishes on a task whose `awaited_outreach_id` is still set and whose outreach is `waiting`.
- **Second caveat:** if the agent extends and the run then *fails*, the task must not be left
  `processing` forever. The existing lease reaper (`reap_expired_task_leases`) returns it to
  `pending`, so it would be re-run rather than re-parked — acceptable, and the same as any failed
  run, but assert it in a test rather than assuming it.

### Option B — enqueue a separate review task

Enqueue a short-lived task in the same company, channel and thread, owned by the same agent
principal, whose only job is to decide. The delegation command still names the **parked** task
(`command.task_id`), which `authorize` checks the ownership of — so this works only because both
tasks are owned by the same agent principal, and the tool's context would need to carry the parked
task id separately from its own lease.

- **In practice:** the parked task never loses its suspension, so no re-park operation and no
  `wait_expires_at` gap. Extend updates the parked row directly, because it really is still
  `waiting_for_third_party_reply`.
- **Under load:** two live tasks on one thread. `list_thread_work_summary`'s `DISTINCT ON
  (thread_id)` would surface whichever the ordering picks, so the mailbox badge and the Tasks board
  can show the review task instead of the wait. That is a visible regression in an existing read for
  the duration of the review.
- **Caveat:** a second task on the thread is a new invariant for `collaboration.rs`'s child-task
  joins to reason about, and the phase-1 `own_delegation_controls` lookup becomes "controls for a
  task other than the one I am running on", which is a strictly wider contract.

**Recommendation: A.** The re-park operation is a contained, fenced addition; Option B's cost is a
new two-tasks-per-thread state that several existing reads quietly assume away. Settle this before
starting — the two options diverge in the first file either touches.

## Sketch, assuming Option A

**Files touched**

| File | Change |
|---|---|
| `src/application/services/task_worker.rs` | route a claimed timeout to the agent or the human |
| `src/application/task_queue.rs` | `repark_awaiting_outreach`; `DueOutreach` gains the owner |
| `src/adapters/persistence/task/operations.rs` | its query, and the owner column on the sweep |
| `src/application/use_cases/thread/dispatch.rs` | re-park a finished run that still awaits an outreach |
| `src/application/services/agent_runner/mod.rs` | nothing, if phase 1 wired the tool unconditionally |

### 3.1 Who may be asked

Three conditions, all read server-side, none of them new authorization:

1. `task.owner_principal_kind = 'agent'` and the outreach's `created_by_principal_id` equals it.
   This is `authorize`'s `is_creator` in different words, and asking an agent that would then be
   refused is the failure mode to avoid. `DueOutreach` does not carry the owner today — add it to
   the sweep's projection (`operations.rs:719-731`) rather than issuing a second read per row.
2. The owning agent holds the grant. Load it with
   `AgentCapabilityReader::load_for_execution` (`src/application/use_cases/skill.rs:160-167`) and
   test the **compiled** grant list the way `dispatch.rs:486` and `:943` already do. Do not re-parse
   `agent.config_json` here: "may this agent use this tool" must have one implementation, or an agent
   could be woken for a tool the runner will then drop from its grants.
3. The bound is not spent. Count this outreach's prior `owning_agent` commands in
   `delegation_control_commands` and stop at **two** agent reviews, after which the timeout goes
   straight to the human. Without a bound an agent can extend a deadline indefinitely and the human
   who would have noticed never hears about it — `src/AGENTS.md`'s "bound work at every external
   boundary", where the boundary is the model's judgement.

Any of the three failing means the current code path, unchanged.

### 3.2 Asking

After a successful `mark_outreach_timeout_pending`, instead of building the `ApprovalSubject`:

- Write the context the agent needs to decide as an instruction the next run consumes —
  `claim_agent_instruction_notes` (`src/application/task_queue.rs:466-471`) is the existing
  mechanism, and `ask_owner_to_act` (`instructions.rs:257`) is the existing writer. Say the deadline
  passed, name the responded and silent targets, and say what the agent may do about it. The wording
  must not imply it can cancel the request or stop the task; it cannot, and a tool failure is a poor
  way to find that out.
- Move the task to `pending` with `run_at = CURRENT_TIMESTAMP`, attributed with a new
  `TaskTransitionReason` (`OutreachTimeoutReview` or similar) so the status ledger says why it woke.
  Fence the update on `status = 'waiting_for_third_party_reply' AND awaited_outreach_id = $outreach`,
  the same shape as `wake_task` (`controls.rs:300-333`) — and, like `wake_task`, do **not** clear
  `awaited_outreach_id`: that is what tells the dispatcher to re-park.
- Leave the outreach in `timeout_pending_approval`. It is an accepted state for all three
  operations, and it keeps the sweep out.

If waking fails, `restore_outreach_waiting` already exists for exactly this and the next sweep
retries. Propagate the error rather than swallowing it — `src/AGENTS.md` on collapsing errors into
defaults.

### 3.3 Escalating anyway

The agent may do nothing, or extend and then still get no answer. Both land back in the sweep:
extending sets the outreach to `waiting` with a new `expires_at`, so the next deadline is picked up
normally, and the bound in §3.1 sends the second or third timeout to the human. An agent that does
nothing must not leave the outreach in `timeout_pending_approval` forever — the run finishing with
the outreach still in that state is the signal to raise the human approval, which the re-park step
in §3.4 is the right place to notice.

### 3.4 Re-parking

Add `repark_awaiting_outreach(lease, outreach_id) -> AppResult<bool>` to `TaskPersistence`, fenced on
the full lease predicate, setting `status = 'waiting_for_third_party_reply'` and
`wait_expires_at = outreach.expires_at`, and refusing (`false`) if the outreach is no longer
`waiting`. The dispatcher calls it where a run ends, before completing the task, when
`awaited_outreach_id` is set. `false` means the outreach settled during the run — then the ordinary
completion path applies, and if the outreach is still `timeout_pending_approval`, raise the human
approval (§3.3).

---

## Tests

Read `src/adapters/persistence/AGENTS.md` and the existing quorum-timeout tests first:
`a_rejected_quorum_timeout_is_recorded_as_the_approval_that_rejected_it`
(`src/adapters/persistence/approval.rs:1058-1237`) is the closest fixture.

**Selection, pure where possible:**

- An outreach whose creator is not the task owner takes the human path.
- An owning agent without the grant takes the human path.
- The third timeout on one outreach takes the human path even with the grant. Count the prior
  commands, scoped to the outreach the test created.

**Behavioural, against the database:**

- A claimed timeout with all three conditions met leaves the outreach `timeout_pending_approval`,
  the task `pending` with `awaited_outreach_id` intact, an instruction row written, and **no**
  `human_approvals` row for that task.
- The agent extends from that run: the outreach is `waiting` with the new `expires_at`, and after
  `repark_awaiting_outreach` the task is `waiting_for_third_party_reply` with `wait_expires_at`
  equal to the outreach's. The task must not be left `processing`.
- `repark_awaiting_outreach` from a different worker id, a stale execution generation, or a stale
  ownership version writes nothing and returns `false`.
- A run that ends with the outreach still `timeout_pending_approval` raises the human approval, and
  raises exactly one.
- Two workers sweeping the same due outreach: only one `mark_outreach_timeout_pending` wins, so only
  one of them wakes the task. This is the existing race, re-asserted because the losing branch now
  does something different.
- The existing quorum-timeout tests pass **unchanged** for an agent without the grant. If any of
  them needed editing, the selection in §3.1 is wrong.

## Done when

- [ ] Option A or B chosen, written down, and the unchosen one's caveats recorded as the reason.
- [ ] A timed-out outreach reaches its owning agent at most twice, then a human, always.
- [ ] No path leaves a task `processing` with an awaited outreach, and none leaves an outreach in
      `timeout_pending_approval` with nobody asked.
- [ ] `controls.rs` is still untouched by all three phases.
- [ ] `cargo test` and `cargo clippy --all-targets -- -D warnings` are green.
