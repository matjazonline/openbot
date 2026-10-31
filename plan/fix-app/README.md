# fix-app — architecture and data-model fixes

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Where this came from

An architecture and database-model review on 2026-10-31 at commit `4e92edd`. Five parallel reviews
covered tenant integrity, schema constraints, the workflow model, persistence code and application
layering, and the highest findings were then re-read by hand. File and line references are as of
that commit: re-check a span before editing it.

The core held up. Tenant foreign keys are composite almost everywhere (196 of 210 between
tenant-owned tables carry `company_id`), queue claims are fenced by execution generation, the
domain layer has no outward imports, and no table or SQL function is dead. These plans cover what
did not hold.

## Plans

| Plan | What it fixes |
|---|---|
| [01 Identity and ownership](01-identity-and-ownership.md) | An old email address stays trusted after an email change. An owner can demote themselves through an invite. Deleting an agent cascades into message history. An invite can be edited after it was accepted. The allowlist takes over grants it doesn't own. |
| [02 Silent stalls](02-silent-stalls.md) | The 64th email send stalls a run for 24 hours. One bad schedule snapshot fails a whole claim page. A slow provider turns successful sends into `outcome_unknown`. A notification can stay `pending` forever. |
| [03 Workflow database guards](03-workflow-database-guards.md) | Run state transitions have no database guard. A guard violation looks like an outage. SQL hard-codes the compiler revision to set budgets. Smaller Rust/SQL mismatches. |
| [04 Authorization in application](04-authorization-in-application.md) | HTTP handlers read tenant rows by bare id and write their own access checks. `company_access` is a capped trait default. Onboarding is a half-committed handler. The SMTP adapter decides sender trust itself. |
| [05 Tenant integrity](05-tenant-integrity.md) | Notification emails have no tenant. A run's binding has no foreign key. A row can reach a channel by two paths that disagree. Inbound dedup is global instead of per tenant. Smaller FK gaps. |
| [06 Workflow storage growth](06-workflow-storage-growth.md) | Witness rows are written on every run and kept forever. Every run stores its own copy of the bundle. Workflow history has no retention. |
| [07 Application layering](07-application-layering.md) | The spam scanner is an adapter living in application. Application depends on `AppConfig`. Confirmation codes are keyed with the JWT secret. Module cycles. `AppState` is a god object. |
| [08 Schema cleanup](08-schema-cleanup.md) | Dead columns and values, missing state-machine constraints, wrong nullability, strict JSONB decoding with no row context. |

## Dependencies

"Depends on" means the later plan builds on a decision or code the earlier one changes. Start it
after the earlier plan has merged.

| Plan | Depends on | Why |
|---|---|---|
| 01 | — | |
| 02 | — | |
| 03 | 02 | 02 defines the one continuation limit and edits the job completion guard (`init_schema.sql:175-211`). 03 classifies that guard's errors and writes the run-state transition table, which must admit what 02 produces. |
| 04 | 01 | 04 moves every owner check onto `company_access`. That is only correct once 01 enforces `companies.user_id` and the owner member row as one fact. 01 also rewrites the invite writes that 04 would otherwise wrap. |
| 05 | — | |
| 06 | 03 | Retention deletes terminal runs, which is safe only once 03 guards the edges that leave a terminal state (retry). 06's witness changes and 03's transition trigger are both `BEFORE UPDATE` triggers on `workflow_runs`. |
| 07 | 04 | Both reshape the `use_cases` ports. 04 removes `ThreadUseCases`' persistence accessors and adds scoped methods. 07 moves ports into a shared module and splits `AppState`. Run concurrently, they rewrite the same modules twice. |
| 08 | 01, 02, 03 | 08 drops identity status `disabled` (01 decides not to use it). It constrains `task_attempts` stop reasons, which are final only after 02 and 03. It copies error-class rules onto the delivery and notification queues that 02 changes. One item, `model_usage_calls` nullability, waits for 06's retention decision. |

```
Wave 1 (now)      Wave 2              Wave 3
01 ─────────────► 04 ───────────────► 07
02 ─────────────► 03 ───────────────► 06
05                                    08  (after 01, 02, 03)
```

## What can run at the same time

- **Wave 1, start now in parallel:** 01, 02 and 05. None depends on another.
- **Wave 2:** 03 once 02 has merged, and 04 once 01 has merged. 03 and 04 are independent of each
  other, and 05 may still be running.
- **Wave 3:** 06 once 03 has merged, 07 once 04 has merged, and 08 once 01–03 have merged. The
  three are independent of each other.

The two critical paths are 02 → 03 → 06 and 01 → 04 → 07. Plans 05 and 08 fit around them.

## Rules for running plans concurrently

- **One init schema.** 01, 02, 03, 05, 06 and 08 all edit
  `migrations/20260817000000_init_schema.sql`.
  - Each plan edits only the statements it names, in place, so parallel branches merge as text.
  - After every merge, run `./scripts/reset-db.sh --all` and the merged plan's focused database
    tests. Two schema edits that are each valid can still conflict at runtime, for example a new
    FK from 05 against a delete path that 01 changes.
- **Known overlaps:**
  - 01 and 05 both edit foreign keys around principals, messages and member removal.
  - 02 and 05 both touch delivery code: 02 changes the lease fence in `delivery/queue.rs`, and 05
    gives notification deliveries a `company_id`.
  - 03 and 06 both add triggers on `workflow_runs`.
  - Whichever of a pair merges second rebases onto the first and reruns both plans' focused tests.
- **Isolation:** run each concurrent plan in its own worktree, with its own test database
  (`mail_agents_fix_app_NN`). Test support truncates every table when a test binary first
  connects, so two plans sharing one test database break each other.
- **`.sqlx/`:** regenerate it (`cargo sqlx prepare -- --all-targets`) after merging any plan that
  changed a macro-checked query in `agent.rs`, `company_invite.rs` or `user.rs`.

## Verification

- **Per batch:** the focused tests the plan names, on its disposable database. Confirm a nonzero
  test count for each filter.
- **Per plan, once at the end:** `./scripts/reset-db.sh --all`, then `scripts/check.sh` (formatting,
  offline compilation, clippy, migrations, sqlx metadata, the database-backed suite and the stack
  budget).
- **Recording:** record the commands and results, and name any gate that could not run. Do not
  claim full verification without it.

## Not in these plans

- Query performance and indexing, deferred in `plan/db_improve/`.
- Credential encryption, in `plan/db_improve/improve-key-credentials.md`.
- The reconciliation evidence verifier and the other items in `plan/todo.md`.
- Splitting the large files wholesale. 07 lists them and splits only what it already touches.
