# 08G-P04 review notes (reviewer B)

## 08G-P04-B1-R1-N1 (2026-11-24)

- **Identity:** reviewer B, model `claude-fable-5-2`, effort medium as assigned by root (not observable).
  Author A is root `1e84a7af`.
- **Artifact:** branch `plan08_ph1`, base `f1eb82dc7822b90f71415bd3800d00f0e570ee0f`, digest
  `4d473b52f9ab85307800f7f4363b2110c27152e697fade620af57069da084b2f`, exclusions `plan/08-goals/phases/p04/docs`
  and `.trail/learnings`. Confirmed before the review and again before returning. Coordination r12.
- **Outcome:** changes required. Two low, record-only findings (F1, F2) and two optional suggestions (S1, S2).
  No code or gate-disposition defect found.

### Coverage and evidence

- **Manifest:** `logs/08G-P04-B1-R1.evidence.json` validated with `check-evidence.py --expect-artifact`
  (5 PASS, 3 FAIL, 2 NOT EXERCISED).
- **Docs (04A), by inspection against code:** the departments route, the `## Channel calls` and `## Goals`
  anchors, both built-ins as opt-in in the catalog, no goal filter on the JSON run listing, and the builder
  tools' `department` slug and `lead` role all match. Not checked by me: that the `provisioning.rs` slug
  lookup has no kind filter.
- **CHECK fix, by inspection:** `agent_tool_bindings` now admits `goal_status`, reserves `goal.status` and maps
  one to the other. Swept by the sibling literals `'channel_call'`, `memory_details` and `"goal_status"` in the
  schema and `src`: no other stored copy of the built-in list.
- **Runtime test, by inspection plus the author's log:** it binds through `AgentToolBinding::builtin` and
  `bind_child_with`, runs the real `workflow_agent` runtime with scripted model turns, and asserts the offered
  tool and the tool message's goal, name, `runs_counted` and `running`. Decisive for the dispatch. It does not
  cover `src/infra/workflow.rs:272` (S1). In `logs/08G-P04-focused.txt`: 234 passed, 1 ignored.
- **Measurement:** read the test and `logs/08G-P04-rollup-cost.txt`. Timings and plan are as recorded; the
  measured statement matches `goal_rollup.rs:50-60` (S2). The two stated limits (no decisions, calls or
  escalations; every run belongs to the goal, so `workflow_runs_goal_idx` is not used) are accurate and are the
  right ones. I did not rerun it.
- **Gate dispositions, from `logs/08G-P04-final-check.txt` (exit 1):**
  - DB suite and stack budget: 5195 passed, 1 failed each, the same doop test, `9` against `7`. Pre-existing:
    the constant was bumped by `358e662a`, not by Plan08. The record misstates the numbers (F1).
  - Clippy: 10 errors in the gate, of which `with_channel_call` unused was p04's and is fixed. The rerun log
    has exactly 9 `redundant_field_names`, all in files with no diff against the base and last touched by merge
    `73afd5a7`.
  - Attribution browser: the same failure is in p02's record (`phases/p02/docs/TASK.md:416`).
  - Transport boundary, query statistics, memory suite and journey: the rerun logs show PASS. The attribution
    contract check still exits 1 for `jsonschema`, recorded as UNAVAILABLE.
  - Docker images (no daemon), Node pin, managed Doop, Linux network scope: UNAVAILABLE, as recorded.
- **Ordering:** `check.sh` finished at 21:37, before the post-gate edits (23:12 to 23:31: helper removed, the
  measurement and its `mod` line added). All three are test-only. The clippy rerun (23:39) and the focused run
  (23:41) postdate them, and TASK discloses this. My own `cargo fmt --check` and `git diff --check` on the
  frozen tree: both exit 0.
- **Completion record:** apart from F1 it matches the logs. "Accepted in phase and not declared complete until
  they run" is consistent with the plan's last line and names every unavailable gate.
- **Not run by me:** any DB-backed test, `check.sh`, clippy.

### Follow-up on my own earlier pass

My B2 pass (08G-P03-B2-R2-N1) missed that the `agent_tool_bindings` CHECKs excluded `goal_status`, so no agent
could be granted `goal.status`. The author found it in p04 by running the tool through the real bind path. It
is the same class as B1-F1, D2-F1 and B2-F1: a new stored discriminator whose stored copies were not swept.
At B2 I read the Rust enum and the dispatch and did not grep the schema for the nearest sibling literal.

### Process feedback

- A test that went through a mock binding hid the defect; the bind path is where a new built-in should be
  proved first.
- Failure values in a completion record should be copied from the current log (F1).

## 08G-P04-B1-R2-N1 (2026-11-24)

- **Identity:** reviewer B, model `claude-fable-5-2`, effort medium as assigned by root (not observable).
- **Artifact:** branch `plan08_ph1`, base `f1eb82dc7822b90f71415bd3800d00f0e570ee0f`, digest
  `c081c405b668c034641caeac45ada2af93e98c1a88d6874c355f2634f4c3fc47`, same exclusions. Confirmed before and after.
- **Outcome:** PASS. F1 and F2 resolved, S1 taken, S2 declined with a reason I accept.
- **Coverage and evidence:**
  - Compared `logs/08G-P04-B1.diff.txt` with `logs/08G-P04-B1-R2.diff.txt` (index lines ignored): the only
    deliverable change is the completion-record row at `04-integration-and-final-verification.md:55`. No source,
    test or schema change, so R1's coverage carries over.
  - `logs/08G-P04-B1-R2.evidence.json` validates against the new digest (5 PASS, 3 FAIL, 2 NOT EXERCISED).
  - Read the corrected TASK:114, the completion row, HANDOFF (r12, next step) and the new mapping row TASK:184.
- **Not run:** no tests; nothing executable changed.
- **Standing limits of the delivery (unchanged, recorded by the author):** the gates UNAVAILABLE here, the
  pre-existing doop, clippy and attribution-browser failures, and the compile-only production wiring.
- **Process feedback:** none new.
