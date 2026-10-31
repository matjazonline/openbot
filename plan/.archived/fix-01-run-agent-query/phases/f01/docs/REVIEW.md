# f01 — review record

Workflow `implement-opus-reviewed-2_2`; task [TASK.md](TASK.md); notes
[agent-review-notes.md](agent-review-notes.md); findings
[agent-review-findings.md](agent-review-findings.md).

## Rounds

| Round | Date | Stage | Reviewed artifact | Outcome | Findings |
| --- | --- | --- | --- | --- | --- |
| [F01-B1-R1](agent-review-notes.md) | 2026-10-30 | B1 batch review (whole plan 01–03) | HEAD `a6dc3be` + working tree, digest `6d32795ab478c5a08c6d0bdfe1d139f77cb0fe6b34dde44dd5ce9a538860fae9`, exclusions `plan/fix-01-run-agent-query/phases/f01/docs` `plan/fix-01-run-agent-query/COORDINATION.md`; coordination rev 1, no notices | Changes required | F01-B1-F1 (Medium), F01-B1-F2 (Low), F01-B1-F3 (Low); optional F01-B1-F4, F01-B1-F5 |
| [F01-B1-R2](agent-review-notes.md) | 2026-10-30 | B1 correction recheck (F1–F5) | HEAD `a6dc3be` + working tree, digest `029f6b315d9ca85fc5667d872514f714bd9e804edcf67f500cf98e47df3fb540`, same exclusions; coordination rev 1, no notices | Changes required (Low only) | F1–F5 fixed; new F01-B1-F6 (Low), F01-B1-F7 (Low) |
| [F01-B1-R3](agent-review-notes.md) | 2026-10-30 | B1 correction recheck (F6, F7) | HEAD `a6dc3be` + working tree, digest `3cd0d212c470888aa55b3ede7edf83d1ac5e5f4de7e5019df20d4ed769563f3b`, same exclusions; coordination rev 1, no notices | **PASS** | F6, F7 fixed; new optional F01-B1-F8 |

## Findings

| ID | Kind | Severity | Summary | Status |
| --- | --- | --- | --- | --- |
| [F01-B1-F1](agent-review-findings.md) | actionable | Medium | D4's per-version lateral read was measured only at 50-row pages; at the 200-row default it reads 80,400 rows (~64% of the dense agent's history); bound claims and the 02-ladder disposition needed correcting | fixed (R2): both page sizes measured, bound stated everywhere, link-table plan written and deferred |
| [F01-B1-F2](agent-review-findings.md) | actionable | Low | run-list.md said the new index is written on insert only and state updates stay HOT | fixed (R2) |
| [F01-B1-F3](agent-review-findings.md) | actionable | Low | Plan 01 step 5: column COMMENT and `application/workflow/reads.rs:372` not updated; D2 rationale void | fixed (R2) |
| [F01-B1-F4](agent-review-findings.md) | optional | Low | Used-by route could call `bindings::company_agent` | fixed (R2, optional taken) |
| [F01-B1-F5](agent-review-findings.md) | optional | Low | Slow-read `returned` counts the look-ahead row | fixed (R2, renamed `rows_read`); naming side effect folded into F6 |
| [F01-B1-F6](agent-review-findings.md) | actionable | Low | Revisit-when reading for `agent=true, state=true` asks for the state value, which neither the line nor request spans carry; plan 03's `returned` reading dropped; deferred plan's trigger ignores agent + common state; `rows_read` collides with the table's "Rows" | fixed (R3): state logged as a run-state name/`none`/`other`, `page_rows`, readings rewritten |
| [F01-B1-F7](agent-review-findings.md) | actionable | Low | `plan/db_improve/run-agent-link-table.md` lacks per-batch focused checks and a final full-suite gate (root `AGENTS.md`) | fixed (R3) |
| [F01-B1-F8](agent-review-findings.md) | optional | Low | Run-state names listed twice (`logged_state`, UI `state_filter`); share one list | open (optional, R3) |
