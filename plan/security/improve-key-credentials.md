# Improve Provider-Credential Security

## Goal

Close the remaining credential-storage and launch gaps. Multi-instance key rotation is no longer one
of them: the rollout protocol, bounded rotation, and convergence proof are implemented in
`src/adapters/persistence/credential_rotation.rs` and `scripts/credential-key-rotation.sh`, and the
runbook is `docs/deploy.md` §"Multi-Machine credential-key rotation".

The result must prevent plaintext persistence by construction, authenticate a credential's row
context, satisfy the repository's envelope-encryption requirement or record an explicit approved
exception, validate every stored envelope fail-closed, make launch secrets recoverable and
reproducible, and revoke credentials previously exposed during development.

## Verified Starting Point

- Provider credentials live in `company_model_connections`, not in `companies`, `agents`, and
  `channels`.
- Production construction requires `CREDENTIAL_ENCRYPTION_KEYS`; a missing or malformed key ring
  already prevents a healthy startup.
- `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` selects the write key independently of the highest readable
  version, and startup rejects an active version absent from the ring.
- Rotation no longer runs at process boot. `mail_agents credentials status` and
  `mail_agents credentials rotate` are explicit subcommands; rotation scans in keyset-cursored
  batches under a PostgreSQL advisory lock, updates compare-and-swap, and emits a JSON convergence
  report.
- AES-256-GCM uses a random 96-bit nonce and records a key version in the stored string.
- Ordinary model-connection projections expose only `has_api_key`; the narrow credential read
  decrypts only when a caller needs the provider key.
- The local database currently contains one encrypted credential and will be reset before this
  work.
- The Fly Postgres Machine exists but currently has no public schema, and the application app has
  not been created. The production migration can therefore be designed before any production
  credential row exists.

Resetting a database does not revoke a provider credential. Any Google, OpenAI, Anthropic, Groq, or
other provider key entered during development remains valid until it is revoked at that provider.

## Security Gaps in Scope

1. `decrypt` accepts non-`enc:` values as plaintext (`credentials.rs:141-143`).
2. `PostgresPersistence` can be constructed without a cipher, and credential reads/writes then pass
   plaintext through (`persistence/mod.rs:53-75`).
3. The database constraint accepts any non-empty string rather than requiring a supported encrypted
   envelope (`20260817000000_init_schema.sql:173-175` checks only non-blank and a length cap).
4. AES-GCM uses empty associated data, allowing valid ciphertext to be moved between company or
   provider rows without authentication failure (`Aad::empty()` in `encrypt` and `decrypt_parsed`).
5. One long-lived environment key directly encrypts every provider credential. That is
   application-layer AEAD, but it is not technically envelope encryption as required by
   `src/adapters/persistence/AGENTS.md`.
6. Fly secret values cannot be retrieved later. `docs/deploy.md` requires escrow and forbids reusing
   a version number, but names no secret manager and defines no key inventory, recovery drill, or
   loss/compromise response.
7. External provider-key revocation is an unaudited manual acceptance statement.

Three gaps from the original list are closed and are not in scope. `needs_rotation`'s prefix test is
gone — `CredentialCipher::inspect` parses and decrypts a row before classifying it. Bootstrap sets and
preflights the encryption secrets (`scripts/deploy.sh`, `scripts/tests/deploy.sh`). Rotation is
bounded rather than loading every credential into memory. The parse/authenticate interface this plan
introduces must keep those consumers working.

## Required Architecture Decision

Resolve this before coding the new envelope format. Record the selected threat model and decision
in `docs/deploy.md` or a security ADR.

### Preferred: KMS-backed envelope encryption

Select a designated KMS or secret-management service and use a non-exportable key-encryption key
(KEK). Generate a random data-encryption key (DEK) per credential, encrypt the provider credential
locally with the DEK, wrap the DEK with the KMS KEK, and store only ciphertext plus the wrapped DEK.
Restrict the application identity to the minimum encrypt/decrypt key permissions and document
availability, latency, audit-log, and disaster-recovery behaviour.

### Bounded launch alternative: local envelope encryption

If adding an external KMS is deliberately deferred, implement the same per-credential DEK design
with a versioned KEK held in Fly secrets. This satisfies the envelope structure and makes KEK
rewrapping possible, but it does not protect against a compromised application process or malicious
deployment because the KEK is exportable to the process. Record that limitation, an owner, and a
review trigger for moving the KEK to KMS.

Do not call the current direct-master-key format envelope encryption. If the project chooses to keep
it instead, update the repository rule through an explicit security decision rather than silently
declaring the existing format compliant.

## Envelope and Context Design

Introduce a new, fully parsed envelope version; do not extend correctness logic with more
`starts_with` checks. The logical contents are:

