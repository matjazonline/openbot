## AS-P04-D1 / r1 — proposed architecture gate

One cohesive p04A–B batch; no dependent source edits before B PASS. Original
requirements are phase04 and the accepted network/OpenAPI contracts, not this design.

1. Extend attribution's initial migration (reset-only) with stores, exact domains,
   installations, merchant credentials, management receipts and an audit log. Each
   child has company/store predicates and composite foreign keys; domain-to-installation
   ownership is enforced in SQL. Store/installation disablement, domain revocation,
   credential revocation and lifecycle all serialize on the existing tenant row.
   No schema touches Openbot. Audit records contain trusted actor/company, action,
   resource UUID and time only, never bearer/verifier/challenge/payload values.
2. Typed setup commands perform bounded normalization in application code. The SQL
   adapter owns transactions. After live session company/permission and CSRF checks,
   every read/write obtains `lock_tenant` with the freshness ticket. Writes recheck
   local session expiry/revocation under a session-row lock (tenant then session),
   so a completed local logout cannot authorize a later commit. Membership authority
   is the live Openbot observation at request entry; there is no distributed lock.
   No network request occurs while a tenant transaction is held. Management
   mutations, audit row and receipt commit together; uncertainty returns 503.
3. Receipt identity is (company, user, method + concrete path, UUID key), bound to
   original timestamp and SHA-256 of normalized typed JSON. Validate the timestamp
   both before work and after acquiring the tenant lock using database wall time;
   >30s future is 400 and >24h old is 410. Compare an existing receipt first for
   changed timestamp/payload (409), within the accepted horizon. Receipt JSON is
   metadata only; creation/rotation return the new secret only to the first committed
   invocation. Replays always return 200 with the declared CredentialReplay recovery
   instructions; they never mint a key or extend overlap. Keep receipts >=48h;
   pruning is deferred to p09 and cannot relax the original-time bound.
4. DNS TXT proof is `_attribution.<canonical-host>` with 256 random bits in a
   public challenge. Use a maintained DNS resolver with fixed public recursive
   servers, no OS search suffixes and one bounded attempt inside a 2s total timeout.
   It queries DNS only; never fetch an HTTP destination or use customer-supplied
   resolvers. Proof also requires nonempty A/AAAA answers containing only globally
   routable addresses. No custom DNS packet implementation. Unit/local-DNS fixture
   adapters prove transport and validation without a public-network dependency.
   Pending/expired domains can verify or reverify against their stable challenge;
   approval expires after 30 days. Revoked records are terminal; re-add produces a
   new domain/challenge and requires a new installation. Unique live host per store.
   Verification obtains proof outside the transaction then locks and rechecks row
   state/challenge before approval. Replay of an accepted verification needs no DNS.
   Proof failures are safe 400, resolver failure/timeout 503; no failed receipt.
5. Canonical host validation owns IDNA and exact match. Accept DNS names only, with
   public-looking labels; reject IP literals, single labels, localhost and reserved
   local/special suffixes, empty labels, trailing dot and percent/userinfo confusion.
   Limit canonical host to 240 bytes so the TXT record name fits 253. Destination
   parsing requires HTTPS, no credentials, no explicit non-443 port, a canonical
   host and a currently approved exact domain for that company/store. DNS proof
   screens local addresses at approval; redirects never fetch the destination, so
   DNS changes cannot induce service-side SSRF. Domain expiry/revocation is checked
   at every dependent use. Common destination policy is consumed by p05/p06.
6. Keys use existing OS-random 32-byte and SHA-256 helpers: `ak_` +43 base64url
   characters; only verifier stored. Five typed scopes from the contract. Credentials
   belong to exactly one enabled store/company. Rotation clones label/scopes and
   original expiry into a new key and clips old expiry to min(original expiry,
   DB-now + overlap[0..86400]); only an unrevoked/unexpired/unrotated credential can
   rotate. Keep rotated-to ID internally; an exact retry is a receipt replay.
   Revoking the new key never changes old expiry. Creation expiry must be future.
7. A merchant authentication adapter accepts exactly one syntactically valid bearer
   header, rejects session cookies and conflicting company/store selectors, and
   verifies scope and local credential before body consumption. It refreshes company
   authority only when freshness requires. The trusted transaction entry re-locks
   tenant then credential and rechecks expiry/revocation/store/scope and ticket;
   last-use update and downstream mutation commit together. Revocation uses the same
   lock: an operation already holding it may commit first; after revocation commits,
   no later use commits. No commerce/report endpoints are prematurely added in p04;
   tests exercise the actual transaction entry with an isolated write assertion.
