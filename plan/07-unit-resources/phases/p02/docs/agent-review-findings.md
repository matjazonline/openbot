# 07UR-P02 agent review findings

Common context for the 07UR-P02-G1-R1 findings below: 2026-11-21, workflow
implement-opus-reviewed-2_4, task [TASK.md](TASK.md) "Architecture gate G1 (decision revision r1,
2026-11-21)" (D1–D9, planned acceptance mapping and contract impact), review
[REVIEW.md](REVIEW.md), round note
[07UR-P02-G1-R1](agent-review-notes.md#07ur-p02-g1-r1-2026-11-21); stage architecture gate G1,
decision revision r1 (before batches B1 = 02A, B2 = 02B, B3 = 02C); reviewer B Claude Opus 5.5
(claude-opus-5-5, effort as set by root); phase 07UR-P02; worktree
/Users/mac02/orca/workspaces/openbot/plan06 (plan07); HEAD 107fd588, digest
67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1 (exclusions
plan/07-unit-resources/phases/p02/docs, plan/07-unit-resources/COORDINATION.md). All findings
originate in the gated decisions (no implementation exists). Line references are at 107fd588.

## 07UR-P02-G1-F1 — D5's `ON DELETE SET NULL (unit_id)` collides with the usage immutability guards; deleting a department with usage fails, and the stated semantics let a deletion lower an ancestor's spend

- **Classification:** actionable defect; **high**; architecture/correctness (schema, deletion
  semantics, cap integrity).
- **Evidence:**
  - D5 adds `unit_id` to `model_usage_calls` and `model_usage_history` with a composite FK to
    departments `ON DELETE SET NULL (unit_id)`. A referential `SET NULL` is executed as an
    `UPDATE` of the referencing rows, which fires their row triggers.
  - `model_usage_calls` has `model_usage_immutable` (init_schema.sql:11499-11510): any change
    outside `input_tokens, output_tokens, usage_source, settled_at` raises 23514 "model usage
    identity and settled usage are immutable". `unit_id` is not in that list (and must not be).
  - `model_usage_history` has `model_usage_history_owned` (init_schema.sql:12097-12113): every
    `UPDATE` raises 23514 "model usage history is immutable".
  - Reproduced on PG 18.7 with the same guard shape: deleting the referenced department fails
    with the guard's error raised from `UPDATE ONLY "public"."usage_calls" SET "unit_id" = NULL`
    and the department survives —
    [logs/07UR-P02-G1-R1-fk-set-null-vs-immutable-guard.txt](logs/07UR-P02-G1-R1-fk-set-null-vs-immutable-guard.txt).
  - So after D5, `delete_department_on` (department/lifecycle.rs:284-330) fails for any
    department that ever had a run attributed to it; `delete_error` (lifecycle.rs:333-342) maps
    only FK violations to Conflict, so the 23514 surfaces as a database error rather than the
    guided refusals `guard_department_delete_on` gives today. Departments with past runs are
    deletable today (`workflow_run_scopes.unit_id` has no FK, init_schema.sql:20367-20390).
  - Even with the guard conflict removed, the stated semantics ("a deleted department's past
    usage ... drops out of every subtree sum") let a unit manager lower an ancestor's committed
    spend mid-period: a parent lead can delete an emptied child department and its spend leaves
    the parent's and the root's sums, evading a root cap the lead cannot change (decision c).
    Departments can also be re-parented (lifecycle.rs:170), which moves a unit's past spend
    between subtrees because D7 computes subtrees from the current tree; D5/D7 do not state that
    either behavior is intended.
  - The planned mapping has no department-deletion or company-teardown case with attributed
    usage (company teardown removes departments after tenant cascades,
    `delete_company_tree` init_schema.sql:20097-20109, so it is probably unaffected, but untested).
- **Required:** choose an attribution/deletion semantics compatible with immutable usage rows
  (for example: `unit_id` as an FK-less snapshot like `workflow_run_scopes.unit_id`; or record the
  unit's ancestor chain at call time so sums do not depend on the current tree; or RESTRICT with
  a guided refusal, accepting that departments with usage become undeletable) and state how a
  deleted or re-parented unit's period spend counts against ancestors. Add a decisive case:
  delete a department with attributed live and archived usage, then admit under an ancestor cap.
- **Instruction lesson:** observation: the decision added an FK action to tables guarded by
  immutability triggers without reading the guards. Candidate (persistence AGENTS, "Preserve
  invariants"): "Before adding `ON DELETE SET NULL/CASCADE` to a table, check its row triggers;
  referential actions run as UPDATE/DELETE and fire them."
- **Resolution history:** 2026-11-21 open (G1 r1).
  - 2026-11-21 recheck 07UR-P02-G1-R2 (decision revision r2, same digest): **fixed in decision**. D5 r2 keeps usage `unit_id` as a plain recorded id without an FK, so a department delete never updates usage rows. Caps read the running totals of user decision (j), under which deletes and moves keep spend already counted above the unit. The mapping adds the delete/move case. The totals table's own FK interacts with settlement; that is new finding 07UR-P02-G1-F11.

## 07UR-P02-G1-F2 — D8 rewrites the `unit_budget_competing_` requirement without a user decision, and its competing tests do not discriminate

- **Classification:** actionable defect; **medium**; requirements/concurrency/verification.
- **Evidence:**
  - Doc 02 focused check: "`unit_budget_competing_`: two admissions racing the last of a cap
    admit at most what the cap allows, under the company lock." D8 reads this as "never admit
    once committed spend reaches the cap" and states that with spent < cap both racing
    admissions may be admitted. That is a different requirement: "the last of a cap" implies a
    positive remainder that the racing admissions compete for. The coordination file's user
    decisions (a)–(i) do not cover it; (h) only fixes unpriced usage at zero.
  - Consequence of D8's contract: a burst of root admissions at the start of a UTC period (for
    example many schedules firing at 00:00) is admitted in full against a day cap, since an
    admission adds no spend and usage rows appear only once calls reserve. The overrun is bounded
    only by each run's own root budget (`workflow_root_budgets`, admission_write.rs:107-127), not
    by the unit cap. Under D8 the company lock contributes nothing to the cap (admissions do not
    change spend), so "under the company lock" in the doc is not explained by D8.
  - Planned test "two admissions at the cap (both refused)" passes identically when run
    sequentially; it cannot fail for any concurrency defect. "A usage reserve committing vs an
    admission (admission refused iff it ordered after the commit)" leaves "ordered" undefined.
    The discriminating case for D8's actual contract is: admission A blocked on the company row
    lock (`authorize_person_with_locks`, authority.rs:81-87) while a reserve that crosses the cap
    commits; A must then be refused. That fails if the spend is read before the lock or from an
    earlier snapshot (the admission transaction is READ COMMITTED, admission.rs:23; that must stay).
  - Root AGENTS.md requires a competing-claimant test that exercises the protocol, not
    sequential mocks.
- **Required:** put the reading of "admit at most what the cap allows" to the user (D8 as
  written, versus a bounded admission-time estimate drawn from existing data, for example
  in-flight root runs' remaining root budget). Record the outcome in coordination. Then specify
  decisive competing cases for the chosen contract: the blocked-admission case above, and the
  spent < cap race asserting the chosen outcome (both admitted under D8).
- **Instruction lesson:** a plan-text reinterpretation that weakens a stated check is a user
  decision, not an author reading. The skill already says "Never weaken criteria". Candidate
  (workflow): "When a gate decision re-reads a focused check's wording, mark it as a question for
  the user in TASK before the gate."
- **Resolution history:** 2026-11-21 open (G1 r1).
  - 2026-11-21 recheck 07UR-P02-G1-R2: **fixed**. The user decided soft caps (k). D8 r2 test (1), an admission blocked on the company lock while a settlement crossing the cap commits, discriminates the contract. Test (2) covers lost updates. Test (3) has no stated outcome (07UR-P02-G1-F11).

## 07UR-P02-G1-F3 — D7's "bounded read" is not bounded: per-admission aggregation grows with a period's usage, under the lock that serializes every admission of the company

- **Classification:** actionable defect; **medium**; architecture/performance.
- **Evidence:**
  - D7 sums both usage tables for every budgeted ancestor of W over the current day or month,
    joined to `model_prices`. For a root budget the subtree is the whole company, so each
    admission reads every usage row of the month, plus one pass per further budgeted ancestor
    and period. That is linear in usage, not bounded. Doc 02B.2 requires "a bounded read".
  - The read runs inside the admission transaction, after `FOR NO KEY UPDATE` on the company row
    (authority.rs:81-87), so it lengthens the critical section that serializes all admissions of
    the company. Claimed (message/schedule) admissions run with `lock_timeout 5s` and
    `statement_timeout 20s` (admission.rs:25-31), so growth surfaces as admission timeouts for
    every desk of the company rather than as a slow page.
  - persistence AGENTS ("Change performance only from production evidence"): "A bounded result
    set does not imply bounded database work". A deferred optimization needs a named duration or
    working-set metric or a threshold log. D5's new indexes are proposed without the EXPLAIN
    evidence that section asks for. The planned mapping has no performance or plan evidence.
- **Required:** state the actual cost bound, or design one. For example: skip the aggregation
  when no unit on W's chain has a budget; aggregate once over the highest budgeted ancestor's
  subtree for the longest active period, with per-(ancestor, period) `FILTER`s instead of a pass
  per ancestor; record representative `EXPLAIN (ANALYZE, BUFFERS)` for the chosen index; add a
  duration metric or threshold log for the admission budget read.
- **Instruction lesson:** cause: the doc's "bounded" was carried into the decision without a
  bound. Candidate: "A gate decision that claims a bounded read names the bound (rows, pages or
  time) and the metric that shows it being approached" (root AGENTS "A raised limit must leave
  something that fails" generalizes to this).
- **Resolution history:** 2026-11-21 open (G1 r1).
  - 2026-11-21 recheck 07UR-P02-G1-R2: **fixed**. The admission read is at most 8 units × 2 periods of PK lookups on `unit_spend` and `unit_budgets`, bounded by depth. The aggregation cost moves to settlement (07UR-P02-G1-F12).

## 07UR-P02-G1-F4 — The spend definition is underspecified: integer overflow, rounding, current-price semantics, and a page that cannot equal the admission sum

- **Classification:** actionable defect; **medium**; correctness/verification.
- **Evidence:**
  - D7 gives "input×input_price + output×output_price per million" with no type. Within the
    table bounds (tokens ≤ 2^31-1, init_schema.sql:11473-11476; prices ≤ 10^12,
    init_schema.sql:11517-11518), a `bigint` product overflows: `SELECT
    2147483647::bigint*1000000000000::bigint` raises "bigint out of range"
    ([logs/07UR-P02-G1-R1-period-start.txt](logs/07UR-P02-G1-R1-period-start.txt)). One such row
    would make every admission of the company fail. metrics.sql:51-57 already casts to `numeric`.
  - Rounding is unstated. The page aggregates exactly in Rust (`u128` micro-USD×tokens,
    aggregate.rs:93-104, rendered at 10^-12 USD), while a per-row "per million" division in SQL
    rounds. D9's "totals equal the SQL sum" and doc 02's "the usage page numbers match the
    ledger" need one definition. persistence AGENTS: a decision implemented in both Rust and SQL
    needs one generated definition or an equivalence matrix.
  - The page scans at most `MAX_CALLS = 10_000` calls company-wide before grouping (report.rs:12,
    71-72, 151-176). For a busy month, a per-unit "spend" and "remaining" taken from that sample
    disagree with what admission enforces. D9 does not say which source the cap/remaining figures
    use.
  - Prices are read at check time (`model_prices` holds only the current price,
    init_schema.sql:11513-11521), so a price edit changes the spend already counted in the
    period, and can turn an admitted state into an exhausted one or back. Probably acceptable,
    but unstated.
- **Required:** define spend once in SQL (numeric; exact comparison such as `Σ(tokens×price) ≥
  cap_micros × 10^6`, or a stated rounding), used by both admission and the page's
  cap/remaining figures, independent of the report's 10,000-call sample; state the current-price
  semantics; add a case with a price change and with more than 10,000 calls (or the sample bound
  lowered in a test) showing that page remaining equals admission's view.
- **Instruction lesson:** cause unknown (the existing report's truncation and Rust aggregation
  were not traced into D9). No instruction change proposed beyond the existing persistence AGENTS
  Rust/SQL equivalence rule.
- **Resolution history:** 2026-11-21 open (G1 r1).
  - 2026-11-21 recheck 07UR-P02-G1-R2: **fixed**. Cost is computed in numeric, rounded up per call, saturated, and priced at settlement (later price edits do not change counted spend). The page's cap/remaining figures come from `unit_spend`, which is exactly what admission enforces. Residual: the running sum itself is not stated as saturating (optional, 07UR-P02-G1-F13).

## 07UR-P02-G1-F5 — D9's subtree filter is inverted, and widening the usage route's authorization would widen price writes that decide every cap

- **Classification:** actionable defect; **medium**; security/authorization.
- **Evidence:**
  - `unit_reaches(checked_company, start, unit)` is true when `unit` is `start` or one of its
    ancestors (init_schema.sql:15850-15865). D9's "rows filtered by `unit_reaches(managed,
    unit)`" therefore selects the managed unit and its ancestors: it would show a child lead the
    root's and parent's spend and hide the child's own sub-units. Doc 02C.2 requires "a unit
    manager sees their subtree". The subtree test is `unit_reaches(company, row_unit, managed)`,
    or per row `unit_management_role(user, company, row_unit) IS NOT NULL` (Plan06 convention).
  - The usage report and price writes share one authorization: `report.rs::authorize`
    (report.rs:50-61, `authorize_company` = company `FOR NO KEY UPDATE` + root management,
    authority.rs:46-68) is called by both `usage_report` (:68) and `save_model_price` (:110). D9
    and the contract-impact row "usage page authority | model_usage routes, dashboard | root →
    managed subtree" do not say that price writes keep root management. Under decision (h)
    prices determine every cap (an unpriced model counts zero), so a unit lead able to delete or
    zero a price would switch off every unit cap for that model, including ancestor caps they do
    not control.
  - The bounded scan applies `LIMIT $4` company-wide before any grouping or filter
    (report.rs:151-161). Unless the subtree filter goes inside that scan, a unit manager's report
    is truncated by other units' usage, and `scanned_calls`/`truncated` disclose company-wide
    volume. `validate_filters` (report.rs:137-147) answers agent/workflow existence company-wide.
  - Dashboard: D9 says "under the same rule" but not what an operator viewing a company they are
    not a member of sees (ui_dashboard.rs:189-235), nor how the panel's aggregation is bounded
    on the SSE refresh path (ui_dashboard.rs:433+).
- **Required:** fix the subtree predicate; state that price writes (and the price table they
  edit) stay root-managed, with an authority case for a unit lead's price write (404); apply the
  subtree filter inside the bounded scan; state the dashboard operator and refresh behavior.
- **Instruction lesson:** observation: an argument-order slip in a shorthand formula inverted an
  authorization relation. Candidate (gate template): "Write authorization predicates in the gate
  with the function's real signature."
- **Resolution history:** 2026-11-21 open (G1 r1).
  - 2026-11-21 recheck 07UR-P02-G1-R2: **fixed**. `unit_reaches(company, call.unit_id, managed_unit)` has the right direction. Price saves keep root management. The filter is applied before the 10,000-call limit. The dashboard panel reads `unit_spend` under the same rule. Residual cost of a per-row recursive check is optional (07UR-P02-G1-F13).

## 07UR-P02-G1-F6 — D1 leaves the model kind's identity, the root-default invariant and replace concurrency undefined

- **Classification:** actionable defect; **medium**; architecture/concurrency.
- **Evidence:**
  - Identity: model connections are keyed `(company_id, provider)` with no uuid
    (init_schema.sql:3842-3854, 6403). The p01 plumbing D1 reuses is uuid-based:
    `ResourceOutsideUnit.resource: Uuid` (application/app_error.rs:176-183) and the HTTP body's
    `"resource"` (http/app_error_impl.rs:40-44), `UnitResourceKind` with wire codes
    (ports/unit_resources.rs:8-23), `move_resource_on(.., id: Uuid, ..)` and `uses_on`
    (unit_resources.rs:80-208). D1 adds the model connection to `check_agent_reach_on` and the
    move protocol without saying what the refusal and move carry for it. It also does not define
    a model connection's "uses" for the move response: agents whose active revision pins the
    provider, versus `agents.provider/model`, which the stranding check reads (company.rs:422-437).
  - Root invariant: "the root must hold exactly one default whenever it holds a connection" is
    not a database constraint, and D1 names neither its enforcement point nor its lock. A move
    *into* a root that holds no connection arrives with `default_for_unit=false` (D1: "a moved
    default stops being a default"), leaving the root with a connection and no default. The
    existing rule "A company must keep at least one model provider configured"
    (use_cases/company.rs:290-296) is not addressed for per-unit forms (a unit must be able to
    clear its own set; can the root?).
  - Concurrency: today's replace serializes through the owner check `SELECT .. FROM companies ..
    FOR UPDATE` (company.rs:337-339). Per-unit management drops that check, and D1 names no
    replacement lock. Its upsert `ON CONFLICT (company_id, provider) DO UPDATE SET api_key=..`
    (company.rs:366-372) overwrites whichever row holds the provider. Without serialization and a
    unit-conditional upsert, two leads adding the same provider concurrently let one silently
    replace the other unit's key. The delete of omitted providers (company.rs:409) and the
    default reset (company.rs:350) must also be per unit. The mapping has no competing case;
    root AGENTS requires one for a changed ownership/concurrency protocol.
  - Optional facet: "a Conflict naming that unit" tells a lead which sibling owns a provider and
    its name. Plan06 conventions answer 404 for what the caller does not manage (cf. p01
    07UR-P01-G1-F11).
- **Required:** define the model kind's refusal and move identity and its use listing; state
  where and under which lock the root-default and at-least-one rules are enforced, including a
  move into or out of the root; state the replace lock and unit-conditional writes; add
  competing (two units add one provider) and move-into-root cases.
- **Instruction lesson:** cause: reuse of the p01 protocol was asserted, not traced against a
  resource with a different key shape. Candidate: "When a gate says 'as p01', list the p01
  functions and wire fields reused and check the new resource fits their types."
- **Resolution history:** 2026-11-21 open (G1 r1).
  - 2026-11-21 recheck 07UR-P02-G1-R2: **fixed in decision**. Connections gain `id uuid UNIQUE`, carried by refusals, moves and uses (agents whose active revision pins the provider). The ≥1-overall and root-default invariants are enforced in every write path under the company lock taken by `authorize_person_with_locks`. A move into a root with no default makes it the default. Writes are unit-conditional, so another unit's row is never touched. A competing test is planned. The Conflict naming the owning unit is kept (disclosure accepted as optional).

## 07UR-P02-G1-F7 — D2: the deferred placement starts at the root, and library copies lose their fallback under reach

- **Classification:** actionable defect; **low**; requirements.
- **Evidence:**
  - D2: "start at the placement department (root when unplaced or when the placement is
    deferred)". `AgentCreationPlacement::Deferred(placement)` carries the department
    (company_agent.rs:39-52). `insert_company_agent_identity_on` inserts that membership before
    publication for Complete and Deferred alike (company_agent.rs:140-147, placement.rs:68-97).
    02A.2 says the placement department. Moot today, since the Builder (the only Deferred user,
    provisioning_builder.rs:91-99) always has an explicit model, but the resolver's rule
    contradicts the requirement.
  - Library copy: `prepare_library_write` (model_preparation.rs:26-57) falls back to the default,
    with warning `library_model_unavailable`, only when the definition's model is not enabled in
    the company. Under D1 reach, a definition whose provider is enabled but owned outside the
    placement's reach keeps its explicit model and then fails at initial publication with
    `ResourceOutsideUnit` instead of falling back to the placement's cascade default. D2 moves the
    pinning into the transaction but not this fallback decision. The warning texts still say
    "company's default".
- **Required:** use the placement department for Deferred as well, and either decide the library
  fallback in the transaction with reach or state that the copy refuses; add the case.
- **Instruction lesson:** none supported beyond tracing every enum variant a rule names.
- **Resolution history:** 2026-11-21 open (G1 r1).
  - 2026-11-21 recheck 07UR-P02-G1-R2: **fixed**. The resolver starts at the placement for Complete and Deferred placements. A library model outside the placement's reach falls back to the cascade. (A warning nuance is optional, 07UR-P02-G1-F13.)

## 07UR-P02-G1-F8 — Contract-impact table misses consumers

- **Classification:** actionable defect; **low**; verification/contract impact.
- **Evidence (consumers not dispositioned):**
  - Agent settings form: the blank choice "Company default", its help text and the
    "(default)" marker (pages/agent_settings.rs:874-903). Under D2, the blank choice pins the
    placement's cascade default, not a company default.
  - Builder `list_agents` directory: lists company-wide `ready_models_on` models
    (provisioning_directory.rs:42). D1 says model offers follow reach.
  - Prompt generation `generate_system_prompt` → `prompt_model` (use_cases/agent.rs:717-740,
    model_preparation.rs:59-91): which unit's default and key a unit lead's generation uses,
    under D1/D4 authority. That usage is outside `model_usage_calls` and so outside every cap.
    Say so.
  - `inbound_event_worker::classify_commit_error` (inbound_event_worker.rs:658-681, exhaustive)
    for the new refusal: terminal Routing versus retry once the period rolls.
    message_admission/schedule are listed; this one is not.
  - Usage price writes (F5) and the department delete guard (F1).
- **Required:** add these rows with a disposition each.
- **Instruction lesson:** the verification-contracts reference already requires literal searches
  for stored fields; `trail grep is_default` found the agent settings and directory consumers.
- **Resolution history:** 2026-11-21 open (G1 r1).
  - 2026-11-21 recheck 07UR-P02-G1-R2: **fixed**. Rows are added for the agent form, the Builder directory, inbound/message/schedule classification, the schema history-columns test, metrics/TOKENS_SQL, and prompt generation (outside caps).

## 07UR-P02-G1-F9 — The planned mapping lacks decisive cases for several requirements

- **Classification:** actionable defect; **low**; verification.
- **Evidence (from doc 02, decisions c/h/i, and D5–D9):**
  - 02B.1 and decision (c): no case for cap write authority (the unit's lead sets their own
    cap; a sibling lead and a member get 404; an ancestor lead may set it).
  - 02B.2: no assertion that the refusal names the *nearest* exhausted unit when a child and an
    ancestor are both exhausted, nor for a unit with both day and month caps.
  - D7 inputs: archived (history) usage counted in the period; unsettled reservations counted
    at reserved tokens; usage before the UTC period start not counted (decision i rollover).
  - D5: JEV reserve attribution (persistence/workflow/jev_runtime.rs:238-240) and child-run attribution through
    copied scopes.
  - 02C/(h): cap and remaining shown, unpriced tokens flagged per unit, the drill-down, and the
    dashboard panel. Only "subtree only; totals equal the SQL sum" is mapped.
  - F1/F5/F6 cases (department deletion with usage, price-write authority, replace race, move
    into the root).
- **Required:** add these cases with decisive assertions, or explicit dispositions.
- **Instruction lesson:** none new (the verification-contracts reference covers it).
- **Resolution history:** 2026-11-21 open (G1 r1).
  - 2026-11-21 recheck 07UR-P02-G1-R2: **fixed**. Mapping additions cover cap authority, the nearest unit, day+month caps, charging (priced/unpriced/JEV/child), unsettled calls, rollover, delete/move, the D8 races, the D1 race, and the page. Exception: the delete race's expected outcome (07UR-P02-G1-F11).

## 07UR-P02-G1-F10 — Smaller observations (optional)

- **Classification:** optional suggestion; **low**; correctness/records.
- **Evidence:**
  - D6's period start uses `now()`, the transaction start. An admission that waits on the
    company lock across 00:00 UTC computes the previous period. Computing it in the check
    statement (`statement_timestamp()`) avoids that. The formula itself is correct under any
    session time zone, and the three-argument `date_trunc(unit, ts, 'UTC')` is equivalent and
    clearer ([logs/07UR-P02-G1-R1-period-start.txt](logs/07UR-P02-G1-R1-period-start.txt)).
  - D9's drill-down lists "the unit's desks' runs" (current desks), while spend is attributed
    by recorded `unit_id`. After a desk move the drill-down no longer adds up to the unit's
    figure.
  - HANDOFF.md is stale (records B as Fable/medium agent a931587a5cc354000, coordination r7,
    "investigation in progress"). TASK.md cites both r7 and r8.
- **Resolution history:** 2026-11-21 open (G1 r1).
  - 2026-11-21 recheck 07UR-P02-G1-R2: **fixed**. Period starts come from `clock_timestamp()` at check time. The drill-down groups recorded usage by the run's desk. HANDOFF.md has been refreshed (it still cites coordination r8; negligible).

## 07UR-P02-G1-R2 findings

Context for F11–F13: round note
[07UR-P02-G1-R2](agent-review-notes.md#07ur-p02-g1-r2-2026-11-21), architecture gate G1 decision
revision r2 (TASK.md "Architecture gate G1 (decision revision r2, 2026-11-21)" and "Acceptance
mapping additions (r2, F9)"). Coordination revision 9 (user decisions j, k). Same HEAD 107fd588
and digest 67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1. Reviewer B Claude
Opus 5.5. All three originate in the r2 decisions.

## 07UR-P02-G1-F11 — Settlement charging vs department delete has no lock protocol or outcome; the r2 shape fails the settlement with an FK violation or a deadlock

- **Classification:** actionable defect; **medium**; concurrency/correctness.
- **Evidence:**
  - D6 r2: `unit_spend` has an FK to the unit `ON DELETE CASCADE`. D7 r2: the settlement trigger
    upserts the call's recorded unit and each current ancestor, in unit-id order, for both
    periods. D5 r2: a call's `unit_id` may already be dangling.
  - D8 r2 lists competing test (3), "settlement vs department delete", without an expected
    outcome or a protocol.
  - The delete takes the department row `FOR UPDATE` first (`lock_department_on`, then `DELETE`,
    department/lifecycle.rs:284-311) and then cascades into `unit_spend`. The settlement locks
    `unit_spend` rows first; each newly inserted row's FK check then takes `FOR KEY SHARE` on the
    department. The two paths lock in opposite orders.
  - Reproduced on PG 18.7 with the r2 table shape
    ([logs/07UR-P02-G1-R2-settlement-vs-department-delete.txt](logs/07UR-P02-G1-R2-settlement-vs-department-delete.txt)):
    - Case 1: a settlement charging a unit whose delete is in flight fails with "violates foreign
      key constraint unit_spend_company_id_unit_id_fkey" once the delete commits.
    - Case 2: a settlement that updated the unit's existing day row and then inserts its new
      month row deadlocks with the delete. The settlement is the victim.
  - Either way the model settlement transaction aborts (checkpoint_models.rs:225-245 runs the
    usage write inside it, under bounded lock/statement timeouts, persistence_bound.rs:213,452).
    The provider call has already been made, so the turn's persistence and its charge depend on
    retry behavior that r2 does not state.
  - Reachable in production: a department is deletable once it has no desks (lifecycle.rs:
    202-283). Runs whose write unit is that department can still be in flight after their desk
    moved away, or when they target an agent placed there.
  - Also unstated: what a call whose unit no longer exists charges. Under (j) there is no "unit
    and every ancestor at that moment", so as written its spend is silently dropped from every
    cap, the root's included.
- **Required:** state the trigger's lock protocol and outcome. For example: lock the chain's
  department rows `FOR KEY SHARE` in a fixed order before touching `unit_spend`, which gives the
  delete's order (departments, then `unit_spend`); skip units that are gone, or charge the root,
  as a stated rule. Alternatively drop the `unit_spend` FK and delete a unit's rows explicitly.
  Give test (3) a decisive assertion: the settlement commits, the delete commits, and the
  ancestor totals include the call.
- **Instruction lesson:** observation: an FK with a cascade on a table that a hot trigger
  writes adds a second lock order. Candidate (persistence AGENTS): "When a trigger writes rows
  referencing a parent that another path deletes, state the lock order between them and test the
  race."
- **Resolution history:** 2026-11-21 open (G1 r2).
  - 2026-11-21 recheck 07UR-P02-G1-R3 (decision revision r3, same digest): **fixed**. Reproduced against the r3 protocol on a reduced schema ([logs/07UR-P02-G1-R3-r3-charging-protocol.txt](logs/07UR-P02-G1-R3-r3-charging-protocol.txt)):
    - T1: a deferred constraint trigger with `AFTER UPDATE OF settled_at … WHEN (OLD.settled_at IS NULL AND NEW.settled_at IS NOT NULL)` charges nothing before COMMIT and charges once per qualifying row at COMMIT. A non-qualifying update charges nothing.
    - T2: the immutability guard refuses any later change to a settled row in the same transaction, so the NEW the trigger sees is the committed row.
    - T3: a failure in a deferred trigger fails only that COMMIT and rolls the settlement back. WriteBound classifies a COMMIT server error as refused/not applied, not ambiguous (persistence_bound.rs:217-225, 280-290).
    - 3a: the delete waits about 1 s for the committing charge, then succeeds; parent and root include the call.
    - 3b (the R2 deadlock shape: an existing day row and a new month row): the settlement's commit waits for the delete, then charges the root. No deadlock, no FK error.
    - 3b' (delete first): the root is charged.
    - 4: 16 concurrent settlements on sibling units all commit, and the root totals equal the sum.
    - Implementation note: step (2) must judge existence from the post-wait locked result (or a later statement), not from the chain resolved before the lock. Test 3b discriminates this.
    - New: the chain-wide KEY SHARE couples every department write to settlements (07UR-P02-G1-F14).

## 07UR-P02-G1-F12 — Every settlement of a company now locks the root's two running-total rows until commit; no contention bound or metric

- **Classification:** actionable defect; **low**; architecture/performance.
- **Evidence:**
  - Every charge upserts the root's day and month rows (the root is in every chain). The row
    locks last until the settlement transaction commits, which happens after `persist_on`
    (checkpoint JSON), `agent_history::accepted_on`, a second token-budget lock and the liveness
    check (checkpoint_models.rs:244-252).
  - All model and JEV settlements of the company therefore serialize on two rows. Those
    transactions run under bounded `lock_timeout`/`statement_timeout` (persistence_bound.rs:
    213,452), so contention surfaces as settlement timeouts.
  - Admission reads are unaffected (MVCC).
  - persistence AGENTS ("Change performance only from production evidence"): a deliberately
    deferred optimization needs a named duration or working-set metric, or a threshold log. r2
    states neither the serialization nor a signal.
  - The unit-id lock order is deadlock-free only because each settlement transaction charges
    exactly one call (agent: one `Mutation::Model`, checkpoint_models.rs:242; JEV: one request,
    jev_runtime.rs:285). r2 does not state that this is the property it relies on.
- **Required:** acknowledge the company-wide serialization point and add a metric or threshold
  log for the charge's lock wait. Consider charging in a `DEFERRABLE INITIALLY DEFERRED`
  constraint trigger so the hot rows are held only during commit. State the
  one-charge-per-transaction assumption, or lock top-down from the root.
- **Instruction lesson:** cause: moving aggregation from read to write moved the hot spot rather
  than removing it. No new instruction beyond the existing performance rule.
- **Resolution history:** 2026-11-21 open (G1 r2).
  - 2026-11-21 recheck 07UR-P02-G1-R3: **fixed**. The charge holds the totals rows only during COMMIT, and one call per settlement transaction is stated. On the missing metric: the settlement op already reports `commit_ms` and warns on not-applied/unknown outcomes against its attempt bound (persistence_bound.rs:383-433). That is the threshold signal persistence AGENTS asks for, and the decision could cite it. No further action.

## 07UR-P02-G1-F13 — Smaller r2 observations (optional)

- **Classification:** optional suggestion; **low**.
- **Evidence:**
  - The running sum `spent_micros + cost` is not stated as saturating. Per-call cost cannot
    exceed about 4.3e15 micro-USD (2·2^31 tokens × 10^12 / 10^6), so only extreme prices reach
    the bigint limit, but a plain `+` would then fail every settlement of the subtree. The same
    applies to `unpriced_tokens`.
  - D9 r2's per-row `unit_reaches(company, call.unit_id, managed)` evaluates a recursive CTE for
    every call in a range of up to 90 days, under the report's 5 s `statement_timeout`
    (report.rs:51). Computing the managed subtree's unit ids once and filtering `unit_id =
    ANY(..)` before the limit is cheaper.
  - `unit_spend` keeps a row per unit per day indefinitely. A retention rule or note would help.
  - Calls that are reserved but never settled are never charged; a failed JEV call stays
    unsettled (`fail_jev`, jev_runtime.rs:78-103). This is recorded as a limit in D7 r2. With
    soft caps (k) that is acceptable, but the page could flag them like unpriced tokens.
  - A library copy that falls back for reach gets no warning. The `library_model_unavailable`
    test runs in the application layer against company-wide data, before the in-transaction
    reach fallback.
  - The page shows ledger figures (current price, reserved tokens for unsettled calls) beside
    `unit_spend` figures (price at settlement, rounded up, settled only). Label which is which.
- **Resolution history:** 2026-11-21 open (G1 r2).
  - 2026-11-21 recheck 07UR-P02-G1-R3: dispositions accepted.
    - Adopted: managed subtree ids computed once and filtered before the limit; running totals labeled separately from ledger figures.
    - Not adopted, recorded: `unit_spend` retention, flagging never-settled calls, a warning on the reach fallback.
    - The adopted saturating sum is not saturating as written; that is new finding 07UR-P02-G1-F15.

## 07UR-P02-G1-R3 findings

Context for F14–F15: round note
[07UR-P02-G1-R3](agent-review-notes.md#07ur-p02-g1-r3-2026-11-21), architecture gate G1 decision
revision r3 (TASK.md "Architecture gate G1 (decision revision r3, 2026-11-21)"). Coordination
revision 9. Same HEAD 107fd588 and digest
67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1. Reviewer B Claude Opus 5.5.
Both originate in the r3 decisions.

## 07UR-P02-G1-F14 — Locking the whole chain `FOR KEY SHARE` at commit makes every department write on the chain stall the commit of every settlement charging through it

- **Classification:** actionable defect; **medium**; concurrency/architecture (cross-subsystem
  lock coupling).
- **Evidence:**
  - D7 r3 step (1) locks every department of the call's chain, the root included, `FOR KEY
    SHARE` inside the settlement's COMMIT. `KEY SHARE` conflicts with `FOR UPDATE`.
  - Every department write takes its department row `FOR UPDATE` (department.rs:9, 207, 233-241).
    Callers include member add and removal, role assignment and clearing, edits and re-parenting,
    deletion, root member removal, and new agent placement (`prepare_new_agent_placement_on`,
    placement.rs:78).
  - The agent placement is held from placement through publication and workflow provisioning
    until the creation commits (company_agent.rs:122-162, 69-120).
  - Reproduced (case 5 in
    [logs/07UR-P02-G1-R3-r3-charging-protocol.txt](logs/07UR-P02-G1-R3-r3-charging-protocol.txt)):
    a transaction holding the root row `FOR UPDATE` for 1.5 s stalled a child unit's settlement
    COMMIT for 1.14 s.
  - Settlement transactions must finish COMMIT within `attempt_bound = min(lease/3, 2 s)`
    (lease.rs:48-50), with server timeouts set to the remaining time (persistence_bound.rs:
    165-167, 213). So any root-department write, such as an agent created into the root or a
    member added or removed, can push the commits of every in-flight model settlement in the
    company past their bound. That gives not-applied or unknown outcomes after the provider call
    was made.
  - Before r3 a settlement never waited on department rows. In r2 it did so only through FK
    checks on a period's first insert.
  - Stress test (4) covers settlements competing with each other, not with department writes.
- **Why the full chain is unnecessary:** only the call's own unit can be deleted while the chain
  stands. Every ancestor in the chain has a child (the next unit down), and
  `guard_department_delete_on` (lifecycle.rs:213-226) and `departments_parent_fk` refuse deleting
  a parent with children. A unit locked `KEY SHARE` cannot be re-parented, because the edit takes
  `FOR UPDATE` first. So locking `NEW.unit_id` alone gives the delete ordering that 3a/3b need.
- **Required:** choose and state one of:
  - (a) lock only the call's unit row. This leaves coupling with writes on that one unit, and the
    root for root-attributed calls.
  - (b) drop `unit_spend`'s FK and the department locks. Charge the chain read in the trigger's
    statement snapshot; have department delete remove the unit's totals rows explicitly; accept a
    harmless orphan row when a charge commits just after a delete.
  - (c) keep r3 and accept the coupling explicitly, with a competing case: a settlement commits
    while a placement or edit holds a chain department `FOR UPDATE`, and its outcome is asserted
    against the settlement bound.
- **Instruction lesson:** observation: a lock added to order against one writer (delete)
  conflicted with every writer of the same row. Candidate (persistence AGENTS): "When adding a
  row lock for one competing path, list every path that locks the same rows and in which mode."
- **Resolution history:** 2026-11-21 open (G1 r3).
  - 2026-11-21 recheck 07UR-P02-G1-R4 (decision revision r4, same digest): **fixed**. r4 takes option (b): no FK from `unit_spend` to departments, no department locks in the charge, and an `AFTER DELETE ON departments` trigger that removes the unit's totals. Executed against r4 on a reduced schema ([logs/07UR-P02-G1-R4-r4-charging-protocol.txt](logs/07UR-P02-G1-R4-r4-charging-protocol.txt), script `.sh.txt` beside it):
    - Test 5: with the root, parent and unit rows held `FOR UPDATE` for 1.5 s, a settlement in the subtree commits in 4 ms.
    - 3a: ancestors keep the call and the unit's rows are gone.
    - 3b: the root is charged and no row is created for the deleted unit.
    - In between, delete first (3c): one orphan row per period (day and month) and ancestors charged. The settlement waited about 0.6 s for the in-flight delete, which holds the unit's totals rows from its trigger until it commits.
    - In between, charge first (3d): the delete waits for the charge, then removes the unit's rows; no orphan.
    - 4: 16 concurrent settlements commit with exact totals.
    - Company teardown leaves no rows.
    - Residual lock-order issue: 07UR-P02-G1-F16.

## 07UR-P02-G1-F15 — The r3 "saturating sum" overflows before it saturates

- **Classification:** actionable defect; **low**; correctness.
- **Evidence:** D7 r3 writes `LEAST(spent_micros + cost, 9223372036854775807)`. With bigint
  operands the addition overflows first: `SELECT LEAST(9223372036854775800::bigint +
  100::bigint, 9223372036854775807)` raises "bigint out of range". The same with the addition in
  `numeric` saturates
  ([logs/07UR-P02-G1-R3-saturating-sum.txt](logs/07UR-P02-G1-R3-saturating-sum.txt); also
  exercised in the protocol log's saturation case). As written, a total near the limit would fail
  the settlement's COMMIT instead of saturating. The same applies to `unpriced_tokens` if summed
  the same way.
- **Required:** state the sum in numeric, for example `LEAST(spent_micros::numeric + cost,
  9223372036854775807)::bigint`, and give the saturation case a test.
- **Instruction lesson:** none; a formula in a gate should be checked as SQL.
- **Resolution history:** 2026-11-21 open (G1 r3).
  - 2026-11-21 recheck 07UR-P02-G1-R4: **fixed**. The sum and the cost are computed in numeric, `LEAST(spent_micros::numeric + cost, 9223372036854775807)::bigint`. The saturation case in the R4 protocol log ends at 9223372036854775807 without error.

## 07UR-P02-G1-R4 findings

Context for F16: round note [07UR-P02-G1-R4](agent-review-notes.md#07ur-p02-g1-r4-2026-11-21),
architecture gate G1 decision revision r4 (TASK.md "Architecture gate G1 (decision revision r4,
2026-11-21)"). Coordination revision 9. Same HEAD 107fd588 and digest
67d600bca2029de8b84de01c80d5b5eb00b310a8e0d5f80f1c10c734e3aa69d1. Reviewer B Claude Opus 5.5.
Originates in the r4 decision.

## 07UR-P02-G1-F16 — The delete trigger's unordered `DELETE` can deadlock with a charge; r4's "races, all non-failing" does not hold as written

- **Classification:** actionable defect; **low**; concurrency.
- **Evidence:**
  - r4 says the `AFTER DELETE ON departments` trigger "deletes that unit's `unit_spend` rows",
    with no visiting order. The charge upserts the chain's rows ordered by `(unit_id, period)`.
  - A `DELETE … WHERE company_id = … AND unit_id = …` visits rows in plan order. With an index
    scan on the PK this matches the charge's order. With a sequential scan (forced here; a
    planner may choose one for a small table) it follows heap order, and a unit's month row
    usually precedes its day row there, since the month row is created first.
  - Reproduced ([logs/07UR-P02-G1-R4-delete-heap-order.txt](logs/07UR-P02-G1-R4-delete-heap-order.txt),
    script beside it): a second charge holds the unit's month row; the delete queues on it; a
    settlement locks the unit's day row and queues on the month row. When the second charge
    commits, the delete takes the month row and waits on the day row → "deadlock detected".
    Here the delete was the victim. Which side PG aborts is not fixed, so a settlement COMMIT
    can be the one that fails.
  - It needs a delete racing two charges on the same unit, so it is rare and the failure is
    retryable. But r4 states that every enumerated race is non-failing.
  - Minor wording in the same paragraph:
    - r4 says department writes "never wait on or delay a settlement". A delete in flight does
      delay a settlement charging its unit until the delete commits (3c, about 0.6 s), because
      the trigger holds that unit's totals rows.
    - The orphan is up to one row per period (two rows), not "one".
- **Required:** have the delete trigger lock the unit's rows in the charge's order before
  deleting, for example `PERFORM 1 FROM unit_spend WHERE company_id = OLD.company_id AND unit_id
  = OLD.id ORDER BY period, period_start FOR UPDATE;` then `DELETE`. Correct the two wording
  points. Add the three-way case (delete, settlement, second charge on one unit) to the
  competing tests. The fix is local, so a recheck can be limited to this delta.
- **Instruction lesson:** observation: lock order must hold for every statement touching the
  rows, including set-based deletes whose visiting order is the planner's choice. Covered by the
  candidate recorded in F11/F14.
- **Resolution history:** 2026-11-21 open (G1 r4).
  - 2026-11-21 recheck 07UR-P02-G1-R5 (decision revision r5, same digest): **fixed**. r5 sets one lock order for `unit_spend`, `(unit_id, period, period_start)`; the delete trigger locks the unit's rows `ORDER BY period, period_start FOR UPDATE` before deleting. The two wording points are corrected (only a delete of the call's own unit delays its charge; the orphan is the day and month rows). Re-ran the R4 heap-order race against the r5 trigger with sequential scans forced ([logs/07UR-P02-G1-R5-delete-heap-order.txt](logs/07UR-P02-G1-R5-delete-heap-order.txt), script beside it). The lock plan is `LockRows → Sort (period, period_start) → Seq Scan`, so locks follow the sort.
    - 6a, the R4 deadlock order (second charge holds the month row, the delete queues, then the settlement arrives): all three complete; two orphan rows; root charged.
    - 6b (settlement takes the day row first, then the delete): all three complete; no orphan.
    - No deadlock in either.

## 07UR-P02-B1-R1 findings

Context for B1-F1 … B1-F6: round note
[07UR-P02-B1-R1](agent-review-notes.md#07ur-p02-b1-r1-2026-11-21), batch B1 = doc 02A (G1 D1–D4
with the r2 amendments to D1/D2), batch review. HEAD 107fd588 plus the working tree, digest
bbfb5e1c2cbba8700411b000a30a30bdc40506c4cd22830826b721b4558c03b1, exclusions unchanged.
Coordination revision 9. Reviewer B Claude Opus 5.5. All findings are in the implementation,
except where a finding says otherwise.

## 07UR-P02-B1-F1 — Decisive cases promised by the `unit_model_default_*` mapping are missing for several changed production paths

- **Classification:** actionable defect; **medium**; verification.
- **Evidence:** the four new tests (delegated_agent_management_tests.rs:929-1335) cover:
  cascade to an ancestor's default and to the root for an unplaced agent and for a sibling;
  frozen pins; an inherited library copy; an explicit out-of-reach refusal at UI/API create;
  child-lead and sibling-lead 404s; Conflict; ≤1 default via the use case; the root's default not
  movable; a move into the root landing non-default; the department delete listing; the FK;
  publication refused after a move; two units racing for one provider. I re-ran them on my own
  databases ([logs/07UR-P02-B1-R1-reviewer-unit-model-default.txt](logs/07UR-P02-B1-R1-reviewer-unit-model-default.txt),
  4 passed). Uncovered changed paths, against the G1 mapping (D1 authority/move/reach, D2
  cascade) and D1/D2 r2:
  - **Admission reach:** `check_agent_model_reach_on` in `agent_manifest/capture.rs:88-95`.
    Nothing asserts a run is refused at admission after the move, nor admitted again once the
    connection moves back up. The mapping's "publication/admission refused / admitted" is half
    covered.
  - **Move:**
    - authority over both units (a lead moving into a unit they don't manage → 404);
    - a move into a root without a default makes it the default (`unit_resources.rs:142-169`);
    - moving the root's only connection away is allowed (TASK's reverted semantics).
  - **Replace authority (decision a widening):** a root admin and a root lead replacing the
    root's connections; a plain member → 404.
  - **Cascade:**
    - a child with its own default pins its own;
    - no default anywhere → the existing "Add a model connection" refusal;
    - D2 r2: a library definition whose explicit model is enabled but outside the placement's
      reach falls back (channel.rs:651-664);
    - the Builder's out-of-reach refusal (provisioning.rs:591-598); provisioning_builder_tests.rs
      is unchanged.
  - **Offers and reads:** `list_company_model_connections(.., Some(agent))` filtering by agent
    reach (use_cases/agent.rs:355-380); `list_model_connections` filtering by managed units
    (use_cases/company.rs:486-514).
  - **UI:** the department "Model connections" card and `POST
    /ui/companies/{c}/departments/{d}/models` (ui_departments.rs:735-793). Nothing tests that only
    managers see or post it, that Conflict/BadRequest render inline, or how the root department
    page behaves (B1-F2). department_ui_tests.rs only adds `models: None`.
- **Required:** add decisive cases for these paths (at least admission reach,
  move-into-root/authority, the root-admin widening, none-anywhere, the library reach fallback,
  the Builder refusal, the reach-filtered offers, and the department route), or record a
  justified disposition per case.
- **Instruction lesson:** the verification-contracts reference already requires mapping each
  planned case to a decisive assertion; TASK's B1 test line lists four tests without a per-case
  mapping. Candidate: "after implementation, replace the planned mapping rows with actual test
  names per case; an unnamed row is a gap."
- **Resolution history:** 2026-11-21 open (B1-R1).
  - 2026-11-21 recheck 07UR-P02-B1-R2 (digest 5a938fa0…4359): **fixed**. Six cases were added and read against the gaps listed above:
    - `unit_model_default_admission_refuses_a_model_moved_out_of_reach` (autonomous_agent_admission_tests.rs:731): a real `WorkflowService::admit` is refused with `admission.resource_outside_unit` naming the connection, then admitted once the agent is placed and again once the connection moves back up.
    - `unit_model_default_moves_and_an_empty_root`: a lead moving into units they don't manage → 404; the root's only connection leaves; an unplaced creation is refused with "Add a model connection"; a move into the default-less root becomes the company default and is pinned.
    - `unit_model_default_root_managers_own_defaults_offers_and_library_fallback`: a root admin and a root lead replace the root; a plain member → 404; a department's own default wins; reach-filtered offers; library reach fallback.
    - `unit_model_default_agent_builder_model_follows_the_placement_reach`: the Builder refuses Invalid outside, provisions inside.
    - `unit_model_default_department_card_saves_for_managers_only`: the route, the inline Conflict, a member with no card and a 404.
    - `unit_model_default_a_department_form_keeps_the_last_connection`.
    B re-ran all nine `unit_model_default_*` tests plus the directory tests on its own databases: 14 passed ([logs/07UR-P02-B1-R2-reviewer-focused.txt](logs/07UR-P02-B1-R2-reviewer-focused.txt)).

## 07UR-P02-B1-F2 — The company-wide "≥1 connection" invariant (D1 r2) is not enforced, and the root's department page bypasses the root form's empty-set refusal

- **Classification:** actionable defect; **low**; requirements/correctness (a deviation from a
  gated decision).
- **Evidence:**
  - D1 r2 (G1-passed): "the company keeps ≥1 connection overall (today's rule, now company-wide
    across units)". `check_model_connection_invariants_on` (company.rs:556-574) checks only the
    root-default rule. TASK's "Reverted after the full suite" note calls that "the gate's rule"
    and drops the ≥1 rule without recording a deviation.
  - Only the root form's use case refuses an empty set (use_cases/company.rs:313-319). The unit
    path accepts an empty set (`replace_unit_model_connections`, :578-592).
  - The department page renders the models card for the root department too (any managed
    department, ui_departments.rs:131-142) and posts it to the unit path. So a root manager can
    empty the root there, and a lead can empty the last unit-owned set, leaving the company with
    no model connection. The settings form refuses exactly that with "A company must keep at
    least one model provider configured."
  - The stranding check still protects pinned agents, so the impact is limited to a company
    with no usable model.
  - The two root editors also differ in validation: exactly one default on the settings form,
    at most one plus persistence's root rule on the department page.
- **Required:** either enforce the company-wide ≥1 rule in persistence for both paths (tests
  that need an empty company can delete rows directly), or record a G1 amendment with the user's
  or root's rationale. Make the root's department page use the root rules, or omit the card for
  the root.
- **Instruction lesson:** an implementation that relaxes a gated invariant should record the
  deviation as such, not as the gate's rule.
- **Resolution history:** 2026-11-21 open (B1-R1).
  - 2026-11-21 recheck 07UR-P02-B1-R2 (digest 5a938fa0…4359): **fixed**. The use case holds the root's department form to the settings rules: non-empty and exactly one default, detected via `managed_unit_choices` (use_cases/company.rs:576-605). Persistence refuses a department-path replace that would leave the company with no connection (company.rs:521-533). The root path keeps its use-case refusal of an empty set. Moves don't change the count. TASK wording corrected. Tested by `unit_model_default_a_department_form_keeps_the_last_connection`.

## 07UR-P02-B1-F3 — The per-unit replace maps every authorization-lock error to "company not found"

- **Classification:** actionable defect; **low**; correctness/observability (src/AGENTS.md "Don't
  collapse errors into defaults on authorization paths").
- **Evidence:** `replace_unit_model_connections` calls `authorize_person_with_locks(..)
  .await.map_err(|_| company_not_found())?` (company.rs:399-407). A lock or statement timeout
  (`DatabaseTimeout`), a database error or an internal error becomes a 404 "Company not found,
  or you do not have permission." Neither the user nor the error log can tell it from a refusal.
  The p01 callers (`authorize_resource_on`, `move_resource_on`, unit_resources.rs:60-66, 92-98)
  propagate with `?`.
- **Required:** map only the authority refusal (`NotFound`) to `company_not_found()` and
  propagate everything else.
- **Instruction lesson:** the existing rule covers this.
- **Resolution history:** 2026-11-21 open (B1-R1).
  - 2026-11-21 recheck 07UR-P02-B1-R2 (digest 5a938fa0…4359): **fixed** (inspection): only `NotFound` maps to `company_not_found()`; other errors propagate (company.rs:407-410).

## 07UR-P02-B1-F4 — Consumers left on company-wide semantics: the Builder directory, the create form, prompt generation and the "(default)" marker

- **Classification:** actionable defect; **low**; requirements/contract impact.
- **Evidence:**
  - G1 r2 contract additions (F8): "Agent Builder directory model list shows each model's
    unit". `provisioning_directory::list_on` (provisioning_directory.rs:41-49) is unchanged and
    lists every ready model without its unit. B1 plan step 4 says the create form shows
    connections "with units". `company_model_selection` (pages/agent_settings.rs:893-930) shows
    only provider/model, so a sibling's provider is offered unlabeled and refused only on submit.
  - `prompt_model` (model_preparation.rs:54-66) and the agent form's "(default)" marker
    (agent_settings.rs:887-892) both find "the company default" as the first `is_default` row,
    relying on `model_connections_on`'s ordering. When the root holds no default (allowed once
    its only connection moves away), the first `is_default` row is a department's default:
    - prompt generation for any author then uses that department's key;
    - `.first().filter(is_default)` marks a department's default "(default)", if that provider
      sorts first.
    `generate_system_prompt` also accepts any provider override across units
    (use_cases/agent.rs:717-740).
  - The warning texts still say "company's default" for copies now pinned to a department's
    default (`library_model_pinned`, use_cases/agent.rs:479-483; `library_model_unavailable`,
    model_preparation.rs:39).
- **Required:**
  - Show each model's unit in the Builder directory and the create form, per the r2 contract,
    or record a disposition.
  - Pick the root's default by `unit_id == root` rather than by list position; refuse prompt
    generation when the root has none.
  - Correct the two warning texts.
- **Instruction lesson:** none new; the gate's contract rows should be checked off against the
  diff.
- **Resolution history:** 2026-11-21 open (B1-R1).
  - 2026-11-21 recheck 07UR-P02-B1-R2 (digest 5a938fa0…4359): **fixed**.
    - `CompanyModelConnection` gains `unit_name` and `in_root` (filled by `model_connections_on`).
    - `prompt_model` and the "(default)" marker select `is_default && in_root`, not list position.
    - The agent form labels a department-owned provider with its department.
    - The Builder directory lists each model's `department`, required in the `list_company_agents` output schema (provisioning_schemas.rs:50-56). The listing callers were checked: the seven directory and Builder tests that failed on the old schema in the R2 focused run all pass in B1-R2-new-and-fixed.txt, with assertions updated to name the department, not weakened. Stored tool contracts embedding the old schema are covered by the no-backward-compatibility reset.
    - Warning texts say "its department's default".
    - Not tested by a discriminating case: the department label, the root-only marker, and `prompt_model` with no root default (optional 07UR-P02-B1-F7).

## 07UR-P02-B1-F5 — The move's use listing reads the agent's draft provider, not its active revision (optional)

- **Classification:** optional suggestion; **low**; correctness.
- **Evidence:** D1 r2 says "uses = agents whose active revision pins that provider". The
  `ModelConnection` arm of `uses_on` (unit_resources.rs:215-221) joins `agent_active_revisions`
  only to require a published agent, then compares `agents.provider`, the current draft field.
  An agent with an unpublished provider change is listed or omitted wrongly. Admission and
  publication use the revision's model.
- **Resolution history:** 2026-11-21 open (B1-R1).
  - 2026-11-21 recheck 07UR-P02-B1-R2 (digest 5a938fa0…4359): **dismissed**: optional, not adopted (author's reason recorded in TASK). Acceptable; it affects only the move response's use listing.

## 07UR-P02-B1-F6 — Builder reach uses the spec's department, not the placement the Builder actually uses (optional)

- **Classification:** optional suggestion; **low**; requirements.
- **Evidence:** `agent_write` checks the explicit model against `spec.department` or the root
  (provisioning.rs:574-598), but the Builder places the agent in the requester's own department
  when the spec names none (`placement_on`, provisioning_builder.rs:132+). A model owned by that
  department is refused although the agent would reach it. `resolve_model_on` then re-checks with
  the real Deferred placement. This is stricter, not looser, and matches p01's skill lookup.
- **Resolution history:** 2026-11-21 open (B1-R1).
  - 2026-11-21 recheck 07UR-P02-B1-R2 (digest 5a938fa0…4359): **dismissed**: optional, not adopted (stricter than the placement, matching p01's skill lookup). Acceptable.

## 07UR-P02-B1-F7 — No discriminating test for the F4 rendering and prompt-default fixes (optional)

- **ID and context:** 2026-11-21, round 07UR-P02-B1-R2 (correction recheck), batch B1, digest
  5a938fa0571d0863f5c823653901ca94f59b6cb7effd63222ca4ebd317494359; reviewer B Claude Opus 5.5.
- **Classification:** optional suggestion; **low**; verification.
- **Evidence:** pages/tests.rs only adds `unit_name: "Root", in_root: true` to existing fixtures.
  No case renders a department-owned provider (the " — {department}" label) or a department's
  default (which must not carry "(default)"), and no case calls `prompt_model` when the root has
  no default (it must refuse). The code is correct on inspection (agent_settings.rs:889-906,
  model_preparation.rs:59-65), but a regression to list-position selection would pass the suite.
- **Suggestion:** add two small pure tests: the agent form with a root default plus a
  department default, and `prompt_model` with only a department default.
- **Resolution history:** 2026-11-21 open (B1-R2, optional; does not block).

## 07UR-P02-B2-R1 findings

Context for B2-F1, B2-F2: round note
[07UR-P02-B2-R1](agent-review-notes.md#07ur-p02-b2-r1-2026-11-21), batch B2 = doc 02B (G1 D5–D8
with r2–r5), batch review. HEAD 55f5f633 (accepted B1) plus the working tree, digest
80557b37f0f8cfabff6563c651842dff085ae5685de3c3882580ffba0e83cd8b, exclusions unchanged.
Coordination revision 9. Reviewer B Claude Opus 5.5. Both are in the implementation's
verification.

## 07UR-P02-B2-F1 — The real settlement paths and the planned lock-order case are not exercised

- **Classification:** actionable defect; **medium**; verification (an obligation carried from the
  G1 PASS).
- **Evidence:**
  - The G1 r5 PASS (07UR-P02-G1-R5) and TASK's status record: "a gate pass accepts the plan, not
    its execution: the real settlement transaction and bounds, JEV and production-size plans are
    B1–B3's tests", and "the planned tests 3a/3b/4/5/6 … must exercise them".
  - Every `unit_budget_*` test reserves with a raw `INSERT … jsonb_populate_record` and settles
    with a raw `UPDATE` (unit_budget_tests.rs:85-123), never through the production settle sites.
  - `unit_spend` is read only by unit_budget_tests.rs and the schedule test (which seeds it
    directly), so no test asserts that an agent turn settled by `checkpoint_models`
    (`persist_checkpoint_usage_on` inside the WriteBound settlement transaction, with its server
    timeouts and COMMIT classification), or a JEV settlement (`jev_runtime::settle_on`), charges
    its unit. Nor does any test assert that the `BEFORE INSERT` attribution sees the production
    reserve's run. The suite passing only shows the deferred trigger does not break settlement.
  - **Test (6) as planned in r5** — "a charge holding the unit's month row, the delete queued, and
    a second settlement holding the day row → no deadlock". The implemented three-way case
    (unit_budget_tests.rs:375-442) has the first charge hold all of the unit's rows. Its rows were
    created day-first by the test's own charges, so the delete's unordered scan would visit them in
    the charge's order anyway. Reverting `forget_unit_spend` to a plain `DELETE` would still pass.
    The deadlock shape reproduced in R4 (logs/07UR-P02-G1-R4-delete-heap-order.txt) needs the month
    row before the day row in heap order and a sequential scan in the deleting session.
  - Not tested from the G1 r2 mapping:
    - "re-parent a unit → … later charges follow the new chain";
    - child-run and JEV attribution (`unit_budget_charge_*`: "JEV/child attribute to the run's
      write unit").
  - The desk-less case does not discriminate either. The company-wide run's unpriced call shows
    up on the root, which every chain includes, and the test never asserts the call's `unit_id`.
- **Required:**
  - (a) A real agent run on a priced model whose settled turn charges its desk's unit and
    ancestors, followed by a refused admission, through the production reserve and settle.
  - (b) A JEV settlement attributed and charged.
  - (c) Test (6) in its planned shape: seeded month-before-day rows, the second charge holding
    the month row, the delete queued with sequential scans forced on its session, then the
    settlement on the day row. It should fail with a plain `DELETE` and pass with the ordered
    lock.
  - (d) The re-parent case.
  - (e) The desk-less call's recorded `unit_id`, asserted.
- **Instruction lesson:** cause: gate-carried obligations ("B1–B3's tests") were not copied into
  B2's plan items. Candidate: "when a gate pass defers verification to batches, list each
  deferred check in the batch plan by name."
- **Resolution history:** 2026-11-21 open (B2-R1).
  - 2026-11-21 recheck 07UR-P02-B2-R2 (digest 60e2c15f…1929; tests only, schema confirmed restored with the ordered `PERFORM`): **fixed**, by part:
    - (a) `unit_budget_charge_production_agent_settlement_on_a_desk`: a real `WorkflowService::admit` on a Billing desk, `claim_io`, then the agent runtime against a scripted model. The production reserve and the checkpoint settlement attribute the call to Billing, and the COMMIT charge gives Billing and the root exactly the priced cost. The follow-up refused admission was not added; that is acceptable, since the admission read is independent of how a total was charged and is covered by `unit_budget_admission_*`.
    - (b) `unit_budget_charge_production_jev_settlement`: a JEV run attributed to the root with its unpriced tokens counted.
    - (c) `unit_budget_competing_delete_takes_totals_in_the_charges_order`: month row before day row in the heap; the holder takes the month row; a delete with index and bitmap scans off is queued; the charge waits on the day row. It completes. The author's negative check (logs/B2-R2-negative-unordered-delete.txt) shows `deadlock detected` without the ordered `PERFORM`, so the case discriminates the F16 fix, consistent with B's R4 reproduction.
    - (d) Re-parent: the old parent keeps its count and the new parent counts from then.
    - (e) The company-wide call's recorded `unit_id` is the root, and a spawned child of a Billing-desk run records Billing through its copied scope.
    B re-ran all 11 `unit_budget_*` tests: passed ([logs/07UR-P02-B2-R2-reviewer-unit-budget.txt](logs/07UR-P02-B2-R2-reviewer-unit-budget.txt)).

## 07UR-P02-B2-F2 — Smaller untested surfaces (optional)

- **Classification:** optional suggestion; **low**; verification.
- **Evidence:**
  - The production message path for a budget refusal (an intent settled `refused` with
    `budget.unit_exhausted`) is covered only by the `classify` unit assertion; schedules have a
    production test.
  - The HTTP 400 body `{code, error, unit, period}` (app_error_impl.rs) has no test, unlike
    `WorkflowRole`'s.
  - The cap routes `PUT`/`DELETE /api/companies/{c}/departments/{d}/budgets/{period}` are tested
    only at persistence.
- **Resolution history:** 2026-11-21 open (B2-R1, optional).
  - 2026-11-21 recheck 07UR-P02-B2-R2 (digest 60e2c15f…1929; tests only, schema confirmed restored with the ordered `PERFORM`): **deferred** to p03 by the author (the production message path, the HTTP body and the cap routes; recorded in TASK). Acceptable for an optional suggestion; carried as an open item for p03's route and final checks.
