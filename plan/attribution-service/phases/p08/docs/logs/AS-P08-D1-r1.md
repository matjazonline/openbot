## AS-P08-D1/r1 — proposed cohesive reporting decision

1. Application query validates explicit RFC3339 start/end, start<end and elapsed
   duration<=93 days; IANA zone via installed chrono-tz, optional store/ad/campaign/
   advertised-product/sold-product/currency, enum grouping (none/day/ad/campaign/
   advertised_product/sold_product), default25/max100 pages. Strict unknown/duplicate
   query rejection, distinct IDs, normalized UTC/timezone/filters before cursor hash.
   Scope every current UUID filter through company/merchant-store ownership; existing
   foreign objects404 even for actors legitimately in both companies. External product
   references are scoped strings, never global object lookups.
2. Reuse current tenant/user/merchant entry/locks. Move real shared Authorized enum
   and company/filter/actor/begin methods from catalog persistence to management owner;
   keep catalog resource permission rules in catalog, add a reusable exact permission/
   key-scope check for report reads. No new service/interface. User requires liveRead,
   merchant requires analytics:read; public IDs cannot read. Merchant query store can
   only equal key's store and last use commits with successful report. Auth precedes
   query processing where current adapters do it. Recheck in transaction.
3. One PostgreSQL statement with independently aggregated clicks, observations,
   paid-order counts/reasons, purchased lines, refund facts and refund lines joined
   only after aggregation by group/currency. Each SQL statement sees one MVCC snapshot.
   Existing tenant lock orders it against accepted producers; no cross-database data.
   Raw click=every redirect, eligible excludes bot OR prefetch. Observations counted by
   accepted kind. Use frozen click/observation/order dimensions, never live catalog
   names/product values for historical grouping. Paid order distinct before lines;
   refund distinct before refund lines; cancellation/pending/conflict facts excluded.
4. Clocks: click.clicked_at, observation.observed_at, order.paid_at, refund.occurred_at;
   all half-open[start,end). Refund joins order dimensions without restricting order
   paid time to the report interval, so period net merchandise may be negative.
   Day grouping uses UTC instant AT TIME ZONE validated zone. Never sum currencies.
   Conversion numerator is attributed paid orders across selected currency filter;
   denominator is filtered eligible clicks once per group (not per currency).
   Rate may exceed1; zero denominator=>null; no clamping or fake zero.
5. Sold-product filter/grouping uses only matching line money/units and counts each
   matching order/refund once in that group. Advertised-product filtering remains
   independent against frozen ad dimensions. With sold-product filtering/grouping,
   omit rate (null plus omitted_sold_product_filter). Click/observation counts reflect
   selected company/store/ad/campaign/advertised-product/time scope, not a sold product;
   for sold-product groups the same contextual counts can repeat and UI/export labels
   explicitly forbid adding them. Tax/shipping stay order/refund-level totals counted
   once per matching fact, because merchant contract does not allocate these to lines.
   Product groups can overlap in order/refund counts and tax/shipping; never sum them
   to overall totals. Overview is its own ungrouped backend aggregation, not detail sums.
6. Group key uses deterministic C collation with null first. Opaque AEAD keyset cursor
   binds normalized query hash including derived company/store and last group key;
   limit changes may vary page size only. Max128-character Unicode product key cannot
   always fit old512-byte cursor; set analytics request/response cursor bound1024 only,
   leave other cursors512. Worst-case128 four-byte Unicode key test must produce<=1024
   and resume exactly; reject1025. This is a public contract correction, not an excuse
   to remove the bound. Query canonical hash avoids carrying every filter in the cursor.
7. SET LOCAL statement_timeout='2s' inside report transaction plus existing request/
   tenant admission/time limits. SQL sums use exact PostgreSQL numeric/bigint conversion;
   reject any aggregate outside published ±9e12 report amount/count contract, never
   clamp/wrap/round. User can narrow filters/range. No preaggregation or new database.
   Examine EXPLAIN ANALYZE on fixed test data (10k clicks,1k orders,1k observations,
   representative refunds); test command/latencies/query plans retained. Two-second
   report budget is implementation regression target, not production SLO approval.
8. Add GET /v1/analytics/export with same session/key scope and normalized filters,
   no pagination bypass. Produce CSV with total10000 data-row cap, same2s query bound;
   stop with explicit invalid-request when too large, no partial successful export.
   Quote/escape CSV and neutralize untrusted text formula prefixes; numeric negative
   amounts remain numeric. Group click/order/reason/rate fields emitted once per group,
   not repeated per currency row. No persisted export files or new queue. Export's
   contract/limits travel with service. Existing README already promises export bounds.
9. Independent service-rendered /reports pages reuse the same report owner as API.
   Use ordinary GET form navigation to cancel superseded reads through browser document
   navigation; avoid a second asynchronous report-state engine. Local date fields convert
   selected zone's midnight to UTC server-side; earliest instant for ambiguous midnight,
   nonexistent midnight=>explicit error. Display actual UTC half-open range and localzone.
   Overview, group selection/table pagination, links to paid-order/refund detail and
   pending/conflict/unresolved events; own setup/link/key UI remains existing shell.
   Reuse commerce scoped read owners, widening crate visibility only, no duplicated SQL.
   Reports link from active-company shell; company selection/account management remain
   existing links. Escape every dynamic text/attribute; responsive tables, captions,
   focus and labels, empty/error states. Detail lists are bounded; never derive totals
   from visible rows. Minimal JS only for existing retry API, CSRF/idempotency metadata;
   no cancellation of writes by report navigation. Retry state remains server durable.
10. UI labels estimated image loads, continuous visible observations, heuristic click
    filtering, missing impressions/blocked tracking, distinct clocks, currency, discount
    merchandise and separate tax/shipping, negative period net, nullable/uncapped rate
    formula, advertised versus sold products and nonadditive product groups. Diagnostics
    expose accepted safe EventResult only; no raw envelope, credentials or customer PII.
    Installation instructions via current setup/WEBSITE docs with key server-only.
11. Publish C007/r1 for p08/p09/p10: report query clock/filter/group semantics, bounded
    analytics cursor/export, preserved financial/dimension consumers. P09 cleanup must
    not expose partially pruned facts to reports and must keep frozen report dimensions;
    no claim that raw counts survive their approved retention. P10 verifies combined UI,
    merchant scoping, export and retention effects.

