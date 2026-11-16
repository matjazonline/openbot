# 04 — Merchant setup and API credentials

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [03](03-single-sign-on-and-company-access.md). A company administrator sets up stores,
approved domains, installations, and merchant credentials entirely in attribution's UI. Attribution
issues, validates, rotates, and revokes these credentials without a per-event Openbot login call.

## Batch 04A — Model stores, domains and installations

1. Store company-scoped merchant installations, store identifiers, approved exact HTTPS hosts,
   and public tracking installation identifiers in attribution's database. Domain approval must
   establish customer control through the agreed verification/administrator process; typing an
   arbitrary hostname is not proof. Define re-verification and revocation behavior.
2. Canonicalize hostnames, including internationalized domains; reject credentials in URLs,
   non-HTTPS destinations, unsafe ports and invalid/local destinations. Match approved hosts
   exactly; a suffix match must not permit `customer.example.attacker.test` or arbitrary subdomains.
   Never fetch a submitted destination to validate it. Secure any domain-verification network
   operation against arbitrary addresses and redirects.
3. Public installation IDs select tenant/store configuration and allowed observation types. They
   are not secrets and cannot authorize a paid order, refund, report or configuration change.
4. Build accessible setup forms in the attribution application, including domain/installation
   status and clear errors. State-changing actions use the user-session permission/CSRF boundary.

Focused checks: `attribution_installation_` and `attribution_destination_` for cross-tenant/store
references, Unicode/host confusion, revoked domains, malformed URLs, script escaping, and concurrent
domain/configuration updates. Verify the UI never embeds a privileged credential in public snippets.

## Batch 04B — Issue and enforce scoped credentials

1. Create high-entropy keys with an identifiable public prefix and show the secret once. Persist
   only a cryptographic verifier for our bearer keys; redact secrets from errors, traces, audit
   events and URLs. Keep provider webhook-signing secrets encrypted because verification needs
   their original value. Separate encryption material and deployment secrets between applications.
2. Bind each merchant key to one company, one store, and explicit scopes such as conversion
   ingestion. Scope comes from the credential; body/path IDs never override it. Management/report
   credentials, if needed, have separate scopes and cannot masquerade as a user session.
3. Expose creation/list/revoke/rotate in attribution's administration UI and authenticated API.
   Listing returns metadata only. Document intentional overlap during secret rotation, expiry,
   last use, and who may administer keys. Rotation is not a schema compatibility mechanism.
4. Authenticate before trusting or fully parsing merchant payloads, impose request/tenant limits,
   and return consistent authentication/permission failures. Native provider webhooks are deferred; future adapters must verify signatures over bounded raw
   bytes and replay protections before calling the trusted ingestion path.
5. Audit key creation/revocation and important configuration changes with trusted actor/company
   identifiers. Local key revocation and company suspension are checked at the ingestion boundary.

Focused checks: `attribution_credentials_` for missing/invalid/expired/revoked keys, wrong scope,
foreign store, secret non-disclosure, rotation overlap, and concurrent revoke versus use with a
documented transaction boundary. Generic merchant fixtures include invalid bearer keys and oversized bodies. Native provider
signature fixtures belong to the later adapter integration. Keep public event endpoints unable to call trusted sales paths.

Completion: browser sessions, merchant keys, public installation IDs, and lifecycle/identity service
credentials have disjoint purposes. Record exact focused commands/counts/results here. Full-suite
verification is reserved for [10](10-integration-verification-and-pilot.md).


## Implementation evidence — phase04 batch

Implemented setup UI/API, exact DNS-approved domains and installations, hash-only
scoped credentials, rotation/revocation, transactional management receipts and
credential/tenant authorization boundaries. Architecture decision AS-P04-D1/r1 passed
independent Astra review AS-P04-G1-N1. Code review/phase acceptance is recorded in
[phase04 REVIEW](phases/p04/docs/REVIEW.md); passing implementation checks alone is
not whole-plan acceptance.

Commands run from `services/attribution` with isolated fixtures/cache:

- `cargo test --locked --offline --lib setup`: 18 passed; the explicitly ignored
  browser test is run separately below. PostgreSQL concurrency and local DNS included.
- `cargo test --locked --offline --lib attribution_setup_browser_ -- --ignored --nocapture`:
  1 passed, eight browser checks, actual setup/PG/DNS with an identity fixture.
- `cargo test --locked --offline --lib adapters::identity:: -- --skip setup_tests`:
  21 existing identity/lifecycle tests passed.
- `python3.13 scripts/verify.py prepare`: fresh owned migration and SQLx regeneration passed.
- `python3.13 scripts/verify.py foundation`: formatting, offline all-target compilation,
  fresh migrations/SQLx check, boundary checks and 14 foundation tests passed.
- `ATTRIBUTION_CONTRACT_PYTHON=<dedicated interpreter> scripts/check-contracts.sh`:
  12 passed. Boundary checker tests: 10 passed.

Retained logs and criterion mapping: [phase04 TASK](phases/p04/docs/TASK.md).
Native-provider adapters, external/public DNS availability, production deployment
and whole-plan full-suite verification remain outside this phase's acceptance claim.

Independent review AS-P04-B1-N1 required two additional discriminating checks.
Both now exercise production transactions: competing store-disable/use in both
orders, and exact key-creation replay after the original submitted expiry passes
in database time. Recheck and final acceptance remain in the phase review record.