- format version;
- KEK/key version;
- data nonce;
- provider-key ciphertext and authentication tag;
- wrapped DEK and its nonce/tag for local wrapping, or the KMS wrapped-DEK blob; and
- enough unambiguous algorithm metadata to support a future migration.

The record identity is associated data, not plaintext included informally in the ciphertext:

```text
credential-context-v1 | company_id | canonical_provider
```

Use a length-delimited or canonical binary encoding so different field combinations cannot collide.
Pass the same context to encrypt, decrypt, DEK wrap, validation, and rotation. Moving an envelope to
another company or provider must fail authentication.

Introduce named types such as `CredentialContext`, `EncryptedCredential`, `KeyVersion`, and
`ParsedCredentialEnvelope`. APIs should have shapes equivalent to:

```text
encrypt(context, secret) -> encrypted envelope
decrypt(context, envelope) -> secret
parse(envelope) -> parsed envelope metadata or a typed error
rewrap_or_rotate(context, envelope, target version) -> encrypted envelope
```

Never include the credential, ciphertext, wrapped DEK, nonce, or raw key material in `Debug`, error
messages, tracing fields, metrics, or durable task payloads. Use secret/zeroizing containers where
the crypto and provider interfaces permit, while recognizing that some provider libraries may
still require an ordinary string at their boundary.

## Implementation Steps

### 1. Establish preconditions and external credential inventory

1. Reset the local database as planned and apply all migrations from scratch.
2. Verify `company_model_connections` is empty locally and in production before removing legacy
   plaintext compatibility.
3. Inventory every provider key used in development without writing the key value into the plan,
   issue tracker, logs, or shell history.
4. Revoke each old key at its provider, create a least-privilege replacement, and record provider,
   owner, revocation date, replacement date, and evidence reference in a secure operational record.
5. Do not enter replacement provider keys until the strict encrypted writer is deployed.

### 2. Implement the selected envelope format

1. Add the KEK/DEK abstraction without importing a cloud SDK into the domain or application layer.
2. Generate independent random DEKs and nonces with the existing system CSPRNG or the selected KMS
   data-key operation.
3. Encrypt provider credentials with AES-256-GCM and bind `CredentialContext` as associated data.
4. Wrap the DEK under the selected KEK with the same contextual binding.
5. Parse the entire envelope strictly: exact field count, supported format/algorithm, valid positive
   versions, decoded lengths, nonce lengths, and authentication tags.
6. Return typed errors that identify the row and failure class but never its secret material.
7. Keep an explicit reader for the current `enc:v1` format only if a verified database inventory
   shows rows that must be migrated. With both databases reset/empty, prefer not shipping a legacy
   reader at all.

### 3. Make encrypted persistence mandatory

1. Remove plaintext fallback from `CredentialCipher::decrypt`.
2. Make `encrypt_credential` and `decrypt_credential` fail if no cipher is configured; they must
   never return the input unchanged.
3. Reshape construction so production credential persistence cannot exist without a cipher. If
   cipher-less persistence remains for unrelated database tests, keep it explicitly test-only or
   ensure every credential method returns an error.
4. Add a new timestamped migration with a `CHECK` constraint requiring a supported encrypted
   envelope. Do not edit `20260817000000_init_schema.sql`, which repository rules mark immutable.
5. Add a rollback-only database test proving plaintext, empty, and unsupported envelopes are
   rejected while the supported envelope is accepted.

The database constraint is defense in depth, not a substitute for application parsing and AEAD
authentication. It need only recognize supported envelope structure; it must not pretend a regex
proves that ciphertext is authentic.

### 4. Route every validity decision through the parsed envelope

`CredentialCipher::inspect` already parses and decrypts a row before classifying it, so the prefix
test is gone. What remains is making that path context-aware and its failures distinguishable.

1. Give `inspect` the row's `CredentialContext`, so an envelope moved between companies or providers
   classifies as a failure rather than as `Active`.
2. Split the collapsed `CredentialState::Malformed` into distinct internal failure classes:
   plaintext, malformed fields, invalid authentication tag, invalid UTF-8, and context mismatch.
   `Unavailable { version }` already stands apart; keep it.
3. Fail closed at operational validation (`credentials status`) and at narrow credential reads.
4. Ensure HTTP callers still receive a generic internal error while structured server logs contain
   only safe row identifiers and failure class.

The bounded status and rotation consumers of this interface already exist in
`src/adapters/persistence/credential_rotation.rs`; port `CredentialState` classification onto the new
parse API rather than adding a second validity path.

### 5. Carry the new format through bootstrap, deploy preflight, and documentation

Bootstrap already sets `CREDENTIAL_ENCRYPTION_KEYS` and `CREDENTIAL_ENCRYPTION_ACTIVE_VERSION` in the
same `fly secrets set` as the other launch secrets, `validate_app_secrets` refuses to deploy an
existing app missing either name, `scripts/tests/deploy.sh` exercises both paths against a fake `fly`,
and the first-deploy command in `docs/deploy.md` names both settings. What remains is whatever the
selected envelope format changes:

