# 04.9 — HTTP resources and registered HTTP tools

Implementation checkpoint; acceptance and evidence are recorded in
[docs/TASK.md](docs/TASK.md) and [docs/REVIEW.md](docs/REVIEW.md).

The application HTTP ports own company HTTP resources and operation registration.
`HttpConnections` management requires current workflow owner/admin authority and
expected revisions. Connection origin, company, identity and authentication mode
are immutable; replacing an origin creates a new resource. Credentials are stored
in tenant-and-connection-bound encrypted envelopes and participate in credential
inventory and key rotation. No credential is part of a published bundle or action.

`HttpPublicationDirectory` captures explicitly selected persisted registrations.
Publication checks their current authoritative schema, capability and recovery
facts; binding/admission additionally check the exact HTTP resource selected for
each alias. Unsupported agent/skill/profile/MCP dependency owners remain closed.
HTTP registrations use reconciliation recovery. Neither a verb nor an arbitrary
idempotency header registers safe repeat. Registered tools without a binding must
have one unambiguous current HTTP resource owner.

`adapters::workflow_http::HttpRuntime` composes the real `WorkflowHandler` and
direct `call_tool` entrances over `ActionService`. Worker construction supplies
the same lease policy to the handler and worker. Actual worker-startup/legacy
harness replacement remains at 06/08; this point supplies the provider and
composition seam. There is no new HTTP management route or authoring UI (09).

The guarded transport accepts an origin-relative absolute path, a supported method,
up to 32 allowlisted headers (8 KiB combined) and a JSON body up to 32 KiB. Paths are
at most 2048 bytes; request arguments at most 64 KiB. Credentials, routing and
hop-by-hop header overrides are unavailable. Responses are limited to32 KiB and
retained envelopes to 64 KiB; only content-type is retained from response headers.
Active credentials and credential-bearing JSON fields are redacted before receipt
storage. Non-success, redirect, timeout and oversized responses never establish
that a write was not applied.

The existing guarded HTTP client supplies HTTPS by default, vetted DNS answers
pinned into a fresh client, no proxy, no redirect, no automatic retry, a 5-second
DNS/connect bound, 10-second read timeout and 30-second total provider deadline.
Operator-owned exceptions are passed to `HttpDestinationPolicy::new` as exact
IP-literal origins; tenants cannot install exceptions and no environment key is
claimed. Current resource/policy/credentials are reloaded after DNS before sending.
The existing action supervisor owns cancellation and lease loss by dropping the
actual HTTP future. Receipts remain authoritative and unknown outcomes retain
the existing reconciliation protocol.

Architecture 04.9-D2 was independently approved in 04.9-R002; whole-batch code
review passed in 04.9-R003. Current verification and acceptance evidence is in
[TASK](docs/TASK.md) and [REVIEW](docs/REVIEW.md). No deployment, migration squash
or commit is part of this continuation.
