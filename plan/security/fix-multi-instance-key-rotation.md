# Fix Multi-Instance Credential-Key Rotation

## Goal

Make credential-encryption key rotation safe and verifiable when several application Machines are
serving traffic during a rolling deployment. No Machine may encounter ciphertext it cannot
decrypt, late writes from an old Machine must be rotated before the old key is removed, and the
operator must have database-backed proof of convergence rather than relying on one startup log.

This plan owns the rotation protocol. The credential format, strict plaintext rejection,
encryption boundary, deployment bootstrap, and key custody work live in
`plan/security/improve-key-credentials.md`.

## Current Behaviour and Failure Mode

`CredentialCipher` chooses the numerically highest key version as active.
`PostgresPersistence::rotate_credentials` runs independently on every process boot, loads every
credential, and compare-and-swap updates rows that do not carry that active version. The CAS makes
two rotators updating the same ciphertext safe, but it does not make a rolling key change safe.

With Machines A and B initially configured only with version 1:

1. Fly starts replacing Machines after the secret is changed to versions 1 and 2.
2. New Machine A immediately treats version 2 as active and may rotate or write version-2 data.
3. Old Machine B still has only version 1. If it reads the version-2 row, decryption fails.
4. Even after A finishes its scan, B can make a late version-1 write.
5. A per-process rotation count cannot prove that no version-1 rows remain, and removing version 1
   can make the next boot fail.

## Design Decisions

### Separate key availability from write activation

Add a required `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` setting. The key ring says which versions a
Machine can decrypt; the active-version setting says which version it uses for new encryption.
Never infer the active version from the highest available version.

This enables two distinct rolling deployments:

- **Distribute:** every Machine receives both old and new keys while continuing to write the old
  version.
- **Activate:** after distribution is complete, every Machine can safely move to the new write
  version; Machines on either side of this second rollout can decrypt both versions.

Startup must reject an active version that is absent from the key ring, version zero, duplicate
versions, malformed keys, and an empty key ring. Changing key bytes without changing their version
must remain explicitly forbidden and tested.

### Make rotation an explicit operation

Remove the full-table mutation from ordinary application startup. Add commands to the existing
`mail_agents` binary so the production image needs no second executable:

```text
/app/mail_agents credentials status
/app/mail_agents credentials rotate
```

Normal startup validates configuration but does not claim that stored credentials have converged.
The deploy runbook invokes the commands after all Machines have crossed the relevant rollout
boundary.

`credentials status` is read-only. It reports structured counts by envelope/key version and counts
of malformed or unavailable versions, without returning ciphertext or plaintext. It exits nonzero
when any row is malformed, refers to an unavailable key, or when a caller-provided
`--require-version <N>` condition is not met.

`credentials rotate`:

- acquires a dedicated PostgreSQL advisory lock on one connection and fails clearly if another
  rotation owns it;
- rotates toward `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` in bounded batches;
- decrypts and authenticates each candidate before writing;
- updates by `(company_id, provider, old ciphertext)` CAS;
- repeats until a final status pass observes zero non-active rows;
- emits structured fields for target version, scanned, rotated, CAS conflicts, invalid rows,
  batches, and duration; and
- exits nonzero on invalid data, unavailable keys, database errors, or incomplete convergence.

The advisory lock prevents duplicate full scans. CAS remains required because ordinary credential
writes can race with the rotator. Do not hold the advisory lock through a connection pool handle
that could return a different session; retain one dedicated connection for the command's lifetime.

### Bound database work

Replace `fetch_all` with a fixed batch size and deterministic keyset ordering on
`(company_id, provider)`. Keep transactions per batch so locks and rollback scope remain bounded.
The batch size is an internal constant with a test that proves the query never returns more than
the bound. Do not expose an unvalidated environment variable for it.

The final verification pass must not infer validity solely from `starts_with`. It must use the
typed envelope parser from `improve-key-credentials.md` and authenticate rows. If the status query
uses SQL to group obvious versions efficiently, all malformed/ambiguous values must still enter an
invalid bucket and cause failure.

## Implementation Steps

### 1. Add the explicit active-version contract

1. Parse `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` as a positive integer.
2. Require the version to exist exactly once in `CREDENTIAL_ENCRYPTION_KEYS`.
3. Store it explicitly in `CredentialCipher`; remove the `max()` selection rule.
4. Add the setting to `.env.example`, `docs/deploy.md`, bootstrap, and production-secret
   validation.
5. Log only the active version and available version numbers at startup, never key material or
   secret digests.

Because the application server has not yet been launched and the Fly database has no schema, this
can be required from the first production boot without a compatibility default. Local development
will be reset before implementation.

