# NBV-P02 agent review findings

## Common context

These fields apply to every finding below unless a finding says otherwise.

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Task record | [TASK.md](TASK.md) |
| Review record | [REVIEW.md](REVIEW.md) |
| Batch | B1 (whole phase 02) |
| Stage | batch review |
| Review round | [NBV-P02-B1-R1](agent-review-notes.md) |
| Reviewer | B, Opus 5.5/xhigh |
| Phase | NBV-P02 |
| Worktree | `/Users/mac02/.codex/worktrees/b109/openbot` |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898` |
| Phase delta | tree `85b05025ef161cd04a7e07081f168bb51f649a75` (NBV-P01 accepted) → tree `ed59ca7f2423bb3c5407fdf61335d9800f0afa2c` |
| Digest | `68e88e1d5fbbcc572cd7bd85b67c416eb7d2cc6e88c9db5e1f52c5df460926f1` |
| Digest exclusions | `plan/new-board-view/phases`, `plan/new-board-view/COORDINATION.md` |

The scratch harnesses cited below are in the reviewer's scratchpad, not in the repository:
`/private/tmp/claude-501/-Users-mac02--codex-worktrees-b109-openbot/147507f8-dc5b-4026-9dc4-af253abbbdfc/scratchpad/harness/`.

---

## NBV-P02-B1-F1 — List filters can be silently undone by a wake-up

The list's new workflow and agent filters lose to a wake-up that arrives while the filter read is still
running, and they stay lost.

- **Classification:** actionable defect · **Medium** · correctness (partial-page race), requirements.

### Evidence

**Two readers on one replace group.** `run_list` (`src/adapters/http/pages/workflow_runs/mod.rs`, `run_list`
format string) renders two readers that share `hx-sync="#run-list:replace"`:

- the filter form, which now carries the `workflow_id` and `agent_id` selects added by this batch through
  `scope_controls`;
- the reload leaf, which reads `hx-get="/ui/workflow-runs/list?{query}&part=entries"` on
  `sse:runs, sse:resync, run-selected from:body`.

The leaf's `{query}` is `RunFilterView::query` fixed at render time. The route test pins it in that form
(`board_view_tests.rs`, `run_board_view_filter_switch_and_links_keep_the_scope`:
`hx-get="/ui/workflow-runs/list?{scope}&amp;part=entries"`).

**What happens.** Suppose any visible run changes (`sse:runs`) or a pane opens while a filter change is in
flight:

1. The leaf aborts the filter request under `replace`.
2. The leaf reloads the entries for the old scope.
3. Because the region was never replaced, every later wake-up keeps reloading the old scope.
4. The select still shows the new choice.

"Load more" has the same baked `{query}` and the same replace group.

**Reproduced.** `list-race-p02.mjs` uses shipped htmx and `WORKFLOW_RUNS_SCRIPT`, with the list markup as
rendered. Log: `logs/review-b1-list-race-harness.log`:

```text
{"reads":["region:1","entries:any","entries:any"],"aborts":["/ui/workflow-runs/list"],"afterWake":{"select":"1","entries":"a"},"afterNextWake":{"select":"1","entries":"a"}}
```

The select stays on workflow 1 while the entries stay unscoped, across later wake-ups too.

**Required.** Three sources apply:

- Plan 02 step 4: "Each control uses `hx-sync="#run-board:replace"` (or `#run-list`) so the last choice
  wins".
- `src/adapters/http/AGENTS.md`: "last user intent wins", plus out-of-order tests for partial-page protocols.
- NBV-N2 asked this of the board only. The author applied it to the board (D3) and covered it in
  `workflow-runs.mjs`, but not to the list, where this batch adds the same kind of controls.

**Context.** The list's state select had the same shape before this batch. That part is pre-existing. This
batch extends the defect to the new workflow and agent controls.

### Impact

The defect is likely on an active company: the list leaf wakes on every visible run change. The list then shows
runs of a different scope than its controls say, until the reader changes a filter again.

### Suggested direction

Mirror the board:

- give the list form an id;
- have the leaf (and "Load more") `hx-include` it together with `#run-list-shown, #run-selected`;
- drop the scope from their URLs.

Add a list case to `workflow-runs.mjs` (a filter change in flight, then a wake-up).

