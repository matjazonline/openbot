# 01 — Filter runs by the agents their version uses

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

First of the run board plans. Order: **fix-01-run-agent-query** →
[fix-02-run-child-duplicates](../fix-02-run-child-duplicates/01-top-level-filter.md) →
[fix-03-run-board-finished-columns](../fix-03-run-board-finished-columns/01-run-finished-at.md) →
[new-board-view](../new-board-view/README.md). This plan is the read path only; the filter
controls belong to [new-board-view 02](../new-board-view/02-filters-and-entry-points.md).

## Outcome

`RunFilter` can select the runs whose workflow version uses an agent, in one company-scoped
statement. [02](02-query-plan-evidence-and-index.md) measures it and decides on an index;
[03](03-slow-read-tripwire-and-release.md) leaves a signal for when it gets slow.

## Context

- `list_runs` (`src/adapters/persistence/workflow/reads.rs:271`) appends only the filters
  present, so the company and binding shapes keep using `workflow_runs_newest` and
  `workflow_runs_binding_newest` (`migrations/20260817000000_init_schema.sql:7317-7318`).
  `RunFilter` (`src/application/workflow/reads.rs:188`) has `binding` and `state`.
- Which agents a version uses is `workflow_versions.agent_ids`: the distinct agents frozen in the
  version and its pinned children. Its column comment (init schema, line 5616) makes it a display
  index that no execution, admission or authorization path reads. Its one reader today is
  `agent_usage` (`src/adapters/persistence/workflow/authoring_reads.rs:288`).
- Neither `agent_ids` nor `workflow_runs.version_id` is indexed.

## Implementation

1. Add `agent: Option<Uuid>` to `RunFilter` (there is no agent id newtype; `agent_usage` takes
   `Uuid`). Its doc comment states the meaning: runs of versions that *use* the agent, including
   branches a run never took. Exact attribution through `model_usage_calls.agent_id` was rejected:
   it exists only after a model call, so queued runs would never match.
2. In `list_runs`, when `agent` is present, append
   `AND version_id IN (SELECT id FROM workflow_versions WHERE company_id = $1 AND $n = ANY(agent_ids))`.
   Both sides carry the company. Keep it one statement — no client-side version-id array, so no
   unbounded bind list and no window between two reads. Keep each SQL fragment in a named `const`
   so the script in 02 extracts the shipped text rather than a copy.
3. Resolve a caller-supplied agent id through a company-scoped lookup before listing, as the
   "Used by" route does (`src/adapters/http/routes/workflows/ui/authoring/tools.rs:396`). Another
   company's agent, or a deleted one, is not found — never an empty list
   (`src/adapters/http/AGENTS.md`, "Scope the object being used").
4. Accept `agent_id` on the JSON runs route (`RunsQuery`,
   `src/adapters/http/routes/workflows/bindings.rs:445`). UI query parameters are new-board-view's.
5. Name the second reader wherever the display-index contract is written:
   `authoring_reads.rs:289`, `src/application/workflow/publication/mod.rs:60`,
   `src/application/workflow/reads.rs:372`, and the column `COMMENT` in the init schema. It stays
   display-only: nothing decides or authorizes on it.

## Focused checks and acceptance

DB-backed tests named `run_agent_filter_*`:

- two versions of one workflow; only runs of the version that uses the agent match;
- a binding re-pointed to a version without the agent: earlier runs match, later ones do not;
- an agent used only inside a pinned child workflow matches the parent's runs (documented here;
  fix-02-run-child-duplicates removes the duplicate on the board);
- combined with `binding`, `state` and the keyset cursor, pages stay full and ordered;
- the JSON route answers not found for another tenant's agent id, and an empty page for this
  company's agent that no version uses.

Run against a disposable migrated database and confirm a nonzero test count:

```sh
createdb mail_agents_run_agent
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_run_agent" \
  cargo test --locked --offline --lib run_agent_filter_
dropdb mail_agents_run_agent
```

Record the commands, test count and result.
