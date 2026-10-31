# f03 review findings — fix-03-run-board-finished-columns

Workflow `implement-opus-reviewed-2_2`. Task [TASK.md](TASK.md); review record [REVIEW.md](REVIEW.md);
notes [agent-review-notes.md](agent-review-notes.md).

Common context for the entries below unless an entry says otherwise: phase `f03`, worktree
`/Users/mac02/.codex/worktrees/b109/openbot`, HEAD = review base `3793244` (no implementation yet),
batch B1 / stage architecture gate G1, review round
[F03-G1-R1](agent-review-notes.md#f03-g1-r1--architecture-gate-g1-decision-revision-1), reviewer
Opus 5.5 (`claude-opus-5-5`), reviewed artifact = gate decisions G1-D1 … G1-D5 **revision 1**
(`logs/G1-r1.md`, sha256 `a808388d73a19216c11a01c582c0e6967f27222d191143000e1069e8e6f87613`,
identical to TASK.md's section). Coordination revision 1, no notices.

## F03-G1-F1 — D1's trigger inventory omits the run-snapshot guard, and its "order does not matter" claim is false for the revision trigger

- **ID and context:** F03-G1-F1, 2026-10-31, gate round F03-G1-R1, decision G1-D1 revision 1.
- **Classification:** actionable defect, **Medium**; category architecture / correctness (schema
  invariant on the run-state path; plan 01 step 2 "Confirm it does not depend on ordering against
  the existing BEFORE UPDATE triggers").
- **Evidence:**
  - D1 states: "No existing BEFORE trigger on `workflow_runs` assigns `NEW.state` (claim
    retirement, state witness, wakeup allocation, revision, parent/association guards only read
    it). So the result does not depend on alphabetical firing order." The first sentence holds
    (reviewer read every BEFORE function; the only `NEW.state :=` in the schema, line 2954, is on
    `workflow_message_actions`). The inventory and the conclusion do not:
  - **Omitted trigger.** `workflow_run_snapshot_immutable` (`BEFORE UPDATE OF company_id, …,
    deadline, created_at, …, delivery_policy`, function `guard_workflow_run_snapshot`,
    `migrations/20260817000000_init_schema.sql:11236-11250`) compares
    `to_jsonb(NEW) - ARRAY['state','waiting_reason','terminal_execution_id','terminal_wakeup_sequence','revision']`
    with OLD. That array is the list of a run's mutable lifecycle columns; D1 adds a new one
    (`finished_at`) without adding it there. `workflow_run_finished` sorts before the guard, so the
    guard sees the stamped value and treats `finished_at` as frozen snapshot data. Reproduced
    (`logs/review-g1-r1-trigger-experiment.sql` / `.txt`): at `3793244`,
    `UPDATE … SET state='succeeded', deadline=deadline` on a running run succeeds (B0); with D1
    applied the same statement fails with `workflow run snapshot is immutable` (E4). No production
    writer names a snapshot column together with `state` today (all 26 `UPDATE workflow_runs SET
    state` statements checked), so this is a latent trap with a misleading error rather than a
    live break — but the decision's stated invariant rests on an inventory that is incomplete.
  - **Ordering is load-bearing.** `advance_workflow_run_revision` (`workflow_run_revision`,
    init schema 72-87) bumps `revision` when `to_jsonb(NEW) - 'revision'` differs from OLD. With
    D1's name the stamp runs first, and the plan's own case "an UPDATE to a terminal state that
    tries `finished_at = NULL`" on an already-terminal run leaves `revision` unchanged (E2,
    revision 2 → 2). Under a name that sorts after `workflow_run_revision` — including the plan's
    literal trigger name `stamp_workflow_run_finished` (plan 01 step 2) — the same statement bumps
    `revision` with no visible change (E7, 2 → 3). `revision` is the controls' optimistic-concurrency
    token (`expected_revision`) and is compared in reconciliation guards
    (`receipt.result_revision=owner.revision` in the retry guard, init schema 132 and 147;
    `command.result_revision=owner.revision`, 2028). So the result does depend on
    firing order; D1 happens to choose a name that orders correctly, but records the opposite.
  - **Pinned catalog not named as an affected contract.** `capture_catalog`
    (`src/adapters/persistence/workflow/action_reconciliation_claim_sql_candidate_support.rs:98-118`)
    asserts the exact list of BEFORE UPDATE row triggers on `workflow_runs`; with D1 the same query
    returns `workflow_run_finished` as well (E0b), so
    `action_reconciliation_claim_sql_candidate_tests.rs` fails until the list is edited. The list
    pins what the reconciliation claim proof assumes runs inside a run update; adding a row-mutating
    trigger to it is a decision to record, not only a fixture edit.
- **Required (decision revision):** add `finished_at` to `guard_workflow_run_snapshot`'s exemption
  array (it is lifecycle state) and say so in D1; correct the ordering statement to "the stamp must
  fire before `workflow_run_revision` and `workflow_run_snapshot_immutable`; the trigger name
  `workflow_run_finished` is chosen for that" and pin it (e.g. the `finished_at = NULL`
  `run_finished_at_*` test also asserts `revision` is unchanged, or a catalog assertion of the
  order); name `capture_catalog`'s pinned list among the affected contracts with a sentence on why
  the new trigger does not change what that test protects.
