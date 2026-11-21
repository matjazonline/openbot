# Website observations and merchant checkout

The attribution service records three separate, untrusted observation kinds:
`image_load`, `landing_page_load`, and `visible_placement`. None is a verified ad
impression, human visit or sale. A public installation belongs to one store and an
active exact approved HTTPS domain. Its ID is public; keep merchant API keys on
your server. Setup must finish domain verification before using these endpoints.

## Browser installation

Serve your site over HTTPS. Copy its public installation UUID from attribution's
settings, then load this pinned asset from your attribution deployment:

```html
<script src="https://attribution.your-company.com/sdk/1.0.0.js"
  integrity="sha384-Rzgpm6KzUO00BIJenyYvjfsO6P1+tN8+10M615R9QAMjGKLP9eRTHBs9aedxdqOt"
  crossorigin="anonymous" referrerpolicy="no-referrer"></script>
<script>
  const attribution = OpenbotAttribution.create("YOUR_PUBLIC_INSTALLATION_UUID");
  // Connect these calls to your existing consent manager's explicit decision.
  consentManager.onGranted(() => attribution.setConsent(true));
  consentManager.onWithdrawn(() => {
    attribution.setConsent(false);
    updateMerchantCart(attribution.handoff());
  });
  // After consent, observe an actual placement belonging to this installation's store.
  // attribution.observeVisible(document.querySelector("#placement"), "YOUR_LINK_UUID");
  // Send attribution.handoff() to YOUR server when updating its existing cart.
</script>
```

`consentManager` and `updateMerchantCart` above are integration points in your shop,
not functions supplied by this SDK. Do not infer consent merely from loading the
script or from a tracking URL. One SDK instance per installation/page is sufficient.
The SDK does not create cookies, read optional local storage or send events before
`setConsent(true)`. It captures a single well-shaped `ob_click` in memory and removes
only that query parameter immediately, retaining unrelated parameters/fragments.
An ordinary page with no captured or consented retained click sends no landing event.

After consent, it retains at most 32 candidates in installation-scoped localStorage,
expiring them 30 days after local capture; a return visit still needs an explicit
consent-manager grant before reading them. Per-click entries and a fresh read at
handoff preserve candidates across tabs. Cleanup removes only entries considered by
that read, preserving clicks another tab adds concurrently. Each handoff is bounded
to 32 candidates; concurrent storage writes are pruned as those reads finish. Blocked storage falls back to bounded
page memory; closing/navigating that page can then lose its history. Withdrawal
clears candidate storage/memory, aborts in-flight attempts and retry timers, stops
visibility observers and signals other tabs. The withdrawal signal stores only a
choice timestamp. Tell your server to clear the cart's candidates on withdrawal too.
Already committed historical observations and frozen checkouts are not rewritten.

`observeVisible(element, linkId)` records at most one event for that registration
when at least 50% intersects continuously for 1000 ms while the document is visible.
It returns a cleanup function. These fixed version-1 parameters are stored separately
from landing/image observations. A client can forge an observation; the label does
not establish verified viewability. Missing IntersectionObserver or placement access
means no visibility measurement. Browser/network blockers may prevent observations.

Each event keeps its original event UUID and time for up to three attempts. Network,
429 and 503 failures retry with bounded backoff and Retry-After; withdrawal/pagehide
cancels pending work. At most 32 events are pending. There is no delivery promise
when a page is closed or blocked. Requests omit credentials and referrers and send
only bounded installation/event/click/link IDs, kind, time and consent. No page URL,
customer email/name, cart contents or merchant key is sent by the SDK.

## Direct events and pixels

`POST /v1/events` accepts the strict Observation shape in
[the contract](contract/attribution.openapi.json), with an 8 KiB limit. Consent must
be true and at least one scoped click or link ID is required. If both are supplied,
the click must belong to that link. Historical click dimensions remain frozen;
link-only observations capture current scoped dimensions. A disabled link prevents
new clicks but does not invalidate an already committed landing through an active
installation. Disabling the installation, store, domain or company blocks new
observations and receipt replay. Origin, when supplied, must match that installation's
exact approved HTTPS origin. CORS permits only an approved exact origin, POST and
Content-Type, with no credentials; it is not authentication. Origin-less public
clients/pixels are permitted and remain untrusted.

For an image request, create the image only after your consent manager grants it:

