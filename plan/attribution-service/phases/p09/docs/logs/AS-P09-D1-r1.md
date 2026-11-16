## AS-P09-D1/r1 — proposed retention and recovery ownership

1. Explicit required service retention keys (integer days1..10000):
   ATTRIBUTION_CLICK_RETENTION_DAYS, ATTRIBUTION_OBSERVATION_RETENTION_DAYS,
   ATTRIBUTION_FINANCIAL_RETENTION_DAYS, ATTRIBUTION_EVENT_RETENTION_DAYS and
   ATTRIBUTION_AUDIT_RETENTION_DAYS. Validate click>=30+eventdelivery+1,
   observations>=2, financial>=refundwindow+eventdelivery+1, event>=delivery+1.
   One-day margins cover future skew and exact cutoffs; no implicit production
   defaults. Test/example values38/30/400/8/90 for existing delivery7/refund365.
   Management receipts retain fixed48h protocolminimum; checkout receipts use click
   retention with at least48h. Tenant terminal tombstones persist indefinitely,
   a fixed safety invariant rather than a configurable ability to resurrect data.
2. Existing shared tenant row lock serializes cleanup with setup, public ingestion,
   merchant writes, settlement and reports. Cleanup does not require upstream
   freshness: retention/deletion remains safe while authority is unavailable.
   Freshness still gates serving operations. Lock order is tenant then order/event.
   SQL deadline2s and whole batch5s; row deletion batches<=100, including children.
   Never raise runtime/request/stack/lease budgets to make cleanup pass.
3. Add tenant maintenance_due_at, maintenance_failure_at and purged_at scheduling
   metadata. A short claim transaction selects<=8 due tenants ordered by due/id with
   FOR UPDATE SKIP LOCKED and advances due by60s before commit. Each owned worker
   processes each claimed tenant once periteration under its tenant lock. No external
   effects or persistent execution lease needed: the transaction owns effects, repeat
   cleanup is idempotent, and the advance-before-attempt schedule survives crash and
   prevents one poison tenant monopolizing subsequent claims. Lock timeout/exception
   records safe failure and leaves scheduled retry; cancellation rolls back in-flight
   work and preserves the committed retry schedule. Loop interval1s, successful full
   batch schedules remaining work1s later, otherwise60s. Join/cancel with existing
   startup worker owner; no detached tasks.
4. Prune observations by received_at only after retention (preserves original-event
   retry horizon). Prune checkout receipts only when old and not referenced by any
   pending/unresolved/leased merchant event; keep their referenced raw clicks while
   those checkout rows exist. Prune raw clicks by clicked_at; paid-order dimensions
   already frozen in acceptedp07 remain immutable. Terminal event envelopes may prune
   after received retention only in accepted/conflict states without execution. Pending,
   unresolved or leased envelopes and their order/refund identities cannot prune.
   Audit uses created_at; management receipts use48h created_at, whose original request
   horizon is24h. Existing ingress rejects original old timestamps before receipt lookup,
   so removal never reaccepts an old delivery. New business identities after contractual
   retention are distinct requests; no permanent business-history promise.
5. Financial retirement must not expose partial order/line/refund cleanup or perform
   unbounded FK cascades. Add nullable attribution_orders.retired_at. Under tenant/order
   ownership, mark an order retired only when greatest(received_at,paid_at) is older
   than financial retention and no pending/unresolved/executing event references it.
   All financial report/detail/list consumers exclude retired orders atomically.
   A subsequent mutation/settlement encountering a retired order fails closed. Delete
   expired order children in bounded chunks (events, refundlines, refunds, orderlines)
   and finally the empty parent; resume marked retirement after restart. Each marking
   and deletion transaction is atomic; the shared lock and statement snapshot mean a
   report sees the full retained fact or no fact, never partial money. Raw observations
   follow their own retention and need not disappear with financial retirement.
   Refund identity lasts with its parent until the validated refund+delivery horizon;
   late admitted durable work is protected regardless of wall-clock age.
6. Deleted tenant purging overrides data retention but never deletes its terminal
   fence. Existing deleted state immediately denies every read/write; bounded purge
   visits child tables in FK-safe order, deleting<=100 rows perbatch and continuing
   only once a prior child table is empty. Credential rotation self-references are
   removed in dependency order, not by an unbounded whole-tenant update. Cover events,
   refund/orderlines/facts,checkouts,observations,clicks,links,ads,campaigns,credentials,
   installations,domains,receipts,audit,stores. No server-side persisted exports exist.
   Purged tombstone keeps companyid/version/deleted state/time and minimal scheduling
   marker; clear names/freshness/reconciliation errors/claims and invalidate local
   freshness. Company-independent user sessions cannot be deleted for unrelated
   companies, but can no longer authorize the tombstoned company. Delayed provision/
   lifecycle attempts cannot recreate any purged tenant data even after restart.
7. Operational signals extend existing structured tracing, using fixed operation
   labels and internal request/event UUIDs, never URL/query/header/body/IP/customer
   values or secret-bearing debug errors. Instrument redirect success/storage failure
   and duration; event acceptance/rejection, lag, duplicate/conflicting outcomes;
   safe attribution-selection reason; pending/unresolved age/worker failures; report
   latency; admission/identity failures; lifecycle freshness; retention progress/failure.
   Signals distinguish attempted selections from committed delivery outcomes. Use
   existing shared request correlation/span and safe error mapping. No unauthenticated
   admin/metrics API or high-cardinality tenant labels. Existing scoped event diagnostics
   and owner/admin retry remain the operator repair surface. Tests capture actual logs
   across success/failure and assert seeded secret/request data absent.
8. Independent backup/restore uses standard pg_dump custom format and pg_restore into
   a fresh isolated owned role/database (no-owner/no-acl); preserve encryption keyring
   separately via the operator secret store. Never restore over a configured existing
   database or reset automatically. Real fixture must preserve paid/refund sums, key
   verification, encrypted session material, pending work and terminal fences, with
   cross-role denial and actual pending settlement on restored service. Record elapsed
   backup/restore/recovery; these are local measurements, not production RPO/RTO approval.
9. Deployment examples use the existing non-root image and actual configkeys, private
   per-app databases/roles, TLS termination, no trusted forwarded identity/IP headers,
   documented callback/authority origins and fixed timeouts. Stable public origin
   survives compute relocation. Use standard trust store verification for HTTPS;
   never add disabled certificate verification to production. Test two topologies
   with identical app artifacts: separatecontainers and separate isolated networks
   linked only by documented HTTPS identity/lifecycle/SSO routes. Deny cross-database,
   filesystem and private-port access; browser/merchant traffic uses public service
   API. This simulates separate hosts/network boundaries locally, not a production
   rollout. Actual Openbot bridge must be exercised, not only a fake authority.
10. Reuse extracted-build.py, container-smoke.py and p03 browser setup where fit.
    Extend real fixtures for full setup->merchant purchase/refund->report across the
    topology; cold restart/outage must fail user actions immediately and machine/
    public use after5minfreshness, then reconcile before recovery. Exact boundary,
    backup and TLS failure checks travel with the extracted service where possible;
    cross-application orchestration may live at root and must not become a runtime
    service dependency. Phase10 repeats the final combined acceptance/fullCI gates.