8. Bounded per-tenant concurrent admission (8 requests; bounded registry tied to
   active permits, 1024 tenants) follows authentication and precedes payload read;
   global 64-request/10s boundary stays in force. The production merchant pre-body
   adapter is exercised by a test-only route. Subsequent phases must use it, not
   duplicate credential logic. Public IDs expose only installation selection,
   allowed observation types (landing/viewport/image) and exact domain; they never
   satisfy merchant authentication or session authorization.
9. Lists use UUID keyset order, default25/max100, optional store filter, and encrypted
   opaque cursors bound to company/list-kind/filter; reuse the attribution cipher
   with distinct purpose. UI uses the existing session shell and external JS/CSS,
   explicit labels/status/error regions and text nodes; no privileged secret in
   markup/public snippets. API-key secret appears once after explicit creation in a
   private response panel and is cleared on navigation. UI exposes pagination,
   store create/edit/enable, domain add/proof/verify/revoke, installation create/enable,
   credential create/list/rotate/revoke. Members get read-only controls plus server
   enforcement; all mutations use one stable key/time per submission/retry.

### Acceptance mapping (planned; execution pending)

| ID / source | Production path and discriminating case | Decisive assertion / focused proof |
| --- | --- | --- |
| AC1 /04A.1,3 | Store/domain/install reads and writes, actual PostgreSQL | Foreign company/store IDs are 404; SQL composite FK cannot bypass ownership; public IDs fail key/session auth |
| AC2 /04A.1–2 | DNS verifier + destination validator | Real local DNS transport fixture proves TXT, all-address policy and timeout; Unicode exact host succeeds; suffix, unsafe URL/local IP, expired/revoked domain fail; no HTTP fetch |
| AC3 /04A.4 | Browser setup through HTTP with live-authority fixture | Keyboard labels/status, escaped names, all setup actions, read-only member, CSRF rejection, one-time secret absent from listing/public snippet |
| AC4 /04B.1–3 | Key SQL/HTTP create/list/rotate/revoke | Hash-only DB plus receipts/audit/log safe; secret syntax/entropy; scopes, expiry, overlap and last use; replay metadata only, no overlap extension |
| AC5 /04B.4–5 | Actual merchant auth/transaction, before body | Missing/invalid/expired/revoked/public/session/service values rejected before invalid/oversized body; wrong scope403; foreign selectors404; current suspension/staleness blocks |
| AC6 / retry contract | Concurrent production setup commands against PG | One resource/audit/receipt/secret response; identical replay200; changed data/time409; future400/old410 even after receipt removal; authority rechecked before replay |
| AC7 / concurrency | Competing key use/revoke, verify/revoke and store disable/use | Lock ordering determines one valid serial outcome; waiter rechecks state after lock and no post-revocation acceptance |
| AC8 / boundaries | Existing identity/foundation/boundary/contract suites | P03 shared failure/router/shell behavior retained; max-page Unicode JSON remains bounded; offline compile, fmt, fresh migration and SQLx metadata |

Run focused `attribution_installation_`, `attribution_destination_`,
`attribution_credentials_` tests plus setup retry/HTTP tests and existing identity
tests affected by router/error/shell changes. Browser proof uses fixture-owned
authority and actual local service/PG; it does not establish live external DNS or
production Openbot deployment. Final whole-plan suites/container gate remain p10.

### Contract impact and traced consumers

| Value/interface | Producers, consumers, wrappers | Disposition and proof |
| --- | --- | --- |
| `IdentityService::company`, `verify_csrf`, `TenantPrincipal` | identity HTTP shell/session; new setup HTTP | Reuse live authority; no permission changes. Session/local commit recheck added only in setup transaction; existing identity HTTP suite retained |
| `lock_tenant`, status freshness | lifecycle SQL, reconciliation, setup and future merchant writes | Reuse same row and lock order; real competing writers and p03 lifecycle tests |
| HTTP router/state/error mapper | startup, identity fixture, foundation shutdown tests | Add setup composition and Gone410; existing global limits/security/errors retained; focused foundation tests |
| Cipher/random/verifier | identity sessions, setup cursors and keys | Reuse maintained primitives with separate purpose; no key material/session schema changes |
| New persisted stores/domains/keys/receipts | setup SQL/UI; p05–p09 future consumers | Composite scope and transaction entry published to later phases after gate via coordination; no Openbot source changes |
| Initial migration | migrate, fresh DB fixtures, SQLx prepare | Reset only owned fixtures; fresh migration and metadata check; deployment remains unapproved |
| Setup wire contract | OpenAPI, API handlers, JS | Preserve declared routes/status/schema, 16KiB, bounded lists and original-time retries; runtime response validation and browser proof |

Discovery: Trail symbol/caller/literal passes inspected actual identity HTTP,
tenant SQL and startup composition. Ambiguous caller edges were supplemented by
literal references; no existing setup/merchant owner exists. No tests removed.
