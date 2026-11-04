# 07 — Orders, refunds and attribution

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [06](06-website-tracking-and-checkout-handoff.md) and the selected checkout platform.
Verified paid orders produce stable ad/product attribution; retries, cancellation, refunds, and
out-of-order delivery cannot duplicate revenue or silently change the chosen click.

## Batch 07A — Authenticate and durably deduplicate merchant events

1. Implement `POST /v1/conversions` for scoped merchant credentials and the selected platform's
   native callback adapter. Verify its actual signature over the raw bounded body, timestamp and
   replay contract before parsing trusted order data. Capture click evidence in that platform's
   server-controlled checkout metadata as proved in phase 06.
2. Normalize to paid-order, refund and cancellation events with store/external event ID, external
   order ID, order-created/paid/event timestamps, click evidence, currency and actual line items.
   Each line includes stable line/product IDs, quantity, discounted merchandise amount, and
   separate tax/shipping allocations as required by the integration.
3. Use integer minor units with an explicit supported currency exponent; reject floating-point
   ambiguity, unsupported currencies, overflow, invalid quantities and inconsistent totals.
   Event IDs and order IDs are distinct. Unique order identity is company + store + external ID.
4. Persist an immutable event envelope/fingerprint with uniqueness on company + store + external
   event ID. Exact retries return the existing result; differing bodies under the same ID conflict.
   Commit accepted event storage and any immediate order mutation in one transaction. A successful
   response implies durable storage; 202 may mean durably pending reconciliation, not a paid sale.
5. Rate-limit and bound ingestion; use retryable failures for temporary storage/admission problems.
   Ensure provider response semantics cause the expected retries. No browser endpoint can invoke
   this trusted path and no API-supplied tenant field overrides the authenticated store.

Focused checks: `attribution_merchant_ingress_` for real provider signature fixtures, malformed totals,
foreign store/click, identical/conflicting retries, and multiple simultaneous deliveries of one
event. Inject transaction failure and commit-acknowledgment loss; retries must leave one durable event.

## Batch 07B — Settle orders, attribution and refunds transactionally

1. Serialize changes on the scoped order row using database constraints/locking, including
   concurrent first creation. Store a unique paid-order fact and stable product lines. A second
   paid event with a different event ID cannot count the order twice; inconsistent paid facts
   become a visible reconciliation conflict instead of overwriting history.
2. At the first accepted paid fact, validate the merchant's selected click: same company/store,
   permitted classification, `order_created_at - 30 days <= clicked_at <= order_created_at`.
   Save chosen click, ad/campaign/advertised-product snapshot, policy/version and eligibility
   reason on the order. Later clicks, refunds and duplicate events cannot reattribute it.
3. Missing, expired or otherwise ineligible evidence leaves the paid order explicitly unattributed
   with a reason. A foreign tenant's click must never expose its details or become attribution.
   Keep received time, click time, order creation and paid time as distinct timestamps.
4. Deduplicate refunds by their stable external refund ID as well as delivery event ID. Record
   line-level refund amounts/quantities with separate tax/shipping amounts; reject over-refunds
   and inconsistent currency. Support partial and full refunds using actual amounts, without
   assuming a quantity return always equals a monetary refund. Paid-order counts remain historical
   paid orders; refund and returned-unit counts are distinct metrics.
5. A cancellation before payment creates no sale. Cancelling a paid order alone does not reverse
   revenue; a verified refund does. A valid out-of-order payment/refund/cancellation sequence must
   converge on the same facts as ordered delivery, using provider facts rather than arrival order.
6. Durably retain events whose prerequisite paid fact has not arrived. Use a bounded supervised
   reconciliation loop in the same application, with database claims, attempt/deadline policy and
   observable unresolved state. Retry safely after crashes and lease loss; an early refund cannot
   be acknowledged then forgotten. Terminal unresolved events remain inspectable/replayable under
   permission. Do not add a separate broker or generic workflow engine.

Focused checks: `attribution_order_`, `attribution_refund_`, `attribution_policy_`, and
`attribution_reconciliation_` against real PostgreSQL. Include competing paid events, distinct refund
events racing on the same balance, duplicate refund IDs with different event IDs, refund before
payment, cancel/payment reordering, missing prerequisite, worker recovery, lease loss, and cleanup
versus reconciliation. Test the exact 30-day boundary, future clicks, multiple currencies and
multi-product orders. Every concurrency protocol needs actual competing claimants.

Completion: a real selected-store test purchase travels from tracking link to paid order and refund,
and replay preserves exact amounts and attribution. Record batch commands/counts/results and any
unavailable provider sandbox checks. Full-suite gate: [10](10-integration-verification-and-pilot.md).
