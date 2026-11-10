# 02 — Company machines and pairing

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-contracts-and-cloudroom-compatibility.md). Persist machine ownership and pair a
single-company Cloudroom installation without placing user sessions or provider credentials in it.

## Batch 02A — Storage and live company authorization

Use application use cases and the existing `managed_company` authority for enrollment, machine
enable/disable, revocation, profile changes, and project advertisement changes. Ordinary reads and
runs also require their existing company/conversation/workflow permissions; membership alone is
not permission to read every output. Foreign IDs remain indistinguishable from missing IDs.

Add normalized persistence for:

- Machines: company, installation ID, display name, state, credential generation, last observation,
  capabilities snapshot and active connection generation/expiry.
- Pairing attempts: hashed random code, creating actor, company, expiry, consumed/revoked state,
  installation binding and claim-verifier hash for safe response-loss recovery.
- Credential verifiers: machine and generation, high-entropy token hash, creation/revocation time.
  Store no plaintext long-lived machine credential in ordinary database rows or responses.
- Profiles and sessions as defined in phase 01. Session links include their authorized Openbot
  association; `(installation_id, cloudroom_thread_id)` is an external identity, not a global ID.
- Operations, observations and connection ownership in preparation for phase 03. Tenant-qualify
  uniqueness and foreign keys; use conditional transitions and database time for expiry.

Keep scopes explicit in indexes and port signatures. Prevent a machine/profile/session from
referencing another company's resource even through direct SQL. A local installation's pairing
identity may not be attached to two companies. Revocation does not release it for silent reuse.
Reset/re-pair creates a new Openbot installation identity and never adopts old thread mappings.

Follow the repository's clean init-schema/reset policy and regenerate `.sqlx` metadata after SQL
changes. Reuse existing secret wrappers/redaction and auth rate-limiting patterns, without treating
machine authentication as an application user's login or Google/app authorization.

## Batch 02B — Pairing protocol

Proposed HTTP surfaces, implemented through company use cases:

- Authenticated company management creates/cancels a pairing code and lists pairing status.
- `POST /v1/local-machines/pair` accepts a code, plugin installation ID, display name, and hashes
  of plugin-generated claim secret and machine credential. Values have bounded canonical encodings.
- `GET /v1/local-machines/pair-status` uses a dedicated secret-bearing authorization header to
  check that same claim after a lost response; no secret appears in a query string.
- Company management can disable, enable, revoke, and explicitly start a replacement pairing.

The plugin generates and durably saves its high-entropy claim secret and machine credential in
secret storage **before** attempting enrollment. Openbot commits only their verifiers. A successful
claim atomically consumes the code and binds those verifiers to the new machine; the response
contains identifiers and status, never a replacement secret that could be lost in transit.

Two claimants with different installation/claim identities cannot both consume a code. A replay
from the winning claim may recover its original machine ID; it cannot bind a new key or company.
The secret-authorized status check proves the same result if the initial response disappeared.
Expiry/revocation is checked in the transaction; existing admin sessions and CSRF protection govern
code creation. Rate-limit failed claims by IP and code/claim hash with bounded storage, returning
generic failures without revealing company names or whether an installation is already enrolled.

The plugin displays the resolved server origin and company before saving the completed pairing.
It accepts HTTPS in production, validates certificates, and follows no credential-bearing redirect
to another origin. Local development exceptions must be explicit and loopback-only. Enrollment
grants management authority over the trusted local account; the code is never an agent prompt.

Machine credentials authenticate only the paired machine's connection/observations. They cannot
create workflows, read arbitrary company data, or call general Openbot APIs. Advertised machine
name, OS and provider catalog are claims from that machine, not additional authority.

## Batch 02C — Disable, revoke and replace

Disable blocks new operations; already accepted work is visible and may be stopped explicitly.
Revoke also invalidates current credentials and connection ownership atomically, closes reachable
connections, and refuses subsequent observations/control acknowledgments under that generation.
Persist the unresolved state of running work; do not label it physically stopped without local
evidence. A disconnected plugin cannot receive a remote stop or wipe instruction.

Use explicit revoke-and-repair for v1 credential replacement, with no overlapping valid secrets or
automatic transfer of old mappings. Retain audit/operation records according to phase 06. A local
disconnect removes this pairing's secret and stops its connector, preserving Cloudroom threads
and workspace files. It must not erase provider logins or unrelated Cloudroom settings.

**Focused checks:** `cloudroom_pairing_`, `cloudroom_company_`, and `cloudroom_revocation_` Rust
tests using real persistence. Include competing pairing claimants, replay after lost response,
expiry racing consumption, revoke racing WebSocket establishment, foreign-company reads/writes,
and unavailable database errors propagating instead of becoming authorization decisions. Use an
isolated `own_database` fixture for unscoped claims/sweeps. Migrate fresh and regenerate metadata.
