# 03 — Reports, retention and final verification

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-visitor-cookie-and-consent.md). Reports show visitors beside clicks, visitor
rows have a lifetime, the store guide says exactly what a shop must do, the deployment is proven
not to trust a client's country, and the whole service passes its full gate.

## Batch 03A — Visitor measures in reports

1. Add to `GET /v1/analytics`, its CSV export and the reports page, for bot-filtered clicks in
   the period:
   - unique visitors: distinct visitors among those clicks;
   - returning visitors: of those, visitors with an earlier retained click before the period;
   - clicks without a visitor: clicks with no visitor reference.
2. Count within each group independently. A visitor who clicked two ads is one visitor in each
   ad's row and one in the overall total, so group rows do not add up. Carry that in the existing
   `nonadditive_measures` column and label it on the page.
3. State the limits next to the numbers: a visitor is a browser that accepted or was in a listed
   country; refusals, cleared cookies, other devices and links on the shared address are counted
   under clicks without a visitor; counts shrink as clicks reach their retention.
4. Aggregate visitors separately from orders and observations before joining, so the new
   measures cannot multiply the existing ones. Inspect the query plan on the pilot volume.
5. No route lists or returns visitor ids.

Focused checks: `attribution_visitor_reports_` against a fixed ledger with two stores, two ads,
one visitor clicking both ads, a returning visitor across the period boundary, withdrawn
visitors, and clicks without visitors; reconcile each figure by hand; confirm every existing
measure is unchanged from the current fixtures. Capture the query plan here.

## Batch 03B — Lifetimes, purge and documents

1. Extend the existing maintenance worker within its current bounds. Remove a visitor row once it
   has no clicks and is older than the cookie lifetime. A visitor whose clicks all expired
   earlier stays until then, so a returning browser is still recognised.
2. Add visitors and tracking hosts to the tenant purge order. A suspended or deleted company gets
   no cookie and no consent answer; its redirect behaves as it does today for such a company.
3. Rewrite the store guide in WEBSITE.md from the README's "What the store needs to do": the CNAME
   record, the automatic certificate and its first-request delay, the new script address and integrity value, the banner wiring
   with a complete example and the built-in banner, the consent setting, the cookie names and lifetimes for the policy
   text, the refusal control, and the known limits. Remove the sentence that the service uses no
   cookies and say precisely when it does.
4. Update the service README (routes, the configuration key and what startup validates),
   operations.md (what a visitor is, what withdrawal deletes, that no IP is stored, the proxy's
   duty to overwrite the country header, the IP database's license and update duty, the
   proxy's certificate account and rate limits), and the
   OpenAPI contract with examples for each row of the behavior table.

Focused checks: `attribution_visitor_retention_` for the exact cutoff, a visitor kept while it
has clicks, removal competing with a redirect that presents the same cookie, and purge
completeness; `scripts/check-contracts.sh`.

## Batch 03C — Deployment proof and the final full-suite gate

Extend the existing topology test with a tracking host behind the sample proxy. Send a request
that carries a forged country header from the client and assert the service acts on the proxy's
value, not the client's. Send one with no header configured and assert consent is required.
Assert the application receives no client IP header. Have the proxy obtain the host's
certificate from a local ACME test server (Pebble) on the first request, and refuse one for an
unregistered host.

Then run once, after everything is integrated, from a clean checkout:

```sh
cd services/attribution
scripts/verify.sh prepare          # only if SQL changed since the last regeneration
scripts/verify.sh                  # format, compile, Clippy, boundary, tests, contracts, browser
python3 scripts/extracted-build.py # builds and tests a copy with no parent files
```

Then the service image and container checks used by the attribution CI workflow, and from the
repository root:

```sh
./scripts/check.sh
```

Record every command, the selected and executed test counts, the results, the redirect latency
before and after, the cookie lifetime observed in each tested browser, and any check that could
not run. A real Safari is not part of the automated suite; if it was not tested by hand, say so
and keep the lifetime limit in the store guide worded as expected, not measured.

After a fix made during final verification, rerun the affected checks, and repeat the full gate
when the fix touches shared code such as the redirect transaction, host routing or request
admission.

Completion: a store can follow the guide to turn the visitor cookie on, and the service proves
it sets the cookie only under the stated rule. Production use of `by_region` still needs
decision D1.
