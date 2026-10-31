# 04 — Put Runs in the rail

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-board-view-and-layout.md).

## Outcome

The rail can link to a page with a query of its own, and its new "Runs" entry opens
`/ui/workflow-runs?view=board`, shown to exactly the members the runs routes admit. The Dashboard
points at run-level state instead of reading as if its job counts were runs.

## Implementation

1. **Fix how the rail builds links.** `icon_rail` (`src/adapters/http/pages/mailbox.rs:1529`)
   formats every destination as `{path}?company_id={company_id}` (`mailbox.rs:1573`), so no
   destination can carry a query. Replace the four-element destination tuples with a named struct
   (`src/AGENTS.md`, "Name your tuples") — section, path, query pairs, glyph, title — and build
   every href in one helper that encodes the pairs, appends `company_id`, and escapes the result
   for the attribute. Every existing destination keeps its exact href.
2. Add `UiSection::Runs` (`mailbox.rs:35`) and a destination after Workflows: path
   `/ui/workflow-runs`, query `view=board`.
3. In `rail_section_visible` (`mailbox.rs:1601`), `Runs` uses `manages_company_operations()`,
   matching `ui.company` on the runs routes, so the rail never offers a page that refuses the
   caller.
4. Both runs views render with `section: UiSection::Runs` instead of `Workflows`
   (`src/adapters/http/pages/workflow_runs/mod.rs:297`), so the rail lights Runs.
5. Add a rail glyph: the Octicons 16px `project` icon, sourced like the others
   (`src/adapters/http/pages/icon.rs`, module doc).
6. **Dashboard.**
   - Its job-queue panel counts `background_tasks` step jobs, which reads easily as runs. Label
     it as step jobs and link to the board for run-level state.
   - `outstanding_row` (`src/adapters/http/pages/dashboard.rs:336`) links only jobs in a
     conversation; its comment says other jobs have "no page until the run page (p09)", and that
     page has shipped. Carry each job's run id on `OutstandingTask`
     (`src/domain/entities/dashboard.rs:272`), through `background_tasks.workflow_execution_id`, and
     link it with `run_href`, still only where `openable` holds. The run page re-checks company and
     channel access itself (`RunLinkAccess`, `pages/workflow_runs/mod.rs`). Replace the comment.

## Focused checks and acceptance

- Extend `icon_rail_lights_the_workspace_the_response_belongs_to`
  (`src/adapters/http/pages/tests.rs:750`):
  - every existing destination's href is unchanged;
  - Runs appears for an owner or admin, with an href carrying `view=board` and `company_id`;
  - Runs is absent for a plain team member;
  - Runs is lit (`aria-current="page"`) on both runs views and not on Workflows.
- In the Dashboard page tests, cover:
  - the step-job label and the board link;
  - an outstanding job without a thread linking to its run;
  - no link for a company the caller does not belong to.

```sh
SQLX_OFFLINE=true cargo test --locked --offline --lib icon_rail_
SQLX_OFFLINE=true cargo test --locked --offline --lib dashboard
npm run check:css
```

The Dashboard's persistence change needs its DB-backed tests against a disposable migrated
database, as in 01. Record the results.
