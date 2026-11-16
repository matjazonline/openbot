# P08 independent review

## AS-P08-G1 — architecture gate — 2026-11-16

**PASS AS-P08-D1/r1**, immutable SHA256
`69dcc8f4b01b904e3b89352ba1c9d2b498baef6740e7b145ebda6c56651b6a2f`.
Note **AS-P08-G1-N1** in [review notes](agent-review-notes.md). No actionable findings.
Dependent implementation may begin after root accepts this gate. No runtime,
implementation, phase or deployment acceptance is implied.

- Retained independent reviewer `/root/attribution_reviewer`, requested Astra/high;
  actual model/effort/context telemetry unavailable. Workflow implement-astra-reviewed-2-3.
- Worktree and p08 record authority:
  `/private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`.
- Detached HEAD/start/review-base `6e55258cbbdc7633940805f366fb17fe240f02aa`.
- Source baseline remains accepted p07
  `2712efab34ac16d2a2d368b4d83909d04b57b6e93eaf0740ef8262d6fbfd091d`.
- Main coordination authority:
  `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
  writer/root revision22; active C001–C006/r1, proposed C007/r1 reviewed.
  Main byte-matches [pin](logs/AS-P08-D1-coordination-r22.md).
- Inputs/evidence: [decision](logs/AS-P08-D1-r1.md), [TASK mapping](TASK.md),
  [identity checks](logs/AS-P08-G1-reviewer-identity.txt),
  [final snapshot](logs/AS-P08-G1-reviewer-end.snapshot.txt).
- Decision hash, HEAD, exact source equality, coordination pin and diff check PASS.
  All planned AC1–AC6 runtime assertions remain NOT EXERCISED; no manifest due at gate.

Exact nine snapshot exclusions:

1. `plan/attribution-service/phases/p01/docs`
2. `plan/attribution-service/phases/p02/docs`
3. `plan/attribution-service/phases/p03/docs`
4. `plan/attribution-service/phases/p04/docs`
5. `plan/attribution-service/phases/p05/docs`
6. `plan/attribution-service/phases/p06/docs`
7. `plan/attribution-service/phases/p07/docs`
8. `plan/attribution-service/phases/p08/docs`
9. `plan/attribution-service/COORDINATION.md`


## AS-P08-B1-R1 — whole implementation review — 2026-11-16

**CHANGES REQUIRED**, note **AS-P08-B1-N1** in [notes](agent-review-notes.md).
Two open actionable P2 findings in [findings](agent-review-findings.md):

- **AS-P08-B1-F1:** CSV duplicates attributed_paid_orders for group and currency
  scopes; named-column readers silently lose the group value.
- **AS-P08-B1-F2:** purchased-product CSV lacks the promised grouping/basis and
  nonadditivity labels for repeated contextual counts and order-level totals.

Reviewed artifact `e31e15080831480b5ad749be84bb980250149b98732741d01b43533caae40e91`;
HEAD/start/base6e55258cbbdc7633940805f366fb17fe240f02aa. Exact nine exclusions above.
Main coordination23/C001–C007/r1 byte-matches
[pin](logs/AS-P08-B1-R1-coordination-r23.md). Complete independent p07 comparison
confirms35-path delta. [Final identity](logs/AS-P08-B1-R1-reviewer-identity.txt) PASS.

Independent analytics8, dashboard Chromium1, catalog12 and boundary11 checks PASS;
production serializer probe reproduces both defects. Reviewed entire new reporting
SQL/domain/HTTP/dashboard/export/tests plus affected shared contracts. Author evidence
manifest7 PASS/2 FAIL valid, with historical failure dispositions retained. No p08
acceptance until both findings are corrected/rechecked. D1/r1 remains valid; p09/p10
cleanup/full integration and known unrelated expiry fixture still remain outside this
implementation review. No source changes by B and no owned process remains.


## AS-P08-B1-R2 — correction recheck — 2026-11-16

**PASS**, reviewer note **AS-P08-B1-N2** in [notes](agent-review-notes.md).
Exact artifact `99d5627b7bc25cb34d828005e9539a07cf5e40eb76c35fb7b1276feeddaa225e`;
HEAD/start/base6e55258cbbdc7633940805f366fb17fe240f02aa; same nine exclusions.
**AS-P08-B1-F1/F2 CLOSED** with [retained dispositions](agent-review-findings.md).
No remaining actionable p08 findings. D1/r1 and coordination23/C001–C007/r1 unchanged.

Independent9 analytics PG tests PASS, including parsed actual HTTP two-currency/
two-product CSV semantics and unchanged10000/10001-row bounds. Both API/dashboard
callers pass normalized grouping. Unique currency metric headers and delivered
basis/nonadditivity labels now match contract. Six changed files plus one new parser
verified; unaffected R1 full review/check evidence reused. Author R2 Chromium1 and
offline/format/contracts15 PASS inspected. [Manifest validation](logs/AS-P08-B1-R2-reviewer-evidence.txt)
10 PASS/2 historical FAIL entries; [final identity](logs/AS-P08-B1-R2-reviewer-identity.txt)
and [snapshot](logs/AS-P08-B1-R2-reviewer-end.snapshot.txt) PASS. Main authority
byte-matches retained R1 revision23 pin.

This pass permits root's p08 acceptance for the exact artifact. P09 actual retention,
p10 full integration and unrelated session-fixture correction remain required; no
production deployment, full-suite or native pilot acceptance is implied.
