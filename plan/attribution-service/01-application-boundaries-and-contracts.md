# 01 — Application boundaries and contracts

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and implementation order

Build attribution as a separate application with its own dashboard, HTTP API, PostgreSQL database,
credentials, sessions, build, and deployment. Initially keep its source at `services/attribution/`
in this repository and run it alongside Openbot. Moving it to another server must require only
deployment, networking, and configuration changes. Moving its source to another repository must
not require extracting application code from Openbot.

This directory is a proposed implementation plan, not implemented functionality. Implement the
numbered files in order. Each phase depends on its predecessors; each batch has focused checks.
Run the final full-suite gate in phase 10 after all implementation is integrated.

| Phase | Deliverable |
| --- | --- |
| [01 — Boundaries and contracts](01-application-boundaries-and-contracts.md) | Ownership, network contracts, product decisions |
| [02 — Independent application](02-independent-application-foundation.md) | Standalone package, database, runtime, build, CI |
| [03 — Login and company access](03-single-sign-on-and-company-access.md) | SSO, local sessions, live authorization, lifecycle integration |
| [04 — Merchant setup and credentials](04-merchant-setup-and-api-credentials.md) | Store setup, approved domains, scoped keys, administration UI |
| [05 — Links and clicks](05-links-and-durable-clicks.md) | Campaigns, ads, link API/UI, durable redirects |
| [06 — Website tracking](06-website-tracking-and-checkout-handoff.md) | Pixel, browser events, consent-aware checkout handoff |
| [07 — Orders and attribution](07-orders-refunds-and-attribution.md) | First merchant integration, deduplication, refunds, attribution |
| [08 — Reports and dashboard](08-analytics-and-attribution-dashboard.md) | Reconciled reporting API and attribution-owned dashboard |
| [09 — Operations and relocation](09-operations-retention-and-separate-deployment.md) | Retention, monitoring, backup restore, separate-server proof |
| [10 — Verification and pilot](10-integration-verification-and-pilot.md) | End-to-end acceptance, final CI gates, pilot evidence |

## Mandatory application separation

| Concern | Openbot owns | Attribution owns | Permitted connection |
| --- | --- | --- | --- |
| Product UI | Openbot pages and navigation | All attribution pages, forms, assets, dashboard | Navigation links between applications |
| User accounts | User identity, account security and account administration | External user references and local sessions only | Explicit SSO and identity API |
| Companies and membership | Company identity, invitations, membership and roles | Local attribution configuration keyed by the same `company_id` | Authorized company-access and lifecycle APIs |
| Tracking and commerce | No attribution records | Stores, domains, credentials, campaigns, ads, links, clicks, events, orders, refunds | Versioned attribution API when a real Openbot feature needs it |
| Authorization | Current account/company authority | Enforcement on every attribution operation | Trusted identity plus current company permission, or a scoped machine credential |
| Persistence | Openbot database and migrations | Attribution database and migrations | No cross-database access |
| Runtime | Openbot server and workers | Attribution server and owned background work | HTTPS; no in-process calls |
| Delivery | Openbot artifact and release | Attribution artifact and release | Independent release jobs and configuration |

The following are forbidden shortcuts, including during the first local deployment:

- No dependency on the `open_bots` Rust package, sibling source files, private Openbot modules,
  shared `AppState`, shared database pool, or shared application/domain library.
- No shared user/company tables, SQL joins across applications, cross-database foreign keys,
  direct reads of the other database, or distributed database transactions.
- No attribution pages in Openbot's route tree, templates, asset bundle, or frontend build. The
  attribution browser talks to its own backend; ordinary dashboard operations never proxy through
  Openbot. Reusing a visual style does not create a runtime/build dependency.
- No shared cookies, session tables, session signing secrets, password hashes, or general API
  master key. Public tracking identifiers, user sessions, merchant keys, and service credentials
  are different credential classes and cannot substitute for one another.
- No cross-application filesystem mounts, repository-relative runtime assets, parent lockfile,
  parent `.env`, or deployment scripts that require the other application's source.

Sharing identity means sharing stable identifiers and an explicit authority contract. Attribution
does not run a second registration, password-reset, invitation, or membership system. Its local
company record is attribution configuration, not a competing company directory. User display
metadata may be cached as non-authoritative data; cached roles must not become access grants.

Openbot implementation changes are limited to the selected identity integration, company-access
API, reliable lifecycle delivery, navigation link, and their configuration/tests/CI. Tracking,
merchant credentials, attribution jobs, reports and dashboard assets belong exclusively to the
attribution project. Each cross-application credential is dedicated to its client, audience and
scope; no application reuses the other's browser-session or merchant secrets.

Independent deployment is the MVP requirement. Independence from Openbot's account service is
not: login and company administration still depend on Openbot. A common account service replacing
that dependency is a separate future architectural decision, not an implicit extra project here.

## Batch 01A — Specify the network boundary

