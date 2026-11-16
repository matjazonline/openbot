# Reports and dashboard

Open **Reports** after choosing a company. Owners, administrators and members can
read. Tracking setup, links, installation instructions and credential management
remain in **Tracking setup and keys**; account/company membership is managed in
Openbot. The service owns every report page, stylesheet and script.

`GET /v1/analytics` requires a live session plus `X-Company-ID`, or a server key
with `analytics:read` scoped to its store. Public installation IDs cannot read.
Provide RFC3339 `start` and `end`, with an IANA `timezone` (for example,
`Europe/Berlin`). The half-open interval includes start and excludes end, at most
93 elapsed days. Local dashboard date fields convert midnight in the selected
zone; DST days can be 23 or 25 hours. Ambiguous midnight uses the earliest instant;
a nonexistent midnight is an error. The page shows the resulting UTC interval.

Optional filters are `store_id`, `ad_id`, `campaign_id`, `advertised_product_id`,
`sold_product_id` and `currency`. UUID objects must belong to the selected company
and key store even when the user belongs to both companies. Group with `group_by`
`day`, `ad`, `campaign`, `advertised_product` or `sold_product`; omit for overall
totals. Groups use frozen identifiers, not current names. The suggestions contain
at most 100 current catalog choices; paste another reference from setup if needed.

Pagination defaults to 25 groups, maximum 100, using scope/filter-bound opaque
cursors. Analytics cursors allow 1024 ASCII bytes to accommodate 128-character
Unicode product keys; other API cursors remain capped at 512. Changing filters
invalidates a cursor; changing page size does not. Unknown/duplicate query fields
are rejected.

Counts have different clocks: raw and heuristic bot/prefetch-filtered clicks use
click time; image loads, landing-page loads and continuous viewport observations
use observation time; paid orders, units and merchandise use paid time; refunds
and returns use refund occurrence time. Order creation determines attribution
eligibility. Blocked tracking and unobserved social impressions are not measured.

Each currency is separate. Merchandise is after discounts, tax and shipping are
separate, and net merchandise is merchandise minus period refunds. Refunds from
older paid orders can make a period's net negative. Every paid order/refund counts
once before line aggregation; duplicate deliveries never add financial facts.
Period conversion = attributed paid orders / eligible clicks in the same period.
It is not a cohort rate, may exceed 100%, and is null with zero eligible clicks.

Advertised products filter frozen ad dimensions. Purchased-product filters and
groups use matching order/refund lines and omit conversion rates. Clicks and
observations describe the surrounding company/store/ad/campaign/advertised-product
scope, not the purchased product. Those contextual counts repeat in product groups.
Order/refund counts and whole-order tax/shipping can overlap across purchased
products; do not add those groups to get overall totals. Use Overview without a
purchased-product filter for independently aggregated overall totals.

`GET /v1/analytics/export` takes the same filters/grouping, without `limit` or
`cursor`, and returns CSV. It rejects more than 10,000 data rows rather than
truncating. Group counts/reasons/rates appear once, on the first currency row;
subsequent currency rows leave those fields blank. All per-currency measures have
unique `currency_` column names, including `currency_attributed_paid_orders`, while
`attributed_paid_orders` is the group-wide numerator. Every row carries `group_by`,
`click_observation_basis`, `financial_basis` and `nonadditive_measures`; purchased-
product exports explicitly warn against adding contextual counts, overlapping
order/refund counts or whole-order tax/shipping across product groups.
Money is integer minor units,
not formatted decimal currency. Text cells are quoted and spreadsheet formula
prefixes neutralized. CSV carries no secrets, raw envelopes or customer fields.

All report SQL has a two-second statement deadline. Amount/count aggregates
outside the published ±9,000,000,000,000 bound return 400; narrow the interval or
filters. Query/storage failure returns retryable 503. These are implementation
bounds, not approved production load targets. Historical financial dimensions
survive source changes; raw counts remain subject to configured retention.

**Orders and refunds** shows scoped immutable details and applied refunds.
**Merchant events** shows safe delivery status, reason and retry timing. Owners
and administrators can retry unresolved events; retries use CSRF and durable
idempotency metadata. Report filters use ordinary document navigation. A completed
write remains durable when its response or subsequent navigation is interrupted.
