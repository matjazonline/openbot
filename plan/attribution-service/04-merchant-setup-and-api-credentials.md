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
   and return consistent authentication/permission failures. Native provider webhooks use verified
   signatures over bounded raw bytes and replay protections, not an assumed bearer-key wrapper.
5. Audit key creation/revocation and important configuration changes with trusted actor/company
   identifiers. Local key revocation and company suspension are checked at the ingestion boundary.

Focused checks: `attribution_credentials_` for missing/invalid/expired/revoked keys, wrong scope,
foreign store, secret non-disclosure, rotation overlap, and concurrent revoke versus use with a
documented transaction boundary. Provider signature fixtures include altered bytes, stale timestamps
where supported, and oversized bodies. Keep public event endpoints unable to call trusted sales paths.

Completion: browser sessions, merchant keys, public installation IDs, and lifecycle/identity service
credentials have disjoint purposes. Record exact focused commands/counts/results here. Full-suite
verification is reserved for [10](10-integration-verification-and-pilot.md).