### 2. Add operational command dispatch

1. Parse process arguments before starting HTTP, SMTP, task, or memory workers.
2. Reuse tracing initialization, database connection configuration, migrations, and cipher
   configuration.
3. Implement `credentials status` and `credentials rotate` as narrow infrastructure operations;
   they must not initialize unrelated external providers or listeners.
4. Ensure both commands are available in the existing non-root production image.
5. Document Fly invocation using a one-off process or SSH command that inherits the app's secrets.

### 3. Implement bounded, single-owner rotation

1. Extract one batch operation returning a named outcome structure rather than a tuple.
2. Acquire the advisory lock on a dedicated connection.
3. Select a bounded ordered batch of non-active rows.
4. Parse and authenticate each envelope with its row context.
5. Re-encrypt under the active version and CAS update it.
6. Commit the batch and continue from a deterministic cursor.
7. Account for CAS misses, since a normal writer may already have replaced the row.
8. Run an authoritative final status pass before returning success.

If keyset pagination could skip a concurrent late write behind the cursor, start another complete
bounded pass whenever the previous pass changed any row. Stop only after a complete pass changes
zero rows and the final status check agrees.

### 4. Remove startup rotation authority

1. Stop invoking `rotate_credentials` from `postgres_persistence()`.
2. Keep startup fail-closed configuration validation.
3. Do not treat an informational startup count as a rotation completion signal.
4. If a lightweight startup status metric is retained, label it as an observation only and keep
   it bounded; deployment correctness must depend on the explicit command.

### 5. Write the multi-Machine runbook

For rotation from version 1 to version 2:

1. **Preflight:** securely back up/escrow both key versions and run
   `credentials status --require-version 1`.
2. **Distribute key 2:** set the key ring to `1:OLD,2:NEW` while keeping
   `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=1`.
3. **Verify distribution:** wait for the Fly rollout to finish and verify every running Machine is
   healthy and on the release containing both keys.
4. **Activate key 2:** set `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION=2` without removing key 1.
5. **Verify activation:** wait until no Machine from the active-version-1 release remains.
6. **Converge:** run `credentials rotate`, followed by
   `credentials status --require-version 2`; require zero old, malformed, and unavailable rows.
7. **Retention:** retain key 1 for the documented backup/recovery window. Restoring an older backup
   must also restore the key versions needed to read it.
8. **Retire:** re-run status, remove key 1, complete the rollout, and run status once more.

Abort before activation if distribution is incomplete. Abort before retirement if any old Machine,
old-version row, malformed envelope, unavailable version, or unaccounted backup remains.

## Tests

### Unit tests

- An explicit active version is required and must exist in the key ring.
- Adding a higher available version does not change the active version.
- A Machine active on version 1 can decrypt version-2 ciphertext after distribution.
- A Machine active on version 2 can decrypt late version-1 ciphertext.
- Reusing a version number with different bytes fails to decrypt existing data and is documented
  as an invalid rotation operation.
- Status categorizes active, old, malformed, and unavailable envelopes without exposing values.

### Database-backed tests

- Two competing `credentials rotate` commands contend for the advisory lock; exactly one owns the
  rotation and the other exits without mutating data.
- A normal credential update racing a rotation cannot be clobbered by the rotator's CAS.
- A simulated old Machine writes version 1 after the first version-2 pass; the next pass finds it
  and convergence succeeds.
- Batches respect their bound and eventually rotate more than one batch.
- A malformed or unavailable-version row prevents a successful completion result.
- A second completed rotation is idempotent and reports zero mutations.

### Deployment-script tests

Use a fake `fly` executable on `PATH` to exercise distribution, activation, verification failure,
and retirement ordering without contacting Fly. Add the tests to CI; a test that exists but is not
run by CI is not an acceptance signal.

## Acceptance Criteria

- No rollout phase can create ciphertext that any still-serving Machine cannot decrypt.
- The active write version is explicit and independent of available read versions.
- Ordinary startup performs no unbounded credential mutation.
- At most one explicit rotator owns the database operation at a time.
- Rotation is bounded, CAS-safe, repeatable, and tested with competing claimants and a late old
  writer.
- Removing an old key requires a completed platform rollout, database convergence proof, and an
  explicit backup-retention decision.
- Logs and command output contain versions and counts only, never keys, ciphertext, plaintext, or
  provider credentials.

## Out of Scope

- Provider API-key revocation; that is covered by `improve-key-credentials.md`.
- Choosing or implementing a KMS/envelope format; the rotation command consumes the cipher
  interface delivered by that plan.
- Automatic time-based rotation scheduling. Establish a correct, rehearsed manual protocol first.
