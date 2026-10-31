# NBV-P02 review record

| Field | Value |
| --- | --- |
| Phase | NBV-P02 (plan `new-board-view` 02, filters and entry points) |
| Workflow | `implement-opus-reviewed-2_2` |
| Task record | [TASK.md](TASK.md) |
| Worktree | `/Users/mac02/.codex/worktrees/b109/openbot` |
| Coordination | `plan/new-board-view/COORDINATION.md` r2: NBV-I1 r1, NBV-N2 r1 applicable |
| Review notes | [agent-review-notes.md](agent-review-notes.md) |
| Findings | [agent-review-findings.md](agent-review-findings.md) |

## Rounds

| Round | Stage | Reviewer | Base → state | HEAD | Digest | Outcome |
| --- | --- | --- | --- | --- | --- | --- |
| [NBV-P02-B1-R1](agent-review-notes.md) | B1 batch review | B, Opus 5.5/xhigh | tree `85b05025` → tree `ed59ca7f` | `92e8c35` | `68e88e1d5fbbcc572cd7bd85b67c416eb7d2cc6e88c9db5e1f52c5df460926f1` | **Changes required** |
| [NBV-P02-B1-R2](agent-review-notes.md) | B1 correction recheck | B, Opus 5.5/xhigh | tree `ed59ca7f` → tree `fac0b467` | `92e8c35` | `c5538eb58dccc02187953dbf1ad7351919b6eefc3092b9028744f132c1858142` | **Changes required** (F6, Low only) |
| [NBV-P02-B1-R3](agent-review-notes.md) | B1 correction recheck | B, Opus 5.5/xhigh | tree `fac0b467` → tree `d68a8f1b` | `92e8c35` | `d6d1ff4f6c031002d47015c92a1bd859f872ee80f84e813c84e2a1d7d8b7ccb0` | **PASS** |

All digests exclude `plan/new-board-view/phases` and `plan/new-board-view/COORDINATION.md`.

## Findings (latest disposition)

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| [NBV-P02-B1-F1](agent-review-findings.md) | actionable | Medium | The list's reload leaf and "Load more" still use the scope fixed at render. A wake-up during a filter change aborts it, and the list stays on the old scope while the selects show the new one | fixed (R2) |
| [NBV-P02-B1-F2](agent-review-findings.md) | actionable | Medium | D6 "no workflow index" does not follow the plan's access-path ladder. The generic plan scans the tenant's whole `(company_id, id, workflow_id)` range per page; the doc misdescribes it; no dense or sparse workflow case | fixed (R2; D6 r2 index added) |
| [NBV-P02-B1-F3](agent-review-findings.md) | actionable | Low | Options are re-read and discarded on every board leaf reload; the agent option read was capped at 200 newest so a cut went undetected (R1 wording corrected in R2) (may be routed to 03) | fixed (R2) |
| [NBV-P02-B1-F4](agent-review-findings.md) | optional | Low | Workflow entry links (Workflows "Runs", pane "Runs board") are untested | fixed (R2) |
| [NBV-P02-B1-F5](agent-review-findings.md) | optional | Low | `board_html(…, with_options: bool)` flag parameter (`src/AGENTS.md`) | taken (R3) |
| [NBV-P02-B1-F6](agent-review-findings.md) | actionable | Low | `run-list.md` says a state change does not write `workflow_runs_workflow_newest`; state changes are never HOT, so it does (the same doc says so above); lease-budget not recorded | fixed (R3) |
| [NBV-P02-B1-F7](agent-review-findings.md) | optional | Low | Agent option labels and order are not pinned by any test (the selected-id fallback masks them) | taken (R3) |
