# AWC-P01 handoff

- Phase p01 of `plan/agent-workflow-call`; workflow implement-opus-reviewed-2_4; records here.
- Root/A: Claude Code `1e84a7af`, Opus 5.5/xhigh (user-confirmed). B: Fable/medium general-purpose agent `a8d0fa1fbd0b8b88e` (spawned 2026-11-22 for the D1 gate; continue with SendMessage).
- Worktree `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`, start `fe7791d3`.
- Coordination: `plan/agent-workflow-call/COORDINATION.md` r5 (writer: this session), AWC-C001 r4, AWC-C002 r1.
  Plan08 coordination r5 (writer: this session), 08G-C001 r2 blocks Plan08 p03 on this plan.
- User decisions (verbatim in the r3 gate file): U1 the child runs the destination binding with its own run_as; U2 the call gate is the calling agent's write right on the desk; U3/U4 the child ceiling follows plan 11 03C, with the allowance at level 0 (public) until plan 11 exists.
- Status: gate AWC-P01-D1 r1 not passed (findings F1–F8). r2 answered them, and r3 (U4) superseded r2 during the recheck. r3 is with B. Snapshot exclusions are `plan/agent-workflow-call/phases/p01/docs` and `.trail/learnings`; r3 digest `6b2e98c3…eed4` on `fe7791d3`. No code changes yet.
- Untracked learnings: `.trail/learnings/2026-11-22-a-desk-call-runs-the-called-mac02.md` (l-c0bc0871) and its replacement `…-mac02-2.md` (l-a5048a8f, which records U1–U4). Commit only with the user's authorization.
- Gate AWC-P01-D1 r3 PASSED (addendum A1 records accepted placements). Batch AWC-P01-B1: R1 at `66d3e78f…e17e` needed changes (F1–F4); R2 corrections frozen at `2ebbc0cd…feaa` (base `fe7791d3`, exclusions `plan/agent-workflow-call/phases/p01/docs`, `.trail/learnings`) and under B recheck.
- **p01 accepted in phase** at `2ebbc0cd…feaa` (TASK "Status"). Uncommitted on `plan08_ph1`.
- Next: p02 (target snapshots and dependency scopes), with its own records under `phases/p02/docs/`. B (`a8d0fa1fbd0b8b88e`) can continue for p02.
- Resources: scratch DBs `openbot_awc_mac02` and `openbot_awc_mac02_test` on localhost:5432 (fresh init migration). Owned by this phase; drop them at plan completion or when the schema changes.