### Instruction lesson

The cross-phase notice (NBV-N2) named the board only, and the same pattern in the sibling view was missed.
Candidate rule, scope `http/AGENTS.md` race-safe section: "When a replace group mixes a filter form with a
reload leaf, the leaf reads the form; check every view that has the pattern, not only the one a notice names."

### Resolution history

- 2026-10-31 R1: open.

---
- 2026-10-31 R2 ([NBV-P02-B1-R2](agent-review-notes.md); digest `c5538eb58dccc02187953dbf1ad7351919b6eefc3092b9028744f132c1858142`; tree `fac0b467`): **fixed**.
  - The list leaf is now `hx-get="/ui/workflow-runs/list?part=entries"` and "Load more" is
    `list?after=…&part=more`. Both `hx-include` `#run-list-filters`, the form, which carries `company_id`,
    `state`, binding, workflow and agent. Neither names a scope itself.
  - Pinned by `board_view_tests.rs` and the `runs_tests.rs` paging test.
  - Reviewer harness on the new markup (`list-race-p02-r2.mjs`, `logs/review-r2-list-race-harness.log`):
    `afterWake {select:1, entries:1}` and the same after the next wake-up. In R1 the entries were `any`.
  - New node test "list: a wake-up during a filter read reads the new filter instead of undoing it"; 6/6 pass
    (`logs/review-r2-workflow-runs-node.log`).
  - Residual note, not a defect: if "Load more" is pressed while a filter change is in flight, it now reads the
    new scope after the old list's cursor, so the list briefly mixes two scopes. The next wake-up re-reads from
    the top with the new scope, so it heals itself.

## NBV-P02-B1-F2 — The decision to add no workflow index (D6) does not follow the plan's evidence ladder

- **Classification:** actionable defect · **Medium** · performance/requirements (decision evidence).

### Evidence

**The plan's procedure.** Plan 02 step 1 says: "decide on an index as fix-01-run-agent-query 02 does".
That procedure is `plan/.archives/fix-01-run-agent-query/02-query-plan-evidence-and-index.md`, step 4:

- "Decide by access path, not local latency";
- "**No change** when every shape stops after about one page of matching index entries."

The doc says the same of its own data: synthetic temp-table plans are "evidence about access paths, not
production latency targets" (`docs/performance/run-list.md`, Dataset).

**The access path measured.** The generic plan is "the one that matters" (same doc), because sqlx caches
prepared statements. For the list's workflow shapes it is (`docs/performance/run-list-query-plan.txt`,
"workflow page 50/200" and "workflow_state failed …", `force_generic_plan`):

- `Index Scan using workflow_runs_company_id_id_workflow_id_key` with
  `Index Cond: ((company_id = $1) AND (workflow_id = $2))`;
- 8,750–8,806 buffers to return 51 or 201 rows;
- for `failed`, 0 rows returned and `Rows Removed by Filter: 6250`.

That index is the unique `(company_id, id, workflow_id)` (`workflow_run_workflow_identity`), so `workflow_id`
is checked across the company's whole `(company_id, id)` range: all 250,000 entries of the heavy tenant,
about 2,600 index buffers. The heap of every one of the workflow's runs is then read and sorted. Each page
therefore reads the tenant's entire index range plus the workflow's entire history.

**The doc misstates this.** It describes the generic plan as "reads the workflow's 6,250 runs and sorts them".
It does not mention the company-wide range scan.

**This is the pattern fix-01 rejected.** Its "Before" table rejected the same thing for the sparse agent: "scans
the tenant's whole `(company_id, id, workflow_id, version_id)` range". That result was a reason fix-01 adopted
an index.

**Missing cases.** The dataset has no dense workflow and no sparse workflow; every workflow is an even fortieth.
fix-01 deliberately measured a dense and a sparse agent.

- *Dense workflow.* A company that runs mostly one workflow is the common production case. There the generic
  plan's heap read becomes most of the tenant's history per page. Unmeasured.
- *Sparse workflow.* The custom plan walks `workflow_runs_newest` and drops non-matching rows (1,989 for an even
  fortieth). For a sparse workflow it walks most of the company's history. Unmeasured.