```js
const query = new URLSearchParams({
  installation_id: publicInstallationId,
  link_id: placementLinkId,
  event_id: crypto.randomUUID(),
  observed_at: new Date().toISOString(),
  consent: "granted"
});
const image = new Image(1, 1);
image.referrerPolicy = "no-referrer";
image.src = "https://attribution.your-company.com/pixel.gif?" + query;
// Keep event_id AND observed_at unchanged if retrying this observation.
```

The event ID is also the cache-buster; all pixel/event responses use no-store. With
no `event_id`, omit `observed_at`: each GET is a distinct server-timed image request.
With an ID, its original `observed_at` is mandatory. The service returns a transparent
GIF (or event 204) only after durable commit/deduplication; storage/uncertain commit
returns retryable 503. HEAD validates without recording and returns 204, never an
image acknowledgement. An image request measures a load, not a visible placement.

Observation IDs deduplicate within one installation. Equal normalized facts replay;
changed facts conflict with 409. Original times may be at most 24 hours old and 30
seconds in the future. Older requests return 410 even after pruning. Uniqueness must
be retained at least 48 hours. These protocol minima do not approve production
retention policy; operational configuration is completed in phase 09.

## Your server owns checkout identity

`handoff()` returns only `{consent, candidate_click_ids}`. Associate those candidates
with your existing server-side cart/session. Maintain its bounded consented history;
never replace it blindly with the browser's last ID. Generate order/checkout identity
and creation time on your server. Freeze and persist the original request before
calling attribution, so concurrent checkout requests or an ambiguous response reuse
the same facts. Use your store's `checkouts:write` key for `POST /v1/checkouts`:

```json
{
  "checkout_id": "your-server-checkout-id",
  "order_created_at": "2026-11-16T12:00:00Z",
  "consent": true,
  "candidate_click_ids": ["cccccccc-cccc-4ccc-8ccc-ccccccccccc1"]
}
```

Do not accept checkout identity/time or company/store overrides from browser fields.
The key selects company/store; user sessions and public installations cannot call
this API. The body limit is 16 KiB, candidate limit 32 distinct UUIDs, and external
checkout ID limit 128 characters without whitespace/control characters. Creation
time has the same 24-hour/30-second horizon, including replays. Keep ID, original
time, consent and candidate set unchanged on retry; order within the set is normalized.
Exact replays return the same frozen result; changed facts return 409. Retrying after
key revocation fails. Inspect current server order state rather than inventing a new
checkout identity to repeat an uncertain operation.

The service chooses the latest eligible server click time, breaking ties by UUID,
in the inclusive interval `[order_created_at - 30 days, order_created_at]`, within
that company/store, excluding bot or prefetch flags. A later ineligible candidate
does not replace an earlier eligible one. No consent returns `no_consent`, no IDs
returns `missing`; otherwise, with no eligible click, the reason precedence is
`foreign_or_missing`, `future`, `expired`, `bot`. No foreign details are returned.
The selected identity/time/dimensions and `last-eligible-click-30d-v1` policy are
frozen once. Later clicks or edits cannot change that checkout. This is the shop's
observed, consented history, not cross-device or unseen-click attribution.

Checkout is not payment. The paid-order API in phase 07 must revalidate the selected
evidence and freeze the eventual order attribution; browser fields/pixels cannot
prove a sale. Native store adapters remain deferred.

## Verification fixture

[browser/website.mjs](browser/website.mjs) is a representative merchant integration,
not a platform adapter. Its own HTTPS server owns the cart cookie, key and original
checkout request, coalesces simultaneous attempts, and retains the same payload on
retry. The browser drives actual SDK/service/PostgreSQL paths; the fixture uses an
ephemeral local certificate/host mapping and a simulated Openbot authority. The actual
Openbot login bridge is exercised separately by the accepted SSO browser fixture.

With the documented test PostgreSQL and Playwright setup, run from this directory:

```sh
cargo test --locked --offline --lib website_tests -- --nocapture
cargo test --locked --offline --lib attribution_checkout_handoff_browser_ -- --ignored --nocapture
```

The browser fixture requires Node, Chromium and openssl. It verifies explicit consent,
Ad A then B, URL removal, return visits, event retries, visibility duration, server cart
freeze, tabs/withdrawal, blocked storage, later invalid candidates, expiry/bounds,
cancelled retries and absence of keys/customer data from public requests. It proves
no native platform placement/support or production retention/load target.
