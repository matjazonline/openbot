# f01 handoff — fix-01-run-agent-query

- Phase `f01`, workflow `implement-opus-reviewed-2_2`; records in this directory; coordination
  `plan/fix-01-run-agent-query/COORDINATION.md` revision 1 (no notices).
- Root = A: Claude Code session, Opus 5.5 / xhigh (user-confirmed). B: agent `ad2d28ffa854b444f`
  (Opus, xhigh), rounds F01-B1-R1 (changes), R2 (changes, Low), R3 (PASS). B finished; not reused.
- Worktree `/Users/mac02/.codex/worktrees/b109/openbot`, start/review base `a6dc3be`.
- **Accepted in phase** for digest `3cd0d212c470888aa55b3ede7edf83d1ac5e5f4de7e5019df20d4ed769563f3b`
  (exclusions `plan/fix-01-run-agent-query/phases/f01/docs`
  `plan/fix-01-run-agent-query/COORDINATION.md`); committed with these records and pushed to `main`.
- Delivered: `RunFilter.agent` + JSON `agent_id` (company-scoped), `workflow_runs_version_newest`
  + per-version lateral read, slow-read tripwire (`state` names, `page_rows`), measurement script
  and `docs/performance/run-list.md`, deferred `plan/db_improve/run-agent-link-table.md`.
- Open: optional F01-B1-F8 carried to fix-03. Known shapes: agent + rare state; versions × page
  bound (both documented, tripwire-gated).
- Contract for successors: `RunFilter { binding, state, agent }`; `run_list_query` /
  `push_run_page` builder; `dump_run_list_query_shapes` + `parameters()` in `reads_tests.rs` must
  be extended with any new filter; `scripts/tests/run-list-query-plan.py` measures dumped shapes.
- Dev database `mail_agents` untouched; it needs `./scripts/reset-db.sh` before `serve` (init schema
  changed).
- Next: start `fix-02-run-child-duplicates` (phase `f02`) on this pushed commit.
