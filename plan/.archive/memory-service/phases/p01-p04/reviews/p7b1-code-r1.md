# Review p7b1-code-r1: P7b substep 1 (configuration, provisioner port, adapter, fake, credentials, schema)

Reviewer B, generation 4 (agent `a5916c4a63f198c6f`, nonce `run=8e7c1e85 role=B gen=4`).
Date 2026-11-08. Read-only: no project file was edited and nothing was committed. One scratch
database (`reviewer_b_p7b1`) was created on the private cluster (127.0.0.1:55432) and dropped
afterwards. No cargo build was run, so the shared `target/` was not touched.

**Verdict: changes required — two Major, six Minor and five Nit findings.** Configuration, token
custody, the rotation inventory and most of the schema conform to E1 and E2. The adapter's error
classification and the contract fake need fixing before the lifecycle worker (and, later, the
delivery intents) are built on them.

## Artifact identity

| Artifact | sha256 (recomputed) | Extent |
| --- | --- | --- |
| `tmp/memory-service-run/frozen/p7b1-code-r1.diff` | `a571f97812e967b4edeec1c0a24d96d2a685334823a6e92332a82d1870a8c52a` (matches) | full diff |
| `tmp/memory-service-run/frozen/p7b1-incremental-from-p7a.diff` | `ea4c3ee95993b55b8ac225608e9591ded08c630adab5d184b431c779e9a168d3` (matches) | 20 files, +1808/−12 |

I reviewed tree `c2e6f6090fe2609dfcd97818d964b8fc34add6e7`, read with `git archive` into scratch.
I did not review the moving working tree.

## Coverage

**Read in full:**

- `src/infra/memory_config.rs` and its tests.
- `src/application/memory/{mod,ports}.rs`.
- `src/adapters/memory_service/{mod,limits,contract_fake,tests}.rs`.
- `src/adapters/persistence/memory/{mod,inventory}.rs`.
- The `credential_rotation.rs`, `envelope.rs`, `setup.rs` and `persistence/mod.rs` hunks (the
  legacy-schema test).
- The init-schema additions.
- The plan and record edits: 05's verification record, the E3, E11 and E12 changes, and the P7b
  handoff notes.

**Checked against:**

- The service contract: `openapi.json` admin request and response schemas, the `Operation` and
  `State` enums, and the service's actual tenant, namespace and credential code
  (`services/memory/src/adapters/postgres/mod.rs`: `change_tenant_once`, `create_namespace`,
  `delete_namespace`, `issue_credential`, credential list and revoke). Also the token format in
  `domain/credential.rs`.
- reqwest 0.13.4's error kinds (`src/error.rs`, `async_impl/client.rs`).

**Run myself:**

- `scripts/transport-boundary-check.sh` on base `a6bed48a` and on the frozen tree. Both fail
  identically (3 lines, one rule, `channel_reply_continuation_views_support.rs:59`). The author's
  "pre-existing" claim is confirmed.
- The frozen init schema applied to a scratch database with `psql -v ON_ERROR_STOP=1`. It applied
  cleanly. I then probed the lifecycle CHECKs (M-1 below).

**Evidence relied on:** the author's logs (`tmp/memory-service-run/logs/a6-p7b-*.log`) show:

| Selection | Selected | Passed |
| --- | --- | --- |
| `memory_integration_` | 9 | 9 |
| `memory_scope_` | 1 | 1 |
| `the_memory_provider_schema_is_gone` | 1 | 1 |
| `credential_rotation` | 13 | 13 |

## Findings

### Major

**M1. The transport and status classifier calls a possibly-executed request `Protocol` instead of
`Ambiguous`.**

Location: `src/adapters/memory_service/mod.rs:117-125`, `:149-169`, and the test at
`tests.rs:146`.

The problem has two parts.

1. Transport errors:
   - `transport()` maps `is_request() && !is_timeout()` to `Protocol`.
   - In reqwest 0.13.4, *every* hyper client call error is `Kind::Request`
     (`async_impl/client.rs:156`, `map_err(crate::error::request)`). That includes a connection
     reset, broken pipe, incomplete message or h2 stream reset after the request was written,
     i.e. after the service may have committed.
   - Only `is_connect()` proves nothing was sent.
