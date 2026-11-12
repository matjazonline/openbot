# p02 REVIEW

Review record for phase `p02` (`plan/01-departments/02-desks-ownership-and-access.md`), workflow
`implement-opus-reviewed-2_2`. Notes: `agent-review-notes.md`; findings: `agent-review-findings.md`.

## Rounds

| Round | Date | Stage | Input identity | Reviewer | Outcome | Findings |
| --- | --- | --- | --- | --- | --- | --- |
| 01DEP-P02-G1-R1 | 2026-11-10 | Architecture gate G1 (DEP-P02-D1..D7) | Decision **r1**, `logs/gate-decisions-r1.md` sha256 `6c5ec0d1e4ceed6957c9943982df51b71579820ae5f5c692b37ead1cdcdbd214`; coordination **r2** (`logs/coordination-r2.md` sha256 `d33f363c…d02f`), I001–I003/r1, C001/r1, C002/r1; HEAD `57adfa7a`; code snapshot (informational) `bc910984…0ddc` | B, Opus 5.5, agent `ad409b2d3d149c2c6` | **Changes required** (no PASS for r1) | Actionable: F1 (High), F2 (Medium), F3 (Medium), F4 (Low), F5 (Low). Optional: F6, F7, F8 |
| 01DEP-P02-G1-R2 | 2026-11-10 | Correction recheck of gate G1 | Decision **r2**, `logs/gate-decisions-r2.md` sha256 `0bee3eb0a7ff04b0e67322e8e97913404da355b3ac1607534bf6f866c9061bd6`; coordination r2 unchanged (`d33f363c…d02f`), C001/C002 acknowledged; HEAD `57adfa7a` | B, Opus 5.5, agent `ad409b2d3d149c2c6` | **PASS for decision r2** | F1–F5 fixed; F6, F8 fixed; F7 fixed in part (re-authorization declined, reason accepted); new optional F9 |
| 01DEP-P02-B2-R1 | 2026-11-10 | Batch review B2 (02C desk wording) | Digest `6cd8b092ebeda61840e7d22172124e546cb80372e1b99ca96c24528115dee5bf` (excl. phase docs + COORDINATION.md), base/HEAD `57adfa7a` + working tree; coordination r2 unchanged; manifest `logs/01DEP-P02-B2-R1.evidence.json` | B, Opus 5.5, agent `ad409b2d3d149c2c6` | **PASS for digest `6cd8b092…e5bf`** | Optional: B2-F1, B2-F2, B2-F3. Pre-existing unrelated failure independently confirmed |
| 01DEP-P02-B1-R1 | 2026-11-10 | Batch review B1 (02A + 02B + D5 + relay/thread-party) | Digest `a3b1ed11e9a8ecbce73549e00d0afebef9dc1f6971e124d6d16e24a435faada8`, base/HEAD `0c5df403` + working tree; decisions r2 (`0bee3eb0…1bd6`); coordination r2 unchanged; manifest `logs/01DEP-P02-B1-R1.evidence.json` | B, Opus 5.5, agent `ad409b2d3d149c2c6` | **Changes required** (no PASS) | Actionable: B1-F1 (Low), B1-F2 (Low), B1-F3 (Low) |
| 01DEP-P02-B1-R2 | 2026-11-10 | Correction recheck of B1 | Digest `cbca0a00bf06f97d453c5191aac2f98984cf9062aae79101246ba6bd8aa5438f` (same exclusions), base/HEAD `0c5df403` + working tree; decisions r2 and coordination r2 unchanged; manifest `logs/01DEP-P02-B1-R2.evidence.json` | B, Opus 5.5, agent `ad409b2d3d149c2c6` | **PASS for digest `cbca0a00…438f`** | B1-F1, B1-F2, B1-F3 fixed |

## Findings status (after B1-R2)

- B1: no open actionable findings. B1-F1, B1-F2 and B1-F3 are fixed.
- Open optional, for B2: B2-F1 (the source scan misses plain strings passed to page helpers), B2-F2 (a few messages still say channel or mix both words), B2-F3 ("Continue to desk").

## Findings status (after B1-R1)

- Open actionable (B1):
  - B1-F1: a department write that removes a principal's membership or role can deadlock with an agent or account deletion of that principal;
  - B1-F2: no test checks that a derived holder's email stays out of the form's allowlist;
  - B1-F3: the JSON ownership fields have no route test.

## Findings status (after B2-R1)

- B2: no actionable findings.
- Open optional for B2:
  - B2-F1: the source scan does not check plain strings passed to page helpers;
  - B2-F2: a few user-visible messages still say channel or mix both words;
  - B2-F3: "Continue to desk".
- `pages::tests::rendered_markup_carries_no_inline_javascript` fails independently of B2. The offender is `workflow_authoring/sample.rs:153`, unchanged since before p01.

## Findings status (after G1-R2)

- Closed: F1–F5 (fixed), F6 and F8 (optional, fixed), F7 (optional, fixed in part; the declined part has an accepted reason).
- Open optional: F9 (a p01 schema test still uses the old grant conflict target; add it to the contract table and rewrite it).

## Open findings at G1-R1

- 01DEP-P02-G1-F1 (High): the grants PK has no provenance, so derived and configured grants for
  one principal collide. A current member can lose access, and an allowlist entry can vanish.
- 01DEP-P02-G1-F2 (Medium): saving the channel form (`ownership: None`) on a members-only desk
  writes `team` or `public`, which the members-only allowlist CHECK refuses.
- 01DEP-P02-G1-F3 (Medium): nothing says derived grants are deleted when a desk leaves members-only,
  so former members could keep access.
- 01DEP-P02-G1-F4 (Low): the residual risks recorded are incomplete:
  - the possible deadlock between inbound commits and the demotion trigger;
  - the revision bump in the demotion trigger without the channel gate.
- 01DEP-P02-G1-F5 (Low): the TASK has no contract-impact table.
- Optional: F6 (gate waits under the 1 s lock timeout), F7 (a named `_on` ownership setter for p03,
  the PUT exception doc, under-lock re-authorization), F8 (posting into existing threads after
  removal).

Evidence: `logs/01DEP-P02-G1-R1-sql-probes.txt` (reviewer SQL probe on a scratch DB).
