# 07UR-P02: Models, budgets and usage

## Identity and authority
- Workflow implement-opus-reviewed-2_4; phase p02 = Plan07 doc 02 (02A models, 02B budgets,
  02C usage pages); record root plan/07-unit-resources/phases/p02/docs.
- Criteria: ../../../README.md (decisions 3, 4, 5, 7; "What does not change"),
  ../../../02-models-budgets-and-usage.md; AGENTS.md (no backward compatibility; competing-claimant
  tests for concurrency protocols; focused checks per batch, full gate in p03).
- Root/A: Claude Code Opus 5.5/xhigh (user-confirmed), session
  73d846ee-63bd-49f3-bbdf-7f3d92fcce54. B: Opus 5.5/xhigh (user, 2026-11-21: "use Opus5.5/xhigh
  reviewer from now"), a new agent (ID recorded at spawn); the p01 Fable reviewer is not reused.
- Worktree /Users/mac02/orca/workspaces/openbot/plan06, branch plan07, phase start 107fd588
  (p01 accepted in phase and committed). Preserved untracked, not part of the phase: the three
  .trail/learnings/2026-11-{19,20}-*-mac02.md files (user declined to commit) and
  plan/06-delegated-administration/phases/p01/docs/logs/*.snapshot.tar.
- Coordination: plan/07-unit-resources/COORDINATION.md r7. No cross-phase notices. Plan-wide
  user decisions: (a) unit managers configure unit resources; (c) a unit's spend cap is set by
  that unit's own manager (its lead or anyone above), ancestor caps still bound the subtree;
  (d) an agent's resources follow the agent's own units; (f) any run in reach may use a connection;
  (g) one model connection per provider, owned by a unit, a unit's default is one of its own;
  (h) unpriced usage counts as zero against caps and is flagged; (i) UTC day/month periods.
  Coordination r8.
- Prerequisites: p01 (unit ownership, `unit_reaches`, `resource_default_unit`, authority by
  `unit_management_role`), Plan05 run scopes (`workflow_run_scopes`), Plan06 management.
- Resources: UTF-8 PG18 cluster 127.0.0.1:55407; fresh databases openbot_p07_p02_* per check run.
- Snapshot exclusions (reused unchanged): plan/07-unit-resources/phases/p02/docs,
  plan/07-unit-resources/COORDINATION.md.

## Requirements (doc 02)
- 02A.1 `company_model_connections` gains `unit_id` (as in 01) and `default_for_unit boolean`, one
  per unit by partial unique index; the company default becomes the root's default.
- 02A.2 agent creation with no explicit model pins the nearest ancestor default of the agent's
  placement department; no department → root's; none anywhere → the existing "add a model
  connection" refusal.
- 02A.3 changing a unit default affects later publications only; pinned models stay frozen.
- 02B.1 `unit_budgets (company_id, unit_id, period day|month, cap_micros, created_by,
  created_at)`, one per unit and period, managed by the unit's own manager (decision c).
- 02B.2 admission sums the current period's usage of the run's write unit and every ancestor
  from `model_usage_calls` and refuses `budget.unit_exhausted` naming the first exhausted unit; a
  bounded read under the company lock, no new ledger; an admitted run is not stopped mid-flight.
- 02B.3 `model_usage_calls` gains `unit_id` from the run's write unit at call time; no write unit
  → root.
- 02C.1 the model usage page and company dashboard gain spend by unit for the period, with cap and
  remaining when a budget exists, and a drill-down into the unit's desks.
- 02C.2 a unit manager sees their subtree; root managers see all.
- Focused checks: `unit_model_default_`, `unit_budget_`, `unit_budget_competing_`.

## Investigation (2026-11-21, read-only sweep at 107fd588; file:line verified spans)
- `company_model_connections` (schema:3842): PK `(company_id, provider)` (schema:6403), one
  `is_default` per company (schema:8170); writer `replace_model_connections_for_user`
  (persistence/company.rs:329-455, owner-only `FOR UPDATE` check :336; clears defaults, upserts on
  `(company_id, provider)`, deletes omitted providers, refuses stranding a pinned agent :424-449);
  use case owner-only (use_cases/company.rs:208-219, exactly one default :297). Readers:
  `model_connections_on` company.rs:462, `ready_models_on` workflow/provisioning.rs:406,
  availability `FOR SHARE` agent_authority.rs:87 and agent_publication/capture.rs:62,
  `prompt_model` model_preparation.rs:59, schema `native_mcp_parent_agent_live` :19934.
- Model pinning: `pinned_default_model` is use_cases/agent.rs:238 (doc cites persistence/agent.rs
  :169); SQL twin `resolve_model_on` company_agent.rs:202 → `no_usable_model()` :252, called in
  `insert_company_agent_identity_on` with the placement in hand (company_agent.rs:138). Library
  copy (`prepare_library_write`, use_cases/agent.rs:463) and onboarding (use_cases/onboarding.rs
  :219, agents Unplaced) pin before the transaction without placement. Agent Builder requires an
  explicit provider/model (no pinning).
- Root budgets (`workflow_root_budgets` schema:5918) are per-root-run count/token limits enforced
  at model-call time (checkpoint_models.rs:102-130, `workflow.root_budget_exhausted`), not
  admission spend caps; no `budget.*` code exists. Admission holds the company lock from
  `authorize_person_with_locks` (authority.rs:81). B2 already settles schedule/message refusals
  through `classify` → `Refused(code)` (production schedule test).
- `model_usage_calls` (schema:11460) stores tokens only; money = join `model_prices` (micro-USD per
  million tokens, schema:11513; unpriced allowed; metrics.sql:58 already computes it). Inserts:
  `reserve_on` model_usage/mod.rs:40 (run, agent known), JEV jev_runtime.rs:238. Archive moves rows
  to `model_usage_history` (schema:12076, no run/unit; storage_retention.rs:122), and
  `schema_constraint_live_usage_requires_run_and_history_has_no_runtime_owners`
  (model_usage_tests.rs:197) constrains history columns. No day/month/timezone logic exists.
- Run write unit: `workflow_run_scopes` (schema:20367) has a base set and per-agent sets; the base
  write unit is the first unit of the desk/target chain (scopes.rs:118), absent when the chain is
  empty; captured last in `admission_write::create` (:66). Child runs copy scopes.
- Usage page (routes/model_usage.rs) root-managers only via `authorize_company`, already groups by
  the desk's current department (report.rs:152-180); dashboard (ui_dashboard.rs) has a department
  filter; `department_views_usage_page_groups_by_department` covers it.

## Architecture gate G1 (decision revision r1, 2026-11-21)
Gated because it changes schema, a concurrency contract (caps vs racing admissions and usage) and
authority (model connections and caps). Batches after the gate: B1 = 02A (D1–D4), B2 = 02B (D5–D8),
B3 = 02C (D9).

**D1 Model connection ownership (02A.1, decision g).** `company_model_connections` gains
`unit_id uuid NOT NULL` with the composite FK to departments `ON DELETE RESTRICT` (as p01) and
`default_for_unit boolean NOT NULL DEFAULT false`; `is_default` and its index are dropped (no
backward compatibility). Partial unique index `(company_id, unit_id) WHERE default_for_unit`. The
key stays `(company_id, provider)`. The company default is "the root's default": the root must
hold exactly one default whenever it holds a connection; a non-root unit holds at most one.
Readers of `is_default` (`model_connections_on`, `ready_models_on`, `prompt_model`/
`pinned_default_model`, `resolve_model_on`, `native_mcp_parent_agent_live` if it reads it) switch
to the root default or to the cascade (D2). Department delete guidance (p01 D5) lists model
connections too.
- Writes (decision a): `replace_model_connections` becomes per unit: the actor must manage the
  unit (`unit_management_role`), replacing only that unit's rows; adding a provider another unit
  owns is a Conflict naming that unit; the pinned-agent stranding refusal is kept (checked across
  the company). The company settings form edits the root's connections (owner, root admins, root
  leads — widening today's owner-only rule); a unit's form sits on its department page. Onboarding
  writes the root's. Key rotation unchanged (credential follows the connection).
- Move: `move_model_connection(actor, company, provider, target)` through the p01 move protocol
  (manage both units, company + person locks, row `FOR UPDATE`); a moved default stops being a
  default (`default_for_unit=false`), and the root's "exactly one default" invariant refuses moving
  the root's only default away.
- Reach (README decision 1 with d): an agent's pinned provider connection must be in the agent's
  reach, checked with the skills/tools at agent publication and admission (`check_agent_reach_on`
  gains the model connection) and model offers in agent authoring are filtered by agent reach. A
  model call already admitted is not re-judged.

**D2 Default cascade at agent creation (02A.2).** One SQL resolver used inside the creating
transaction for every company creation path: start at the placement department (root when
unplaced or when the placement is deferred), walk ancestors (≤8), take the first unit holding a
`default_for_unit` connection with a key, pin its first model; none → existing `no_usable_model()`
message. Library copy and onboarding stop pinning in the application layer before the transaction
(`prepare_library_write` no longer calls `pinned_default_model`); they leave the model empty and
`resolve_model_on` pins with the placement (onboarding agents are unplaced → root). Agent Builder
keeps requiring an explicit model (unchanged), but its model must be in reach of the spec's
department (D1 reach).

**D3 Frozen pins (02A.3).** No change: an agent's model is pinned at creation/edit; changing a
unit default affects later creations only. The doc's "later publications" means the same.

**D4 Authority surface.** All model connection reads for configuration (`list_model_connections`)
follow unit management (a manager sees the units they manage); authoring reads follow agent reach.

**D5 Usage attribution (02B.3).** `model_usage_calls` and `model_usage_history` gain `unit_id uuid`
with composite FK `(company_id, unit_id)` to departments `ON DELETE SET NULL (unit_id)` (a deleted
department's past usage keeps its rows but drops out of every subtree sum); written at reserve time
from the run's base write unit (`workflow_run_scopes` row with `acting_principal_id IS NULL AND
access='write'`), the root when absent; JEV reserve the same; archive copies it. Index
`(company_id, unit_id, requested_at)` on both tables. Per-agent scope sets are not used for
attribution (the doc's "run's write unit"; a desk-less agent run attributes to the root).

**D6 Budgets (02B.1, decisions c, i).** `unit_budgets (company_id, unit_id, period text CHECK
(period IN ('day','month')), cap_micros bigint CHECK (cap_micros > 0), created_by jsonb provenance,
created_at, updated_at)`, PK `(company_id, unit_id, period)`, FK to departments `ON DELETE CASCADE`
(a budget is configuration of its unit, not a resource to move). Set/replace/delete by whoever
manages that unit (`unit_management_role` on the unit itself: its lead or anyone above). Period
start: `date_trunc('day'|'month', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'`.

**D7 Admission check (02B.2, decision h).** In `admission_write::create`, after the run scopes are
captured (still inside the admission transaction holding the company lock): W = base write unit or
root; for each ancestor A of W (W included, ≤8) with budgets, spent(A, period) = Σ over live and
history usage rows with `unit_id` in subtree(A) and `requested_at >= period_start` of
`input×input_price + output×output_price` per million (settled tokens, else reserved), priced rows
only (unpriced = 0). If spent ≥ cap → `AppError::BudgetExhausted { unit, name, period }` with code
`budget.unit_exhausted`, naming the first exhausted unit nearest W. Root runs only (child runs and
replays are not re-checked; an admitted run is never stopped by a cap). Plumbing as p01's refusal:
HTTP 400 `{code, error, unit}`, message/schedule `classify` → Refused(code), schedule refusal CHECK
admits the code, author-facing renderers show it.

**D8 Concurrency contract (unit_budget_competing_).** Admissions serialize on the company lock;
usage reserves do not take it. The contract is: an admission sees every usage row committed
before its check; it does not reserve spend. So two admissions racing while spent ≥ cap are both
refused; with spent < cap both may be admitted (the overrun is bounded by runs already admitted,
like the root budget's "not stopped mid-flight"). The doc's "admit at most what the cap allows" is
read as "never admit once committed spend reaches the cap"; reserving estimated spend at admission
is rejected (no ledger, decision h's zero-priced unknowns). Competing tests: two admissions at the
cap (both refused); a usage reserve committing vs an admission (admission refused iff it ordered
after the commit).

**D9 Usage pages (02C, decision h).** The usage report groups by the recorded `unit_id` (replacing
the desk's current department), shows per unit: spend, unpriced tokens, and for each budget the
cap and remaining for the current UTC period; drill-down lists the unit's desks' runs for the
period. Authorization: root managers see all units; a unit manager sees their managed subtrees
(the page entry is `any_unit_management_role`; rows filtered by `unit_reaches(managed, unit)`);
the read no longer takes the company `FOR NO KEY UPDATE` lock. The dashboard gains a spend-by-unit
panel under the same rule.

### Acceptance mapping (planned, r1)
| Criterion | Production path and case | Decisive assertion | Check |
| --- | --- | --- | --- |
| D1 schema | fresh schema | FK refuses foreign unit; second default in a unit refused; root default required on replace | `unit_model_default_schema_*` |
| D1 authority/move | replace as root admin, unit lead (own unit / sibling), member; move | managed → saved; unmanaged → 404; provider of another unit → Conflict; root's only default not movable | `unit_model_default_authority_*` |
| D1 reach | agent pinned to a sibling unit's provider; moved up | publication/admission refused / admitted | `unit_model_default_reach_*` |
| D2 cascade | create in child with no default (parent has), in child with its own, unplaced, none anywhere; library copy and onboarding | pins nearest; root; refusal message; library/onboarding by placement | `unit_model_default_cascade_*` |
| D3 frozen | change a unit default after creation | existing agent's model unchanged | `unit_model_default_frozen_*` |
| D5 attribution | desk run, desk-less run, archived row | unit_id = write unit / root; history keeps it | `unit_budget_attribution_*` |
| D6/D7 caps | child cap vs child and sibling; ancestor cap vs both; unpriced only | child refused, sibling admitted; both refused; unpriced not counted | `unit_budget_admission_*` |
| D7 settling | schedule and message refused by a cap | occurrence failed with the code; intent refused | `unit_budget_refusal_*` |
| D8 race | two admissions at the cap; reserve vs admission | both refused; ordered outcome | `unit_budget_competing_*` |
| D9 pages | manager of a child vs sibling vs root; numbers vs ledger | subtree only; totals equal the SQL sum | `unit_budget_usage_page_*` |

### Contract impact (planned)
| Changed contract | Producers / consumers | Disposition |
| --- | --- | --- |
| `is_default` → `default_for_unit` + `unit_id` | company.rs replace/read, provisioning ready models, agent authority/capture FOR SHARE, model_preparation, onboarding, credential rotation, settings page, schema `native_mcp_parent_agent_live` | all switched; rotation untouched |
| model pinning | create_company_agent_on, library copy, onboarding, Agent Builder | one in-transaction resolver |
| `model_usage_calls`/history `unit_id` | reserve_on, JEV reserve, archive, report, dashboard, metrics.sql, schema history-columns test | written and read; test updated for the new column |
| new refusal `budget.unit_exhausted` | admission, HTTP, classify, schedule CHECK, renderers | as p01's refusal |
| usage page authority | model_usage routes, dashboard | root → managed subtree |

## Architecture gate G1 (decision revision r2, 2026-11-21)
Answers 07UR-P02-G1-R1 (F1–F10) with user decisions (j) running totals and (k) soft caps
(coordination r9). r1 stands except where changed here.

**D1 r2 (F6).** Model connections gain `id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE` (key
stays `(company_id, provider)`, decision g); refusals, moves and uses carry it
(`UnitResourceKind::ModelConnection`). Uses listed on a move = agents whose active revision pins
that provider. Invariants, checked in every write path under the company lock (person locks via
`authorize_person_with_locks`, which serializes replaces and moves company-wide): the company keeps
≥1 connection overall (today's rule, now company-wide across units); the root holds exactly one
default whenever it holds a connection, a non-root unit at most one. A connection moved into the
root while the root has no default becomes its default; moving the root's default away is refused
while the root keeps another connection; a moved connection otherwise lands non-default. Per-unit
replace: rows of other units are untouched; adding a provider owned by another unit is a Conflict
naming that unit (the upsert's `ON CONFLICT … DO UPDATE … WHERE unit_id = EXCLUDED.unit_id` must
affect the row, else Conflict), so a key is never overwritten across units. Competing test: two
sibling leads add the same provider at once → one saved, one Conflict, the saved key intact.

**D2 r2 (F7).** The resolver starts at the placement's department for `Complete` and `Deferred`
placements (Deferred carries it), the root for `Unplaced`. Library copy: an explicit library model
whose connection is outside the placement's reach falls back to the cascade (as an absent model),
so publication never fails on it.

**D5 r2 (F1, decision j).** `model_usage_calls.unit_id` and `model_usage_history.unit_id` are
plain recorded ids (`uuid NOT NULL`, no FK): the attribution of the call (base write unit at
reserve; root when absent); deleting a department leaves them dangling (the page shows "deleted
department"), never blocks the delete and never updates usage rows. Caps do not read usage rows.

**D6 r2.** Unchanged table `unit_budgets` (caps). New running totals `unit_spend (company_id,
unit_id, period text CHECK (period IN ('day','month')), period_start timestamptz, spent_micros
bigint NOT NULL DEFAULT 0 CHECK (spent_micros >= 0), unpriced_tokens bigint NOT NULL DEFAULT 0,
updated_at)`, PK `(company_id, unit_id, period, period_start)`, FK unit `ON DELETE CASCADE`.

**D7 r2 (F3, F4, F10, decision j).** Charging: one schema trigger `AFTER UPDATE OF settled_at ON
model_usage_calls` (fires once per call, when `OLD.settled_at IS NULL AND NEW.settled_at IS NOT
NULL`; covers both settle sites, agent and JEV). cost = ceil((input × input_price + output ×
output_price) / 1e6) micro-USD computed in `numeric` with the price row current at settlement,
saturated to the bigint range; unpriced → cost 0 and tokens added to `unpriced_tokens`. Periods
from `NEW.requested_at` (UTC day and month containing it). The trigger upserts the call's unit and
each current ancestor (≤8, ordered by unit id to keep lock order) for both periods. Later price
edits do not change counted spend; reserved-but-never-settled calls are never counted (recorded
limit). Prompt generation (authoring, no run) and per-run root budgets are outside caps.
Admission check: after scopes are captured, W = base write unit or root; read `unit_budgets` ⋈
`unit_spend` for W's chain (≤8 units × 2 periods, PK lookups) with period starts from
`clock_timestamp()` at check time; refuse with `budget.unit_exhausted` (unit, name, period) at the
first unit from W upward with spent ≥ cap (day before month at the same unit). Bounded by depth,
not volume.

**D8 r2 (F2, decision k).** Soft caps: admissions serialize on the company lock, settlement does
not take it; an admission refuses iff its check (after the lock) sees committed spend ≥ cap.
Competing tests: (1) an admission blocked on the company lock while a settlement crossing the cap
commits → refused after the lock is released; (2) two settlements on the same unit and period
concurrently → the total equals the sum (no lost update); (3) settlement vs department delete.

**D9 r2 (F5, F10).** Report authority is separate from price writes: price saves keep root
management; the report/page entry needs `any_unit_management_role`, and the SQL filters calls to
units inside a managed subtree (`unit_reaches(company, call.unit_id, managed_unit)`, i.e. the
managed unit is the call's unit or an ancestor) **before** the 10,000-call limit; root managers
see all. Per-unit cap/remaining on the page come from `unit_spend` (exactly what admission
enforces); the call breakdown stays the ledger. Drill-down groups the unit's recorded usage by the
run's desk. The read drops the company `FOR NO KEY UPDATE` lock (plain person check). Dashboard
spend-by-unit panel reads `unit_spend` under the same rule.

**Contract impact additions (F8).** Agent form: the "Company default" option becomes "Default for
its department" (pinned by the cascade at creation) and the "(default)" marker marks the root's
default. Agent Builder directory model list shows each model's unit; validation requires reach of
the spec's department. `inbound_event_worker::classify_commit_error` and message/schedule
`classify` treat `BudgetExhausted` like `ResourceOutsideUnit`. Schema history-columns test gains
`unit_id`. metrics.sql and dashboard `TOKENS_SQL` unchanged (tokens), the new panel reads totals.

### Acceptance mapping additions (r2, F9)
| Criterion | Case | Decisive assertion | Check |
| --- | --- | --- | --- |
| decision c | set/replace a cap as the unit's lead, an ancestor lead, a child lead, a member | own/ancestor → saved; child lead/member → 404 | `unit_budget_authority_*` |
| nearest unit | caps on child and root both exhausted | refusal names the child | `unit_budget_admission_*` |
| day+month | unit with both caps, only month exhausted | refused naming month | `unit_budget_admission_*` |
| charging | settle a priced and an unpriced call; JEV call; child run | totals on unit and ancestors; unpriced tokens flagged; JEV/child attribute to the run's write unit | `unit_budget_charge_*` |
| unsettled | reserved, unsettled call | not counted | `unit_budget_charge_*` |
| rollover | call requested before 00:00 UTC settled after | charged to the previous day | `unit_budget_charge_*` |
| delete/move | delete a charged child; re-parent a unit | ancestor totals unchanged; later charges follow the new chain | `unit_budget_charge_*` |
| D8 | the three competing cases above | as stated | `unit_budget_competing_*` |
| D1 race | two leads add one provider | one saved, one Conflict, key intact | `unit_model_default_competing_*` |
| D9 page | lead of child, sibling, root; cap/remaining equal `unit_spend`; unpriced flag; drill-down; price save as a lead | subtree only; equal; flagged; by desk; 404 | `unit_budget_usage_page_*` |

## Architecture gate G1 (decision revision r3, 2026-11-21)
Answers 07UR-P02-G1-R2 (F11, F12; optional F13 partly adopted). r2 stands except where changed.

**D7 r3 charging protocol (F11, F12).** The charge runs in a `CONSTRAINT TRIGGER … AFTER UPDATE OF
settled_at ON model_usage_calls DEFERRABLE INITIALLY DEFERRED` (same once-per-call condition), so
the totals rows and department locks are held only during the settling transaction's commit.
Steps, in this order: (1) resolve the chain from `NEW.unit_id` upward (≤8) and lock those
`departments` rows `FOR KEY SHARE` ordered by id; (2) if `NEW.unit_id` no longer exists, the
chain is the root alone (rule: spend of a deleted unit is charged to the root, so no root cap is
escaped; intermediate ancestors of a deleted unit are not known and are not charged — recorded
limit); (3) upsert the chain's `unit_spend` rows ordered by `(unit_id, period)` with a saturating
sum `LEAST(spent_micros + cost, 9223372036854775807)`. Ordering: a department delete takes the
row `FOR UPDATE` before cascading into `unit_spend`, so it either waits for a committing charge
(which then holds `KEY SHARE` and its totals) or commits first (the charge then sees the unit gone
and charges the root); neither order fails the settlement. Each settlement transaction settles
exactly one call (agent checkpoint and JEV settle one request each), so the per-call ordering is
the global ordering for the totals rows. Hot rows: the root's day and month rows serialize charges
only for the commit's upsert; recorded as a known throughput limit with a stress test (below), no
metric is added.

**Competing tests r3 (F11, F12).** (3a) a charge committing while a delete of its unit is
attempted → the delete waits, then succeeds; the ancestor totals include the call. (3b) delete
commits first, then the settlement commits → settlement succeeds, the call is charged to the root.
(4) 16 concurrent settlements across sibling units of one company → all commit, the root's totals
equal the sum, no deadlock.

**F13 (optional) disposition.** Adopted: saturating sum (above); the report computes the managed
unit ids once and filters `unit_id = ANY(managed_subtree_ids)` before the limit; the page labels
running-total figures ("counted toward caps") apart from ledger figures. Not adopted: `unit_spend`
retention (rows are per unit per day; left for a later retention pass, recorded), flagging
reserved-never-settled calls, and a warning on a library copy's reach fallback (the pinned model
is shown on the agent page).

## Architecture gate G1 (decision revision r4, 2026-11-21)
Answers 07UR-P02-G1-R3 (F14, F15). r3 stands except where changed.

**D6/D7 r4 (F14): no department locks, no totals FK.** `unit_spend` keeps `company_id` → companies
`ON DELETE CASCADE` but has **no FK to departments**; the charge takes **no lock on `departments`**,
so department writes (members, roles, edits, placement) never wait on or delay a settlement. A
schema trigger `AFTER DELETE ON departments` deletes that unit's `unit_spend` rows (covers direct
and company-cascade deletes; ancestors keep what was counted). The deferred charge (still
`CONSTRAINT TRIGGER … DEFERRABLE INITIALLY DEFERRED`, once per call) resolves the chain from
`NEW.unit_id` in committed `departments` rows at commit time; a unit not found charges the root
(r3 rule). Races, all non-failing: delete commits before the charge → the root is charged (and no
row for the deleted unit is created, since it is not found); charge commits before the delete →
the delete's trigger removes the unit's rows, ancestors keep the call; a delete racing the
charge's commit in between can leave one orphan `unit_spend` row of a deleted unit — never read
(no budget can reference a deleted unit, pages join live departments) and recorded as a cleanup
item for the deferred retention pass; a re-parent racing a commit charges either the old or the
new chain. Totals upserts stay ordered by `(unit_id, period)`.

**F15.** The sum is computed in numeric: `LEAST(spent_micros::numeric + cost,
9223372036854775807)::bigint` (and `cost` itself saturated the same way).

**F12 signal.** The settling operation's existing `commit_ms` log and time-bound warning
(`persistence_bound.rs:383-433`) are the contention signal; nothing new is added.

**Tests r4.** (3a) charge commits, then the unit is deleted → ancestor totals include the call,
the unit's rows are gone; (3b) delete commits, then the settlement commits → succeeds, the root is
charged; (4) 16 concurrent sibling settlements → all commit, root totals equal the sum; (5) a
department row held `FOR UPDATE` (a member change in progress) while a settlement in its subtree
commits → the settlement does not wait.

## Architecture gate G1 (decision revision r5, 2026-11-21)
Answers 07UR-P02-G1-R4 (F16). r4 stands except where changed.

**F16.** One lock order for `unit_spend` rows everywhere: `(unit_id, period, period_start)`
(`'day'` sorts before `'month'`). The charge upserts in that order; the `AFTER DELETE ON
departments` trigger first locks the unit's rows `SELECT … ORDER BY period, period_start FOR
UPDATE`, then deletes them. Test (6): a charge holding the unit's month row, the delete queued, and
a second settlement holding the day row → no deadlock, all three complete. Wording corrected: a
department write other than deleting the call's own unit never delays a settlement; an in-flight
delete of that unit delays its charge until the delete commits; the possible orphan is the deleted
unit's day and month rows (two rows).

## B1 plan (02A: D1–D4 with r2 D1/D2; on 107fd588)
1. Schema: `company_model_connections` adds `id uuid NOT NULL DEFAULT gen_random_uuid()` (UNIQUE
   `(company_id, id)`), `unit_id uuid NOT NULL` + composite FK `ON DELETE RESTRICT` + index;
   `is_default` → `default_for_unit`; index `(company_id, unit_id) WHERE default_for_unit`
   replaces `company_model_connections_one_default_idx`. SQL `unit_default_model(company, start)`
   (nearest ancestor default with a key, ≤8). Raw-SQL fixtures (9 inserts) set the root unit.
2. Entity `CompanyModelConnection` gains `id`, `unit_id`; Rust `is_default` keeps its name and
   means "default of its unit" (documented); write struct unchanged.
3. Replace per unit: `replace_model_connections_for_user(user, company, unit: Option<Uuid>,
   writes)` — person+company locks (`authorize_person_with_locks`), `unit_management_role` on the
   unit (root when None) else not found; clear/upsert/delete only that unit's rows; the upsert's
   `DO UPDATE … WHERE unit_id = EXCLUDED.unit_id` and the keyless UPDATE must hit, else Conflict
   naming the owning unit; after writes: company keeps ≥1 connection; root has exactly one default
   when it has any; stranding check unchanged. Use case drops `owned_company`; validation keeps
   bounds/duplicates and "at most one default" (root's exactly-one is persistence's).
4. Reads: `model_connections_on` returns id/unit; `list_model_connections` for configuration
   follows management (root form: root managers); authoring list filtered by agent reach (edit) or
   shown with units (create).
5. Move: `UnitResourceKind::ModelConnection` in `move_resource_on` (`company_model_connections
   WHERE company_id=$1 AND id=$2`), moved row non-default; into a root without default → default;
   refuse moving the root's default while the root keeps another connection; uses = agents pinning
   that provider. Department delete listing includes model connections.
6. Cascade: `resolve_model_on(company, start_unit)` uses `unit_default_model`; start = placement
   department for Complete/Deferred, root for Unplaced; library copy and onboarding stop pinning
   in the application layer; a library explicit model whose connection is outside the placement's
   reach falls back to the cascade; Agent Builder's explicit model must be in reach of the spec's
   department; `prompt_model` uses the root default.
7. Reach: `check_agent_model_reach_on` (pinned provider's connection unit in `agent_reaches`) at
   agent publication and admission (every use mode).
8. UI: company settings edits the root's connections and lists others with their unit; the
   department page gets the unit's model form; agent form "Company default" → "Default for its
   department", "(default)" marks the root's default.
9. Tests `unit_model_default_*` per the G1 mapping (schema, authority, competing, reach, cascade,
   frozen) and adapted existing tests.

## B1 implementation (02A, 2026-11-21; uncommitted on 107fd588)
- Schema: `company_model_connections` gains `id` (unique with company), `unit_id` (composite FK
  RESTRICT + index), `default_for_unit` (replaces `is_default`; partial unique per unit);
  `unit_default_model(company, start)` resolves the cascade (≤8). Raw-SQL fixtures set the root.
- Persistence (company.rs): one `replace_unit_model_connections` behind the root replace
  (`replace_model_connections_for_user`) and the new `replace_unit_model_connections_for_user`;
  person+company locks, `unit_management_role` on the unit, unit-conditional upsert/update,
  Conflict naming the owning department, delete only the unit's omitted providers, then
  `check_model_connection_invariants_on` and the stranding check. **Refinement of D1 r2 (for
  review):** the root must always hold exactly one default (not only when it holds a connection),
  so the company default always exists for unplaced agents; a company with no connections yet
  (creation without providers) is untouched until its first write. **Reverted after the full
  suite** (and F2 then restores the gate's "company keeps ≥1 connection" for the forms, see
  B1-R1 corrections): three tests clear every connection through persistence (modelling a company with no
  hosted model), which persistence always allowed; the invariant is the gate's rule (the root
  holds exactly one default whenever it holds a connection), the root form's use case still
  refuses an empty replace, and moving the root's only connection away is allowed (the root then
  has no default; unplaced creations refuse with the existing message). `model_connections_on` returns
  id/unit, root default first; `model_connections_in_agent_reach`; `move_model_connection` via
  `move_resource_on` with `UnitResourceKind::ModelConnection` (moved row non-default unless the
  root has none; invariants after; uses = active agents pinned to the provider); department
  delete listing names model connections.
- Use cases: `replace_model_connections` (root; persistence authorizes — widened from owner to
  root managers), `replace_unit_model_connections` (bounds, ≤1 default),
  `list_model_connections` (managed units), `list_unit_model_connections` (one unit, root when
  None), `move_model_connection`; agent authoring `list_company_model_connections(.., agent)` filters
  by agent reach for an existing agent.
- Cascade: `resolve_model_on(.., department)` uses `unit_default_model` from the placement's
  department (Complete and Deferred) or the root; explicit models must be in the department's
  reach (`model_in_reach_on`); library copies drop an out-of-reach library model in
  `prepare_library_agent_on` (persistence, with the unit) and no longer pin in the application
  layer; Agent Builder refuses an out-of-reach explicit model as an Invalid provisioning refusal.
- Reach: `check_agent_model_reach_on` at agent publication and admission (every use mode).
- UI: settings form edits the root's own connections (`list_unit_model_connections(None)`); the
  department page shows a "Model connections" card for its managers (shared form renderer
  `unit_model_connection_fields` and parser `submitted_model_connections`), POST
  `/ui/companies/{c}/departments/{d}/models`; agent form option "Default for its department", the
  "(default)" marker only on the root's default. Move: JSON `POST /api/companies/{c}/
  model-connections/{id}/move` (no UI control; recorded).
- Tests: `unit_model_default_cascade_pins_the_nearest_default_and_stays_frozen`,
  `unit_model_default_authority_moves_and_department_delete`,
  `unit_model_default_reach_refuses_publication_after_a_move`,
  `unit_model_default_competing_units_add_one_provider` — 4 passed (fresh openbot_p07_p02b1).
  Full lib suite (logs/B1-lib-suite.txt, before the revert): 4922 passed, 5 failed = the three
  clearing tests (fixed by the revert), the mock library use-case test (the double does not pin;
  updated to assert the app layer leaves the copy unpinned — pinning is persistence's, now
  covered by a library copy in the cascade test), and the deferred Doop 8 vs 7. After fixes:
  `unit_model_default` + those tests 8 passed; clippy -D warnings clean; sqlx prepare --check
  clean (fresh openbot_p07_p02b1_sqlx); fresh schema apply exit 0. Focused rerun of touched areas
  (logs/B1-focused.txt: company agent department ui_ onboarding model library provisioning
  credential pages unit_resource): 1743 passed, 1 failed = the deferred Doop.
- Frozen: base/HEAD 107fd588 + working tree, digest
  bbfb5e1c2cbba8700411b000a30a30bdc40506c4cd22830826b721b4558c03b1 (exclusions unchanged,
  logs/B1-author-freeze.snapshot.txt); manifest logs/07UR-P02-B1-R1.evidence.json.

## B1-R1 corrections (for recheck 07UR-P02-B1-R2, 2026-11-21)
| Finding | Change | Test |
| --- | --- | --- |
| F1 missing cases | added the planned decisive cases | `unit_model_default_admission_refuses_a_model_moved_out_of_reach` (admission refused naming the connection; placed / moved back → admitted), `unit_model_default_moves_and_an_empty_root` (move needs both units; root's only connection may leave; unplaced creation then refused "Add a model connection"; moved into a default-less root it becomes the company default and unplaced agents pin it), `unit_model_default_root_managers_own_defaults_offers_and_library_fallback` (root admin and root lead replace the root, plain member 404; a department's own default beats its parent's; existing-agent offers limited to reach; a library copy on Sales's model placed in Support pins Support's default), `unit_model_default_agent_builder_model_follows_the_placement_reach` (Invalid outside, provisioned inside), `unit_model_default_department_card_saves_for_managers_only` (route: card for managers, saved; conflict inline; member no card, 404), `unit_model_default_a_department_form_keeps_the_last_connection` |
| F2 ≥1 connection | the root's department form is held to the settings form's rules (non-empty, exactly one default) in the use case; persistence refuses a department-path replace that would leave the company with no connection; the root path keeps today's behaviour (fixtures clear it; the use case refuses empty). TASK wording corrected | `unit_model_default_a_department_form_keeps_the_last_connection` |
| F3 error collapse | only `NotFound` from the lock/authority step maps to company not-found; other errors propagate | by inspection |
| F4 company-wide consumers | `CompanyModelConnection` gains `unit_name`, `in_root`; the agent form labels a department's provider with its department and marks "(default)" only on the root's default; `prompt_model` uses the root's default only; Builder directory lists each model's `department`; library warnings say "its department's default" | page tests (updated fixtures); directory by inspection |
| F4 follow-up | the Builder directory's `department` field required in the `list_company_agents` output schema (provisioning_schemas.rs); the focused rerun before this caught 7 directory/Builder tests failing output validation (logs/B1-R2-focused.txt; its 8th failure is the deferred Doop); fixed and their assertions updated | `department_directory`, `provisioning`, `builder_` 73 passed with `unit_model_default` (logs/B1-R2-new-and-fixed.txt) |
| F5 (optional) | not adopted: move uses keep matching the draft provider (recorded) | — |
| F6 (optional) | not adopted: the Builder checks the spec's named department, stricter than the placement (recorded) | — |

- B1-R2 frozen: base/HEAD 107fd588 + working tree, digest
  5a938fa0571d0863f5c823653901ca94f59b6cb7effd63222ca4ebd317494359
  (logs/B1-R2-author-freeze.snapshot.txt); manifest logs/07UR-P02-B1-R2.evidence.json; clippy clean.

## Status
- p02 started 2026-11-21. Investigation recorded; user decisions (g)(h)(i) taken; G1 r1 reviewed
  (07UR-P02-G1-R1 changes required, F1–F9 actionable, F10 optional); decisions (j)(k) taken; G1 r2
  written; recheck 07UR-P02-G1-R2: F1–F10 resolved, new F11 (medium), F12 (low), F13 optional; G1 r3
  written; recheck 07UR-P02-G1-R3: F11–F13 resolved, new F14 (medium), F15 (low); G1 r4 written,
  recheck 07UR-P02-G1-R4: F14, F15 resolved, new F16 (low); G1 r5 written.
- **G1 r5 PASS** (07UR-P02-G1-R5, digest 67d600bc…, coordination r9). Recorded limits: spend of a
  deleted unit settling after the delete goes to the root only; reserved-never-settled calls not
  counted; orphan rows and `unit_spend` retention left for a later pass; F13 items not adopted;
  F6's optional naming of the owning unit. A gate pass accepts the plan, not its execution: the
  real settlement transaction and bounds, JEV and production-size plans are B1–B3's tests.
- B1 (02A: D1–D4 with r2 D1/D2) started.
- **B1 accepted in phase** 2026-11-21: B (Opus 5.5/xhigh) PASS 07UR-P02-B1-R2 at
  5a938fa0571d0863f5c823653901ca94f59b6cb7effd63222ca4ebd317494359 (rounds B1-R1 changes required
  F1–F4 + optional F5/F6, B1-R2 PASS); root re-digest identical
  (logs/B1-root-acceptance.snapshot.txt); coordination r9. Optional 07UR-P02-B1-F7 (tests that
  discriminate the department label, the root-only "(default)" marker and `prompt_model` with no
  root default) carried to p03. trail check: 5 keyword matches on the p01 unit-FK comment block
  (definition-creation authority, cascade ordering, a moved document's cached facts, company-list
  sizing, agent-tool edges) — not those mechanisms. Committed locally. B2 (02B) next.

