# Step 5 — Business Connections and the First Adapter

## Outcome and dependencies

Provide company-scoped, verified CRM reads that a package can request by capability. Depends on
[steps 1–3](README.md). The initial adapter is HubSpot for the customer-quote example; it is not a
general API proxy or an extension of the messaging transport enum.

## Implementation

1. Add `BusinessConnection`, provider/account IDs, typed capability keys/versions, and statuses
   `active`, `disabled`, `reauthorization_required`, and `revoked`. Use separate
   `business_connections` and credential rows with composite company/account scope. Keep
   `IntegrationInstallation`/`ChannelBinding` as the messaging contract. Reuse shared HTTP-client,
   encryption, and account-status primitives without making a CRM a messaging transport.

2. Define application ports for account management, narrow credential access, readiness checks,
   and `CustomerReader`/`ProductReader`. Return bounded typed customer/product records and source
   references, not raw provider JSON. Record provider account, record ID, fetched time, selected
   field values/hash, and provider version/modified time when available. Stable capabilities are
   `crm.customer.read.v1` and `crm.product.read.v1`; the adapter translates them to provider APIs.

3. Implement HubSpot OAuth account linking and unlinking through the wizard. HubSpot documents
   OAuth for apps installed by multiple accounts; tokens carry the granted app scopes and may
   expose more data than the installing user's own CRM visibility. Therefore require explicit
   company/channel use grants and show the data scope during setup. Do not infer local record
   authorization from the installer's HubSpot UI permissions.
   [HubSpot OAuth documentation](https://developers.hubspot.com/docs/apps/developer-platform/build-apps/authentication/oauth/working-with-oauth).

4. Bind one-time OAuth state to authenticated user, company, setup draft, intended provider, and a
   short expiry. Verify it atomically at callback, reload current company-management authority,
   reject replay, and use only a registered redirect target. Keep codes/tokens out of request
   logs and error pages. Request the exact read scopes required by the chosen endpoints; check
   granted scopes and actual account access before reporting readiness. Missing product access is
   an actionable setup error, not an empty product list. Record the verified endpoint/scope matrix
   and API version in connector documentation; recheck official docs during implementation.

5. Reuse `enc:v2` envelope encryption with a distinct authenticated credential context containing
   company, business-connection ID, provider, and credential kind. Extend the shared credential
   inventory/rotation implementation and its tests for the new table. Store access/refresh tokens
   through a narrow `SecretString` port; no setup/package/agent JSON receives them. Implement
   bounded, generation-fenced token refresh so competing workers cannot overwrite newer tokens.
   If exchange/refresh outcome is ambiguous and cannot be recovered safely, require reauthorization.

6. Implement explicit customer lookup by approved record ID or exact email and product lookup by
   selected product IDs/SKUs. Request only fields the quote workflow needs. HubSpot supports
   contact retrieval by record ID or email, and products expose catalogue properties including
   prices. Map missing records, ambiguous matches, missing fields, and unsupported currency into
   distinct typed outcomes, preserving source evidence.
   [Contacts API](https://developers.hubspot.com/docs/api-reference/legacy/crm/objects/contacts/guide),
   [Products API](https://developers.hubspot.com/docs/api-reference/legacy/crm/objects/products/guide).

7. Expose implemented operations through the existing tool catalogue/harness compilation path.
   Scope each invocation to the current task, run, step, connection slot, and allowed record set.
   The server resolves the account; the model cannot select a company/account ID, URL, credential,
   or arbitrary property list. Apply current company/channel data grants as well as the installed
   capability ceiling. Provider account permission alone does not authorize exposing its records
   to every member of the company.

8. Use fixed provider origins, bounded response bodies/pagination, shared clients and timeouts,
   per-company/account concurrency, retry budgets, and rate-limit backoff. Proposed initial
   ceilings: 10 seconds per HTTP request, 1 MiB response, 100 records/page, 3 pages/tool invocation,
   2 concurrent requests/account, and 3 retryable attempts within the task deadline. These are
   application ceilings, not claims about provider limits. Honor stricter provider responses.
   Do not retry missing authorization or run a permanent polling loop for every installation.

   Enforce account concurrency across server instances using bounded leased request permits,
   with generation-fenced release and expiry recovery. Each call still obeys the task deadline;
   a per-process semaphore alone does not implement the advertised per-account ceiling.

9. Revocation commits the account status/version first, then wakes affected runs/installations.
   Runtime checks the current status immediately before a call and before accepting its result;
   cancel in-flight work where possible and discard a result after revocation. A request already
   accepted by a remote provider cannot be retracted. Never acquire installation locks while
   holding a connection lock; reconciliation occurs after the connection transaction commits.

## Verification and acceptance

- Use a local HTTP stub for successful reads, missing data, malformed/oversized responses,
  pagination bounds, timeouts, 401/403, 429, and provider failures; no live secrets in CI.
- Race two OAuth callbacks and two refresh claimants; prove single ownership and stale-write
  rejection. Test refresh owner death and bounded recovery.
- Race request-permit claimants across two workers and prove the account ceiling, lease-expiry
  recovery, and rejection of a stale permit release.
- Test another company's account/credential ID, a mismatched envelope context, revoked access
  during a tool call, and a channel with no data grant.
- Test encryption key rotation over existing and new credential tables.
- Perform a separate real-account pilot check of OAuth, customer/product lookup, granted scopes,
  and account features. Record results without credentials or customer data in the repository.
- Accept when setup can verify a real connection and a scoped task can read the required records,
  while no connector path can create or modify CRM records.
