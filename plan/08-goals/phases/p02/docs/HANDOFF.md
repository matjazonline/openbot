# 08G-P02 handoff

## Identity

- **Phase:** p02 of `plan/08-goals` (unit lifecycle: archive and restore).
- **Workflow:** implement-opus-reviewed-2_4. Record root: `plan/08-goals/phases/p02/docs`.
- **Agents:**
  - Root/A: Claude Code session `plan08-ph1-51`, Opus 5.5/xhigh (user-confirmed).
  - B: Fable/medium general-purpose reviewer, agent `a51801b0670ea8aad`. It is idle; continue it with SendMessage only if this phase reopens.
- **Worktree:** `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`.
- **Commits:**
  - Start: `73afd5a7` (merge of origin/main, plans 04–07).
  - Review base: `42bc327e` (coordination r3).
  - `ce0bb1e1`: another session's plan-11 commit (plan docs, learnings, 6 notice lines in `src/main.rs`). It holds none of this phase's edits.
  - The phase work, its records and its learnings (`.trail/learnings/2026-11-21-*`) are committed on top of `ce0bb1e1` in "Archive and restore units (Plan08 p02)".
  - **Not pushed, not deployed.**

## Status

- **Accepted in phase:**
  - B1 at `a10ec4c2…`.
  - B2 at `b066cce6…` (TASK "B2 acceptance", B PASS note `08G-P02-B2-R3-N1`).
  - Phase content is unchanged since acceptance. The snapshot digest `90f18e31…` on base `ce0bb1e1` differs only because the base moved.
- **Integrated:** not yet. The work is on branch `plan08_ph1` only; it is not merged to `main`.
- **Final full gate (TASK "Final gate"):**
  - **PASS:**
    - formatting, offline compilation, fresh-database migrations, SQLx metadata, compiling the tests;
    - the boundary, protocol and evidence gates;
    - lease budget, memory backup, OpenBot and memory integration.
  - **Database test suite:** 5026 passed, 1 failed. The failure is the pre-existing `doop_agent_current_preparation_reaches_ready_before_model_io` (version 8 vs 7).
  - **Stack budget:** no overflow. Two doop document load timeouts; both pass alone at 2 MiB.
  - **No gate-driven changes.**
- **Not verified on this machine:**
  - Clippy fails on 9 pre-existing `redundant_field_names` lints from `73afd5a7`, outside the phase diff.
  - Environment-blocked gates:
    - Docker images;
    - Node 24 plugin package;
    - the Python 3.11 `tomllib` memory and attribution suites;
    - the Chrome Doop gate;
    - `pg_stat_statements`;
    - `rg` on PATH for the transport self-test;
    - the attribution SSO browser test, which already failed the same way in the plan04/p03 gate.

## Decisions and known limits

- **User decisions:**
  - Proposals 2 and 3.
  - U3: the parent manager cancels open work, audited.
  - U4: archived desks allow only delete and move out.
  - F3 "desk history only": membership and roles in an archived unit grant reading its desk history and nothing else. Its knowledge and memory are read only from above.
- **Known limits:**
  - `channel.call` has no producer, so the 02B.3 call clause is NOT EXERCISED. The agent-workflow-call plan must require it.
  - No race test for the archive's principal-lock order. It is serialized by the company write lock.
  - The disposition of an in-flight direct-address run is untested.
- **`trail check`:** 6 blocking hits, all word-overlap matches on SQL comments in the init migration. Two of them are outside this phase's hunks, since the check diffs against `main`. None of the cited learnings concerns archive triggers. The reason is recorded in the commit message.

## Resources and coordination

- **Resources:** the scratch DBs `openbot_merge08_mac02` and `openbot_merge08_mac02_test` are DROPPED, and no `openbot_check_*` DBs are left. Running the tests again needs fresh scratch DBs with `sqlx migrate run`.
- **Coordination:** `plan/08-goals/COORDINATION.md` revision 3 (sha256 `4ea05356…`), rechecked 2026-11-22. No applicable notices and no pending cross-phase requests.

## Next action

- Push or merge `plan08_ph1` when the user authorizes it.
- Optionally fix the 9 pre-existing clippy lints in a separate change.
- Run the unavailable gates in CI or a full environment before claiming whole-suite verification.