**The stated reason is latency.** D6 and the doc's Decision rely on "here it would replace a 20 ms read" and
"production evidence". That is the latency argument the ladder rules out. The persistence AGENTS rule also
says "A bounded result set does not imply bounded database work".

**What is fine.** The tripwire satisfies the AGENTS deferral rule: `workflow=` on both slow lines, and the
"Revisit when" row. The board's workflow scope uses the materialized working set like a binding, and its
evidence is fine.

### Impact

The list's workflow filter does work that grows with the tenant's retained history on every page. It is now one
click away: the workflow pane's "Runs board" link and the list and board selects. Today it is invisible below
the 500 ms tripwire.

### Suggested direction

Pick one, and correct the doc's access-path description either way:

1. **Follow the ladder.** Measure `(company_id, workflow_id, created_at DESC, id DESC)`, before and after,
   with dense and sparse workflow cases and a state filter. Add it in the init schema in place
   (NBV-I1: reset, no migration) if it reads about a page. Record the write cost as the version index
   section does.
2. **Defer by access path.** Add the dense and sparse workflow measurements and argue the deferral from them.
   For example, show the write cost outweighs a bounded read. Not from a 20 ms figure.

This is an author decision (D6) with no architecture gate held. Root decides.

### Instruction lesson

A deferral can cite the persistence rule against the plan's own ladder. Candidate for the plan template or
persistence AGENTS: "When a plan names an index decision ladder, the decision states which rung the evidence
meets, by access path. Every new filter dimension is measured dense and sparse." Evidence: this finding.

### Resolution history

- 2026-10-31 R1: open.

