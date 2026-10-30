# 03 — Verify and release

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-top-level-filter.md) and [02](02-child-rollup-on-parent.md).

## Implementation

1. Cross-check with fix-01-run-agent-query: an agent used only in a child workflow shows the parent
   once on an agent-filtered `TopLevel` read, and that parent's rollup counts the child.
2. The existing list behaviour is untouched: the route test
   `the_list_pages_over_runs_an_admin_cannot_see_and_labels_reruns_and_children`
   (`src/adapters/http/routes/workflows/ui/runs/runs_tests.rs:489`) passes without edits.
3. Doc comments on `RunLineage` and `child_rollup` state the decisions: board top-level, list
   any; direct children only; how child visibility was settled in 02.

## Final verification gate

Focused first, against a disposable migrated database: `run_top_level_`, `run_agent_filter_`, and
`cargo test --locked --offline --lib routes::workflows::ui::runs::`. Then run `scripts/check.sh`
once (formatting, offline compilation, clippy, migrations, sqlx metadata, database-backed suite).
Record commands and results, and name any gate that could not run.
