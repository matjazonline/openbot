# 03 Goal units and tracking

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A unit can be marked as a goal. Every run done on a goal's behalf, in any department, records the
goal, and one page shows what is open, who it waits on and what it has cost. Starts after
[02](02-unit-lifecycle.md) and after `channel.call` from
[agent-workflow-call](../agent-workflow-call/README.md) is implemented: a `message.send` to another
desk starts no run there (`docs/inter_channel_agent_communication.md`), so without the call a goal
cannot start work in a department. Continues in
[04](04-integration-and-final-verification.md).

## Batch 03A Goal units

1. `departments` (`init_schema.sql:15492`) gains `kind text NOT NULL DEFAULT 'standing'` with a
   check for `standing` or `goal`, and `target_date date`, allowed only on a goal. The root is
   standing. `DepartmentKind` sits beside `DepartmentRole` in
   `src/domain/entities/department.rs`.
2. The definition of done is the existing `charter`. A goal unit is otherwise a department: it
   gets its desk, members, roles, memory, resources and budgets by the same paths.
3. Kind is set at creation and may be changed while the unit is active by whoever manages its
   parent. Changing it rewrites no history; item 03B.1 is why.
4. Every path that creates a department takes the kind and, for a goal, an optional target date
   and sponsors, assigned in the creation transaction through `assign_department_role_on`
   (`src/adapters/persistence/department.rs:528`).
5. The sample list gains `src/application/workflow/fixtures/goal-plan-approve-dispatch.yaml`:
   `agent.run` with `role: lead` to plan → `decision.human` with the `sponsor` role as reviewer →
   `agent.run` with `role: lead` and the `channel.call` capability to dispatch → `reply`. It is
   published once and bound on each goal's desk.

## Batch 03B The trail

1. `workflow_runs` (`init_schema.sql:5887`) and `threads` (`:4566`) gain `goal_unit_id uuid`, with
   a composite foreign key to `departments` and `ON DELETE RESTRICT`. On a run it is immutable
   after insert; on a thread it is set at creation only. A goal with recorded work therefore
   cannot be deleted, only archived, and a later change of a desk's unit or a unit's kind does not
   move past work.
2. One pure function `goal_of(parent_goal, thread_goal, desk_chain)` in
   `src/application/workflow/goals.rs` returns the first that applies:
   - the parent run's goal, for a child run;
   - the thread's goal, for a run on an existing thread;
   - the nearest goal unit in the desk's chain, the desk's own unit first;
   - none.

   A parent's goal wins over the destination desk's, because the trail answers "on whose behalf".
   A candidate that is archived counts as none, so a late reply on an old thread is admitted by its
   department and is not added to a closed goal.
3. Admission (`create`, `src/adapters/persistence/workflow/admission_write.rs:7`) computes it with
   the department chain already loaded for role resolution and scopes. `channel.call` writes the
   calling run's goal on the fresh destination thread it allocates. A rerun takes the goal of the
   run it reruns.
4. The model cannot set or change it. `channel.call` gains no parameter; the internal relay trace
   (`EmailRelayTrace`, `src/application/transport/delivery.rs:495`) does not carry a goal, and no
   mail header is read for one.
5. Archiving a goal counts open runs recorded against it, wherever they run, in the open-work
   refusal of [02](02-unit-lifecycle.md), batch 02B item 1.

## Batch 03C A goal's budget

1. `unit_budgets` from 07 accepts the period `total` on a goal unit only. `model_usage_calls`
   (`init_schema.sql:11333`) gains `goal_unit_id`, written from the run's goal at call time, beside
   the `unit_id` 07 adds.
2. After the chain check from 07, admission of a run with a goal sums that goal's usage for each of
   its caps and refuses with `budget.goal_exhausted` naming the goal. The work also stays under the
   caps of the unit that does it. The read is bounded and under the company lock, as in 07; a run
   already admitted is not stopped.
3. A refused child returns to its caller as a `denied` closure with that reason.

Batch 03C applies only if proposal 4 of the [README](README.md) is confirmed. Without it a cap on
a goal unit bounds the runs on the goal's own desks, as 07 defines for any unit.

## Batch 03D Status and analysis

1. One read, `goal_rollup(company, goal)`, in the department persistence:
   - runs by state and by the unit that did the work (the write unit in `workflow_run_scopes`);
   - decisions waiting on a person, with their age (`workflow_decisions`, `init_schema.sql:5266`);
   - open escalations (`workflow_run_escalations`);
   - calls made and their closures: `done`, `denied`, `handed_off`;
   - spend in total, against the cap, and by the unit that did the work;
   - time from first run to last finish, time spent waiting on people, and last activity.

   It is one bounded statement set over the goal index on runs; it has a row limit and says when it
   was truncated.
2. A goals page lists active goals with the lead, the decider, the target date, the open, waiting
   and failed counts and spend against the cap; archived goals sit behind a toggle. A goal's page
   shows the rollup with a table by department and the waiting list. The run board (`RunsView`,
   `src/adapters/http/pages/workflow_runs/board.rs:24`) gains a goal filter beside the department
   filter.
3. The JSON API returns the same rollup for a goal, so analysis outside the product reads what the
   page reads.
4. A read-only built-in, `goal.status`, returns the rollup to a run that carries the goal and
   whose agent holds `lead` or `sponsor` on it. The lead needs it to drive the goal: the closure of
   a call says how that call ended, not what is still open. It takes no arguments, so a run cannot
   ask about another goal.
5. Who sees it: whoever may view the goal's desk, and whoever manages the goal. For work done in
   other departments the rollup gives counts, states, closures and spend, never content. Each run
   link is authorized as it is today, so a goal member who cannot view a Sales desk sees that
   Sales has two runs open and cannot open them. A stranger gets `NotFound`.

## Focused checks

- `goal_unit_`: kind and target date round-trip; a target date on a standing unit is refused; the
  root cannot be a goal; a kind change leaves recorded goals as they were.
- `goal_of_`: unit coverage of the pure function for every branch, including parent over desk,
  thread over desk, nested goals, an archived candidate and the depth bound.
- `goal_trail_`: with the scripted-model `Deployment` harness, a run on a goal desk calls a
  department desk and the child and its thread carry the goal; a later reply on that thread
  carries it; the same reply after the goal is archived carries none and is admitted; a rerun
  keeps it; a run on an unrelated desk has none; a goal with recorded runs refuses deletion.
- `goal_budget_`: a `total` cap refuses a child in another department and names the goal; the
  department's own cap still applies; `total` on a standing unit is refused.
- `goal_rollup_`: counts, waits and spend match the rows for a goal with work in three
  departments; truncation is reported; `goal.status` returns the same numbers and refuses a run
  with no goal.
- `goal_authorization_`: a goal member sees counts for a desk they cannot view and gets `NotFound`
  on its run; a stranger gets `NotFound` on the goal; a manager of the parent sees it.
- `goal_competing_`: two admissions racing the last of a goal cap admit at most what it allows; an
  archive racing a child admission leaves either a refused archive or a refused child.
- Rendering tests for the goals page, the goal page and the board filter.