---
- 2026-10-31 R2 ([NBV-P02-B1-R2](agent-review-notes.md); digest `c5538eb58dccc02187953dbf1ad7351919b6eefc3092b9028744f132c1858142`; tree `fac0b467`): **fixed** (NBV-P02-D6 r2).
  - `workflow_runs_workflow_newest (company_id, workflow_id, created_at DESC, id DESC)` is added in place in
    `migrations/20260817000000_init_schema.sql:7322` (NBV-I1).
  - Before and after runs: `logs/qp-list-before.txt`, `logs/qp-list-after.txt`, `logs/qp-board-*.txt`. A
    "no runs" (sparsest) workflow case was added to the script.
  - The reviewer parsed both pairs:
    - Every list workflow shape moves onto the new index in both plan modes. Generic plan, a fortieth:
      23.6 → 0.11 ms. No runs: 7.5 → 0.01 ms.
    - The only remaining range read is workflow + a rare state (10.5 ms, the workflow's own range), which the
      doc names.
    - No non-workflow list shape changes its index set.
    - None of the 198 board shapes changes its index set or slows more than 2×.
  - The doc now states the access path correctly ("walks the tenant's whole `(company_id, id, workflow_id)`
    unique index").
  - No dense-workflow case was measured. A keyset index reads one page for a dense scope by construction, so
    this is accepted.
  - The write-cost sentence is wrong: logged as F6.

## NBV-P02-B1-F3 — Scope options are re-read on every board reload and the agent read is unbounded

- **Classification:** actionable defect, Low (may be routed to 03) · efficiency, bounding.

### Evidence

**Options read on every board reload.** `Ui::board_html` (`routes/workflows/ui/runs/mod.rs`) calls
`scope_options` on every `part=columns` read. That covers the window and filter form, and also the reload
leaf, which fires after every card click and, in 03, on wake-ups. The leaf keeps only
`#run-board-columns > section` (`hx-select`), so the options are thrown away on every leaf read. Each such
read adds:

- `list_definitions` (200);
- `verify_company_manager`;
- `list_company_agents`.

The list avoids this for its partial answers (D4). The board cannot, because the form and the leaf both send
`part=columns`.

**Unbounded agent read.** `list_company_agents` → `list_by_company_id` reads every agent row of the company,
and `scope_options` truncates to 200 afterwards. The bound applies to rendering, not to the read.

**Required.** Plan 02 step 4 says options are "bounded like the binding form's options". Those are cut in SQL:
`authoring_reads.rs`, `LIMIT MAX_BINDING_OPTIONS + 1`. `src/AGENTS.md` ("Bound work at every external
boundary") also applies. Unbounded `list_company_agents` is an existing pattern elsewhere; its first use on a
periodically refreshed path is new here.

### Impact

Small today. Once 03 puts wake-ups on the leaf, it costs three extra reads per wake-up per viewer, and the
agent read grows with the company's agents.

### Suggested direction

- Give the leaf its own part, for example `part=cards`, that skips options, or read options only when the
  toolbar is rendered.
- Bound the agent read in SQL, as the binding options are.
- Or record a disposition routing both to 03, whose rate bound covers the leaf.

### Instruction lesson

None beyond the existing bounding rule.

### Resolution history

- 2026-10-31 R1: open.

---
- 2026-10-31 R2 ([NBV-P02-B1-R2](agent-review-notes.md); digest `c5538eb58dccc02187953dbf1ad7351919b6eefc3092b9028744f132c1858142`; tree `fac0b467`): **fixed**, with a **reviewer correction to the R1 evidence**.
  - R1 called the agent read "unbounded". That was wrong: `list_by_company_id`
    (`src/adapters/persistence/agent.rs:332-343`) already had `ORDER BY created_at DESC, id DESC LIMIT 200`.
  - The actual defect behind it: the read stopped at the 200 *newest* agents. `agents.len() > 200` could never
    be true, so the cut note never showed, and agents beyond the newest 200 were dropped silently instead of
    the list being cut by name.
  - The correction fixes both:
    - New `AuthoringReads::agent_options` with `ORDER BY name, id LIMIT $2 (201)`; `truncated` is computed from
      the extra row (`authoring_reads.rs`).
    - The board leaf sends `reload=columns` through `hx-vals`, and `board_html` then skips `scope_options`.
  - The new query is a runtime string. It runs on every whole-region test read, so the column names are
    exercised; its labels are not pinned (F7).
  - The new `with_options: bool` parameter is F5.

## NBV-P02-B1-F4 — (optional) The workflow entry points are untested

- **Classification:** optional suggestion · Low · verification.

### Evidence

Two links added by this batch are not asserted by any test:

- the Workflows page "Runs" → `view=board` link (`pages/workflow_authoring/mod.rs`);
- the workflow pane's "Runs board" → `view=board&workflow_id` link (`workflow_pane`).

The agent card link is tested (`run_board_view_filter_switch_and_links_keep_the_scope`). Plan 02 step 5 put
the workflow link "in the library". D5 places it in the pane header instead, with a stated reason; that is
acceptable and is recorded here as context, not as a defect.

### Suggested direction

Assert both hrefs in an existing authoring page test.

### Resolution history

- 2026-10-31 R1: open (optional).
- 2026-10-31 R2 ([NBV-P02-B1-R2](agent-review-notes.md); digest `c5538eb58dccc02187953dbf1ad7351919b6eefc3092b9028744f132c1858142`; tree `fac0b467`): **fixed**. `run_board_view_filter_workflow_pages_link_to_the_board`
  (`routes/workflows/ui/authoring/tests.rs`) asserts both hrefs. It is included in `run_board_view_`, 11/11
  (`logs/review-r2-run_board_view_.log`).

---

## NBV-P02-B1-F5 — (optional) `board_html` takes a flag parameter

- **Context:** found in round [NBV-P02-B1-R2](agent-review-notes.md), digest
  `c5538eb58dccc02187953dbf1ad7351919b6eefc3092b9028744f132c1858142`. Otherwise as in the common context.
- **Classification:** optional suggestion · Low · code shape (`src/AGENTS.md`, "No flag parameters").
- **Evidence:**
  - `Ui::board_html(company, ids, window, selected, with_options: bool)` is called with
    `query.reload.as_deref() != Some("columns")` and with `true` (`routes/workflows/ui/runs/mod.rs`).
  - The bool selects behaviour: whether to read the toolbar's options.
  - The rule asks for an enum or separate functions. An example enum would be one naming what the answer
    replaces, the region or only its columns.
- **Resolution history:**
  - 2026-10-31 R2: open (optional).

---
  - 2026-10-31 R3 ([NBV-P02-B1-R3](agent-review-notes.md); digest `d6d1ff4f6c031002d47015c92a1bd859f872ee80f84e813c84e2a1d7d8b7ccb0`; tree `d68a8f1b`): **taken** (optional). `enum BoardRead { Region, Columns }` is parsed once by `BoardRead::of`
    from `reload`. The full page passes `Region`; `part=columns` passes `BoardRead::of(&query)`, so only the
    leaf's `reload=columns` skips the options.

## NBV-P02-B1-F6 — The new index's write cost is misstated in `docs/performance/run-list.md`

- **Context:** found in round [NBV-P02-B1-R2](agent-review-notes.md), digest
  `c5538eb58dccc02187953dbf1ad7351919b6eefc3092b9028744f132c1858142`. Otherwise as in the common context.
- **Classification:** actionable defect · **Low** · documentation accuracy (performance evidence).

### Evidence

`docs/performance/run-list.md:183-188`, in the "Workflow filter" section, says of
`workflow_runs_workflow_newest`: "Every insert writes it; a state change does not (no state column)".

The same doc says the opposite a few lines up (`run-list.md:147-150`): "`state` is already in
`workflow_runs_attention` and `workflow_runs_open_by_channel`, so a state change is never a HOT update. Every
insert and every state change therefore writes an entry into this index, as it does into the others."

The earlier statement is correct PostgreSQL behaviour, and it holds for this index too:

- A non-HOT update writes a new tuple version.
- A new tuple version gets an entry in every btree index, whether or not that index's columns changed.
- A state change is never HOT, because `state` is an indexed column.

The `finished_at` stamp on finishing is covered the same way, since it is in `workflow_runs_finished_by_state`.

### Impact

The doc is the evidence record the "Revisit when" rows rely on. It understates the new index's write cost on
the state-change path that leased operations take. The fix-01 version index recorded a `scripts/lease-budget.sh`
check for the same reason; none is recorded for this index.

### Suggested direction

- Correct the sentence: every insert and every state change write an entry.
- Run `scripts/lease-budget.sh` and record the result, or note that the 05 final gate (`scripts/check.sh`,
  which includes the lease budget) covers it.

### Instruction lesson

None. The doc's own earlier section already states the rule.

### Resolution history

- 2026-10-31 R2: open.

---
- 2026-10-31 R3 ([NBV-P02-B1-R3](agent-review-notes.md); digest `d6d1ff4f6c031002d47015c92a1bd859f872ee80f84e813c84e2a1d7d8b7ccb0`; tree `d68a8f1b`): **fixed**.
  - `run-list.md:187-189` now says every insert and every state change write the index: a state change is
    never HOT. The doc points to the version-index section.
  - The author ran `scripts/lease-budget.sh` (`logs/lease-budget.log`): 130 passed, and every work-phase share
    is under the 50% bound. The highest are `budget.reserve` 358/1000 ms and `lease.renew` 311/2000. TASK.md
    reports these unchanged from calibration (357 / 310).
  - The reviewer inspected that log but did not rerun the budget.

## NBV-P02-B1-F7 — (optional) No test pins the agent option labels

- **Context:** found in round [NBV-P02-B1-R2](agent-review-notes.md), digest
  `c5538eb58dccc02187953dbf1ad7351919b6eefc3092b9028744f132c1858142`. Otherwise as in the common context.
- **Classification:** optional suggestion · Low · verification.
- **Evidence:**
  - The route tests that check the agent select assert `<option value="{agent}" selected>`.
  - `scope_controls` also emits that markup for a chosen id that is not among the options, as a short-id
    fallback. So those assertions would pass even if `agent_options` returned no rows or wrong labels.
  - No test asserts an agent's name as an option label, the name order, or the 200-row cut at the SQL level.
- **Suggested direction:** in an existing filter test, assert `>assistant</option>` and `>idle</option>` in
  name order.
- **Resolution history:**
  - 2026-10-31 R2: open (optional).
  - 2026-10-31 R3 ([NBV-P02-B1-R3](agent-review-notes.md); digest `d6d1ff4f6c031002d47015c92a1bd859f872ee80f84e813c84e2a1d7d8b7ccb0`; tree `d68a8f1b`): **taken** (optional). `run_board_view_filter_scopes_both_views_alone_and_combined` now asserts, on
    an unscoped board, the label options `<option value="{assistant}">assistant</option>` and
    `<option value="{idle}">idle</option>`, in name order. These are not the selected-id fallback.
    11/11 pass (`logs/review-r3-run_board_view_.log`).
