# AWC-P06 handoff

- Phase `p06` of `plan/agent-workflow-call`, workflow implement-opus-reviewed-2_4. Records are in this directory.
- Root/A: Claude Code `1e84a7af`, Opus 5.5/xhigh.
- B: a Fable/medium reviewer per U9 (2026-11-23, "continue and use fable for review"), agent `a34d3fab3f32abacc`
  (spawned 2026-11-23 for AWC-P06-A-R1). The exact model it reports goes in REVIEW.md. B' `a4f44ca532412935f` is superseded for p06.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, base/HEAD `fe7791d3`. Nothing is committed.
- Starting digest `5d818a95…ef70`, with exclusions `plan/agent-workflow-call/phases/p06/docs`, `.trail/learnings`.

## State (2026-11-23)

- p01–p05 accepted in phase. COORDINATION r25 (U9).
- Batch A (tests G1–G8, all in TASK.md "Batch A progress") is implemented and frozen for review AWC-P06-A-R1:
  - digest `d3bee2a425e9d3508223741ca231608e8c9336715dbece7ca497f5d061c8576f` (includes COORDINATION r25);
  - manifest `logs/AWC-P06-A-R1.evidence.json`, validated with 3 PASS: 119 `channel_call_` tests, the stack
    budget, clippy 1.95 and fmt.
- No production code changed in p06.

- AWC-P06-A-R1: changes required (Fable 5.2 `claude-fable-5-2`; F1–F3 actionable, F4 optional). Corrected in one
  round (TASK.md "AWC-P06-A-R1 corrections"). Refrozen for recheck AWC-P06-A-R2 at
  `c7fba3400cd27117204379ea11848105f5d96846018aa35d3b27a3bef9d97431`; manifest `logs/AWC-P06-A-R2.evidence.json`
  is valid (3 PASS, 4 NOT EXERCISED dispositions).

## Next

1. p06 accepted in phase (2026-11-24): A at `c7fba340…7431`; B at `bc8b9841…764f`, Fable PASS AWC-P06-B-R2-N1.
   - Records-only edits came after the review (doc 06 completion record, COORDINATION r26). The post-record digest
     is in TASK.md.
   - The plan is not declared complete. The unavailable gates and the crash-test gap are listed in doc 06.
2. Return to Plan08 p03 after settling Plan08 README proposals 4 and 5 with the user.

## Settled with the user (2026-11-24)

- Committed as `358e662a`, with the learnings in `c7fbbbbe`.
- Scratch databases `openbot_awc_mac02` and `openbot_awc_mac02_test` were dropped.
- Plan08 README proposals 4 and 5 were confirmed (Plan08 COORDINATION r7).
