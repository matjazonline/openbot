# 08G-P04 review findings (reviewer B)

Append only. Each finding keeps its history; nothing is deleted.

## 08G-P04-B1-R1-F1 (low, actionable): the doop failure's cause is misreported in the completion record

- **Context:** batch review 08G-P04-B1-R1, digest `4d473b52f9ab85307800f7f4363b2110c27152e697fade620af57069da084b2f`.
- **Classification:** record accuracy (completion record and gate disposition). No code defect.
- **Evidence:**
  - `phases/p04/docs/TASK.md:112-114` says the test "expects prepared version 7, while
    `PREPARED_AGENT_VERSION` has been 8 since `2edd0a73` (p02 TASK)".
  - `04-integration-and-final-verification.md:55` says "doop prepared version 7 vs 8".
  - The retained log says otherwise: `logs/08G-P04-final-check.txt:6180-6181` and `11639-11640` show
    `left: Number(9)`, `right: 7`.
  - `src/application/workflow/agent.rs:29` is `PREPARED_AGENT_VERSION: u8 = 9`, introduced by `358e662a`
    (agent-workflow-call), which is a commit on this branch.
  - p03's record had it right (`phases/p03/docs/TASK.md:398-399`: literal 7 against 9, AWC p06 recorded it).
- **Why it matters:** the disposition "pre-existing, not Plan08" still holds (no Plan08 commit touches the
  constant or the test), but the record names the wrong version and the wrong origin commit. A reader who
  checks the claim against the log finds a mismatch, and the completion record is the thing the delivery is
  judged on.
- **Requested change:** in both places say 7 expected against 9 actual, name `358e662a` as the bump, and point
  at p03 TASK or AWC p06 rather than p02.
- **Instruction lesson:** copy a failure's values from the current log, not from an earlier phase's record of
  the same test.
- **Resolution history:** R1 opened. R2 resolved: TASK:114 and the completion record row (04 doc:55) now say 7 expected, 9 actual, from `358e662a`; matches the log.

## 08G-P04-B1-R1-F2 (low, actionable): HANDOFF.md is stale

- **Context:** as F1.
- **Classification:** record accuracy (resume state).
- **Evidence:** `phases/p04/docs/HANDOFF.md` says "Coordination: r11"; `COORDINATION.md:29` is r12 and records
  the p04 final gate. Its "Next" still lists "Freeze, write the manifest, and get B's batch review" as step 1,
  although the freeze and manifest (`logs/08G-P04-B1-R1.evidence.json`, 23:44) exist.
- **Requested change:** bring the handoff to r12 and the current step.
- **Instruction lesson:** update the handoff in the same edit that bumps coordination.
- **Resolution history:** R1 opened. R2 resolved: HANDOFF says r12 and the next step is the recheck.

## 08G-P04-B1-R1-S1 (optional): state that the production wiring of `goal.status` is compile-only

- **Evidence:** `goal_rollup_status_tool_through_the_agent_runtime` builds its runtime in
  `workflow_tool_support.rs:413` (`.with_goal_status(...)`). The production line is `src/infra/workflow.rs:272`.
  `workflow_agent/mod.rs:279` returns an error when the dispatch is absent, so a dropped production line would
  compile and pass every test. `channel.call` has the same shape, so this is existing practice, not a
  regression.
- **Suggestion:** add one line to TASK's verification mapping saying the infra composition is not exercised.
- **Resolution history:** R1 opened, optional. R2 taken: verification mapping row at TASK:184.

## 08G-P04-B1-R1-S2 (optional): the measured EXPLAIN statement is a hand copy

- **Evidence:** `goal_rollup_cost_tests.rs` repeats the base statement of `department/goal_rollup.rs:50-60`. It
  matches today (checked line by line), but nothing ties them together. The test is ignored and a measurement,
  so the risk is only a misleading future plan.
- **Suggestion:** note it in the test's doc comment, or expose `BASE` to the test.
- **Resolution history:** R1 opened, optional. R2 declined by the author with a reason (TASK "Review 08G-P04-B1-R1"); accepted, the test is an ignored measurement.
