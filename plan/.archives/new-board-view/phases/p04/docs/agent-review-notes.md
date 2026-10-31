# NBV-P04 agent review notes

## NBV-P04-B1-R1 — batch review, round 1

### Identity

| Field | Value |
| --- | --- |
| Date | 2026-10-31 |
| Workflow | `implement-opus-reviewed-2_2` |
| Records | task [TASK.md](TASK.md); review [REVIEW.md](REVIEW.md); findings [agent-review-findings.md](agent-review-findings.md) |
| Batch / stage | B1 (whole phase 04), batch review |
| Reviewer | B, Opus 5.5/xhigh |
| Phase | NBV-P04 |
| HEAD | `92e8c35e67dfb71e73db87895301dee74f380898`, uncommitted |
| Review base | tree `f07c076b2242a35b0bd97bf400b1bcc854d04603` (NBV-P03 accepted at 38ee1d9e) |
| Phase state | tree `7a120c2817ecca236772a033beb3e9d69a6f735c` |
| Digest | `fc4955c759c6af2f6b146644847630ab7b43416ea8806ba0bbbfa2a88f61cf57` |
| Digest exclusions | `plan/new-board-view/phases`, `plan/new-board-view/COORDINATION.md` |
| Gated decisions | none |
| Author decisions | NBV-P04-D1..D4, reviewed |

- **Artifact checks:** the digest was confirmed at the start and before returning. The working tree equals
  tree `7a120c28`, checked through a temporary index.
- **Coordination:** r4. NBV-I1 r1 is satisfied: no schema change, and the Dashboard read adds a primary-key
  `LEFT JOIN`. No notice targets p04.

### Coverage and evidence

**Requirements read**

- `README.md` and `04-runs-in-navigation.md`.
- AGENTS files: root, `src` ("Name your tuples"), `http`, `pages` (escaping, icon sourcing), `persistence`
  (SQL style, the steps for adding a field).

**Delta read in full**

| Area | What was read |
| --- | --- |
| Rail | `RailDestination` and its `href` builder (form-urlencoded pairs, `company_id` appended, attribute-escaped) |
| Rail | `UiSection::Runs`, placed after Workflows |
| Rail | `rail_section_visible`: Runs uses `manages_company_operations`, matching `ui.company` |
| Runs views | both pages set `section: UiSection::Runs` |
| Icon | `Icon::Project` |
| Dashboard SQL | `OUTSTANDING_SQL`: `LEFT JOIN workflow_executions AS execution ON (company_id, id)` |
| Dashboard SQL | `workflow_executions_pkey` is `(company_id, id)`, confirmed in the init schema |
| Dashboard SQL | `LIMIT OUTSTANDING_LIMIT` |
| Domain | `OutstandingTask.run_id` |
| Dashboard page | `task_queue_panel`: same markup as `stat_row`, plus the note and the "Runs by state" link |
| Dashboard page | `outstanding_row`: a thread first, else the run through `run_href`, only where `openable` |
| Tests | rail, dashboard, persistence |

**Callers and contracts traced**

- The Dashboard's `linked_company` and `can_open` both use `load_managed_company` (owners and admins), the
  same set the runs routes admit. So no board or run link points at a page that refuses the caller.
- The run page re-checks channel visibility.
- No other `UiSection` match needs Runs.
- `url` is an existing crate.

**Glyph verified**

- Fetched `https://unpkg.com/@primer/octicons@19.15.0/build/svg/project-16.svg` into the scratchpad as
  untrusted data, then compared it in isolated Python.
- The path is byte-identical to `Icon::Project`: 434 characters (`logs/review-b1-octicons-compare.log`).

**Checks run** (DB-backed, `-- --test-threads=4`)

| Check | Result | Log |
| --- | --- | --- |
| `icon_rail_` | 2/2 | `logs/review-b1-icon_rail_.log` |
| `dashboard` | 49/49, including `workflow_job_projection_dashboard_outstanding_jobs_name_their_run` | `logs/review-b1-dashboard.log` |
| `run_board_view_` | 14/14 | `logs/review-b1-run_board_view_.log` |
| `pages::` | 176/176 | `logs/review-b1-pages__.log` |
| `npm run check:css` | exit 0 | `logs/review-b1-check-css.log` |

**Inspected, not rerun:** the author's `adapters::http::routes::` 232 and the clippy log.

**Not run:** a served-page browser pass, which is the 05 gate, and the full suite.

### Outcome

**PASS** for digest `fc4955c759c6af2f6b146644847630ab7b43416ea8806ba0bbbfa2a88f61cf57`.

| Finding | Kind | Severity | Summary |
| --- | --- | --- | --- |
| [F1](agent-review-findings.md) | optional | Low | The glyph's pinned source is recorded only in TASK.md; the icon module doc names Iconstack. The path itself is verified identical. |

No actionable findings.

### Process and instruction feedback

The batch was small and well-scoped. D1–D4 were precise and made each decision checkable. No instruction
change is supported.
