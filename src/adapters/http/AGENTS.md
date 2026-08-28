These rules govern HTTP routes, authentication, sessions, middleware, and responses in addition to
`src/AGENTS.md`. The nested `pages/AGENTS.md` adds HTML-rendering rules.

# Authenticate machine ingress before parsing it

Every public webhook has an authenticated trust boundary: a provider signature or a required
startup-validated secret compared in constant time. Reject an invalid request before parsing a
large body or trusting sender, authentication, spam, or replay fields. Message-ID deduplication is
idempotency, not caller authentication.

Set an explicit body limit appropriate to each route and align mail-ingress limits across HTTP and
SMTP. Add endpoint and tenant/sender rate limits before expensive parsing, uploads, password
hashing, database fan-out, or provider calls. Network calls made during a request use a shared
client with an explicit timeout.

# Scope the object being used, not a sibling object

Every caller-supplied entity id is loaded through an ownership predicate containing the active
company and, where applicable, channel or parent id. Validating a schedule or channel does not
authorize a separate raw `thread_id` from the path/query. Prefer persistence methods whose
signature requires the scope, and re-check a loaded child's parent before displaying it or using it
to derive another operation.

Global monitoring, metrics, and operator data require an explicit operator authorization check;
an authenticated tenant user is not enough. Tests must attempt a valid id belonging to another
tenant for every new detail, nested, download, and mutation route.

# Harden browser session boundaries

State-changing browser requests require CSRF tokens or strict Origin/Referer validation in addition
to `SameSite`; cookies alone are not the defense. Apply CSP, HSTS where TLS termination permits,
frame, content-type, and referrer headers centrally. Browser scripts/styles are vendored or pinned
to immutable versions with integrity metadata; do not ship browser compilers or floating CDN
majors on authenticated pages.

Authentication endpoints are rate-limited and use a dummy password hash for unknown users so
existence is not exposed by timing. Sessions support revocation (for example `jti` or user token
version), and password/security changes invalidate existing sessions. Approval links intended for
external recipients must be reachable under their token policy and use an HTTPS public base URL.

# Keep request tracing narrow

Skip full request bodies, headers, credential-bearing params, and write structs from tracing
instrumentation. Explicitly record safe identifiers instead. Never derive `Debug` span fields for
types containing API keys, passwords, session tokens, approval tokens, or raw email content.

# Make partial-page updates race-safe

Treat SSE events as wake-ups, never as state. Subscribe before querying, reconcile current state
on every connect/reconnect, and use a durable cursor for append-only streams. Keep the element that
owns `sse-connect` mounted; swap a child, and preserve live attributes in every OOB replacement.

Read-only htmx controls that can issue competing requests to one target must use
`hx-sync="#target:replace"`, so the last user intent wins. Do not abort accepted writes: disable or
queue them instead. Tests must cover an event between render and subscribe and two reads completing
out of order whenever a new live or partial-page update protocol is introduced.
