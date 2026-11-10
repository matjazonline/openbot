# Review p6a-code-r1: P6a = P5 minors m1–m3 + batch 04A up to durable acceptance (reviewer B gen 2)

## Artifact identity

- Reviewer: role B gen 2, nonce `run=d756cf6c role=B gen=2`, agent `a80bac1035a115508`.
- Frozen full diff `tmp/memory-service-run/frozen/p6a-code-r1.diff`:
  - sha256 recomputed as `01b6957fe99ab8c09e2259f18c07d9d1e76f9902afe0ac9c2f9836dcbb66bd9b`; matches.
- Incremental diff from P5 (`0e841af1`), `p6a-incremental-from-p5.diff`:
  - sha256 recomputed as `7f695f92f2153ba66719c1e686f6d93aaa5bb3d0d0bdfd664e3eecfb16431d05`; matches.
  - 32 files.
- Worktree tree rebuilt with a temporary index: `668cd7d8c5733d1987c32b266eae2bc251465b0f`, identical to the frozen tree.
- Originals I read: 04 (04A in full), the P6a implementation notes, D9 as summarized there, and the claude-api skill's facts on the Messages API (the skill bundled with Claude Code 2.1.319).

**Context limit.** I reached about 49% of my context and reviewed by priority. I did not read in full:
- `application/observation_tests.rs`
- the `recall.rs` m2 two-pass code
- the outcome-mapping tail of `application/generation.rs` (from `ProviderError::Rejected` on)

For those I relied on the passing focused tests and the notes. A follow-up recheck should cover them; see "Remaining required checks".

## Checks I ran

- `scripts/verify.sh` on the private cluster, reusing the shared target only.
  - Result: all gates passed, **133/133** under macOS network isolation.
  - Log: `logs/b2-p6a-verify.log`.
- No network in tests: the Anthropic adapter tests bind `127.0.0.1:0` (one test points at a closed port for `not_sent`). The use-case tests use the counted local provider.

## Recheck of the P5 minors

