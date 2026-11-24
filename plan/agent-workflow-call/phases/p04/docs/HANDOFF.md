# AWC-P04 handoff

- Phase `p04` of `plan/agent-workflow-call`, workflow implement-opus-reviewed-2_4. Records are in this directory.
- Root/A: Claude Code `1e84a7af`, Opus 5.5/xhigh. B: Fable/medium `a8d0fa1fbd0b8b88e`, continued via SendMessage.
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, HEAD/base `fe7791d3`.
  Nothing is committed; p01–p03 are accepted in phase and uncommitted.
- Coordination `plan/agent-workflow-call/COORDINATION.md` r17. New: AWC-C005 r1 (published by p04 B1). It
  records that a call child stops the user-scope walk and that the publisher accepts call owners.
- Snapshot exclusions: `plan/agent-workflow-call/phases/p04/docs`, `.trail/learnings`.

## State

- Gate AWC-P04-D1 r2: PASSED (D1–D8, F8).
- Batch B1 (D1–D5, D7, D8): implemented and frozen at digest
  `555771ee2d824f5a556e08cf1b7e1746528a754fadb9e8daeb1020911b2b0a7c` (`logs/AWC-P04-B1-R1-snapshot.txt`). Review
  round AWC-P04-B1-R1 is next. The verification mapping and checks are in TASK.md (Status, B1 entry).
- Checks on the frozen code:
  - stack budget at 2 MiB, `channel_call_ channel_reply_`: 396/396;
  - source_modules: 8/8;
  - clippy 1.95.0 all targets: clean, except 10 findings in files this branch did not touch (committed in
    `73afd5a7`: 9 `redundant_field_names` in test files, 1 `nonminimal_bool` in `src/adapters/http/attribution/mod.rs:388`).
    With those two lints allowed, the run exits 0;
  - fmt `--check`: clean;
  - `cargo sqlx prepare --check -- --all-targets`: clean.
- Known unrelated failure: `doop_agent_current_preparation_reaches_ready_before_model_io` (format 7 vs 8;
  recorded by p02 and p03).

## Review status (2026-11-23)

- R1: changes required (AWC-P04-B1-R1-N1: F1 medium, F2 low, F3 optional). All are fixed; see TASK.md.
- R2 recheck was requested at digest `7af1aa2ed955dfc0599bfa4bd4c0501c3ebacefc9d143b2d4ed84f3bb84cc83b`
  (manifest `logs/AWC-P04-B1-R2.evidence.json`). B stopped mid-recheck on the Fable usage limit (HTTP 429,
  resets 21:00 Europe/Berlin). It saved no R2 records.
- User decision (2026-11-23): "Fresh Opus reviewer now". A substitute reviewer B' `a4f44ca532412935f` (Opus 5.5, medium effort, fresh
  context, general-purpose, no fork) does round AWC-P04-B1-R2 as a full batch review at `7af1aa2e…c83b`. It also
  rechecks F1–F3 and covers the areas R1 left unread. It writes the same p04 records. The original B
  `a8d0fa1fbd0b8b88e` is not resumed for this round.

- R2 (B', full batch): changes required, F4 and F5 low, F6 optional; F1–F3 closed.
- R3: **PASS** at `7b3f300c9f6f1de7a926f8b000d3a106beae939a26bd276470d74708b2f88a13`. **B1 accepted in phase**
  (root digest match, 2026-11-23).

- B2-R1: **PASS** at `46e25d956441f5ef6901f1294b044abfa81842d199f39a64c36243111dfa995c`. **B2 and p04 accepted in
  phase** (2026-11-23). The optional AWC-P04-B2-F1 is dispositioned in TASK.md.

## Next

1. p05 (authoring, inspection and samples). The hand-over items are in TASK.md: editor labels for
   `channel.deny`/`channel.hand_off` (`graph.rs:219`, `catalogue.rs:54`), `BUILDER_STEP_TYPES`, and the native
   template's response-step note.
2. p06: the final full gate, which also fills in the plan docs' "Implementation evidence" lines.

## Pending with the user

- Commit authorization (none given).
- The trail learning commit question ("y/n") is unanswered.
- Plan08 README proposals 4 and 5 must be settled before Plan08 p03.
- Drop the scratch DBs `openbot_awc_mac02` / `_test` at plan completion.
