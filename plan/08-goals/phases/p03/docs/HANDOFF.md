# 08G-P03 handoff

- **Phase:** `p03` of `plan/08-goals` (goal units and tracking). Workflow implement-opus-reviewed-2_4. Records are in
  this directory.
- **Root/A:** Claude Code `1e84a7af`, Opus 5.5/xhigh.
- **B:** a fresh Fable/medium reviewer, agent `a6ec66ca8201b8d62` (spawned 2026-11-24 for gate D1 r1).
- **Worktree:** `/Users/mac02/orca/workspaces/openbot/plan08_ph1`, branch `plan08_ph1`. Phase start `c7fbbbbe`.
  Uncommitted at start: the Plan08 proposal confirmations and the AWC p06 HANDOFF note.
- **Scratch databases:** `openbot_p08p03_mac02` and `openbot_p08p03_mac02_test`.
- **Coordination:** Plan08 COORDINATION r8. 08G-C001 is satisfied and 08G-C002 acknowledged.

## State (2026-11-24)

- Gate 08G-P03-D1 PASSED at r2.
- **B1 (03A+03B) accepted in phase** at `6caa7f19…4e2b` (B PASS 08G-P03-B1-R2-N1). The accepted diff is saved
  under `logs/08G-P03-B1-accepted*.txt`. Not committed.
- Optional S2 (open-work wording for a standing ancestor) is deferred to B2.
- Recreate the test DB after any schema edit, otherwise you get `VersionMismatch(20260817000000)`.

## Next

1. Gate 08G-P03-D2 r1 is drafted in TASK.md and under review by B. Then implement B2 (03C budget with goal spend counters; 03D rollup, pages, API, `goal.status`,
   authorization). Have B review it, then implement, review and accept.
2. Then Plan08 p04 (integration and final verification).

## Pending with the user

- Commit authorization for the p03 work (nothing in p03 is committed yet).
- Open product question: whether cancel-and-archive of a goal should also cancel a goal run elsewhere that
  started outside the goal's own trees (for example a later reply on a call thread). B1 says no: the archive is
  refused as not cancellable and that department ends the run. `docs/departments.md` documents this.
