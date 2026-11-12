# p01 REVIEW

Review record for phase `p01` (`plan/01-departments/01-departments-and-membership.md`), workflow
`implement-opus-reviewed-2_2`. Detailed notes are in `agent-review-notes.md` and findings in
`agent-review-findings.md` (this directory); this file links them.

## Rounds

| Round | Date | Stage | Input identity | Reviewer | Outcome | Findings |
| --- | --- | --- | --- | --- | --- | --- |
| 01DEP-P01-G1-R1 | 2026-11-09 | Architecture gate G1 (DEP-P01-D1..D5) | Decision **r1**, `logs/gate-decisions-r1.md` sha256 `c72805f2d0d41d433dfa37e386daf1ad14c11c62fcc6fd72e526f302cadeb572`; coordination r1 (`logs/coordination-r1.md` sha256 `babfa9b2…128b`), I001–I003/r1, no notices; HEAD `3752ea35`; code snapshot (informational) start `71e8276a…d130`, end `ca321ded…7eed` | B, Opus 5.5, agent `ad409b2d3d149c2c6` | **Changes required** (no PASS for r1) | Actionable: F1 (High), F2 (Medium), F3 (Medium), F4 (Low), F5 (Low). Optional: F6, F7, F8, F9 |
| 01DEP-P01-G1-R2 | 2026-11-09 | Correction recheck of gate G1 | Decision **r2**, `logs/gate-decisions-r2.md` sha256 `184eef17f46d7644b3536c004882ccde9b4f733161bf071ec8147e1a408823cd`; coordination r1 unchanged (`babfa9b2…128b`), I001–I003/r1, no notices; HEAD `3752ea35`; code snapshot (informational) `ca321ded…7eed` | B, Opus 5.5, agent `ad409b2d3d149c2c6` | **PASS for decision r2** | F1, F2, F4, F5 fixed. F3 fixed in decision, with a verification obligation carried to B1. F6–F8 fixed; F9 dismissed. New optional: F10 |
| 01DEP-P01-B1-R1 | 2026-11-09 | Batch review B1 (whole phase 01) | Digest `c2c75fadaa78619940c7b9c7b02ee20f9983d4d5116faa075d85a35d24ae07ed` (excl. phase docs + COORDINATION.md), HEAD `3752ea35` + working tree; decisions r2 (`184eef17…23cd`); coordination r1 (`babfa9b2…128b`), I001–I003/r1, no notices; manifest `logs/01DEP-P01-B1-R1.evidence.json` | B, Opus 5.5, agent `ad409b2d3d149c2c6` | **Changes required** (no PASS) | Actionable: B1-F1 (Low). Optional: B1-F2, B1-F3, B1-F4. G1-F3 closed; G1-F10 fixed |
| 01DEP-P01-B1-R2 | 2026-11-09 | Correction recheck of B1 | Digest `ccc4ee26be865cf9806a1e192f05bdddb8753eb4993704e2b8f98fcc78f03880` (same exclusions), HEAD `3752ea35` + working tree; decisions r2 unchanged; coordination r1 unchanged; manifest `logs/01DEP-P01-B1-R2.evidence.json` | B, Opus 5.5, agent `ad409b2d3d149c2c6` | **PASS for digest `ccc4ee26…3880`** | B1-F1 fixed; optional B1-F2, F3, F4 fixed (trivial doc leftover under F4) |

## Findings status (after B1-R2)

- No open actionable findings. B1-F1 is fixed, and B1-F2, B1-F3 and B1-F4 (optional) are fixed.
- All G1 findings are closed, fixed or dismissed.
- Reviewer evidence: `logs/01DEP-P01-B1-R2-reviewer-department-tests.txt` (34 passed).

## Findings status (after B1-R1)

- Open actionable: 01DEP-P01-B1-F1 (Low). No test checks that department writes are re-authorized inside the transaction; the schedule-test equivalent is missing.
- Open optional: B1-F2 (the schema comment gives the withdrawn NO ACTION reason), B1-F3 (route tests lack the failing-persistence and non-member list cases), B1-F4 (tidy-ups).
- G1-F10 (optional): fixed.
- G1-F3's carried obligation is met (closed).

## Findings status (after G1-R2)

The R1 summaries below are kept as written. Current dispositions are in `agent-review-findings.md`.

- Closed by r2: F1, F2, F4, F5 (fixed); F6, F7, F8 (optional, fixed); F9 (optional, dismissed with reason).
- F3: fixed in the decision. **Carried to the B1 batch review:** a name-only PATCH keeps parent, charter, description and slug, and `parent_id: null` re-roots the department.
- Open optional: F10 (test the kind race in the reverse order; take the principal `FOR KEY SHARE` before writing join rows).

## Open findings at G1-R1

- 01DEP-P01-G1-F1 (High): principals leave the company by demotion to `external`, not deletion, so
  department member and lead rows survive with external principals. D1's CASCADE rationale is wrong.
- 01DEP-P01-G1-F2 (Medium): D3's lock order leaves out the channel gate, the p02 channel-settings
  writer rule, and what the department row lock strength is relied on for. As written, p02 would get
  either a lost update or a deadlock.
- 01DEP-P01-G1-F3 (Medium): D5's PATCH has full-replacement semantics, so a name-only PATCH re-roots
  the department, clears its charter and description, and re-derives its slug.
- 01DEP-P01-G1-F4 (Low): the decisions don't say which JSON reads a plain member may use. The AC6
  "404 for member" row conflicts with `visible_departments`.
- 01DEP-P01-G1-F5 (Low): the competing-claimant assertions don't distinguish the company lock from
  the unique backstops, overlap is not forced, and the depth boundaries are not exact.
- Optional suggestions:
  - F6: the NO ACTION rationale is inaccurate, since RESTRICT also allows teardown;
  - F7: take the actor's user row before the company lock;
  - F8: re-authorization NotFound wording, and the route tests' 404-versus-`denial_cases` choice
    should be stated;
  - F9: a lock-witness type for the `_on` functions.

Evidence:

- `logs/01DEP-P01-G1-R1-sql-probes.txt` (R1)
- `logs/01DEP-P01-G1-R2-sql-probes.txt` (R2, sha256 `c54867cc…8dfb`)

Both are reviewer SQL probes on a scratch DB.

B1-R1 reviewer runs:

- `logs/01DEP-P01-B1-R1-reviewer-department-tests.txt` (32 passed)
- `logs/01DEP-P01-B1-R1-reviewer-affected-tests.txt` (80 passed)
