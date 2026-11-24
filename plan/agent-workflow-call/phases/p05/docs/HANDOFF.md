# AWC-P05 handoff

- Phase `p05` of `plan/agent-workflow-call`, workflow implement-opus-reviewed-2_4. Records are in this directory.
- Root/A: Claude Code `1e84a7af`, Opus 5.5/xhigh. B: Opus substitute B' `a4f44ca532412935f` (Opus 5.5/medium) for all
  of p05 by user decision U7. It is continued via SendMessage. The Fable B `a8d0fa1fbd0b8b88e` is not used in p05.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, base/HEAD `fe7791d3`. Nothing
  is committed; p01–p04 are accepted in phase and uncommitted.
- Coordination `plan/agent-workflow-call/COORDINATION.md` r22 (U6–U8; AWC-C002 r1 and AWC-C005 r1 acknowledged).
- Snapshot exclusions: `plan/agent-workflow-call/phases/p05/docs`, `.trail/learnings`.
- Scratch DBs `openbot_awc_mac02` / `_test`. Reset them after any init-schema edit. Prepared spec is v9, and the DB
  pins it in two places.

## State

- Gate AWC-P05-D1 r2: PASSED (with F8–F10).
- **B1 (doc 05 A plus the p04 hand-overs): accepted in phase** at
  `22fdcacd89aa94c2b7d60e432a336910df017210e970d3eb0ba0abd471ecba13` after R1 (F1–F4) and R2 PASS. The mapping,
  deviations and checks are in TASK.md "B1 implementation".
- Gate AWC-P05-D2 r2: PASSED (with F8, F9).
- **B2 (doc 05 B: inspection, samples, signals): accepted in phase** at
  `905b73f8f75ac938e26cb397b96e61108afd5478c951deb7b393c8d8f23c8d5a` after R1 (F1 fixed, F2 declined) and R2 PASS.
  The mapping, deviations (8), corrections and checks are in TASK.md "B2 implementation".
- **p05 is complete in phase.**
- Known unrelated failure: `doop_agent_current_preparation_reaches_ready_before_model_io` (asserts version 7).

## Next

1. p06: the final full gate, which fills in the plan docs' "Implementation evidence" lines.

## Pending with the user

- Commit authorization (none given).
- The trail learning commit question ("y/n") is unanswered.
- Plan08 README proposals 4 and 5 must be settled before Plan08 p03.
- Drop the scratch DBs at plan completion.
