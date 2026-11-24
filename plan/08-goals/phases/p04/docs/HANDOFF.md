# 08G-P04 handoff

- **Phase:** p04 (integration and final verification), workflow implement-opus-reviewed-2_4.
- **Records:** `plan/08-goals/phases/p04/docs/`.
- **Agents:** root/A is Claude Code `1e84a7af` (Opus 5.5); B is Fable `a6ec66ca8201b8d62`.
- **Worktree:** `/Users/mac02/orca/workspaces/openbot/plan08_ph1` on `plan08_ph1`; start `f1eb82dc`.
- **Coordination:** r13.

## State (2026-11-24)

- **Implemented:**
  - the 04A docs;
  - the `goal.status` agent-runtime test;
  - the `agent_tool_bindings` CHECK fix (a p03 defect the test found);
  - the dead `with_channel_call` helper removed (a clippy regression from the gate);
  - the ignored measurement `goal_rollup_cost_at_the_run_limit`.
- **Final gate:** done and recorded in TASK "Final gate". Its environment-blocked gates were rerun with local
  stand-ins (the `rg` shim, Python 3.13, a private `pg_stat_statements` cluster).
  - Still UNAVAILABLE: the Node 24.21.0 package, the Docker images, managed Doop (Chrome), the attribution
    `jsonschema` contract check, and the Linux network scope.
  - The pre-existing failures: the doop prepared version, attribution browser cookies, and the 9 clippy lints.
- **Completion record:** filled in `04-integration-and-final-verification.md`.
- **Scratch DBs:** `openbot_p08p03_mac02` and `_test`; `_test` was recreated after the measurement. The private
  cluster in the scratchpad is stopped.

## Accepted

B PASS 08G-P04-B1-R2-N1 at `c081c405…fc47`, recomputed by root. Not complete: see the unavailable gates in the 04
completion record.

## Next

1. Commit p04, once the user authorizes it.
2. Drop the scratch DBs `openbot_p08p03_mac02` and `_test` (tell the user).
