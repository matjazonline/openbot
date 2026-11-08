# Resume on a different computer

Read this package's README first. Treat the new checkout path as the repository root. Do not try to open the old computer's absolute paths or restore old BB agent handles. Map historical `/Users/mac03/dev/openbot` paths to this new root; phase worktrees are `.worktrees/fix-app-p03` and `.worktrees/fix-app-p04`. Keep historical review records and their hashes unchanged. Before implementation, the new manager must explicitly update COORDINATION.md authority/writer/resource paths for this computer and have each phase acknowledge that update.

Read, in order:

1. `plan/fix-app/COORDINATION.md` (revision 9 at capture).
2. `plan/fix-app/integration/resumed-all/docs/HANDOFF.md`.
3. Each worktree's `plan/fix-app/phases/p03/docs/HANDOFF.md` or p04 equivalent, plus the original numbered plan and current applicable AGENTS.md.
4. Exact selected workflow: `.recovery-workflow/implement-astra-reviewed-2-2/SKILL.md` (a copy of the installed skill at capture).

The user selected separate Sol 6.1/xhigh authors and independent Astra/high reviewers. Start fresh workers without inherited context; prior worker handles are stopped. Preserve targeted-test-only scope. p03 has 31 unresolved affected-test failures; p04 has 15 passing authorization tests but broader checks/code review remain. Neither source batch is accepted. p06/p07/p08 have not started. The user already chose 90-day eligible terminal-tree retention with usage/cost reporting preserved; do not re-ask that choice.

Disposable databases, local `.env` credentials, build artifacts and the p03 discovery graph cache are deliberately omitted. Create isolated PostgreSQL fixtures on the new host; these phase baselines use `mail_agents` names, while captured main uses `open_bots`. Re-run affected checks on the new environment and record new artifact identities if anything changes. Historical source snapshot hashes are retained in manifest.json; changing machine paths/environment does not create a fresh test pass or acceptance.

The user paused implementation. Restoring files is authorized recovery work; resume implementation only when explicitly instructed.
