# Plan coordination: new-board-view

- Authority: /Users/mac02/.codex/worktrees/b109/openbot/plan/new-board-view/COORDINATION.md
- Writer: root session (Opus 5.5, user-confirmed effort medium or above), worktree /Users/mac02/.codex/worktrees/b109/openbot
- Revision: 7
- Moved (NBV-I2): this plan, with its phase records, now lives at /Users/mac02/.codex/worktrees/b109/openbot/plan/.archives/new-board-view; records keep the paths they were written with.

## Plan-wide instructions
- NBV-I2 r1 (user, 2026-10-31): 05 step 3 archives as commit 601440f did — move plan/new-board-view with its phase records to plan/.archives/new-board-view and repoint the list entries and links; delete nothing.
- NBV-I1 r1: No backward compatibility (plan README, AGENTS.md). Phases run sequentially in one worktree, each accepted before the next starts.

## Phase directory
| Phase | Owner/session | Worktree | Authoritative TASK/HANDOFF paths |
| --- | --- | --- | --- |
| p01 (accepted 158ecfe9) | root | /Users/mac02/.codex/worktrees/b109/openbot | plan/new-board-view/phases/p01/docs/{TASK,HANDOFF}.md |

| p02 (accepted d6d1ff4f) | root | /Users/mac02/.codex/worktrees/b109/openbot | plan/new-board-view/phases/p02/docs/{TASK,HANDOFF}.md |
| p03 (accepted 38ee1d9e) | root | /Users/mac02/.codex/worktrees/b109/openbot | plan/new-board-view/phases/p03/docs/{TASK,HANDOFF}.md |

| p04 (accepted fc4955c7) | root | /Users/mac02/.codex/worktrees/b109/openbot | plan/new-board-view/phases/p04/docs/{TASK,HANDOFF}.md |

| p05 (accepted f7da3b6c; plan complete, uncommitted) | root | /Users/mac02/.codex/worktrees/b109/openbot | plan/new-board-view/phases/p05/docs/{TASK,HANDOFF}.md |

## Cross-phase notices
- NBV-N1 r1 (resolved in p03, D3 r2) (from p01 → p03): on a phone, after going back from the pane to the board, an SSE-triggered pane reload (e.g. the 30 s tick) pulls the reader back to the pane, because the shell's beforeSwap handler (mailbox.rs ~410) treats any xhr swap inside `.ui-pane-detail` as navigation. p03 decides: fix (e.g. only treat a request whose target is `#run-pane` itself / not from an SSE leaf as navigation) or record a deferral. Evidence: p01/docs/agent-review-findings.md NBV-P01-B1-F9.
- NBV-N2 r1 (from p01 → p02): every scope control added to the board toolbar must reach the reload leaf: include the whole toolbar form in the leaf's `hx-include`, not only `#run-board-window`.
- NBV-N4 r1 (from p02 → p05): the init schema changed (workflow_runs_workflow_newest); reset every local database (incl. the shared test DB) before the full gate. Shared test DB mail_agents_run_board was reset at p02 acceptance.
- NBV-N3 r1 (from p01 → p05): the 05 browser pass owes the served-page light/dark check (NBV-P01-D7) and must look at phone vertical overflow of a 50-card column (seen on the list too).