2. Statuses: `classify()` maps 500 and 502 (and anything outside the taxonomy) to `Protocol`, and
   treats 503 as `Unavailable` even without the service's error envelope. Behind a proxy
   (06B.3's separate-host topology), a 502 or 503 from the proxy can follow an executed request.

E1 defines `Ambiguous` as "a write that may have run, retried only with its key". Later code will
treat `Protocol` as terminal. For deliveries, that reports a saved memory as failed or abandons a
replayable key. For the reconciler, it breaks the "one request per remote resource in flight"
reasoning and stray-issue handling.

Required:

- Transport:
  - `is_connect()` (including a connect timeout) → `Unavailable`.
  - `is_builder()` → `Protocol`.
  - Everything else after `send()` → `Ambiguous`, including `Kind::Request` without a timeout.
- Statuses:
  - 503 → `Unavailable` only when the body is the service's error envelope with code
    `unavailable`. This mirrors D9's "definite only with the provider's envelope".
  - 500, 502, 503 without the envelope, and 504 → `Ambiguous`.
  - 400 and 413 stay `Protocol`.
- Tests:
  - Update `tests.rs:146` (500 → `Ambiguous`).
  - Add a fake fault that applies the operation and then drops the connection before responding.
    Assert `Ambiguous`, and assert the fake's state shows the write.

**M2. The contract fake can hide contract violations, and it is more permissive than the service
in ways the reconciler will rely on.**

Location: `src/adapters/memory_service/contract_fake.rs:147-180`, `:243-271`.

The problem has two parts.

1. *Violations do not fail tests.* `assert_valid` and the `spec.is_object()` assert run inside the
   axum handler, on the spawned server task (`:79-81`). There is no panic catching (no
   `CatchPanicLayer`). A request or response that violates the contract therefore panics the
   connection task. The client then sees a dropped connection: `Ambiguous` today, `Protocol`
   under the M1 code paths. Any test that expects an error (`is_err()`, `matches!(Err(..))`), or
   whose reconciler simply backs off, still passes while the contract is violated. This is the
   fake's whole purpose (E11: "validating every request and response").
2. *Fidelity gaps*, verified against the service code:

| Case | Service | Fake |
| --- | --- | --- |
| `namespaces/delete` of a never-created namespace | 404 `not_found` (`delete_namespace` `ok_or_else(not_found)`) | 200 `deleted`, and it records a tombstone that blocks a later create |
| `credentials/issue` naming a missing or deleted namespace | 400 (`"Namespace … is not active"`) | 200 |
| `namespaces/create` and `credentials/issue` on a *deleted* tenant | 409 `conflict` (`require_active`) | 403 `tenant_disabled` |
| `tenants/delete` | leaves credentials unrevoked (unusable through the tombstone) | marks them revoked |

The first gap matters most. E2's absent-namespace step and the ambiguous-create path depend on
exactly that answer. Under the real service, deleting a namespace whose create never landed is a
404 that leaves no tombstone. A delayed create can then still land. The fake would make that
reconciler test pass for the wrong reason.

Required:

- *Violations:* record each violation in `FakeState` and answer it with a distinctive 500. Add a
  `ContractFake::assert_clean()`, or a check in `Drop` that panics on the test thread, so every
  test fails on any violation.
- *Fidelity:* make the four rows above match the service. Add fake tests for them, mirroring the
  service's `memory_authority_` behaviour.
- *E2 record:* applying the real 404 behaviour, state the namespace absent path as create-then-
  delete, exactly as for tenants. Create on a tombstone answers 409 `namespace_deleted`, which
  means done. Otherwise create, then delete; only a 200 completes. Or state that a 404 is accepted
  only after a quiescence bound (persistence AGENTS, remote resources).

### Minor

**m1. The lifecycle lease CHECK lets non-claimed rows keep lease data.**

- Location: init schema, `memory_lifecycles` (`CHECK ((state = 'claimed') = (lease_owner IS NOT
  NULL AND lease_until IS NOT NULL))`).
- Problem: probed on the scratch database: a `pending` row with `lease_owner` set and
  `lease_until` NULL is accepted. Persistence AGENTS requires that pending and terminal rows keep
  no lease data.
- Required: `CHECK ((state = 'claimed') = (lease_owner IS NOT NULL))` and
  `CHECK ((lease_owner IS NULL) = (lease_until IS NULL))`, plus a rollback test.

**m2. The new rotation arm is never exercised.**

- Location: `credential_rotation.rs:471`, `:663-686`; `memory/inventory.rs`.
- Problem: the 13 `credential_rotation` tests are the existing ones. No test seals a memory
  envelope, rotates it, opens it with `memory_client_credential(company, namespace)`, and checks
  that a swapped context or a concurrent re-issue (the CAS) is not clobbered. AGENTS treats a table
  missing from the walk as never rotating. A wrong cursor or key arm here would equally never
  rotate.
- Required: one DB test covering the round trip, keyset paging across a page boundary, and the CAS
  refusal.

**m3. The legacy-schema test is not the column allowlist E10 promised.**

- Location: `src/adapters/persistence/mod.rs` (`the_memory_provider_schema_is_gone`).
- Problem:
  - Tables are an exact list, but the new tables' *columns* are not allowlisted. 05A.6 requires
    that the corpus never land in OpenBot, and a later `memory_namespaces.content`-style column
    would pass.
  - `hydradb%` *tables* are not checked; only columns are.
  - The legacy-name loop after the exact `assert_eq!` can never fail.
- Required: an exact sorted list of `table.column` for every `memory_%` table, a `hydradb%` table
  check, and deletion of the tautological loop.

**m4. The six keys are undocumented.**

- Location: `.env.example`, `docs/`.
- Problem: no file outside `src/` names `MEMORY_SERVICE_*`. E1 says "the docs say so" (that
  startup never contacts the service), and AGENTS requires documentation to name the keys the
  application actually reads and not to promise validation it does not perform.
- Required: add the keys, defaults and ranges to `.env.example` and the deploy docs. State the
  stray-key refusal and that the URL and token are not checked against the service at startup.

**m5. `429 spend_exhausted` is classified as `Capacity` (retryable).**

- Location: `mod.rs:162-164`.
- Problem: D6 has two 429 codes: `capacity_exceeded` (retry after) and `spend_exhausted`. The
  second is a terminal refusal on generation acceptance (a reservation above the whole cap). As
  `Capacity`, a generation intent would retry until it expires.
- Required: 429 with code `spend_exhausted` → `Refused { reason: "spend_exhausted" }` (or a
  dedicated variant). Test it. This is needed before P7f.

**m6. The record's U2 addendum adds tracked state for a condition that is always true.**

- Location: record E3 ("The run therefore tracks whether it consumed authored configuration").
- Problem: every `agent.run`, `ai.classify` and `decision.agent` consumes instructions. Every step
  consumes workflow literals, and every schedule run consumes its params. So a per-run "consumed
  configuration" flag is effectively constant true, and maintaining it adds an audit surface for no
  difference in outcome.
- Required: state it as a rule. A company-scope save always sends `consumed: unknown` (the ceiling
  fallback); agent and user scope use `content_exact`. Keep the E11 case. The user decision is
  unaffected; this only simplifies how it is implemented.

### Nits

- **n1.** `memory_lifecycles`: add `CHECK (owner_kind <> 'company' OR owner_id = company_id)` for
  namespace rows too. The scratch probe accepted a company-owned namespace lifecycle with a foreign
  `owner_id`.
- **n2.** The port enums `RemoteTenantState` and `RemoteNamespaceState` derive serde and are used
  directly as wire types (`ports.rs`). An unknown future service state then becomes `Protocol`.
  Prefer adapter-private wire enums mapped into the port.
- **n3.** `credentials/list` is unpaginated. Revoked strays accumulate per namespace re-issue and
  are bounded only by the 4 MiB cap. Note it for 06A retention.
- **n4.** The token is read untrimmed (`memory_config.rs:126`). A trailing newline from a secrets
  file fails parse. That fails closed; consider trimming it, as the other keys are trimmed.
- **n5.** Record E3: the edited line ("the actor's clearance for manual runs (run-as is the schedule
  case). With a level-0") is over 100 characters.

## Conformance confirmed

- **Configuration** (E1):
  - Exactly the six keys, with the stated bounds.
  - https, or http only to loopback; no userinfo, query or fragment.
  - Stray keys refused without the URL.
  - The recall deadline must be below the timeout.
  - The token shape matches the service's `msk_adm_<simple uuid>_<43 base64url>`, and the
    credential id is parsed.
  - `Debug` omits the token, and no error string contains it.
  - Per-company shares are fixed at 2 and 1.
  - Startup logs the credential id only, never contacts the service, and warns with up to 20
    pending lifecycles when the URL is unset.
- **Custody:**
  - The provisioner token exists only as a `SecretString` in the adapter and is sent in a
    `set_sensitive` header.
  - `IssuedClientCredential` has no `Debug`.
  - The client has no redirects, no retries and no proxy, and bodies are capped at 4 MiB + 64 KiB.
  - Nothing reachable from an agent-controlled connection holds a token yet. The E5 origin refusal
    is a later substep.
- **Pools:** the per-company permit is taken before the global one, the map prunes itself, and a
  wait is bounded by the deadline and answers `Unavailable`. The tests cover the share, the global
  limit and pool independence.
- **Request shapes:** they match the OpenAPI admin schemas (field names, `GrantSpec` with
  `read_max`/`write_max` 9, `CLIENT_OPERATIONS` within the `Operation` enum, label ≤ 128). Tenant
  semantics in the fake match D3: idempotent create, uniform 409, own-tombstone delete 200, foreign
  404.
- **Schema** (E2): `revision` and `disable_epoch` are separate; `provisioner_credential_id` is on
  both tables; scope-shape and credential CHECKs; the unique live-scope index; no company FK on
  lifecycles; tenant uniqueness and a due index. It applies cleanly to a fresh database.
- **Credential context and rotation:** the context binds company and namespace, and the rotation
  arm uses keyset paging (limit ≤ 100, matching the batch size) with a CAS rewrap.
- **Record:** the U2 addendum is consistent with E12 and the E11 case is added. The r4 carries
  (R4-m1 nullable `messages.content_exact`; R4-m2 settings text) are applied.
