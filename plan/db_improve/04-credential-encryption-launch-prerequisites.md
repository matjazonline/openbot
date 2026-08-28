# Credential Encryption: Launch Prerequisites

## Goal

Close the operational half of audit item 7, and take the one hardening step that is only available
while the database is still empty.

## Current Risk

The in-code half needs no work. `PostgresPersistence::rotate_credentials`
(`src/adapters/persistence/mod.rs:76-105`) runs on every process boot from `src/infra/mod.rs:20`,
scans `companies`/`agents`/`channels`, and re-encrypts anything not under the active key version.
Updates are compare-and-swap (`WHERE id = $1 AND api_key = $3`), so a concurrent write cannot be
clobbered.

The audit's remaining instruction was to rotate provider credentials after migration. With no
production deployment and an empty database, that collapses to a small, cheap action — reissue
whatever keys were typed into dev — and the substantive risk moves elsewhere:

- **`scripts/deploy.sh` does not prompt for `CREDENTIAL_ENCRYPTION_KEYS`**, unlike `DATABASE_URL`,
  `JWT_SECRET`, and `SMTP_*`. It is documented in `.env.example` and `docs/deploy.md:110`, but a
  first deploy can start without it.
- **`CredentialCipher::decrypt` silently accepts plaintext.** At
  `src/adapters/persistence/credentials.rs:92-94`, any stored value not starting with `enc:` is
  returned unchanged. That branch exists solely to backfill legacy plaintext rows. On an empty
  database it can never legitimately fire — but it will stay in the code as a permanent silent
  accept for any unencrypted value that reaches the column later, from a manual `UPDATE`, an
  import, or a future writer that skips the cipher.
- **`infra/mod.rs:20` discards the `u64` that `rotate_credentials` returns**, so a rotation pass
  leaves no evidence it ran.

## Design

- Make the plaintext branch in `decrypt` a hard error. Version-to-version rotation is unaffected:
  `rotate_credentials` only re-encrypts values that decrypt successfully, and every value written
  after this change is `enc:`-prefixed by construction. `needs_rotation` (`credentials.rs:128-130`)
  keeps its current meaning — it is `true` for a value under a non-active version, which is the
  only case that can now occur.
- Add `CREDENTIAL_ENCRYPTION_KEYS` to the bootstrap secrets flow in `scripts/deploy.sh`, generated
  with the existing `scripts/generate-credential-encryption-key.sh` and set via `fly secrets set`.
  Fail the deploy when it is unset.
- Log the row count returned by `rotate_credentials` at startup.
- Write the launch checklist into `docs/deploy.md`.

Do the `decrypt` change before the first production row is written. Afterwards it needs a
verified-empty audit of three tables first, and stops being a free change.

## Implementation Steps

1. Replace the passthrough at `credentials.rs:92-94` with an `AppError::Internal` naming the
   column as holding an unencrypted value. Update `CredentialCipher`'s unit tests, which currently
   assert passthrough behaviour.
2. Add the secret to the prompts and validation in `scripts/deploy.sh`.
3. Capture and log the return value at `src/infra/mod.rs:20` — one info line with the count.
4. Add to `docs/deploy.md`: reissue any provider keys used during development; confirm the secret
   is set before first boot; confirm the startup line reports zero rotations on a fresh database.

## Tests

- `decrypt` returns an error for a non-`enc:`-prefixed value.
- Round-trip encrypt/decrypt still passes, including decrypting a value written under an older key
  version while a newer one is active.
- `rotate_credentials` re-encrypts a value written under version 1 when version 2 is active, and
  reports zero on a second boot.
- `scripts/deploy.sh` refuses to proceed when `CREDENTIAL_ENCRYPTION_KEYS` is absent.

## Acceptance Criteria

- An unencrypted value in an `api_key` column is a loud failure, not a silent pass.
- A deploy cannot start without an encryption key configured.
- Each boot reports how many credentials it rotated.
- No provider credential used during development remains valid at its provider after launch.

## Note for Later

`rotate_credentials` reads all three tables fully into memory on every boot with no batching. Fine
at current scale; revisit when the tables are large.
