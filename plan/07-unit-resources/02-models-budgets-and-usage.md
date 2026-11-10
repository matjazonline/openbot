# 02 Models, budgets and usage

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A unit can set a default model and a spend cap, admission enforces the cap along the ancestor chain,
and usage pages show spend by department. Continues in
[03](03-integration-and-final-verification.md).

## Batch 02A Models

1. `company_model_connections` gains `unit_id` as in 01 and a `default_for_unit boolean`, one per
   unit by partial unique index. The company default (`pinned_default_model`,
   `src/adapters/persistence/agent.rs:169`) becomes the root's default.
2. Agent creation with no explicit model pins the nearest ancestor default of the agent's placement
   department; agents in no department pin the root's. A unit with no default and no ancestor
   default refuses creation with the existing "add a model connection" message.
3. Changing a unit default affects later publications only; pinned models are frozen as today.

## Batch 02B Budgets

1. `unit_budgets (company_id, unit_id, period text CHECK (period IN ('day','month')), cap_micros
   bigint, created_by, created_at)`, one per unit and period. Managed by the unit's manager (06).
2. Admission (`admission_write.rs:7`, after the root budget link) sums the current period's usage
   for the run's write unit and each ancestor from `model_usage_calls` (`init_schema.sql:11198`)
   attributed by unit, and refuses with `budget.unit_exhausted` naming the first exhausted unit.
   The check is a bounded read under the company lock; no new ledger. A run already admitted is not
   stopped mid-flight by a cap, matching the root budget's behavior.
3. `model_usage_calls` gains `unit_id`, written from the run's write unit at call time. Runs with no
   write unit attribute to the root.

## Batch 02C Usage pages

1. The model usage page (`src/adapters/http/routes/model_usage.rs`) and the company dashboard gain a
   department dimension: spend by unit for the period, with the cap and remaining amount when a
   budget exists, and a drill-down into the unit's desks.
2. A unit manager sees their subtree; root managers see all.

## Focused checks

- `unit_model_default_`: cascade from child to root, refusal with no default anywhere, pinned models
  unaffected by a later default change.
- `unit_budget_`: a child cap refuses a child run and not a sibling's; an ancestor cap refuses both;
  attribution follows the write unit; a run without a write unit attributes to the root; the usage
  page numbers match the ledger.
- `unit_budget_competing_`: two admissions racing the last of a cap admit at most what the cap
  allows, under the company lock.
