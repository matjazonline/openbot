# 01 — Board view and layout

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on all three fix plans (see [README](README.md)).

## Outcome

`GET /ui/workflow-runs?company_id=…&view=board` renders the runs workspace with the board in place
of the sidebar list, and the existing run pane opens beside it when a card is chosen. There is no
new path.

## Implementation

1. `workspace` (`GET /ui/workflow-runs`, `src/adapters/http/routes/workflows/ui/runs/mod.rs`)
   takes `view=list|board`. Absent or unknown means the list, so `run_href`, the binding "runs"
   link and run links in conversations are unchanged. With `view=board&part=columns` it answers
   only the `#run-board` region, as the pane route answers `part=body`. Authorization is the
   workspace's own `Ui` / `ui.company`: company managers only.
2. The handler reads the board as
   [fix-03-run-board-finished-columns 02](../fix-03-run-board-finished-columns/02-bounded-board-read.md)
   describes — counts, visibility, cards — with `as_of` taken per request, then child rollups and
   workflow titles (`run_views().workflow_titles`, as `run_page` does).
3. Page in a new `src/adapters/http/pages/workflow_runs/board.rs`. `<main>` keeps
   `sse-connect="/ui/workflow-runs/events?company_id=…"` and stays mounted
   (`src/adapters/http/AGENTS.md`). Inside it: a toolbar (List/Board toggle, filters from 02,
   window select); `#run-board` with six columns side by side, scrolling horizontally when narrow;
   and `#run-pane` at a fixed width when `run_id` is set, absent otherwise so the board takes the
   full width.
4. Column header: state label, visible total, the window under finished columns ("finished in the
   last 7 days"), and when cut "Showing 50 of 312 · open in list" linking to `/ui/workflow-runs`
   with the same scope and that `state`.
5. Card: workflow title, short run id, `run_state_badge` with the waiting reason
   (`src/adapters/http/pages/workflow_runs/mod.rs:73`), age (since creation for open runs, since
   finishing for finished ones), the rerun label the list shows, and the child rollup ("2 child
   runs · 1 waiting"). A click `hx-get`s the existing pane route into `#run-pane`, pushes `run_id`
   into the URL with `view=board` kept, and moves the highlight through `#run-selected` as the
   list does.
6. Escape every value for its context (`src/adapters/http/pages/AGENTS.md`, "Escape for the
   output context"): workflow titles, binding labels, agent names in filter options.
7. Visible progress (pages AGENTS): a filter or window change marks `#run-board`
   `aria-busy="true"` until the swap; a card click marks the pane busy.
8. New Tailwind classes need `npm run build:css` and the regenerated `assets/app.css` committed.
   Check the light and dark themes on computed values (pages AGENTS).

## Focused checks and acceptance

Route tests named `run_board_view_*`, beside `runs_tests.rs`:

- six columns in order with their totals;
- a cut column shows "Showing 50 of …" and its link keeps the scope;
- a hostile workflow title is escaped in text and attribute positions;
- a plain member gets no board, and another tenant's `company_id` is refused;
- `run_id` opens the pane and highlights its card; `view=board&part=columns` returns only the
  region; an absent or unknown `view` renders the list exactly as before.

```sh
createdb mail_agents_run_board
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_run_board" \
  cargo test --locked --offline --lib run_board_view_
dropdb mail_agents_run_board
npm run check:css
```

Confirm a nonzero test count and record the results.