1. Write OpenAPI contracts and an authentication sequence for the two applications. Openbot owns
   the identity/company contract; attribution owns the tracking/management/reporting contract.
   Share wire schemas as artifacts, not application source or a shared database model. Specify
   status codes, bounded payloads, pagination, timeouts, retryability, idempotency, and errors.
2. Specify the three cross-application paths: browser SSO into attribution; attribution's
   authenticated identity/company permission checks; and reliable company suspension/deletion
   delivery from Openbot. Public tracking and merchant callbacks go directly to attribution.
3. Keep Openbot dashboard API integration optional. If an actual Openbot feature later needs
   attribution data, use a company-scoped attribution key stored only by Openbot's backend. The
   attribution dashboard itself must not need those keys or an Openbot rendering proxy.
4. Define types for company/user external references, store/ad/link/click/event/order identities,
   currency and money. Keep ad identifiers distinct from per-request click identifiers. Specify
   tenant predicates and permissions for every resource and nested resource.

Focused checks: contract examples cover every credential class, two companies, two stores, and
permission denial; no caller-controlled company/user field grants authority. During implementation
add `attribution_contract_` tests and contract validation for both APIs. Trace current owners before
editing Openbot's authentication or membership paths.

## Batch 01B — Set the smallest pilot and resolve material choices

Defaults to carry through the plan:

- One attribution HTTP backend and PostgreSQL; its dashboard uses ordinary refreshes. No queue
  broker, ClickHouse, SpacetimeDB, live dashboard engine, or separate analytics service.
- One Openbot company maps to one attribution tenant using the same `company_id`; several stores
  can belong to that tenant. No additional workspace hierarchy or duplicate company mapping ID.
- Last eligible tracked click within 30 days before order creation; freeze the chosen click and
  policy on the first accepted paid order. No fingerprinting or cross-device matching.
- Merchandise revenue uses actual line amounts after discounts; taxes and shipping stay separate.
  Refunds reduce merchandise revenue according to their line allocation. Never add currencies.
- Fail a redirect with a retryable 503 if its click cannot be durably stored; never issue an
  apparently tracked click ID whose row was not committed. Confirm this pilot behavior explicitly.

Before dependent implementation, record these decisions and the responsible owner in this file:

- The first real checkout platform/store and its paid-order/refund signature and retry contracts.
  A generic signed merchant adapter can be a test fixture, but does not count as the promised
  first production checkout integration.
- The supported SSO provider arrangement and maintained protocol components. Prefer standard
  OIDC Authorization Code with PKCE. Determine how the provider maps to Openbot's authoritative
  users; do not assume Openbot is already an OIDC issuer or silently build a new identity platform.
  If provider adoption changes account ownership, resolve that architectural choice before phase 03.
- Expected daily events, peak requests, retention periods, deployment region, consent behavior,
  allowed tracking placements, and numeric redirect/report/error-budget targets for the pilot.
- The exact mapping from existing Openbot roles to attribution read, link-management, and
  integration-administration permissions; company suspend/delete propagation deadline.

These are explicit implementation prerequisites, not reasons to leave this planning task unsaved.
Record unanswered items as open; do not advertise defaults as approved deployment policy.

Evaluate Shlink only against a demonstrated per-request click-ID checkout handoff and total
maintenance cost. A native Rust implementation is the baseline; adoption must reduce overall
complexity. Dub conversion licensing needs a separate decision and fresh license verification.
Official platform impressions/spend, app installs, affiliate payouts, cross-device attribution,
additional commerce adapters, currency conversion, and click-cohort reporting remain later scope.

Focused checks: reconcile a concrete two-ad, multi-product, refund example on paper; validate the
selected merchant can supply reliable payment/order/refund identifiers and checkout linkage. Record
the SSO choice and pilot assumptions before starting the batches that depend on them.

## Current repository anchors and evidence discipline

- [HTTP state](../../src/adapters/http/app_state.rs): current Openbot composition; do not extend it
  with attribution stores, reporting queries, or tracking handlers.
- [Session authority](../../src/adapters/http/session.rs): Openbot sessions and revocation; this
  is not evidence that an SSO issuer exists.
- [Company routes](../../src/adapters/http/routes/company.rs): existing company management, reused
  through its owning use cases when adding the narrowly scoped integration API.
- [CI](../../.github/workflows/ci.yml) and [local checks](../../scripts/check.sh): current gates;
  compare the live CI file at implementation time rather than assuming the local script is complete.

All new test prefixes and service commands in this directory are future requirements. Per batch,
record exact commands, nonzero selected/executed test counts, outcomes, and unavailable checks in
the corresponding phase file. List filtered tests before running them. Use disposable databases;
regenerate SQLx metadata in the owning application after SQL edits. Keep final full-suite work in
[10](10-integration-verification-and-pilot.md); broaden checks earlier at shared identity boundaries.