1. Update `scripts/generate-credential-encryption-key.sh` to emit the KEK/KMS configuration the
   selected format needs, and update bootstrap and the first-deploy command to match.
2. Extend the preflight required-name list and `scripts/tests/deploy.sh` together if the format adds
   a setting. Preflight checks names only; runtime parsing remains authoritative.
3. Document that a missing or invalid key causes startup failure, and that secret values must never
   be placed in `fly.toml`, command output captured by CI, or repository files.

### 6. Define key custody and recovery

1. Name the external secret manager/password vault that is the source of truth for exportable KEKs,
   or document KMS resource identifiers and access policy for non-exportable KEKs.
2. Store key version, creation time, active/retired state, custodian, rotation reason, and recovery
   contact without storing raw keys in operational logs.
3. Define how database backups retain the key versions necessary to decrypt them.
4. Rehearse recovery in an isolated environment using a database backup and escrowed keys.
5. Define the response to key loss and suspected compromise, including provider-key revocation when
   confidentiality can no longer be assured.
6. Keep retired keys until the corresponding backup retention window ends, then destroy them and
   record the destruction.

### 7. Add safe observability

Emit structured fields for envelope format, key version, validation outcome, and row counts. Never
emit provider credentials, ciphertext, wrapped DEKs, raw keys, or full connection objects. Add a
named validation/rotation duration and working-set metric so future scale decisions use evidence.

`CredentialRotationReport` already carries per-batch counts, duration, and a convergence flag; extend
it rather than adding a parallel channel.

## Tests

### Cryptography and parsing

- Current-format encrypt/decrypt round-trip with the correct row context.
- Ciphertext copied to another company or provider fails authentication.
- Ciphertext, tag, wrapped DEK, nonce, algorithm, format version, and key version tampering fail.
- Plaintext and encrypted-looking junk fail rather than pass through.
- Unknown and unavailable versions fail without exposing values.
- Independent encryptions of the same provider key produce different ciphertext and DEKs.
- Keys, secrets, and ciphertext do not appear in `Debug` or error output.

### Persistence and migrations

- Every credential write stores only the supported encrypted envelope.
- Cipher-less credential reads and writes fail.
- The database rejects plaintext and unsupported envelope formats.
- Narrow reads decrypt successfully; list/detail projections still return only `has_api_key`.
- A row-context substitution fails at the persistence boundary.

### Configuration and deployment

- Missing, empty, malformed, duplicate, wrong-length, and unavailable active keys fail startup.
- Bootstrap supplies all required encryption settings without echoing them.
- Existing-app deploy preflight rejects missing secret names.
- Fake-Fly script tests cover success and failure and run in CI.
- Documentation examples name only settings the application actually reads.

### Recovery and operations

- A documented recovery drill restores and decrypts a test backup using escrowed keys.
- External provider-key revocation has a checklist with accountable sign-off; tests must not pretend
  to verify provider-side revocation.

Also run formatting, offline compilation, migrations, the database-backed suite, Clippy, and the
existing stack-budget check required by repository policy.

## Rollout Order

1. Approve the KMS/local-envelope architecture decision and threat model.
2. Reset local development data and verify both credential tables are empty.
3. Implement the new envelope, associated data, strict parsing, and mandatory persistence boundary.
4. Add and test the additive database constraint migration.
5. Implement bootstrap/preflight and key custody documentation.
6. Configure and escrow the initial production key material.
7. Deploy to the empty production database and run `credentials status`.
8. Revoke development provider keys, create least-privilege replacements, and enter only those
   replacements through the application.
9. Verify stored rows are supported envelopes and complete the secure operational sign-off.

## Acceptance Criteria

- No supported production or test construction can persist or return plaintext by fallback.
- PostgreSQL rejects plaintext credential storage.
- Every credential is AEAD-authenticated against its company and canonical provider.
- The implementation either uses true envelope encryption/KMS or carries an explicit approved
  exception that changes the repository requirement and documents the threat-model limitation.
- Malformed, tampered, moved, and unavailable-version envelopes fail closed and are safely
  observable.
- Launch tooling supplies all required settings, runtime validates their values, and key recovery is
  rehearsed.
- Every provider credential exposed during development is revoked independently of database reset.
- All new tests, including deployment-script tests, run in CI.

## Out of Scope

- The two-phase multi-Machine rollout, advisory-lock ownership, bounded rotation command, and old-key
  retirement sequence. These are implemented; `docs/deploy.md` is the runbook.
- General provider-token scoping capabilities that a provider does not offer.
- Protection from a fully compromised application process. Even KMS-backed encryption cannot stop
  authorized application code from requesting plaintext when it legitimately needs to call a
  provider; KMS primarily improves key custody, auditability, and blast-radius control.
