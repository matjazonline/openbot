# P08 independent findings

## AS-P08-B1-F1 — duplicate CSV header discards the group numerator — OPEN

- Date/review: 2026-11-16, AS-P08-B1-R1, note AS-P08-B1-N1;
  implement-astra-reviewed-2-3, p08 whole implementation. Independent retained B
  /root/attribution_reviewer, requested Astra/high, actual telemetry unavailable.
- Artifact: `e31e15080831480b5ad749be84bb980250149b98732741d01b43533caae40e91`;
  base/HEAD6e55258cbbdc7633940805f366fb17fe240f02aa, isolated worktree resolved in REVIEW.
- Classification: actionable correctness/contract defect, **P2**.
- Evidence: services/attribution/src/adapters/analytics/export.rs:14–33 declares
  `attributed_paid_orders` both in the group header (column9 zero-based) and the
  per-currency fields (column16). Lines49–71 serialize the group numerator;75–84
  serialize a different per-currency measure under that same name.
- Reproduction: compiled the actual production export.rs with production Analytics
  types in the reviewer-only probe. A group with one attributed EUR order and one
  attributed USD order correctly has group numerator2 and per-currency numerator1.
  The CSV first row contains both2 and1 under the duplicate name. Python's standard
  csv.DictReader keeps only1, silently losing the group-wide value. On subsequent
  currency rows the same named field contains1 despite the documented group field
  being blank. This breaks reconciliation by column name and makes the documented
  once-per-group numerator ambiguous. It affects API and dashboard downloads, both
  of which call this serializer.
- Required correction: make group-level and currency-level metric names unique and
  explicit in the export contract. Add a parsed CSV regression with attributed
  orders in at least two currencies, proving both measures survive by name and
  group fields are populated only on the first currency row. This is an implementation
  correction under D1/r1, not a reason to change aggregation or currency semantics.
- Evidence files: [production CSV](logs/AS-P08-B1-R1-reviewer-csv.csv),
  [analysis](logs/AS-P08-B1-R1-reviewer-csv-analysis.txt),
  [probe](logs/AS-P08-B1-reviewer-csv-probe/main.rs),
  [build output](logs/AS-P08-B1-R1-reviewer-csv-build.txt).
  The probe tests representation of a constructed report, not SQL/ingress; actual
  two-currency SQL behavior was separately inspected/tested.
- Instruction lesson: AC4/AC5 export checks verified download/substrings, bounds and
  formula escaping but never parsed column identity/meaning. Candidate scoped rule:
  for flattened nested report exports, parse the artifact and reconcile differently
  scoped same-named measures through unique headers. No instruction files edited.
- Resolution history: 2026-11-16 OPEN; author correction/recheck required.

## AS-P08-B1-F2 — product CSV omits its nonadditive metric basis — OPEN

- Date/review/settings/artifact: same AS-P08-B1-R1 / AS-P08-B1-N1 identity above.
- Classification: actionable requirements/semantic-contract defect, **P2**.
- Requirement: accepted AS-P08-D1/r1 item5 explicitly requires UI/export labels to
  forbid adding repeated contextual click/observation counts and overlapping product
  order/refund counts/tax/shipping. C007 carries those nonadditive meanings to consumers;
  original08A.5 requires advertised-product versus purchased-product distinctions.
- Evidence: export.rs:28–47,49–84 writes generic `group`, raw/eligible clicks and
  order/refund/tax/shipping columns. It carries neither grouping/filter basis nor
  any warning that those measures repeat across purchased products. The only clue,
  `omitted_sold_product_filter`, explains rate omission, not overlapping count or
  amount semantics. The HTML explains this in views.rs:176, and REPORTING/OpenAPI
  prose explains it, but none of that labeling travels in the downloaded CSV.
- Reproduction: the retained production-serializer CSV contains raw_clicks1 for
  product-x and raw_clicks1 for product-y although both reference the same one
  contextual click. All raw-click cells look additive, and the file does not even
  identify whether `group` is a sold product, advertised product or other grouping.
  Whole-order tax/shipping and distinct order/refund counts can likewise overlap
  by the accepted design. A detached downloaded report can therefore be interpreted
  as two product clicks or summed into inflated order-level totals without warning.
- Required correction: carry explicit grouping/metric-basis and nonadditivity labels
  with the export itself, including contextual observations/clicks and overlapping
  order/refund/tax/shipping totals for purchased-product groups. Preserve matching-line
  money/units, rate omission, the one-per-group currency-row rule and10,000-data-row
  bound. Add a parsed multi-product export assertion that checks both numbers and
  these labels; external documentation alone does not satisfy the gated export-label
  promise. Exact CSV representation remains the author's implementation choice.
- Evidence: same production CSV/probe/analysis links as F1; decision
  [AS-P08-D1/r1](logs/AS-P08-D1-r1.md), item5, and active
  [coordination23](logs/AS-P08-B1-R1-coordination-r23.md), C007/r1.
- Instruction lesson: the approved design identified this consumer risk correctly;
  implementation tests omitted the exported semantic labels. Candidate scoped rule:
  verify required metric caveats on each delivered artifact, not only the dashboard
  or its documentation. No instruction edits authorized/performed.
- Resolution history: 2026-11-16 OPEN; author correction/recheck required.


## 2026-11-16 — AS-P08-B1-R2 resolution recheck

Artifact `99d5627b7bc25cb34d828005e9539a07cf5e40eb76c35fb7b1276feeddaa225e`,
note **AS-P08-B1-N2**. Same reviewer/worktree/base/settings and nine exclusions;
coordination23/C001–C007/r1 unchanged. Original R1 findings and reproduction retained.

- **AS-P08-B1-F1: CLOSED — fixed.** export.rs now names every currency metric
  `currency_<metric>`, retaining `attributed_paid_orders` solely for the group-wide
  numerator. The real HTTP regression creates one attributed EUR order and one
  attributed USD order, parses actual downloaded bytes with stdlib csv.DictReader,
  asserts unique headers, group2 versus currency1, and blank later group fields.
  Independently executed this regression as part of9 analytics tests: PASS.
- **AS-P08-B1-F2: CLOSED — fixed.** every exported row carries group_by,
  click_observation_basis, financial_basis and nonadditive_measures. For purchased
  product groups these explicitly state contextual/non-product counts, matching-line
  money/units, whole-order/refund counts/tax/shipping and the prohibition on summing
  across product groups. Both API and dashboard pass validated grouping to the same
  serializer. Parsed real two-product/two-currency output proves product2400/3500
  money versus repeated1180 tax/500 shipping with required labels on every row.
  Independently executed: PASS. Existing exact10000-row acceptance/10001 rejection
  and formula-escaping assertions remain passing with the added columns.

Evidence: [independent9-test output](logs/AS-P08-B1-R2-reviewer-analytics.txt),
[delta audit](logs/AS-P08-B1-R2-reviewer-delta-check.txt),
[identity](logs/AS-P08-B1-R2-reviewer-identity.txt),
[manifest validation](logs/AS-P08-B1-R2-reviewer-evidence.txt).
Author R2 Chromium1, offline compilation, format and contracts15 PASS evidence
inspected; unaffected R1 SQL/authorization/browser/boundary evidence reused only
after exact source-delta verification. No open p08 findings remain.