- **Instruction lesson:** observation — the trigger's definition sits at line 11245, far from the
  block of `workflow_runs` triggers near line 8300-8900 that the inventory was evidently taken from;
  a `pg_trigger` listing on a migrated database would have shown it. Candidate (low confidence, one
  occurrence; scope `src/adapters/persistence/AGENTS.md` "1. Migration"): "When adding a column or
  trigger to a table, list its triggers from `pg_trigger` on a freshly migrated database rather than
  from the schema text, and check every function that compares whole rows (`to_jsonb(NEW)`) for an
  exemption list the new column belongs in."
- **Resolution history:**
  - 2026-10-31 (F03-G1-R1): open.
  - 2026-10-31 (F03-G1-R2, decision revision 2 `logs/G1-r2.md`, sha256 `5b7724e16980e3220a03fc87abbda00df852fb86d2dc956924942e229f833654`): **fixed in the decision.** D1 r2 states the name is load-bearing (stamp before
    `workflow_run_revision` and `workflow_run_snapshot_immutable`) and pins it with
    `run_finished_at_keeps_the_revision_when_the_stamp_undoes_a_write`; adds `finished_at` to
    `guard_workflow_run_snapshot`'s exemption array on both sides, pinned by
    `run_finished_at_stamps_beside_a_snapshot_column`; names `capture_catalog` as an affected
    contract; and gives a complete trigger inventory (reviewer re-read the parent/association guards:
    they only raise). Reviewer recheck on a fresh database with D1 r2 applied
    (`logs/review-g1-r2-trigger-experiment.sql` / `.txt`): the F1 reproduction now succeeds and stamps
    (R2-E4); a real snapshot change beside a state change is still refused (R2-E4b); the
    `finished_at = NULL` same-terminal write keeps the stamp and the revision (R2-E2); the catalog
    order shows `workflow_run_finished` before `workflow_run_revision` and the guard (R2-E0). The
    implementation and the two tests are for the B1 batch review to verify.

## F03-G1-F2 — D4's evidence plan cannot show the two working-set dimensions that decide the index: lineage and channel visibility

- **ID and context:** F03-G1-F2, 2026-10-31, gate round F03-G1-R1, decision G1-D4 revision 1.
- **Classification:** actionable defect, **Low**; category verification / performance evidence
  (`src/adapters/persistence/AGENTS.md` "Change performance only from production evidence";
  carried f02 item; F02-B1-F5 precedent).