| Item | Status |
| --- | --- |
| m1: `sources_sha256` + `changed` on provenance drift; `valid_until` includes provenance-target expiries | **Fixed** (per notes and tests; the shared provenance filter now returns each target's expiry) |
| m2: full packs select on exact served sizes before loading bodies | **Fixed in principle.** `octet_length(to_json(body)::text)` equals serde_json's string length: both escape only `"`, `\` and U+0000–U+001F (`\b \f \n \r \t` short forms, the rest `\u00XX`), and both leave `/`, DEL, U+2028/2029 and non-ASCII raw. The test covers quotes, backslashes, control characters, U+2028, emoji and accents. I did not read the rerun-on-correction path (see Context limit). |
| m3: `byte_count` excludes the n+1 array bytes and the envelope; the token bound applies to the service's encoding | **Fixed** (docs) |

## Coverage of 04A

| 04A item | Status |
| --- | --- |
| 1. Provider-neutral envelope | **Met.** Opaque `source_ref`, revision, `event_id`, `occurred_at` ordering, per-event level and role, bounded text (≤ 512 events, ≤ 512 KiB). No Claude Code, OpenBot or filesystem coupling. |
| 2. Evidence, save and inference kept separate; the request carries window, namespace, ceiling, consumed watermark, exact/fallback, expiry and allowance | **Met for the use case.** Acceptance and durable records are P6b. The `generations.*` operations stay `501`, so nothing exposed promises execution. The P6b deferrals are coherent: `MEMORY_ANTHROPIC_*` is neither read nor documented. |
| 3. Complete-window generation | **Met.** |
| 4. Typed, bounded output with an explicit nothing-to-save | **Met.** The provider schema has all properties required, `additionalProperties: false`, and no constraints. Local validation is stricter: unknown fields (such as `access_level`) are malformed, there are at most 16 items, D6 text bounds apply, cited event ids must be a subset of the window, and the summary must be present exactly when requested. Empty output, malformed output, `max_tokens` truncation and refusal map to distinct outcomes. |
| 5. Privacy before capture and provider entry, rechecked at the boundary; labels from trusted state; destination check | **Partly met:** labels and destination are met; privacy has F1 and m2–m4. Labels: `derive_label` runs over the ceiling, the consumed evidence and every sent event's level. All outputs share that floor, and none of it comes from the model. Any event above the ceiling, excluded or not, refuses the request. The destination check (`max_level`) runs before the call. Redaction runs again in `provider_request`. |

The Anthropic adapter request shape matches the claude-api skill:
- `anthropic-version: 2026-10-21` is a real version, and on it sampling parameters are a 400, so sending none is correct.
- `output_config.format = {type: json_schema, schema}` and `output_config.effort` (GA, inside `output_config`).
- `claude-sonnet-5-5` is a valid model id that supports both.
- Error types: `api_error` (500), `overloaded_error` (529) and `rate_limit_error` are retryable.
- The error body also carries `request_id`; the code keys on the `request-id` header, which is fine.
- `refusal` is a stop reason.
- A connection error maps to `not_sent`, and any send-then-unparsable answer maps to `unknown` and is never re-sent. That is correct.

## Findings

### F1 major: an unsalted SHA-256 of the raw request is stored for `sources.record`, defeating redaction against offline guessing
`services/memory/src/application/memory.rs`: in the `sources.record` arm, `authorize_write` hashes the request (about line 258) **before** redaction (about lines 280–283).

- The stored `idempotency_records.request_sha256` is SHA-256 over the canonical *raw* request, kept for 7 days or until the deadline.
- Everything else in that canonical request is in the database next to it:
  - the redacted content, with placeholder positions;
  - `external_ref`, revision, `occurred_at` and level;
  - namespace (via join) and `expires_at` (`request_expires_at`).
- So anyone who can read the database can confirm guesses or run a dictionary attack on exactly what was removed: `<private>…</private>` spans (often short phrases), and the low-entropy values the `secret` pattern takes (`password=hunter2`, PINs).
- 04A.5 requires privacy redaction before durable capture, and this hash is a durable, testable derivative of the raw text. (`content_sha256` is over the redacted text, which is fine.)
- Required:
  - Compute the canonical identity over the redacted request: redact first, then hash. A retry that differs only inside redacted spans then replays, which is harmless because the stored outcome is identical. Alternatively, use an HMAC with a server-held key.
  - Record in D5/D9 that idempotency identities are computed after privacy. Apply the same rule to P6b's generation acceptance: excluded events and private spans must not enter its hash.
  - Add a test asserting that two requests differing only in a `<private>` span or a `password=` value yield the same `request_sha256`.

### Minor

- **m1, Sonnet 5.5 thinks by default** (`services/memory/src/adapters/anthropic/mod.rs:114-125`).
  - Omitting `thinking` runs adaptive thinking on Sonnet 5.5, so thinking tokens come out of `max_tokens`, which is the client's `max_output_tokens` (≤ 8,192). The result is more `output_truncated` outcomes than the client's budget implies, and `thinking` blocks in `content` (filtered out, but billed).
  - For this single-shot extraction, send `thinking: {"type": "between_tools"}`. Sonnet 5.5 accepts it at effort ≤ `high` and it takes no other field; the effort here is `low`.
  - Alternatively, document that `max_output_tokens` includes thinking.
  - Update the exact-body test accordingly.
- **m2, the `credential` pattern redacts prose** (`services/memory/src/domain/generation.rs`, the `credential` pattern `(?i)\b(bearer|basic)(\s+)[A-Za-z0-9._~+/=\-]{8,}`).
  - "the basic configuration" becomes "the basic [REDACTED:credential]", which corrupts ordinary memories.
  - Required: require a credential-looking token (base64 shape with a digit, `+`, `/` or `=`, or ≥ 20 characters), or anchor it to `Authorization:`. Add a prose case to the redaction tests.
- **m3, a literal marker defeats idempotence** (`domain/generation.rs`, `redact`). The rule leaves any match whose text *contains* `[REDACTED:` unchanged, so `token=abc[REDACTED:x]def` keeps `abc…def` raw. Required: skip only when the captured *value* is exactly a marker.
- **m4, identifiers are not redacted.**
  - `external_ref`, revision and `event_id` are stored or sent unredacted. A URL ref with `?token=…` or a key in an event id survives, and `event_id` is sent to the provider.
  - The untrusted `description` is redacted, but its level is implicitly the consumed watermark.
  - Required: run the secret patterns over refs and ids too, or refuse refs and ids that match them. State in D9 that the description is covered by the client's consumed attestation.

## Assumptions challenged / scope

- **No refusal fallback.** The skill recommends `fallbacks: "default"` for Sonnet 5.5, but a fallback would send the window to a model other than the one the user decided on. Mapping `refusal` to terminal `provider_refused` is the right call here; keep it recorded in D9.
- **Excluded or privacy-emptied events do not raise the label.** Correct, because they never reach the generation context, while above-ceiling refusal still covers them. This matches 04A.5's "consumed by the generation context".
- **Speculative scope:** none. `reqwest` with rustls and `regex` are needed; `aws-lc-sys` comes from rustls's default provider. P6b should check the boundary and image build with that C dependency.

## Remaining required checks

1. Fix F1 and m1–m4, then rerun focused `memory_observation_`, `memory_contract_` and `memory_storage_` (sources), then `verify.sh`.
2. A follow-up reviewer should read what I could not: `observation_tests.rs`, the m2 rerun-on-correction path in `recall.rs`, and the outcome-mapping tail of `generation.rs`.
3. CI only, carried over: the first `memory-service.yml` run (Linux isolation, model cache, extracted copy, now including `aws-lc-sys`) and the image job.

## Verdict

**Not passed, on F1 (major).** Everything else reviewed is sound: labels, refusals, destination check, schema split, outcome mapping and request shape. verify.sh passed 133/133.
