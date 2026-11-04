# 08 — Analytics and the attribution dashboard

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [07](07-orders-refunds-and-attribution.md). Attribution serves its own complete dashboard
and reporting API; report totals reconcile with durable order/refund facts and observable tracking.
Openbot contains only navigation and the identity/company integration from phase 03.

## Batch 08A — Define and implement reporting semantics

1. Implement `GET /v1/analytics` with authorized company scope, bounded date ranges, store, ad,
   campaign, advertised-product and sold-product filters, currency, and pagination. Validate IANA
   reporting zones and use half-open timestamp intervals with explicit daylight-saving behavior.
2. Return raw/filtered clicks, distinct observation types, attributed paid orders, unattributed
   orders/reasons, sold units, merchandise amounts after discounts, refunded amounts/returned
   units and net merchandise revenue. Keep taxes and shipping separate, and never sum currencies.
3. Specify clock semantics in the API and UI: clicks by click time; observations by observation
   time; paid orders and sales by paid time; refunds by refund occurrence time. Attribution
   eligibility uses order creation time. All stored instants are UTC; grouping uses the selected
   zone. Period net revenue can be negative when refunds relate to earlier-period orders.
4. Label period conversion rate as attributed paid orders in the period divided by bot-filtered
   clicks in the period. It is not a click-cohort rate, may exceed 100%, and is undefined with zero
   clicks. Do not silently clamp or replace undefined with a meaningful-looking zero.
5. Preserve advertised-product versus other-purchased-product breakdowns. A sold-product filter
   cannot imply that all clicks were on that product: label its denominator explicitly or omit
   the rate. Keep advertised-product click filtering distinct from sold-product order filtering.
   Count multi-product orders distinctly; aggregate clicks/orders/lines/refunds independently before
   joining so one-to-many joins cannot multiply metrics. Do not sum repeated click denominators
   across currency rows.
6. Start with indexed PostgreSQL queries; inspect representative query plans and enforce query
   and export bounds. Add preaggregation only for measured need. ClickHouse and replayable
   analytics replication remain later work with a separate measured justification.

Focused checks: `attribution_analytics_` using a fixed ledger with two companies/stores, two ads,
multi-product orders, unattributed orders, two currencies, partial/full refunds, zero clicks and
timezone/DST boundaries. Reconcile query outputs by hand and via independent expected totals.
Capture query plans/latency on the agreed pilot data volume; include foreign-object filter attacks.

## Batch 08B — Complete the independent dashboard

1. Build report overview, ad/campaign/product breakdowns, order/refund detail, unmatched events,
   link creation, store setup and credential administration in attribution's frontend. The UI
   consumes its own backend; it never reads Openbot tables or embeds Openbot-specific templates.
2. Clearly label estimated pixel loads, viewport observations, heuristic bot-filtered counts,
   reporting zone, date basis, currency, revenue definition and period conversion-rate formula.
   State that blocked tracking and unobserved social impressions are not measured.
3. Use ordinary refresh/filter navigation with accessible labels, keyboard focus, empty/error
   states and responsive tables. Bound queries and cancel superseded reads safely; accepted
   writes must not disappear because a browser navigation is cancelled.
4. Expose installation instructions and merchant delivery/attribution diagnostics without secrets
   or customer PII. Manage-account/company/member links take users to Openbot's authorized settings.
   API keys are shown only once on authorized creation, never in analytics or public snippets.

Focused checks: `attribution_dashboard_`, browser journeys for role-based controls, CSRF, HTML/script
escaping, tenant switching, asynchronous filter races where applicable, and reconciliation with
the API fixtures. Assert dashboard assets/routes are entirely owned by the service build.

Completion: users can configure tracking, inspect a sale/refund and report its ad/product outcome
without using Openbot's UI for attribution work. Record commands/counts/results; final gate in
[10](10-integration-verification-and-pilot.md).