- **Evidence:**
  - **Lineage.** The board reads only top-level runs (plan 02 contract, `RunLineage::TopLevel`), but
    both candidates — (A) `(company_id, finished_at DESC, id DESC) WHERE finished_at IS NOT NULL`
    and (B) `(company_id, state, finished_at DESC, id DESC) WHERE finished_at IS NOT NULL` — index
    child runs too, so every finished child in the window is read and dropped by the heap filter
    `parent_run_id IS NULL`, in the counts and in each finished card column, on every board read.
    D4 answers the carried f02 item ("must not read child-only versions to their end") by noting the
    board never reads per-version ranges — true, and the bound stays the window rather than history
    — but the cost then scales with children-per-parent inside the window, which is exactly the shape
    F02-B1-F5 found missed once ("record that rather than assume it"). The script's only child-heavy
    data (`CHILD_HISTORY`, `scripts/tests/run-list-query-plan.py:141-157`) creates 100,000 succeeded
    children 30 days back, so with any reasonable `finished_at` seed it lies outside the default week
    window and the default-window measurement would not show it. A top-level partial predicate
    (`… WHERE finished_at IS NOT NULL AND parent_run_id IS NULL`; the board statements carry the
    literal `parent_run_id IS NULL`, so it matches in generic plans as
    `workflow_runs_version_top_level` does) is not among the candidates.
  - **Channel visibility.** The seed leaves `channel_id` NULL on every run
    (`scripts/tests/run-list-query-plan.py:44-63`), so the cards statement's distinguishing predicate
    `(channel_id = ANY($visible) OR (channel_id IS NULL AND $company_visible))` is measured only in
    its degenerate form (everything company-associated, all visible). "Columns stay full instead of
    showing gaps for hidden runs" (plan 02 step 3) means a viewer who sees few channels makes each
    column read past hidden runs; how far, and which plan a generic plan picks for the OR with a
    boolean parameter, is not measured.
  - **Unstated choices.** Which windows are measured (Month is the largest working set) and how A vs
    B is chosen when both "read only the window" (D4's only criterion) are not stated.
- **Required (decision revision of D4's evidence plan):** seed `finished_at` so a child-heavy section
  falls inside the measured windows (or add one), measure week and month; add a top-level candidate
  (C) with `parent_run_id IS NULL` in the predicate, or record from the measurement why children in
  the window are accepted; seed runs across channels and measure cards with a partial visible set and
  with `$company_visible = false`, in both plan modes; state the selection criterion (e.g. buffers in
  both plan modes, then index size and write cost — F01-B1-F2 recorded that state updates are not
  HOT, so every candidate is written on every terminal transition).
- **Instruction lesson:** recurrence of F02-B1-F5's lesson ("check what happens to a key whose rows
  all fail the filter, and measure it"), here for a time-ranged rather than a per-key read. Candidate
  (moderate confidence, two occurrences; scope persistence AGENTS "Change performance only from
  production evidence"): "When a read always applies a fixed filter (lineage, visibility), measure
  it on data where that filter drops most rows inside the range read, and consider carrying the
  filter in the partial index predicate."
- **Resolution history:**
  - 2026-10-31 (F03-G1-R1): open.
  - 2026-10-31 (F03-G1-R2, decision revision 2 `logs/G1-r2.md`, sha256 `5b7724e16980e3220a03fc87abbda00df852fb86d2dc956924942e229f833654`): **fixed in the decision.** Both candidates now carry `AND parent_run_id IS NULL`;
    the seed gains `finished_at = created_at + 10 minutes`, 32 channels plus company-associated runs,
    and keeps 10% children spread through history (so children fall inside every window); day, week
    and month are measured in both plan modes, with all channels and 8 of 32 visible; the selection
    rule is explicit (counts read only the window's top-level finished runs plus open runs, finished
    cards stop at 51 matches; then fewer summed buffers; then (B)). R1 also asked for a
    `$company_visible = false` shape; that was unnecessary — the route authorizes the company
    association before the read, so `Visibility` always allows it, and r2 rightly omits it. The
    measurements themselves are for the B1 batch review.

## F03-G1-F3 — D5 folds the child rollup into the one elapsed time plan 03 reads as "working set vs pool or visibility"

- **ID and context:** F03-G1-F3, 2026-10-31, gate round F03-G1-R1, decision G1-D5 revision 1.
- **Classification:** actionable defect, **Low**; category requirements / observability (plan 03
  step 2; `src/adapters/persistence/AGENTS.md` "When an optimization is deliberately deferred …
  add a named duration or working-set metric … that can supply the evidence needed to revisit it";
  F01-B1-F6 precedent).
- **Evidence:** plan 03 step 2 times "both reads and the visibility step between them" and documents
  the reading "`eligible_runs` small with a high `elapsed_ms` points at pool contention or the
  visibility checks". D5 times "counts + visibility + cards + rollup" in one `elapsed_ms` and logs no
  rollup figure. `child_rollup` is bounded in parents (≤ 300) but not in children: its grouped read
  over `workflow_run_parents_by_parent` scales with how many children the shown parents have
  (`src/adapters/persistence/workflow/reads.rs:471-510`). A slow rollup on a board with a small
  working set would be read, per the documented reading, as pool contention or visibility — the wrong
  remedy for the deferred-rollup decision plan 03 step 3 hangs on this line. D5 also does not say
  whether `returned_cards` counts look-ahead rows (F01-B1-F5 noted the same ambiguity for
  `returned`).
- **Required:** log the phases separately (e.g. `counts_ms`, `visibility_ms`, `cards_ms`,
  `rollup_ms`, with `elapsed_ms` their sum) or keep `elapsed_ms` to plan 03's three steps and give
  the rollup its own duration and size field; define `returned_cards` (shown cards, look-ahead
  excluded or not); write the run-list.md reading for each.
- **Instruction lesson:** cause: a decision extended the timed span beyond what the plan's reading
  assumed without extending the reading. Candidate (low confidence): "A tripwire's documented
  reading must be re-derived when the timed span changes."
- **Resolution history:**
  - 2026-10-31 (F03-G1-R1): open.
  - 2026-10-31 (F03-G1-R2, decision revision 2 `logs/G1-r2.md`, sha256 `5b7724e16980e3220a03fc87abbda00df852fb86d2dc956924942e229f833654`): **fixed in the decision.** D5 r2 logs `elapsed_ms` plus `counts_ms`,
    `visibility_ms`, `cards_ms`, `rollup_ms`, and `cards_read` (look-ahead included) / `cards_shown`;
    the threshold applies to the total; run-list.md's "Revisit when" reads each phase.

## F03-G1-F4 — D3's JSON route resolves `binding_id` only company-scoped and has no route test plan

- **ID and context:** F03-G1-F4, 2026-10-31, gate round F03-G1-R1, decision G1-D3 revision 1.
- **Classification:** actionable defect, **Low**; category security / verification
  (`src/adapters/http/AGENTS.md` "Scope the object being used, not a sibling object").
- **Evidence:** the route is justified — without a caller `visible_board` is dead code under
  `check.sh`'s clippy, new-board-view 01 step 2 reuses the same read, and the JSON surface already
  mirrors the list's filters — and its company authorization and `company_agent` resolution are
  right. Two gaps:
  - D3 sends `binding_id` "through the company-scoped binding read (another tenant's id → not
    found)". The company-scoped read (`binding_summary`) finds a binding on a channel the caller
    cannot see, so the board answers 200 (with that binding's runs filtered to zero) where
    `GET …/workflow-bindings/{id}` answers 404 through `visible_binding`
    (`src/adapters/http/routes/workflows/bindings.rs:153-170`). That makes the board an existence
    check for bindings the caller may not see. `visible_binding` already does company scope plus
    association visibility.
  - The decision adds a new public route but no verification for it. http AGENTS: "Tests must
    attempt a valid id belonging to another tenant for every new detail, nested, download, and
    mutation route." Nothing names: another tenant's `company_id`, `binding_id` and `agent_id` → not
    found; a plain member → not found; a binding on a hidden channel → not found; a run on a hidden
    channel absent from `total` and `items`; `/board` not captured by `/{run_id}`; an unknown
    `window` value (400 or fallback — say which).
- **Required:** resolve `binding_id` through `visible_binding`; add the route tests above to D3's
  verification (they can share the `run_board_read_*` fixtures).
- **Instruction lesson:** none supported beyond the existing http rule (one occurrence).
- **Resolution history:**
  - 2026-10-31 (F03-G1-R1): open.
  - 2026-10-31 (F03-G1-R2, decision revision 2 `logs/G1-r2.md`, sha256 `5b7724e16980e3220a03fc87abbda00df852fb86d2dc956924942e229f833654`): **fixed in the decision.** `binding_id` resolves through `visible_binding` (a hidden
    binding is 404, as `GET …/workflow-bindings/{id}`); unknown `window` is a 400 query rejection;
    `run_board_read_route_*` covers another tenant's `company_id` / `binding_id` / `agent_id`, a plain
    member, a hidden binding, a hidden run absent from counts and cards, the 400, and `/board` through
    the router with `cookie_for_test` (helper exists in `src/adapters/http/session_test_support.rs:72`;
    router-level `oneshot` tests already exist under `routes/`).

## F03-G1-F5 — D2 puts the open/terminal split in Rust beside the SQL one with no equivalence check

- **ID and context:** F03-G1-F5, 2026-10-31, gate round F03-G1-R1, decisions G1-D2 / G1-D1 revision 1.
- **Classification:** actionable defect, **Low**; category verification
  (`src/adapters/persistence/AGENTS.md` "Encode queue state machines as constraints": "When a domain
  decision is implemented in both Rust and SQL, keep one generated definition or add a
  database-backed equivalence matrix that exercises every case").
- **Evidence:** D2 adds `RunStateName::is_open` (Rust); D1 adds the terminal set to SQL twice (CHECK
  `workflow_run_finished_shape`, `stamp_workflow_run_finished`), next to the existing open set in
  `workflow_runs_open_by_channel`'s predicate. The board decides which columns are windowed by
  `finished_at` from `is_open` (D3/D4), while the database decides which rows have `finished_at`. D1's
  planned `run_finished_at_*` tests check each state against hard-coded expectations, so they would
  not notice `is_open` drifting from the SQL set.
- **Required:** state in D1/D2's verification that one `run_finished_at_*` test drives a run into each
  of `RunStateName::ALL` and asserts `finished_at.is_none() == state.is_open()` (and that `ALL`
  matches the `workflow_run_state` CHECK's list), so the Rust and SQL definitions are pinned together.
- **Instruction lesson:** none beyond the existing rule; it was not cited by the decision.
- **Resolution history:**
  - 2026-10-31 (F03-G1-R1): open.
  - 2026-10-31 (F03-G1-R2, decision revision 2 `logs/G1-r2.md`, sha256 `5b7724e16980e3220a03fc87abbda00df852fb86d2dc956924942e229f833654`): **fixed in the decision.** D1 r2 verification adds the test over `RunStateName::ALL`
    asserting `finished_at.is_none() == state.is_open()`; D2 r2 cites it. (It pins Rust ⊆ SQL; a state
    added to the SQL CHECK alone would surface as an unknown-state decode error in the board's
    counts — loud, so acceptable.)

## F03-G1-F6 — Optional: `max(total, shown)` can report a cut column as "50 of 50"

- **ID and context:** F03-G1-F6, 2026-10-31, gate round F03-G1-R1, decisions G1-D3 step 6 / G1-D5.
- **Classification:** optional suggestion, Low; category correctness (presentation invariant).
- **Evidence:** with the count read seeing 50 visible failed runs and a run failing before the card
  read, the card read returns 51 rows: `cut = true`, `shown = 50`, `total = max(50, 50) = 50`, and
  new-board-view 01 step 4 renders "Showing 50 of 50 · open in list". The plan's formula
  (`max(total, shown)`) is followed; it just allows `cut` with `total == shown` until the next
  wake-up.
- **Suggested:** take `total = max(visible sum, rows read)` (look-ahead included) so `cut ⇒ total >
  shown` holds, and assert it in the assembly. Keeping that assembly a pure function over
  (visible counts, cards, cap) also gives plan 02's race test ("a run inserted between the count and
  card reads") a seam: call the two port reads with an insert between them, then the pure assembly.
- **Instruction lesson:** none.
- **Resolution history:**
  - 2026-10-31 (F03-G1-R1): open (optional).
  - 2026-10-31 (F03-G1-R2, decision revision 2 `logs/G1-r2.md`, sha256 `5b7724e16980e3220a03fc87abbda00df852fb86d2dc956924942e229f833654`): **taken.** D3 r2: `cut = cards_read > cap`, `total = max(visible sum, cards_read)`;
    column assembly is the pure, unit-tested `assemble_columns(counts_visible, cards, cap)`.

## F03-G1-F7 — Optional: read-side shape notes for B1

- **ID and context:** F03-G1-F7, 2026-10-31, gate round F03-G1-R1, decisions G1-D1 (read side) and
  G1-D3.
- **Classification:** optional suggestion, Low; category maintainability / requirements.
- **Evidence and suggestions:**
  - `RunRow` (`src/adapters/persistence/workflow/reads.rs:105-120`) is already a 14-element tuple;
    D1 makes it 15. `src/AGENTS.md` ("Name your tuples"; "don't extend the violation") and persistence
    AGENTS §3 (a `sqlx::FromRow` row struct) both point at a named `#[derive(sqlx::FromRow)]` struct;
    the column is appended to `RUN_COLUMNS` either way.
  - `run_json` (`bindings.rs:426-444`) does not carry `finished_at`, yet the JSON board orders its
    finished columns by it and new-board-view 01 step 5 shows "age since finishing". Adding it to
    `run_json` makes the JSON route's order checkable.
  - `bindings.rs` is 743 lines; the board orchestration, its types and route would take it near the
    ~1,000-line split point (`src/AGENTS.md`) — a sibling `board.rs` keeps it navigable.
  - The cards statement always reads `BOARD_COLUMN_CAP + 1`; a constant `LIMIT` gives generic plans
    the real row goal instead of the default fraction a `LIMIT $n` gets (the evidence measures both
    modes, so this is only worth doing if the generic plans differ).
- **Instruction lesson:** none.
- **Resolution history:**
  - 2026-10-31 (F03-G1-R1): open (optional).
  - 2026-10-31 (F03-G1-R2, decision revision 2 `logs/G1-r2.md`, sha256 `5b7724e16980e3220a03fc87abbda00df852fb86d2dc956924942e229f833654`): **taken.** `RunRow` becomes a named `sqlx::FromRow` struct; `run_json` gains
    `finished_at`; orchestration moves to `src/adapters/http/routes/workflows/board.rs`; the cards
    statement writes the constant `LIMIT 51` into its text (port no longer takes `per_column`).
    Note for B1: `visible_binding` is a private fn in `bindings.rs` and needs `pub(super)` for
    `board.rs`.

## F03-G1-F8 — Optional: revision 2 drops revision-1 text without marking it, including D1's failure modes

- **ID and context:** F03-G1-F8, 2026-10-31, gate recheck
  [F03-G1-R2](agent-review-notes.md#f03-g1-r2--gate-recheck-decision-revision-2), decisions G1-D1 and
  G1-D4 **revision 2** (`logs/G1-r2.md`, sha256 `5b7724e16980e3220a03fc87abbda00df852fb86d2dc956924942e229f833654`).
  Other context as at the top of this file.
- **Classification:** optional suggestion, Low; category requirements / record completeness.
- **Evidence:** r2 says "Changed text is marked **(r2)**", but a diff against `logs/G1-r1.md` shows
  unmarked removals and condensations. None changes behaviour; two carry content a later reader
  needs:
  - D1's "Failure modes" list is gone — notably that a write setting `finished_at` without `state`
    is allowed while the CHECK holds (the trigger is `UPDATE OF state` and r2 exempts the column from
    the snapshot guard). Tests that place runs just inside or outside a window rely on it to backdate.
    Same-terminal and terminal-to-terminal writes keeping the first finish are still implied by
    the trigger rule.
  - D4 no longer says the adopted candidate and its plans are recorded in
    `docs/performance/run-list.md` (plan 02 step 5); r2 only says the list figures are refreshed.
    It also drops "never from input" for the literal states, which r2's "They come from `RunStateName`"
    implies.
  - Smaller: D2's note that the domain `RunState` is unchanged, and D3's `RunBoard` return shape and
    "cutoff = `as_of − window`" step.
- **Suggested:** restore the D1 failure-mode lines and D4's "record the adopted index and its plans
  in run-list.md", or mark the removals. Not acceptance-blocking: plan 02 step 5 and the TASK
  acceptance criteria still require the record, and the semantics are unchanged.
- **Instruction lesson:** observation: a revision marked by additions only hides removals. Candidate
  (low confidence, one occurrence; scope workflow skill, architecture gate): "A revised decision marks
  removed or condensed text as well as additions, or the reviewer diffs against the preserved
  revision."
- **Resolution history:**
  - 2026-10-31 (F03-G1-R2): open (optional).
  - 2026-10-31 (F03-B1-R1, digest `9b583f28bb6f22a813e513dcf017077e71fc24ecf5ca4a0f445813d465ee63fc`):
    **resolved outside the gate text.** TASK.md A1 records both dropped lines: direct `finished_at`
    writes are allowed while the CHECK holds (tests backdate with them), and the adopted index and
    its plans go in run-list.md. Both are borne out: `run_finished_at_stamps_beside_a_snapshot_column`
    moves the stamp alone, and run-list.md "Run board" records (B). The record's accuracy is F03-B1-F1.

## F03-B1-F1 — The index record misstates the evidence: (A) reads only the window in auto mode, and (B) does not stop scoped finished columns at 51

- **ID and context:** F03-B1-F1, 2026-10-31, batch review
  [F03-B1-R1](agent-review-notes.md#f03-b1-r1--batch-review-of-b1-whole-plan), batch B1, stage batch
  review, reviewer Opus 5.5 (`claude-opus-5-5`), phase `f03`, worktree as above, base/HEAD `3793244`,
  digest `9b583f28bb6f22a813e513dcf017077e71fc24ecf5ca4a0f445813d465ee63fc`, gate G1 revision 2.
- **Classification:** actionable defect, **Low**; category performance evidence / record accuracy
  (`src/adapters/persistence/AGENTS.md` "Change performance only from production evidence": record
  the before/after plans with the change). Implementation, not the gated decision.
- **Evidence:**
  - TASK.md A4 says "(A) fails D4's rule: with the scope fenced, scoped card columns read history in
    **both plan modes**". `docs/performance/run-list.md` ("Run board", "(A)" bullet) says the same:
    "Scoped cards … go back to history in both plan modes". `docs/performance/run-board-query-plan.txt`
    shows otherwise. In `plan_cache_mode = auto`, every scoped card shape under (A) reads the window
    through `board_finished_a`:
    - `board_cards_binding heavy week all visible`;
    - `board_cards_binding … 8 of 32`: 13.9 ms, 6,530 buffers;
    - `board_cards_agent dense …`: 31.1 ms, about 19,900 buffers;
    - `board_cards_agent sparse …`.

    Only `force_generic_plan` takes a bitmap over `workflow_runs_company_id_id_channel_id_thread_id_key`
    and reads 200,000–750,000 rows of history (about 229 ms). The table in run-list.md lists (A)
    only in generic mode, so a reader cannot see this. Adopting (B) is still right: D4's rule requires
    both modes, and (A) fails in generic.
  - run-list.md says "(B) meets it" of the rule "each finished column stops at 51 matching rows". In
    fact (B) meets it for unscoped columns. Scoped finished columns under (B) read the state's whole
    window and sort it (e.g. 591 rows per column in `board_cards_binding … [(B)]`, both modes), as
    A3 intends. The next paragraph says so, so the record contradicts itself.
  - The dataset section does not say that every measured scope has **no finished runs inside any
    window**:
    - the binding is workflow 0, whose runs are all open (`n % 40 = 0` implies `n % 20 = 0`);
    - the dense agent's even versions end before the last 5% of history;
    - the sparse agent is at the old end.

    So the scoped finished columns were measured only at their read-the-whole-window bound, and
    "Binding counts … 6,250 runs" is the binding's open runs, not its history. The fenced inner read
    does not depend on the scope, so the cost figures remain a valid upper bound; the reader still
    needs to know.
- **Required:** correct A4 and the run-list.md "(A)" bullet to say that (A) reads history for scoped
  cards in generic plans, and only the window in auto. Add (A)'s auto figures or note them in the
  table. Say that (B) meets "stop at 51" for unscoped columns, and that scoped finished columns are
  bounded by their window (A3). In the dataset section, state the scopes' property above. No
  code change.
- **Instruction lesson:** observation: the summary was written from the generic-mode table only.
  Candidate (low confidence, one occurrence): "When a plan-mode-dependent result decides an index,
  record it per mode."
- **Resolution history:**
  - 2026-10-31 (F03-B1-R1): open.
  - 2026-10-31 (F03-B1-R2, digest `42c1472d862a198d1830c1949de8878bb85c5674f4cf4559e3cdae1ec41fa484`): **fixed.** A4 now says (A) reads history for scoped cards under generic plans, and
    under auto only for the recent agent with 8 of 32 channels. I verified that one in the evidence:
    a bitmap on `workflow_runs_company_id_id_channel_id_thread_id_key`, about 99,900 rows removed per
    finished column, 170 ms. run-list.md "Run board":
    - gains an "(A), auto" column;
    - limits "stop at 51" to unscoped columns, saying that a scoped column reads its working set;
    - states which scope holds what: workflow 0 open-only, workflow 1 finished-only, dense and sparse
      agents with nothing in any window, the recent agent `agent1-40` with finished runs in every
      window.

    The new scopes are measured in both modes (A12). Residual nit, no action needed: "about 60 a
    week" for workflow 1. The evidence shows 52 top-level finished runs in the week (17 + 18 + 17 in
    `board_cards_binding finished … all visible [(B)]`).

## F03-B1-F2 — Plan 02's DB-backed race case ("a run inserted between the count and card reads") is covered only by a pure unit test

- **ID and context:** F03-B1-F2, 2026-10-31, batch review F03-B1-R1; other context as F03-B1-F1.
- **Classification:** actionable defect, **Low**; category requirements / verification (plan 02
  "Focused checks and acceptance").
- **Evidence:** plan 02 lists, among the "DB-backed tests named `run_board_read_*`": "a run inserted
  between the count and card reads shows as `max(total, shown)` and is counted on the next read".
  The batch has no such DB-backed test. The property is asserted only in the pure unit test
  `application::workflow::board::tests::a_column_past_the_cap_is_cut_and_reads_more_than_it_shows`,
  and only for a cut column (counted 50, read 51). There is no test that:
  - calls `run_board_counts`, admits a run, then calls `run_board_cards`;
  - checks `assemble_columns` gives `total` = cards read for a column that is not cut;
  - checks that the next read counts the run.

  The seam G1 F6 asked for (`assemble_columns` pure, ports separate) exists, and
  `run_board_tests.rs`'s `read` helper already performs the two reads.
- **Required:** add `run_board_read_*` coverage for that scenario, e.g. counts → `admitted(...)` →
  cards → assemble (total = cards read, not cut) → read again (counted).
- **Instruction lesson:** none supported (the author covered the logic, not the listed DB shape).
- **Resolution history:**
  - 2026-10-31 (F03-B1-R1): open.
  - 2026-10-31 (F03-B1-R2, digest `42c1472d862a198d1830c1949de8878bb85c5674f4cf4559e3cdae1ec41fa484`): **fixed.** `run_board_read_counts_a_run_that_arrives_between_its_reads_on_the_next_read`
    calls counts, admits a run, then calls cards and assembles. The column is not cut, its total is
    2 (the cards read), and the next read counts 2. It passed in the reviewer's run
    (`logs/review-b1-r2-focused.txt`).

## F03-B1-F3 — Optional: the child rollup's match keeps a catch-all, so a new run state would be counted as neither open nor failed

- **ID and context:** F03-B1-F3, 2026-10-31, batch review F03-B1-R1; other context as F03-B1-F1.
- **Classification:** optional suggestion, Low; category maintainability (`src/AGENTS.md`: parse a
  status into an enum "so the *match* is exhaustive and a new variant is a compile error").
- **Evidence:** `child_rollup` (`src/adapters/persistence/workflow/reads.rs`) now reads
  `match RunStateName::parse(&state) { Some(state) if state.is_open() => …, Some(RunStateName::Failed)
  => …, Some(_) => {}, None => return Err(invalid()) }`. The unknown-string case is fixed (F02-B1-F2).
  But `Some(_)` absorbs any future `RunStateName` variant silently.
- **Suggested:** spell `Some(RunStateName::Succeeded | RunStateName::Cancelled) => {}` (the guard
  arm can stay), so adding a state forces a decision here.
- **Instruction lesson:** none.
- **Resolution history:**
  - 2026-10-31 (F03-B1-R1): open (optional).
  - 2026-10-31 (F03-B1-R2, digest `42c1472d862a198d1830c1949de8878bb85c5674f4cf4559e3cdae1ec41fa484`): **dismissed** (A14). The author's rationale holds, and my suggestion was flawed:
    - guarded arms do not count toward exhaustiveness, so with the `Some(state) if state.is_open()`
      arm, replacing `Some(_)` by `Some(Succeeded | Cancelled)` would not compile without naming the
      open states again;
    - naming them again would make a second definition of "open" beside `RunStateName::is_open`,
      which is already an exhaustive `const fn`.

## F03-B1-F4 — Optional: the evidence logs TASK.md cites are gitignored and will not be committed

- **ID and context:** F03-B1-F4, 2026-10-31, batch review F03-B1-R1; other context as F03-B1-F1.
- **Classification:** optional suggestion, Low; category records / evidence retention (workflow
  skill: "Retain notes, findings, gated-decision text, and supporting evidence after completion").
- **Evidence:** `.gitignore:3` ignores `*.log`. TASK.md's "Check results" cites:
  - `logs/B1-focused-1..6.log`;
  - `logs/check-1.log` and `logs/check-2.log`;
  - `logs/lease-budget-1.log`.

  `git check-ignore -v` confirms they are ignored, so the commit will drop them. f01 and f02 did the
  same: f02's TASK.md cites `logs/check-2.log`, `logs/focused-1.log` and others, and none is in
  `git ls-files`. The `.md` and `.txt` evidence is kept.
- **Suggested:** save check output as `.txt`, as the review logs are, or `git add -f` the cited logs
  at commit.
- **Instruction lesson:** recurring across f01, f02 and f03. Candidate (moderate confidence; scope
  workflow skill, "Implementation, review, and acceptance" step 1): "Write check logs under
  `<phase-docs>/logs/` with an extension the repository does not ignore (`.txt`), and confirm with
  `git check-ignore` before citing them."
- **Resolution history:**
  - 2026-10-31 (F03-B1-R1): open (optional).
  - 2026-10-31 (F03-B1-R2, digest `42c1472d862a198d1830c1949de8878bb85c5674f4cf4559e3cdae1ec41fa484`): **fixed** (A15).
    - The focused and lease logs are `.txt`. `check-1.txt` and `check-2.txt` are summaries (gate
      table and totals) of the full `.log` files, which stay local.
    - `git check-ignore` reports nothing ignored among `logs/*.txt` and `logs/*.md`, and TASK.md no
      longer cites a `.log` path.
    - The author's trailing-whitespace strip also touched the reviewer's `review-*.txt` files
      (disclosed; whitespace only), which is acceptable.
