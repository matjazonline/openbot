# 01 — Profiles, identifiers and tags

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on the implemented attribution service and on decisions D1 and D2 in the
[README](README.md). A server key or a permitted user can resolve, create, read and update a
profile, attach identifiers and tags, and record a do-not-contact entry. Competing writers that
name the same business end with one profile.

## Batch 01A — Records, constraints and key scopes

1. Add to the one migration, each scoped by `company_id` and `store_id` with composite foreign
   keys like the existing tables:
   - `attribution_profiles`: id, `kind` (`person` or `organization`), display name (1–200
     characters), optional `organization_id` naming an organization profile in the same store,
     `attributes`, `revision`, created and updated times.
   - `attribution_profile_identifiers`: `kind` (`email`, `phone`, `domain`, `external`), the
     normalized value, the owning profile. Unique on company, store, kind and value. For email,
     the domain part is a stored generated column with an index, used to match domain-wide
     do-not-contact entries.
   - `attribution_profile_tags`: profile and tag. Unique on profile and tag.
   - `attribution_suppressions`: identifier kind and normalized value, a reason from a fixed
     short list (`asked`, `bounced`, `complaint`, `operator`), created time, actor. Unique on
     company, store, kind and value. It has no foreign key to a profile.
2. Attributes are one flat JSON object: at most 64 keys matching `^[a-z][a-z0-9_]{0,63}$`; each
   value is a string, number, boolean or an array of at most 32 strings; the encoded object is at
   most 16 KiB. Enforce the shape in the application and the size and object type in a check
   constraint. Long text such as a full website review does not belong here; store a reference.
3. Normalize identifiers in one application function per kind, used by every write and lookup:
   email is trimmed and lowercased, with no provider-specific rewriting; phone is E.164; domain
   reuses the service's canonical host rules; external is `namespace:value` with a
   `^[a-z][a-z0-9_]{0,31}$` namespace and a value of at most 200 bytes. A value that looks like a
   secret or carries control characters is refused.
4. Add the scopes `profiles:read`, `profiles:write`, `activities:write` and `definitions:write`
   to `KeyScope`, to the `attribution_credentials` scope constraint and to its cardinality bound,
   and to the key administration page. Existing scopes keep their meaning.
5. Add the new tables to the tenant purge order and to the suspended and deleted company
   refusals, so a deleted company's profiles are unreachable at once and removed by maintenance.

Focused checks: `attribution_profiles_schema_` for every constraint against real PostgreSQL
(foreign store, foreign company, oversized and non-object attributes, duplicate identifier,
organization link across stores) and `attribution_profiles_normalize_` for the identifier
functions, including Unicode, case, whitespace and secret-shaped values.

## Batch 01B — Resolve, read, update and tag

1. `POST /v1/profiles/resolve` takes 1–8 identifiers and optional initial fields. With no match
   it creates the profile and its identifiers in one transaction. With one match it returns that
   profile and adds any new identifiers. When the identifiers match two different profiles it
   returns 409 naming both profile ids and changes nothing.
2. Serialize competing resolves on the identifier, using the unique constraint as the arbiter:
   a writer that loses the insert re-reads and returns the winner's profile. Do not hold a lock
   across the request, and do not rely on a read-then-insert without the constraint.
3. `GET /v1/profiles` lists with bounded pagination and the service's opaque, scope-bound
   cursors; it filters by identifier, tag and kind. `GET /v1/profiles/{id}` returns the profile,
   its identifiers, tags and contact status.
4. `PATCH /v1/profiles/{id}` requires the current `revision` and returns 409 on a stale one.
   Attribute updates are per key: a key set to `null` is removed, absent keys are unchanged. The
   merged object must still satisfy the bounds.
5. `PUT` and `DELETE /v1/profiles/{id}/tags/{tag}` are idempotent. Tags match
   `^[a-z0-9][a-z0-9_-]{0,63}$`, at most 64 per profile. `GET /v1/tags` lists the store's
   distinct tags. There is no tag registry.
6. Every write records an audit row with the existing actor kinds and never stores identifiers,
   attributes or tag text in the audit table or in logs.

Focused checks: `attribution_profiles_` for permission and scope denial per route, a key reaching
another store, stale revisions, attribute merge and bounds, tag idempotency and pagination; and
`attribution_profiles_resolve_` with at least eight competing resolves of one new identifier and
of overlapping identifier sets, asserting exactly one profile and no lost identifiers.

## Batch 01C — Do-not-contact

1. `POST /v1/suppressions` adds an entry by identifier; it is idempotent. `DELETE` requires
   `integrations:manage` and a user session; a key cannot lift a suppression.
2. A profile read reports `contact_status`: `suppressed` when any of its identifiers, or the
   domain of any of its email identifiers, has an entry; otherwise `allowed`.
3. `POST /v1/contactability` takes up to 32 identifiers and answers `allowed` or `suppressed` for
   each, without revealing whether a profile exists.
4. Listing suppressions requires `integrations:manage`. This table is the one place an erased
   contact's identifier remains; say so in the service README.

Focused checks: `attribution_suppressions_` for idempotent adds, domain-level matches,
normalization equality between the entry and the lookup, key denial on removal, and a competing
suppression and resolve on the same identifier ending with the profile reported as suppressed.

Completion: profiles can be written and read by a store key and by a permitted user, with one
profile per identifier under competition and suppression visible on every read. Record commands,
counts and results here; regenerate SQLx metadata; update the OpenAPI contract and its examples.
Full-suite gate: [06](06-retention-erasure-and-final-verification.md).
